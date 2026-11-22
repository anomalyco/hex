import Foundation
import os
import Speech

/// One in-keyboard dictation's live words (Settings › Dictation › Show words as you speak):
/// started with the capture's first audio when a switch wants them, fed every chunk the page
/// sends, and stopped when the capture stops, keeping its words on screen until the
/// dictation goes in. It never changes the dictation itself: whenever it can't run (a switch
/// off, short of memory, the language's model not on the iPhone, an error, or no first words
/// within `firstWordsPatience`) the keyboard records and inserts exactly as without it.
@MainActor
final class KeyboardLiveDictation {
    private let text: LiveDictationText
    private let sampleRate: Double
    private var source: LiveTranscriptSource?
    /// Audio from before the source exists (the language's lookup), replayed into it.
    private var held: [Data] = []
    private var heldBytes = 0
    private var stopped = false
    private var resolving: Task<Void, Never>?
    private var latest: LiveTranscript?
    private var publishScheduled = false
    private let startedAt = Date()
    private var firstAudioAt: Date?
    private var firstWordsAt: Date?
    private var updates = 0
    private var rewrites = 0
    private var shownWords: [Substring] = []
    private let footprintAtStart = KeyboardParakeet.memory().footprint
    private var peakFootprint = 0

    /// Memory the keyboard must still have once its capture runs: the live recogniser's own
    /// share in the keyboard (about 2 MB measured, its model runs in Apple's speech service),
    /// with room left for the dictation's transcription after it.
    static let headroomNeeded = 24 * 1_048_576
    /// Audio held while the language is looked up; past it the live words give up.
    static let maximumHeldSeconds: Double = 4

    private let settings: LiveDictationSettings

    private init(text: LiveDictationText, sampleRate: Double, settings: LiveDictationSettings) {
        self.text = text
        self.sampleRate = sampleRate
        self.settings = settings
    }

    /// Starts live words for a capture at `sampleRate`, or returns nil (today's behaviour)
    /// when neither switch wants them or memory is short.
    static func start(sampleRate: Double, text: LiveDictationText?) -> KeyboardLiveDictation? {
        guard let text else { return nil }
        let settings = LiveDictationSettings()
        guard settings.wantsLiveWords else { return nil }
        let available = os_proc_available_memory()
        guard available == 0 || available >= headroomNeeded else {
            KeyboardProbeLog.shared.write("live: skipped, short of memory \(KeyboardProbeLog.memory())")
            return nil
        }
        let live = KeyboardLiveDictation(text: text, sampleRate: sampleRate, settings: settings)
        live.resolve()
        return live
    }

    func append(_ pcm: Data) {
        guard !stopped else { return }
        if firstAudioAt == nil { firstAudioAt = Date() }
        if let source {
            source.append(pcm)
            return
        }
        held.append(pcm)
        heldBytes += pcm.count
        if Double(heldBytes / 2) / sampleRate > Self.maximumHeldSeconds {
            giveUp("no recogniser after \(Int(Self.maximumHeldSeconds)) s of audio")
        }
    }

    /// The capture stopped: the recogniser goes now, its words stay until the dictation is in.
    func stop() {
        guard !stopped else { return }
        stopped = true
        resolving?.cancel()
        source?.stop()
        source = nil
        held = []
        publish()
        let first = firstWordsAt.map { "\(Int($0.timeIntervalSince(firstAudioAt ?? startedAt) * 1000)) ms after first audio" } ?? "none"
        KeyboardProbeLog.shared.write(
            "live: stopped after \(String(format: "%.1f", Date().timeIntervalSince(startedAt))) s, first words \(first), \(updates) updates, \(rewrites) words rewritten, footprint +\(String(format: "%.1f", Double(max(0, peakFootprint - footprintAtStart)) / 1_048_576)) MB at most \(KeyboardProbeLog.memory())"
        )
    }

    /// iOS warned the keyboard about memory: the recogniser goes, the dictation goes on.
    func memoryWarning() {
        guard !stopped else { return }
        KeyboardProbeLog.shared.write("live: stopped for a memory warning")
        stop()
    }

    /// Finds the language the dictation will be transcribed in and starts Apple's recogniser
    /// for it, only if its model is already on the iPhone.
    private func resolve() {
        guard #available(iOS 26.0, *) else {
            giveUp("needs iOS 26")
            return
        }
        let rate = sampleRate
        resolving = Task { [weak self] in
            let settings = LanguageSettings()
            let picked = await AppleSpeechLanguages.resolve(settings: settings)
            var installed = false
            if let picked {
                installed = await AssetInventory.status(forModules: [AppleSpeechModule(picked).module]) == .installed
            }
            guard let self, !self.stopped else { return }
            guard let picked, installed else {
                if let replay = LiveTranscriptReplay.load(log: { KeyboardProbeLog.shared.write($0) }) {
                    self.attach(replay)
                    return
                }
                self.giveUp(picked.map { "\($0.identifier) model not on this iPhone" } ?? "no language")
                return
            }
            guard let source = AppleLiveTranscriber(
                locale: picked.locale,
                module: picked.module == .dictation ? .dictation : .speech,
                sampleRate: rate,
                log: { KeyboardProbeLog.shared.write($0) }
            ) else {
                self.giveUp("no audio format")
                return
            }
            self.attach(source)
        }
    }

    /// The preview shows from here (empty until the first words), never while the language is
    /// still looked up, so a dictation without live words never flashes it.
    private func attach(_ source: LiveTranscriptSource) {
        text.begin(showsPreview: settings.showsPreview, look: settings.look)
        source.onUpdate = { [weak self] transcript in self?.received(transcript) }
        source.onFailure = { [weak self] reason in self?.giveUp(reason) }
        self.source = source
        let audio = held
        held = []
        heldBytes = 0
        audio.forEach { source.append($0) }
    }

    private func received(_ transcript: LiveTranscript) {
        guard !stopped else { return }
        if firstWordsAt == nil, !transcript.isEmpty { firstWordsAt = Date() }
        latest = transcript
        peakFootprint = max(peakFootprint, KeyboardParakeet.memory().footprint)
        // A recogniser's results come in bursts, a word at a time: the screen takes the last.
        guard !publishScheduled else { return }
        publishScheduled = true
        DispatchQueue.main.async { [weak self] in
            MainActor.assumeIsolated { self?.publish() }
        }
    }

    private func publish() {
        publishScheduled = false
        guard let latest else { return }
        self.latest = nil
        updates += 1
        let words = latest.text.split(whereSeparator: \.isWhitespace)
        rewrites += Self.changedWords(from: shownWords, to: words)
        shownWords = words
        text.update(latest)
    }

    /// How many words already on screen `next` changes or takes away.
    nonisolated static func changedWords(from shown: [Substring], to next: [Substring]) -> Int {
        var same = 0
        while same < min(shown.count, next.count), shown[same] == next[same] { same += 1 }
        return shown.count - same
    }

    private func giveUp(_ reason: String) {
        guard !stopped else { return }
        KeyboardProbeLog.shared.write("live: none for this dictation (\(reason))")
        stop()
        // Without words, the keys come back as they are without the preview.
        if text.transcript?.isEmpty ?? true { text.end() }
    }
}

/// The simulator has no speech models: a recorded run of Apple's recogniser (`live-replay.json`
/// in the App Group, from `AppleLiveTranscriber` on a Mac) replayed at its own pace from the
/// capture's first audio, so the preview can be seen in the real keyboard there. Never on an
/// iPhone.
@MainActor
final class LiveTranscriptReplay: LiveTranscriptSource {
    private struct Step: Decodable {
        let t: Double
        let settled: String
        let tentative: String
    }

    var onUpdate: ((LiveTranscript) -> Void)?
    var onFailure: ((String) -> Void)?
    private let steps: [Step]
    private var task: Task<Void, Never>?

    private init(steps: [Step]) {
        self.steps = steps
    }

    static func load(log: (String) -> Void) -> LiveTranscriptReplay? {
        #if targetEnvironment(simulator)
        // The App Group, or the keyboard's own Library where an unsigned build has no group.
        let folders = [
            FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup),
            FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask).first,
        ].compactMap { $0 }
        guard let data = folders.lazy.compactMap({ try? Data(contentsOf: $0.appendingPathComponent("live-replay.json")) }).first,
            let steps = try? JSONDecoder().decode([Step].self, from: data), !steps.isEmpty else { return nil }
        log("live: replaying \(steps.count) recorded results (simulator)")
        return LiveTranscriptReplay(steps: steps)
        #else
        return nil
        #endif
    }

    func append(_ pcm: Data) {
        guard task == nil else { return }
        let steps = steps
        let start = Date()
        task = Task { [weak self] in
            for step in steps {
                let wait = step.t - Date().timeIntervalSince(start)
                if wait > 0 { try? await Task.sleep(for: .seconds(wait)) }
                guard !Task.isCancelled else { return }
                self?.onUpdate?(LiveTranscript(settled: step.settled, tentative: step.tentative))
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
        onUpdate = nil
    }
}
