import FluidAudio
import Foundation
import os

/// Experiments › Multilingual model (Parakeet): Parakeet Ultra (`ParakeetUltraModel`) through
/// FluidAudio's v3 decoder, in Hex's own process. It hears its 25 languages without being told
/// which, mixed within a sentence, so `languageIdentifier` only names the dictation language
/// for the steps after it (formatting, lists, cleanup).
actor ParakeetUltraTranscriber: LocalTranscribing {
    enum TranscriberError: HexDescribedError {
        case notPrepared
        case emptyTranscript
        case downloadFailed
        case notEnoughMemory

        var errorDescription: String? {
            switch self {
            case .notPrepared:
                String(localized: "Parakeet Ultra is not ready.")
            case .emptyTranscript:
                String(localized: "Parakeet did not detect any speech.")
            case .downloadFailed:
                String(localized: "Parakeet Ultra didn't download. Check your connection and try again.")
            case .notEnoughMemory:
                String(localized: "Parakeet Ultra needs more memory than Hex has here.")
            }
        }
    }

    static let name = "Parakeet Ultra"
    /// `ParakeetLoadGuard`'s and `ParakeetCompileRecord`'s keys, apart from V2's.
    static let loadGuardModel = "parakeetUltra"
    static let compileRecordKey = "parakeetUltra.compiledForSystem"

    /// What loading it takes in Hex, as `ParakeetTranscriber.loadMemory` for V2: the encoder
    /// is 594 MB of int8 weights, which the Neural Engine may hold as 16-bit (1.2 GB) before
    /// the A17, then the other three models (a joint four times V2's) and a first transcription.
    nonisolated static let loadMemory: UInt64 = 1_500 << 20

    /// Whether `available` (`os_proc_available_memory`) leaves room to load it. 0 means iOS
    /// didn't say (the simulator).
    nonisolated static func hasRoomToLoad(available: UInt64) -> Bool {
        available == 0 || available >= loadMemory
    }

    nonisolated let displayName = ParakeetUltraTranscriber.name
    let languageIdentifier: String
    private var manager: AsrManager?

    init(language: String) {
        languageIdentifier = language
    }

    var languageName: String {
        SpeechLanguagePicker.displayName(for: languageIdentifier)
    }

    /// The language the experiment dictates in now, or nil for the engine Hex uses without it:
    /// off, an iPhone without the memory, a language it doesn't know, a load that ended Hex
    /// (`ParakeetLoadGuard`, until the experiment is turned off and on), or less memory left
    /// than loading takes (`os_proc_available_memory`, 0 where iOS sets no limit).
    nonisolated static func chosenLanguage(
        language: String? = MultilingualParakeetSettings().language(),
        availableMemory: UInt64 = UInt64(max(0, os_proc_available_memory())),
        setAside: Bool = ParakeetLoadGuard(model: loadGuardModel).lastLoadWasInterrupted
    ) -> String? {
        guard let language, !setAside, hasRoomToLoad(available: availableMemory) else { return nil }
        return language
    }

    /// "632.2 MB", kept on one line.
    nonisolated static var downloadSize: String {
        ByteCountFormatter.string(fromByteCount: ParakeetUltraModel.totalBytes, countStyle: .file)
            .replacingOccurrences(of: " ", with: "\u{00A0}")
    }

    /// What the bar says while it downloads: the size, and that Wi-Fi is better for it.
    nonisolated static var downloadDetail: String {
        String(localized: "Downloading \(downloadSize) · Wi-Fi recommended")
    }

    func prepare(
        progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void
    ) async throws {
        guard manager == nil else { return }

        let compiled = ParakeetCompileRecord(key: Self.compileRecordKey)
        let downloads = !ParakeetUltraModel.isDownloaded()
        let progress = ParakeetProgressFeed(
            ParakeetPreparationProgress(
                downloads: downloads,
                compiledBefore: compiled.isCurrent,
                downloadDetail: Self.downloadDetail
            ),
            emit: progressHandler
        )
        let download: Task<Void, Error>? = downloads ? Task { try await Self.download(progress) } : nil
        let ticker = Task {
            while !Task.isCancelled {
                progress.tick()
                // Turned off mid-download: stop rather than finish a download it then deletes.
                if download != nil, !MultilingualParakeetSettings().isEnabled {
                    download?.cancel()
                }
                try? await Task.sleep(for: .milliseconds(250))
            }
        }
        defer { ticker.cancel() }

        if let download {
            try await withTaskCancellationHandler {
                try await download.value
            } onCancel: {
                download.cancel()
            }
        }

        // As V2: only the load, where iOS can end Hex short of memory, counts for the guard,
        // and too little left to load it sets it aside for the engine Hex uses without it.
        let loads = ParakeetLoadGuard(model: Self.loadGuardModel)
        guard Self.hasRoomToLoad(available: UInt64(max(0, os_proc_available_memory()))) else {
            loads.setAside()
            throw TranscriberError.notEnoughMemory
        }
        loads.begin()
        var loaded = false
        defer { loads.end(loaded: loaded) }
        progress.update { $0.startLoading(at: Date.now.timeIntervalSinceReferenceDate) }
        let models = try ParakeetUltraModel.load {
            let now = Date.now.timeIntervalSinceReferenceDate
            progress.update { $0.finishModel(at: now) }
        }
        let manager = AsrManager(config: .default)
        try await manager.loadModels(models)
        self.manager = manager
        loaded = true
        compiled.markCurrent()
        progress.update { $0.finish() }
    }

    /// Resumes each file from its `.partial`; a download that stops moving for
    /// `ParakeetTranscriber.downloadStallLimit` ends, as V2's does.
    private static func download(_ progress: ParakeetProgressFeed) async throws {
        let fraction = ProgressReading()
        do {
            try await StallWatch.run(
                limit: ParakeetTranscriber.downloadStallLimit,
                progress: { fraction.value },
                operation: {
                    try await ParakeetUltraModel.download { value in
                        fraction.value = value
                        progress.update { $0.download(value) }
                    }
                }
            )
        } catch is ParakeetUltraModel.DownloadError {
            throw TranscriberError.downloadFailed
        }
    }

    func transcribe(_ audioURL: URL) async throws -> String {
        guard let manager else {
            throw TranscriberError.notPrepared
        }
        let decoderLayers = await manager.decoderLayerCount
        var decoderState = TdtDecoderState.make(decoderLayers: decoderLayers)
        let result = try await manager.transcribe(audioURL, decoderState: &decoderState)
        let text = result.text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            throw TranscriberError.emptyTranscript
        }
        return text
    }
}

extension ParakeetUltraTranscriber.TranscriberError: NoSpeechError {
    var meansNoSpeech: Bool { self == .emptyTranscript }
}
