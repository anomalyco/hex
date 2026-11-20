import CoreML
import Foundation
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

    /// Experiment: an on-device tagger removes ums, repeats, and spoken corrections after
    /// dictation. Off by default.
    var isEnabled: Bool {
        get { defaults?.bool(forKey: Key.enabled) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.enabled) }
    }
}

/// Experiment (Clean up what I say): a small BERT token classifier (4 layers, 512 wide,
/// int8 Core ML weights) labels each spoken word KEEP or DELETE, and
/// `CleanupReconstruction` joins what is kept. It runs on this iPhone only.
///
/// The model and vocabulary ship once, inside the keyboard extension; the app reads them
/// from its embedded `HEXKeyboard.appex`. The model loads when a dictation starts
/// recording with the experiment on (`prepareIfEnabled`), so the load (about 150 ms on the
/// CPU in the simulator) is over before the transcript arrives, stays loaded while the
/// process lives, and is released on a memory warning.
nonisolated final class CleanupTagger: @unchecked Sendable {
    static let shared = CleanupTagger()

    /// P(DELETE) a word must exceed to be removed (1/45 false deletes on hand-written
    /// clean sentences, against 3/45 at 0.95, with the same self-correction accuracy).
    static let deleteThreshold = 0.98
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
    }

    private struct Loaded {
        let model: MLModel
        let tokenizer: CleanupTokenizer
    }

    /// Owns `loaded`: every load, prediction, and release runs here, one at a time.
    private let queue = DispatchQueue(label: "anomaly.hex.cleanup-tagger", qos: .userInitiated)
    private let computeUnits: MLComputeUnits
    private var loaded: Loaded?
    private var memoryWarningObserver: NSObjectProtocol?

    /// The Neural Engine where there is one, else the CPU. Not the GPU: in the simulator its
    /// Metal pipeline took 0.8 s to load and added 30 MB to the footprint, while the CPU path
    /// loads in 0.15 s and adds 3 MB, keeping the int8 weights memory-mapped.
    init(computeUnits: MLComputeUnits = .cpuAndNeuralEngine) {
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
    func clean(_ text: String) throws -> String {
        try queue.sync { try cleanOnQueue(text) }
    }

    /// P(DELETE) for each whitespace-separated word, in order.
    func deleteProbabilities(for words: [String]) throws -> [Double] {
        try queue.sync { try probabilitiesOnQueue(words) }
    }

    /// The cleaned transcript within `budget`, else `text` unchanged: any failure, a late
    /// answer, or a cleanup that removes every word keeps the transcript as heard. A late
    /// run still finishes in the background, so the model is ready for the next dictation.
    func clean(_ text: String, within budget: TimeInterval) async -> String {
        let gate = CleanupTaggerOnceGate()
        return await withCheckedContinuation { (continuation: CheckedContinuation<String, Never>) in
            queue.async {
                let cleaned = (try? self.cleanOnQueue(text)) ?? text
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
    /// before translation and formatting. Off, Verbatim, or blank keeps `text` as is.
    static func cleanIfEnabled(
        _ text: String,
        style: DictationStyle,
        settings: CleanupTaggerSettings = CleanupTaggerSettings(),
        tagger: CleanupTagger = .shared
    ) async -> String {
        guard settings.isEnabled,
              style != .verbatim,
              !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return text
        }
        return await tagger.clean(text, within: timeBudget)
    }

    /// Starts loading the model when a dictation begins recording, so cleanup is ready by
    /// the time the transcript is. Does nothing while the experiment is off or for Verbatim.
    static func prepareIfEnabled(
        style: DictationStyle,
        settings: CleanupTaggerSettings = CleanupTaggerSettings(),
        tagger: CleanupTagger = .shared
    ) {
        guard settings.isEnabled, style != .verbatim else { return }
        tagger.prepare()
    }

    // MARK: On the queue

    private func cleanOnQueue(_ text: String) throws -> String {
        let words = Self.words(in: text)
        guard !words.isEmpty else { return text }
        let deletes = try probabilitiesOnQueue(words).map { $0 > Self.deleteThreshold }
        guard deletes.contains(true) else { return text }
        return CleanupReconstruction.reconstruct(words, deletes: deletes)
    }

    private func probabilitiesOnQueue(_ words: [String]) throws -> [Double] {
        let state = try loadedOnQueue()
        var probabilities: [Double] = []
        probabilities.reserveCapacity(words.count)
        for chunk in Self.chunks(words.map(state.tokenizer.encode)) {
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
