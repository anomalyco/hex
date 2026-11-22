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
        case notEnoughMemory

        var errorDescription: String? {
            switch self {
            case .notPrepared:
                String(localized: "The speech model isn't ready.")
            case .emptyTranscript:
                String(localized: "No speech heard.")
            case .downloadFailed:
                String(localized: "The speech model didn't download. Check your connection and try again.")
            case .notEnoughMemory:
                String(localized: "Not enough memory for the speech model.")
            }
        }
    }

    /// Whether this iPhone has the memory to load Parakeet V2: 6 GB iPhones report about
    /// 5.5 GB, 4 GB ones (iPhone 11, 12, 13) under 4 GB.
    nonisolated static var fitsInMemory: Bool {
        ProcessInfo.processInfo.physicalMemory >= 5 << 30
    }

    /// What loading Parakeet V2 takes in Hex: its encoder is 425 MB on disk, 6-bit palettized,
    /// and 1.19 GB of 16-bit weights once Core ML compiles it (the simulator's compiled program),
    /// which an iPhone's Neural Engine holds unpacked before the A17; then the other three
    /// models and a first transcription (70 MB more in the simulator).
    nonisolated static let loadMemory: UInt64 = 1_400 << 20

    /// Whether `available` (`os_proc_available_memory`) leaves room to load it. 0 means iOS
    /// didn't say (the simulator).
    nonisolated static func hasRoomToLoad(available: UInt64) -> Bool {
        available == 0 || available >= loadMemory
    }

    /// How long the download may sit at one fraction before Hex stops waiting. Core ML's
    /// compile, after it, reports nothing and can't be cancelled, so it isn't watched.
    static let downloadStallLimit: Duration = .seconds(90)

    private var manager: AsrManager?

    nonisolated let displayName = "Parakeet V2"
    let languageName = "English"

    /// In the App Group (`SpeechModelStore`), so Experiments › Speech model in keyboard can load
    /// it too; FluidAudio finds it by its repo's folder name.
    nonisolated static var cacheDirectory: URL {
        SpeechModelStore.root.appendingPathComponent(Repo.parakeetV2.folderName, isDirectory: true)
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
        // Too little left to load it: Hex would end, so it takes another engine instead.
        guard Self.hasRoomToLoad(available: UInt64(max(0, os_proc_available_memory()))) else {
            loads.setAside()
            throw TranscriberError.notEnoughMemory
        }
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
        // Loaded, so every file is there (a download from before the marker, too).
        SpeechModelStore.markComplete(Self.cacheDirectory)
        progress.update { $0.finish() }
    }

    /// FluidAudio's repo download alone, which resumes each file from its `.partial`.
    private func download(_ progress: ParakeetProgressFeed) async throws {
        try await Self.downloadModel { fraction in
            progress.update { $0.download(fraction) }
        }
    }

    /// The download, for the app's engine and for Experiments › Speech model in keyboard;
    /// `progress` gets the share downloaded. Marks the folder complete once every file is there.
    nonisolated static func downloadModel(progress: @escaping @Sendable (Double) -> Void) async throws {
        let fraction = ProgressReading()
        SpeechModelStore.prepareRoot()
        let directory = cacheDirectory.deletingLastPathComponent()
        do {
            try await StallWatch.run(
                limit: downloadStallLimit,
                progress: { fraction.value },
                operation: {
                    // FluidAudio's download reports 0–50%, leaving the rest for a compile.
                    try await ModelHub.download(.parakeetV2, to: directory, progressHandler: { event in
                        fraction.value = event.fractionCompleted
                        progress(min(1, event.fractionCompleted * 2))
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
            SpeechModelStore.markComplete(cacheDirectory)
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
    /// What the bar says while the download runs.
    let downloadDetail: String
    private(set) var downloaded = 0.0
    private(set) var loadStartedAt: TimeInterval?
    private(set) var modelsLoaded = 0
    /// The load's fraction at the last model Core ML finished, and when that was.
    private var loadBase = 0.0
    private var segmentStartedAt: TimeInterval = 0
    private(set) var isFinished = false

    init(
        downloads: Bool,
        compiledBefore: Bool,
        downloadDetail: String = String(localized: "Downloading the speech model")
    ) {
        self.downloads = downloads
        self.compiledBefore = compiledBefore
        self.downloadDetail = downloadDetail
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
                    detail: downloadDetail
                )
            }
            return ModelPreparationProgress(fractionCompleted: 0, detail: String(localized: "Checking the speech model"))
        }
        let fraction = downloadShare + (1 - downloadShare) * loadFraction(at: now)
        let detail: String
        if compiledBefore {
            detail = String(localized: "Loading the speech model")
        } else if now - loadStartedAt < expectedLoadSeconds * 2 {
            detail = String(localized: "Optimizing for this iPhone, about a minute")
        } else {
            detail = String(localized: "Still optimizing. The first time can take a few minutes.")
        }
        return ModelPreparationProgress(fractionCompleted: min(0.99, fraction), detail: detail)
    }
}

/// Feeds `ParakeetPreparationProgress` to the handler, at most ten times a second (the
/// download alone reports thousands of times) and whenever its words change.
nonisolated final class ParakeetProgressFeed: @unchecked Sendable {
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
    static let v2Key = "parakeet.compiledForSystem"
    private let key: String
    private let defaults: UserDefaults
    private let system: String

    init(
        key: String = v2Key,
        defaults: UserDefaults = .standard,
        system: String = ProcessInfo.processInfo.operatingSystemVersionString
    ) {
        self.key = key
        self.defaults = defaults
        self.system = system
    }

    var isCurrent: Bool {
        defaults.string(forKey: key) == system
    }

    func markCurrent() {
        defaults.set(system, forKey: key)
    }
}

/// Notes a Parakeet load that ended with Hex itself. iOS ends an app that passes its memory
/// limit while Core ML compiles the model (an iPhone 11 sat at "50%", then closed), and a
/// launch that loaded it again at once ended the same way every time. After that, Hex takes
/// another engine (`SpeechEngineChoice`): Apple's, where it has the language, else it waits
/// for a tap on Set up instead of loading Parakeet at launch. In the app's own defaults.
///
/// Hex ending mid-load is no evidence by itself: iOS ends a backgrounded app, people swipe it
/// away, an update installs. So a load is set aside only after two in a row ended Hex (or a
/// load found too little memory), and the decision lasts a day and only for this build of
/// Hex: then it tries again by itself.
nonisolated struct ParakeetLoadGuard {
    static let message = String(localized: "Hex closed while loading the speech model. Close other apps, then tap Set up.")
    /// How long a set-aside load stays set aside.
    static let setAsideDuration: TimeInterval = 24 * 60 * 60

    private let loadingKey: String
    private let interruptionsKey: String
    private let setAsideAtKey: String
    private let setAsideBuildKey: String
    /// The lasting flag older builds kept; read only to forget it.
    private let legacyInterruptedKey: String
    private let defaults: UserDefaults
    private let build: String
    private let now: () -> Date

    /// `model` names the keys: "parakeet" for V2, as before, "parakeetUltra" for Ultra.
    init(
        model: String = "parakeet",
        defaults: UserDefaults = .standard,
        build: String = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "",
        now: @escaping () -> Date = Date.init
    ) {
        loadingKey = "\(model).loadStartedAt"
        interruptionsKey = "\(model).loadInterruptions"
        setAsideAtKey = "\(model).setAsideAt"
        setAsideBuildKey = "\(model).setAsideBuild"
        legacyInterruptedKey = "\(model).loadInterrupted"
        self.defaults = defaults
        self.build = build
        self.now = now
    }

    /// Once per launch: a load still marked as running belonged to a process that ended.
    /// The second in a row sets Parakeet aside.
    func noteLaunch() {
        if defaults.object(forKey: legacyInterruptedKey) != nil {
            // An older build's lasting flag: the load gets another try.
            defaults.removeObject(forKey: legacyInterruptedKey)
        }
        guard defaults.object(forKey: loadingKey) != nil else { return }
        defaults.removeObject(forKey: loadingKey)
        let interruptions = defaults.integer(forKey: interruptionsKey) + 1
        defaults.set(interruptions, forKey: interruptionsKey)
        if interruptions >= 2 {
            setAside()
        }
    }

    /// Set aside now: two loads in a row ended Hex, or one found too little memory, within
    /// the last day and on this build.
    var lastLoadWasInterrupted: Bool {
        guard let at = defaults.object(forKey: setAsideAtKey) as? Double,
              defaults.string(forKey: setAsideBuildKey) == build else { return false }
        return now().timeIntervalSince1970 - at < Self.setAsideDuration
    }

    func begin() {
        defaults.set(now().timeIntervalSince1970, forKey: loadingKey)
    }

    /// Too little memory left to load Parakeet: set aside for a day, so Hex picks another
    /// engine (`SpeechEngineChoice`) instead of loading it.
    func setAside() {
        defaults.set(now().timeIntervalSince1970, forKey: setAsideAtKey)
        defaults.set(build, forKey: setAsideBuildKey)
    }

    /// A load that returned, loaded or not; only a loaded one clears what came before.
    func end(loaded: Bool) {
        defaults.removeObject(forKey: loadingKey)
        if loaded {
            forget()
        }
    }

    /// Forgets interrupted loads, for a person who asks to try again.
    func forget() {
        for key in [loadingKey, interruptionsKey, setAsideAtKey, setAsideBuildKey, legacyInterruptedKey] {
            defaults.removeObject(forKey: key)
        }
    }
}

extension ParakeetTranscriber.TranscriberError: NoSpeechError {
    var meansNoSpeech: Bool { self == .emptyTranscript }
}
