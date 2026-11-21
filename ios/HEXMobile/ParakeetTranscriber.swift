import FluidAudio
import Foundation

struct ModelPreparationProgress: Sendable {
    let fractionCompleted: Double
    let detail: String
}

protocol LocalTranscribing: Actor {
    nonisolated var displayName: String { get }
    var languageName: String { get }
    /// The BCP 47 identifier of the language it transcribes.
    var languageIdentifier: String { get }

    func prepare(
        progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void
    ) async throws

    func transcribe(_ audioURL: URL) async throws -> String
    func transcribe(_ audioURL: URL, vocabulary: [String]) async throws -> String
}

extension LocalTranscribing {
    /// English unless the engine says otherwise.
    var languageIdentifier: String { "en-US" }

    /// Engines without a recognition hint ignore the list and transcribe exactly as before.
    nonisolated func transcribe(_ audioURL: URL, vocabulary: [String]) async throws -> String {
        try await transcribe(audioURL)
    }
}

actor ParakeetTranscriber: LocalTranscribing {
    enum TranscriberError: HexDescribedError {
        case notPrepared
        case emptyTranscript
        case downloadFailed

        var errorDescription: String? {
            switch self {
            case .notPrepared:
                String(localized: "Parakeet V2 is not ready.")
            case .emptyTranscript:
                String(localized: "Parakeet did not detect any speech.")
            case .downloadFailed:
                String(localized: "Parakeet V2 didn't download. Check your connection and try again.")
            }
        }
    }

    /// Whether this iPhone has the memory to load Parakeet V2: 6 GB iPhones report about
    /// 5.5 GB, 4 GB ones (iPhone 11, 12, 13) under 4 GB.
    nonisolated static var fitsInMemory: Bool {
        ProcessInfo.processInfo.physicalMemory >= 5 << 30
    }

    /// How long the download may sit at one fraction before Hex stops waiting. Core ML's
    /// compile, after it, reports nothing and can't be cancelled, so it isn't watched.
    static let downloadStallLimit: Duration = .seconds(90)

    private var manager: AsrManager?

    nonisolated let displayName = "Parakeet V2"
    let languageName = "English"

    private nonisolated static var cacheDirectory: URL {
        AsrModels.defaultCacheDirectory(for: .v2)
    }

    nonisolated static var hasCachedModel: Bool {
        AsrModels.modelsExist(at: cacheDirectory, version: .v2)
    }

    func prepare(
        progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void
    ) async throws {
        guard manager == nil else { return }

        let compiled = ParakeetCompileRecord()
        let progress = ParakeetProgressFeed(
            ParakeetPreparationProgress(downloads: !Self.hasCachedModel, compiledBefore: compiled.isCurrent),
            emit: progressHandler
        )
        let ticker = Task {
            while !Task.isCancelled {
                progress.tick()
                try? await Task.sleep(for: .milliseconds(250))
            }
        }
        defer { ticker.cancel() }

        if !Self.hasCachedModel {
            try await download(progress)
        }

        // Only the load, where iOS can end Hex short of memory, counts for the guard: a
        // download cut off by a quit or a suspended app ending resumes at the next one.
        let loads = ParakeetLoadGuard()
        loads.begin()
        var loaded = false
        defer { loads.end(loaded: loaded) }
        progress.update { $0.startLoading(at: Date.now.timeIntervalSinceReferenceDate) }
        // `load` alone: `downloadAndLoad` loads (and on a first run compiles) every model
        // once to check the download and again to return it.
        let models = try await AsrModels.load(
            from: Self.cacheDirectory,
            version: .v2,
            progressHandler: { event in
                // FluidAudio ends each model's load with an empty compiling name.
                guard case .compiling(let name) = event.phase, name.isEmpty else { return }
                let now = Date.now.timeIntervalSinceReferenceDate
                progress.update { $0.finishModel(at: now) }
            }
        )
        let manager = AsrManager(config: .default)
        try await manager.loadModels(models)
        self.manager = manager
        loaded = true
        compiled.markCurrent()
        progress.update { $0.finish() }
    }

    /// FluidAudio's repo download alone, which resumes each file from its `.partial`.
    private func download(_ progress: ParakeetProgressFeed) async throws {
        let fraction = ProgressReading()
        let directory = Self.cacheDirectory.deletingLastPathComponent()
        do {
            try await StallWatch.run(
                limit: Self.downloadStallLimit,
                progress: { fraction.value },
                operation: {
                    // FluidAudio's download reports 0–50%, leaving the rest for a compile.
                    try await ModelHub.download(.parakeetV2, to: directory, progressHandler: { event in
                        fraction.value = event.fractionCompleted
                        progress.update { $0.download(min(1, event.fractionCompleted * 2)) }
                    })
                    let vocabulary = Self.cacheDirectory.appendingPathComponent(ModelNames.ASR.vocabularyFile)
                    if !FileManager.default.fileExists(atPath: vocabulary.path) {
                        // The repo's listing brings it; this is for one that didn't.
                        let data = try await ModelHub.fetchFile(
                            from: ModelRegistry.resolveModel(Repo.parakeetV2.remotePath, ModelNames.ASR.vocabularyFile),
                            description: ModelNames.ASR.vocabularyFile
                        )
                        try data.write(to: vocabulary, options: .atomic)
                    }
                }
            )
        } catch let error as DownloadError {
            // FluidAudio's descriptions carry Hugging Face paths and HTML; say it plainly.
            switch error {
            case .invalidResponse, .rateLimited, .downloadFailed, .htmlErrorResponse, .invalidArtifact:
                throw TranscriberError.downloadFailed
            default:
                throw error
            }
        }
    }

    func transcribe(_ audioURL: URL) async throws -> String {
        guard let manager else {
            throw TranscriberError.notPrepared
        }

        let decoderLayers = await manager.decoderLayerCount
        var decoderState = TdtDecoderState.make(decoderLayers: decoderLayers)
        let result = try await manager.transcribe(
            audioURL,
            decoderState: &decoderState
        )
        let text = result.text.trimmingCharacters(in: CharacterSet.whitespacesAndNewlines)
        guard !text.isEmpty else {
            throw TranscriberError.emptyTranscript
        }
        return text
    }
}

/// What Parakeet's preparation shows. The download moves with its bytes; Core ML says only
/// when each model starts and ends loading, and the first load on an iPhone compiles the
/// encoder for its Neural Engine for a minute or more, so each model's share fills on a time
/// estimate instead of sitting still.
nonisolated struct ParakeetPreparationProgress: Sendable, Equatable {
    /// Seconds each model takes to load, in FluidAudio's order (preprocessor, encoder,
    /// decoder, joint): the first time on an iPhone 14, and once Core ML has compiled them.
    static let firstLoadSeconds: [Double] = [3, 60, 4, 4]
    static let laterLoadSeconds: [Double] = [0.5, 3, 0.5, 0.5]
    /// The bar's share for a download, when there is one.
    static let downloadShare = 0.7

    let downloads: Bool
    let compiledBefore: Bool
    private(set) var downloaded = 0.0
    private(set) var loadStartedAt: TimeInterval?
    private(set) var modelsLoaded = 0
    /// The load's fraction at the last model Core ML finished, and when that was.
    private var loadBase = 0.0
    private var segmentStartedAt: TimeInterval = 0
    private(set) var isFinished = false

    init(downloads: Bool, compiledBefore: Bool) {
        self.downloads = downloads
        self.compiledBefore = compiledBefore
    }

    mutating func download(_ fraction: Double) {
        downloaded = max(downloaded, min(1, fraction))
    }

    mutating func startLoading(at time: TimeInterval) {
        downloaded = 1
        loadStartedAt = time
        segmentStartedAt = time
    }

    mutating func finishModel(at time: TimeInterval) {
        let shown = loadFraction(at: time)
        modelsLoaded = min(modelsLoaded + 1, estimates.count)
        let done = estimates.prefix(modelsLoaded).reduce(0, +) / expectedLoadSeconds
        loadBase = min(1, max(shown, done))
        segmentStartedAt = time
    }

    mutating func finish() {
        isFinished = true
    }

    private var estimates: [Double] {
        compiledBefore ? Self.laterLoadSeconds : Self.firstLoadSeconds
    }

    /// The estimate for the whole load, which the copy rounds.
    var expectedLoadSeconds: Double { estimates.reduce(0, +) }

    /// Between Core ML's reports the rest fills on the time the models left should take:
    /// half of it by then, two thirds by twice that, and it keeps creeping however long a
    /// slower iPhone takes, never reaching the end before Core ML says it's done.
    private func loadFraction(at now: TimeInterval) -> Double {
        let remaining = max(0.5, estimates.dropFirst(modelsLoaded).reduce(0, +))
        let x = max(0, now - segmentStartedAt) / remaining
        return loadBase + (1 - loadBase) * x / (x + 1)
    }

    func snapshot(at now: TimeInterval) -> ModelPreparationProgress {
        if isFinished {
            return ModelPreparationProgress(fractionCompleted: 1, detail: String(localized: "Ready"))
        }
        let downloadShare = downloads ? Self.downloadShare : 0
        guard let loadStartedAt else {
            if downloads, downloaded > 0 {
                return ModelPreparationProgress(
                    fractionCompleted: downloadShare * downloaded,
                    detail: String(localized: "Downloading the speech model")
                )
            }
            return ModelPreparationProgress(fractionCompleted: 0, detail: String(localized: "Checking the speech model"))
        }
        let fraction = downloadShare + (1 - downloadShare) * loadFraction(at: now)
        let detail: String
        if compiledBefore {
            detail = String(localized: "Loading the speech model")
        } else if now - loadStartedAt < expectedLoadSeconds * 2 {
            detail = String(localized: "Optimizing for this iPhone… about a minute")
        } else {
            detail = String(localized: "Still optimizing. The first time can take a few minutes.")
        }
        return ModelPreparationProgress(fractionCompleted: min(0.99, fraction), detail: detail)
    }
}

/// Feeds `ParakeetPreparationProgress` to the handler, at most ten times a second (the
/// download alone reports thousands of times) and whenever its words change.
private nonisolated final class ParakeetProgressFeed: @unchecked Sendable {
    private let lock = NSLock()
    private var state: ParakeetPreparationProgress
    private var lastEmit: TimeInterval = 0
    private var lastDetail = ""
    private let emit: @Sendable (ModelPreparationProgress) -> Void

    init(_ state: ParakeetPreparationProgress, emit: @escaping @Sendable (ModelPreparationProgress) -> Void) {
        self.state = state
        self.emit = emit
    }

    func update(_ change: (inout ParakeetPreparationProgress) -> Void) {
        let now = Date.now.timeIntervalSinceReferenceDate
        let snapshot: ModelPreparationProgress? = lock.withLock {
            change(&state)
            let snapshot = state.snapshot(at: now)
            guard state.isFinished || snapshot.detail != lastDetail || now - lastEmit >= 0.1 else {
                return nil
            }
            lastEmit = now
            lastDetail = snapshot.detail
            return snapshot
        }
        if let snapshot { emit(snapshot) }
    }

    func tick() {
        update { _ in }
    }
}

/// The system Core ML last compiled Parakeet for. Core ML keeps what it compiled for the
/// Neural Engine between launches, so only the first load (and the first after an iOS
/// update) is slow; this picks the words and estimate. In the app's own defaults.
nonisolated struct ParakeetCompileRecord {
    private static let key = "parakeet.compiledForSystem"
    private let defaults: UserDefaults
    private let system: String

    init(defaults: UserDefaults = .standard, system: String = ProcessInfo.processInfo.operatingSystemVersionString) {
        self.defaults = defaults
        self.system = system
    }

    var isCurrent: Bool {
        defaults.string(forKey: Self.key) == system
    }

    func markCurrent() {
        defaults.set(system, forKey: Self.key)
    }
}

/// Notes a Parakeet load that ended with Hex itself. iOS ends an app that passes its memory
/// limit while Core ML compiles the model (an iPhone 11 sat at "50%", then closed), and a
/// launch that loaded it again at once ended the same way every time. After that, Hex waits
/// for a tap on Set up instead of loading it at launch. In the app's own defaults.
nonisolated struct ParakeetLoadGuard {
    private static let loadingKey = "parakeet.loadStartedAt"
    private static let interruptedKey = "parakeet.loadInterrupted"

    static let message = String(localized: "Hex closed while loading the speech model, likely short of memory. Close other apps, then tap Set up to try again.")

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    /// Once per launch: a load still marked as running belonged to a process that ended.
    func noteLaunch() {
        guard defaults.object(forKey: Self.loadingKey) != nil else { return }
        defaults.removeObject(forKey: Self.loadingKey)
        defaults.set(true, forKey: Self.interruptedKey)
    }

    var lastLoadWasInterrupted: Bool {
        defaults.bool(forKey: Self.interruptedKey)
    }

    func begin() {
        defaults.set(Date().timeIntervalSince1970, forKey: Self.loadingKey)
    }

    /// A load that returned, loaded or not; only a loaded one clears the interruption.
    func end(loaded: Bool) {
        defaults.removeObject(forKey: Self.loadingKey)
        if loaded {
            defaults.removeObject(forKey: Self.interruptedKey)
        }
    }
}

extension ParakeetTranscriber.TranscriberError: NoSpeechError {
    var meansNoSpeech: Bool { self == .emptyTranscript }
}
