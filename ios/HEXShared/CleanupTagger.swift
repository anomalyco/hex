import CoreML
import Foundation
import os
import UIKit

/// The Clean up what I say experiment's switch in the shared App Group.
nonisolated struct CleanupTaggerSettings {
    private enum Key {
        static let enabled = "experiments.cleanupTagger"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// An on-device tagger removes ums, repeats, and spoken corrections after dictation. On
    /// by default since `CleanupGuard` keeps names, numbers and content words; a stored
    /// choice, on or off, stays as it is.
    var isEnabled: Bool {
        get { defaults?.object(forKey: Key.enabled) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.enabled) }
    }
}

/// Clean up what I say: a small BERT token classifier (4 layers, 512 wide, int8 Core ML
/// weights) gives each spoken word P(DELETE), `CleanupGuard` lets only known disfluencies
/// go, and `CleanupReconstruction` joins what is kept. It runs on this iPhone only.
///
/// The model and vocabulary ship once, inside the keyboard extension; the app reads them
/// from its embedded `HEXKeyboard.appex`. The model loads when a dictation stops
/// recording with the experiment on (`prepareIfEnabled`), so the load (about 150 ms on the
/// CPU in the simulator) overlaps transcription, stays loaded while the process lives, and
/// is released on a memory warning. The keyboard skips it when it is short of memory.
nonisolated final class CleanupTagger: @unchecked Sendable {
    static let shared = CleanupTagger()

    /// P(DELETE) a word must exceed to be marked (1/45 false deletes on hand-written
    /// clean sentences, against 3/45 at 0.95, with the same self-correction accuracy);
    /// `CleanupGuard` then decides which marked words go.
    static let deleteThreshold = CleanupGuard.deleteThreshold
    /// Longest Core ML input, [CLS] and [SEP] included; longer transcripts are tagged in
    /// chunks of whole words.
    static let maxTokens = 128
    /// The model's enumerated sequence lengths; each chunk is padded to the next one.
    static let sequenceLengths = [16, 32, 64, 128]
    /// Dictation waits this long for a cleanup before inserting the transcript as heard.
    static let timeBudget: TimeInterval = 0.150

    static let modelName = "HexCleanupTagger"
    static let vocabularyName = "HexCleanupTaggerVocab"
    private static let keyboardBundleName = "HEXKeyboard.appex"

    enum TaggerError: Error {
        case missingResource(String)
        case badOutput
        case lowMemory
    }

    /// Whether this process is the keyboard extension, which iOS ends the moment it passes
    /// its memory limit (the keyboard vanishes and the system keyboard takes over).
    static let isKeyboardExtension = Bundle.main.bundlePath.hasSuffix(".appex")
    /// Memory the keyboard must have left before it loads the model; with less, dictation
    /// inserts the transcript as heard rather than risk iOS ending the keyboard.
    static let keyboardHeadroomNeeded = 80 * 1_048_576

    private struct Loaded {
        let model: MLModel
        let tokenizer: CleanupTokenizer
    }

    /// Owns `loaded`: every load, prediction, and release runs here, one at a time.
    private let queue = DispatchQueue(label: "anomaly.hex.cleanup-tagger", qos: .userInitiated)
    private let computeUnits: MLComputeUnits
    private var loaded: Loaded?
    private var memoryWarningObserver: NSObjectProtocol?

    /// The app may use the Neural Engine; the keyboard extension runs on the CPU only, which
    /// keeps the int8 weights memory-mapped (about 3 MB added) instead of compiling a Neural
    /// Engine copy while the microphone and speech recognizer are already open. Not the GPU:
    /// in the simulator its Metal pipeline took 0.8 s to load and added 30 MB.
    init(computeUnits: MLComputeUnits = CleanupTagger.isKeyboardExtension ? .cpuOnly : .cpuAndNeuralEngine) {
        self.computeUnits = computeUnits
        memoryWarningObserver = NotificationCenter.default.addObserver(
            forName: UIApplication.didReceiveMemoryWarningNotification,
            object: nil,
            queue: nil
        ) { [weak self] _ in
            self?.unload()
        }
    }

    deinit {
        if let memoryWarningObserver {
            NotificationCenter.default.removeObserver(memoryWarningObserver)
        }
    }

    /// Whether the model is in memory now.
    var isLoaded: Bool {
        queue.sync { loaded != nil }
    }

    /// Loads the model in the background if it isn't loaded yet.
    func prepare() {
        queue.async {
            _ = try? self.loadedOnQueue()
        }
    }

    /// Frees the model; the next cleanup loads it again.
    func unload() {
        queue.async {
            self.loaded = nil
        }
    }

    /// The cleaned transcript, or `text` itself when nothing is removed. Blocks the caller
    /// until the model has loaded and run; never call it on the main thread.
    /// `protectedTerms` (custom vocabulary, learned words) go only in a self-correction.
    func clean(_ text: String, protectedTerms: [String] = []) throws -> String {
        try queue.sync { try cleanOnQueue(text, protectedTerms: protectedTerms) }
    }

    /// P(DELETE) for each whitespace-separated word, in order, as the model scores it (a
    /// hyphen between letters dropped, `CleanupGuard.modelWord`).
    func deleteProbabilities(for words: [String]) throws -> [Double] {
        try queue.sync { try probabilitiesOnQueue(words) }
    }

    /// The cleaned transcript within `budget`, else `text` unchanged: any failure, a late
    /// answer, or a cleanup that removes every word keeps the transcript as heard. A late
    /// run still finishes in the background, so the model is ready for the next dictation.
    func clean(_ text: String, within budget: TimeInterval, protectedTerms: [String] = []) async -> String {
        let gate = CleanupTaggerOnceGate()
        return await withCheckedContinuation { (continuation: CheckedContinuation<String, Never>) in
            queue.async {
                let cleaned = (try? self.cleanOnQueue(text, protectedTerms: protectedTerms)) ?? text
                let result = cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? text : cleaned
                if gate.claim() {
                    continuation.resume(returning: result)
                }
            }
            DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + budget) {
                if gate.claim() {
                    continuation.resume(returning: text)
                }
            }
        }
    }

    /// Clean up what I say, as dictation runs it: after transcription and learned fixes,
    /// before translation and formatting. Off, Verbatim, or blank keeps `text` as is. The
    /// tagger only knows English (on Spanish it dropped negations), so Spanish, German and
    /// Norwegian get `SpokenCleanupRules` instead and other languages stay as heard. A nil
    /// `language` counts as English. Spoken symbols ("john at the rate gmail dot com" to
    /// "john@gmail.com", `SpokenCleanupRules.spokenSymbols`) and a spoken count in digits
    /// ("Test one, 2, 3." to "Test 1, 2, 3.", `SpokenCleanupRules.spokenCounts`) apply in every
    /// style but Verbatim whether the experiment is on or off, before the tagger: both the
    /// app's and the keyboard's dictation pass through here (`DictationPipeline`).
    /// `protectedTerms` are custom vocabulary and learned words, which the tagger removes only
    /// in a self-correction.
    static func cleanIfEnabled(
        _ text: String,
        style: DictationStyle,
        language: String? = nil,
        protectedTerms: [String] = [],
        numbers: Bool = false,
        settings: CleanupTaggerSettings = CleanupTaggerSettings(),
        tagger: CleanupTagger = .shared
    ) async -> String {
        guard style != .verbatim, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return text
        }
        let counted = SpokenCleanupRules.spokenCounts(
            in: SpokenCleanupRules.spokenSymbols(in: text, language: language),
            language: language
        )
        // Before the tagger, so a number in digits is a word it never removes outside a
        // self-correction (`CleanupGuard`).
        let text = numbers
            ? SpokenNumbers.digits(in: counted, language: language, protectedTerms: protectedTerms)
            : counted
        guard settings.isEnabled else { return text }
        guard SpeechLanguagePicker.isEnglish(language) else {
            return SpokenCleanupRules.clean(text, language: language)
        }
        return await tagger.clean(text, within: timeBudget, protectedTerms: protectedTerms)
    }

    /// Starts loading the model when a dictation stops recording, while it transcribes, so
    /// cleanup is ready by the time the transcript is. Does nothing while the experiment is
    /// off, for Verbatim, or for a language other than English.
    static func prepareIfEnabled(
        style: DictationStyle,
        language: String? = nil,
        settings: CleanupTaggerSettings = CleanupTaggerSettings(),
        tagger: CleanupTagger = .shared
    ) {
        guard settings.isEnabled, style != .verbatim, SpeechLanguagePicker.isEnglish(language) else { return }
        tagger.prepare()
    }

    // MARK: On the queue

    private func cleanOnQueue(_ text: String, protectedTerms: [String]) throws -> String {
        let words = Self.words(in: text)
        guard !words.isEmpty else { return text }
        let deletes = try CleanupGuard.deletes(
            words,
            probabilities: probabilitiesOnQueue(words),
            protectedTerms: CleanupGuard.protectedTerms(protectedTerms)
        )
        guard deletes.contains(true) else { return text }
        return CleanupReconstruction.reconstruct(words, deletes: deletes)
    }

    private func probabilitiesOnQueue(_ words: [String]) throws -> [Double] {
        let state = try loadedOnQueue()
        var probabilities: [Double] = []
        probabilities.reserveCapacity(words.count)
        for chunk in Self.chunks(words.map { state.tokenizer.encode(CleanupGuard.modelWord($0)) }) {
            probabilities += try predict(chunk, model: state.model)
        }
        return probabilities
    }

    private func loadedOnQueue() throws -> Loaded {
        if let loaded { return loaded }
        guard let modelURL = Self.resourceURL(Self.modelName, withExtension: "mlmodelc") else {
            throw TaggerError.missingResource(Self.modelName)
        }
        guard let vocabularyURL = Self.resourceURL(Self.vocabularyName, withExtension: "txt") else {
            throw TaggerError.missingResource(Self.vocabularyName)
        }
        if Self.isKeyboardExtension {
            // 0 means iOS didn't say (the simulator); only a known shortfall skips cleanup.
            let headroom = os_proc_available_memory()
            if headroom > 0, headroom < Self.keyboardHeadroomNeeded {
                throw TaggerError.lowMemory
            }
        }
        let tokenizer = try CleanupTokenizer(vocabulary: String(contentsOf: vocabularyURL, encoding: .utf8))
        let configuration = MLModelConfiguration()
        configuration.computeUnits = computeUnits
        let model = try MLModel(contentsOf: modelURL, configuration: configuration)
        let state = Loaded(model: model, tokenizer: tokenizer)
        loaded = state
        return state
    }

    /// One model run: [CLS] ids [SEP], zero-padded to an enumerated length, with the real
    /// length masking the padding. Returns P(DELETE) at each word's first subword.
    private func predict(_ chunk: Chunk, model: MLModel) throws -> [Double] {
        let count = chunk.ids.count
        guard let length = Self.sequenceLengths.first(where: { $0 >= count }) else {
            throw TaggerError.badOutput
        }
        let inputIDs = try MLMultiArray(shape: [1, NSNumber(value: length)], dataType: .int32)
        inputIDs.withUnsafeMutableBufferPointer(ofType: Int32.self) { buffer, strides in
            let step = strides[1]
            for position in 0..<length {
                buffer[position * step] = position < count ? chunk.ids[position] : CleanupTokenizer.padID
            }
        }
        let realLength = try MLMultiArray(shape: [1], dataType: .int32)
        realLength[0] = NSNumber(value: count)
        let input = try MLDictionaryFeatureProvider(dictionary: [
            "input_ids": MLFeatureValue(multiArray: inputIDs),
            "length": MLFeatureValue(multiArray: realLength),
        ])
        let output = try model.prediction(from: input)
        guard let logits = output.featureValue(for: "delete_logit")?.multiArrayValue,
              logits.count >= count else {
            throw TaggerError.badOutput
        }
        return chunk.firstTokens.map { position in
            guard let position else { return 0 } // a word with no tokens is kept
            let logit = logits[[0, NSNumber(value: position)]].doubleValue
            return 1 / (1 + exp(-logit))
        }
    }

    // MARK: Words and chunks

    struct Chunk: Equatable {
        /// [CLS], the words' WordPiece ids, [SEP].
        var ids: [Int32]
        /// Where each word's first subword sits in `ids`; nil for a word with no tokens.
        var firstTokens: [Int?]
    }

    /// The transcript's whitespace-separated words.
    static func words(in text: String) -> [String] {
        text.split(whereSeparator: \.isWhitespace).map(String.init)
    }

    /// Whole words per chunk, each chunk within `maxTokens` with [CLS] and [SEP]. A single
    /// word longer than that keeps only its first tokens; only its first subword is scored.
    static func chunks(_ words: [[Int32]]) -> [Chunk] {
        var chunks: [Chunk] = []
        var current = Chunk(ids: [CleanupTokenizer.clsID], firstTokens: [])
        for var tokens in words {
            if !current.firstTokens.isEmpty, current.ids.count + 1 + tokens.count > maxTokens {
                current.ids.append(CleanupTokenizer.sepID)
                chunks.append(current)
                current = Chunk(ids: [CleanupTokenizer.clsID], firstTokens: [])
            }
            if tokens.count > maxTokens - 2 {
                tokens = Array(tokens.prefix(maxTokens - 2))
            }
            current.firstTokens.append(tokens.isEmpty ? nil : current.ids.count)
            current.ids += tokens
        }
        if !current.firstTokens.isEmpty {
            current.ids.append(CleanupTokenizer.sepID)
            chunks.append(current)
        }
        return chunks
    }

    /// The keyboard extension finds the model in its own bundle, the app in the keyboard
    /// extension it embeds.
    static func resourceURL(_ name: String, withExtension ext: String) -> URL? {
        if let url = Bundle.main.url(forResource: name, withExtension: ext) {
            return url
        }
        guard let plugIns = Bundle.main.builtInPlugInsURL,
              let keyboard = Bundle(url: plugIns.appendingPathComponent(keyboardBundleName)) else {
            return nil
        }
        return keyboard.url(forResource: name, withExtension: ext)
    }
}

/// Resumes a cleanup race exactly once, whichever of the tagger or the budget finishes first.
private final class CleanupTaggerOnceGate: @unchecked Sendable {
    private let lock = NSLock()
    private var claimed = false

    func claim() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard !claimed else { return false }
        claimed = true
        return true
    }
}
