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
    /// compile, the second half, reports nothing and can't be cancelled, so it isn't watched.
    static let downloadStallLimit: Duration = .seconds(90)

    private var manager: AsrManager?

    nonisolated let displayName = "Parakeet V2"
    let languageName = "English"

    nonisolated static var hasCachedModel: Bool {
        AsrModels.modelsExist(
            at: AsrModels.defaultCacheDirectory(for: .v2),
            version: .v2
        )
    }

    func prepare(
        progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void
    ) async throws {
        guard manager == nil else { return }

        let loads = ParakeetLoadGuard()
        loads.begin()
        var loaded = false
        defer { loads.end(loaded: loaded) }
        let fraction = ProgressReading()
        let models: AsrModels
        do {
            // FluidAudio downloads in 0–50% and compiles in 50–100%.
            models = try await StallWatch.run(
                limit: Self.downloadStallLimit,
                progress: { fraction.value < 0.5 ? fraction.value : nil },
                operation: {
                    try await AsrModels.downloadAndLoad(
                        version: .v2,
                        progressHandler: { progress in
                            fraction.value = progress.fractionCompleted
                            progressHandler(
                                ModelPreparationProgress(
                                    fractionCompleted: progress.fractionCompleted,
                                    detail: Self.detail(for: progress.phase)
                                )
                            )
                        }
                    )
                }
            )
        } catch let error as DownloadError {
            // FluidAudio's descriptions carry Hugging Face paths and HTML; say it plainly.
            switch error {
            case .invalidResponse, .rateLimited, .downloadFailed, .htmlErrorResponse:
                throw TranscriberError.downloadFailed
            default:
                throw error
            }
        }
        let manager = AsrManager(config: .default)
        try await manager.loadModels(models)
        self.manager = manager
        loaded = true
    }

    private static func detail(for phase: DownloadPhase) -> String {
        switch phase {
        case .listing:
            "Checking model files"
        case .downloading(let completedFiles, let totalFiles):
            "Downloading file \(min(completedFiles + 1, totalFiles)) of \(totalFiles)"
        case .compiling(let modelName):
            "Compiling \(modelName)"
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
