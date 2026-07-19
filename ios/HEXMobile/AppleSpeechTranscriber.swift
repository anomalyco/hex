import AVFAudio
import Foundation
import Speech

@available(iOS 26.0, *)
private actor AppleSpeechAssetReservations {
    static let shared = AppleSpeechAssetReservations()

    private enum State {
        case acquiring(Int, Task<Void, Error>, Int)
        case reserved(Int)
        case releasing(Int, Task<Void, Never>)
    }

    private var generation = 0
    private var states: [String: State] = [:]

    func acquire(_ locale: Locale) async throws {
        let key = locale.identifier(.bcp47)
        while true {
            switch states[key] {
            case nil:
                generation += 1
                let currentGeneration = generation
                let acquisition = Task {
                    _ = try await AssetInventory.reserve(locale: locale)
                }
                states[key] = .acquiring(currentGeneration, acquisition, 1)
                try await finishAcquisition(acquisition, generation: currentGeneration, key: key)
                return
            case .reserved(let references):
                states[key] = .reserved(references + 1)
                return
            case .acquiring(let currentGeneration, let acquisition, let references):
                states[key] = .acquiring(currentGeneration, acquisition, references + 1)
                try await finishAcquisition(acquisition, generation: currentGeneration, key: key)
                return
            case .releasing(let currentGeneration, let release):
                await release.value
                if case .releasing(let generation, _) = states[key],
                   generation == currentGeneration {
                    states[key] = nil
                }
            }
        }
    }

    private func finishAcquisition(
        _ acquisition: Task<Void, Error>,
        generation currentGeneration: Int,
        key: String
    ) async throws {
        do {
            try await acquisition.value
            if case .acquiring(let generation, _, let references) = states[key],
               generation == currentGeneration {
                states[key] = .reserved(references)
            }
        } catch {
            if case .acquiring(let generation, _, _) = states[key],
               generation == currentGeneration {
                states[key] = nil
            }
            throw error
        }
    }

    func release(_ locale: Locale) async {
        let key = locale.identifier(.bcp47)
        guard case .reserved(let references) = states[key] else { return }
        if references > 1 {
            states[key] = .reserved(references - 1)
        } else {
            generation += 1
            let currentGeneration = generation
            let release = Task {
                _ = await AssetInventory.release(reservedLocale: locale)
            }
            states[key] = .releasing(currentGeneration, release)
            await release.value
            if case .releasing(let generation, _) = states[key],
               generation == currentGeneration {
                states[key] = nil
            }
        }
    }
}

@available(iOS 26.0, *)
actor AppleSpeechTranscriber: LocalTranscribing {
    enum TranscriberError: LocalizedError {
        case authorizationDenied
        case emptyTranscript
        case unavailable

        var errorDescription: String? {
            switch self {
            case .authorizationDenied:
                "Speech Recognition access is required to use Apple Speech."
            case .emptyTranscript:
                "Apple Speech did not detect any speech."
            case .unavailable:
                "Apple Speech is not available for your preferred languages on this iPhone."
            }
        }
    }

    nonisolated let displayName = "Apple Speech"

    private let requestedLocale: Locale

    private var preparedAnalyzer: SpeechAnalyzer?
    private var preparedTranscriber: SpeechTranscriber?
    private var locale: Locale?
    private var reservedLocale: Locale?

    var languageName: String {
        Self.displayName(for: locale ?? requestedLocale)
    }

    init(locale: Locale) {
        requestedLocale = locale
    }

    nonisolated static var isAvailable: Bool {
        SpeechTranscriber.isAvailable
    }

    static func preferredLocale() async -> Locale? {
        var identifiers = Locale.preferredLanguages
        identifiers.append(Locale.current.identifier)
        identifiers.append("en-US")

        var checked = Set<String>()
        for identifier in identifiers {
            let candidate = Locale(identifier: identifier)
            let normalized = candidate.identifier(.bcp47)
            guard checked.insert(normalized).inserted else { continue }
            if let supported = await SpeechTranscriber.supportedLocale(
                equivalentTo: candidate
            ) {
                return supported
            }
        }
        return nil
    }

    func prepare(
        progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void
    ) async throws {
        guard locale == nil else { return }
        guard await Self.requestAuthorization() == .authorized else {
            throw TranscriberError.authorizationDenied
        }
        guard let locale = await SpeechTranscriber.supportedLocale(
            equivalentTo: requestedLocale
        ) else {
            throw TranscriberError.unavailable
        }

        try await AppleSpeechAssetReservations.shared.acquire(locale)
        reservedLocale = locale
        do {
            let transcriber = SpeechTranscriber(locale: locale, preset: .transcription)
            let status = await AssetInventory.status(forModules: [transcriber])

            if status != .installed,
               let request = try await AssetInventory.assetInstallationRequest(
                   supporting: [transcriber]
               ) {
                let progress = request.progress
                let monitor = Task {
                    while !Task.isCancelled && !progress.isFinished {
                        progressHandler(
                            ModelPreparationProgress(
                                fractionCompleted: min(0.85, progress.fractionCompleted * 0.85),
                                detail: "Downloading Apple \(Self.displayName(for: locale)) speech"
                            )
                        )
                        try? await Task.sleep(for: .milliseconds(100))
                    }
                }
                defer { monitor.cancel() }
                try await request.downloadAndInstall()
            }
            guard await AssetInventory.status(forModules: [transcriber]) == .installed else {
                throw TranscriberError.unavailable
            }
            progressHandler(
                ModelPreparationProgress(
                    fractionCompleted: 0.9,
                    detail: "Preheating Apple Speech"
                )
            )

            let analyzer = makeAnalyzer(transcriber: transcriber)
            try await analyzer.prepareToAnalyze(in: nil)

            self.locale = locale
            preparedTranscriber = transcriber
            preparedAnalyzer = analyzer
            progressHandler(
                ModelPreparationProgress(
                    fractionCompleted: 1,
                    detail: "Apple Speech ready"
                )
            )
        } catch {
            await releaseAssets()
            throw error
        }
    }

    func transcribe(_ audioURL: URL) async throws -> String {
        guard let locale else {
            throw TranscriberError.unavailable
        }

        let transcriber: SpeechTranscriber
        let analyzer: SpeechAnalyzer
        if let preparedTranscriber, let preparedAnalyzer {
            transcriber = preparedTranscriber
            analyzer = preparedAnalyzer
            self.preparedTranscriber = nil
            self.preparedAnalyzer = nil
        } else {
            transcriber = SpeechTranscriber(locale: locale, preset: .transcription)
            analyzer = makeAnalyzer(transcriber: transcriber)
        }

        let audioFile = try AVAudioFile(forReading: audioURL)
        try await analyzer.start(inputAudioFile: audioFile, finishAfterFile: true)

        var transcript = AttributedString()
        for try await result in transcriber.results where result.isFinal {
            transcript += result.text
        }

        let text = String(transcript.characters)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            throw TranscriberError.emptyTranscript
        }
        return text
    }

    private func makeAnalyzer(transcriber: SpeechTranscriber) -> SpeechAnalyzer {
        SpeechAnalyzer(
            modules: [transcriber],
            options: SpeechAnalyzer.Options(
                priority: .userInitiated,
                modelRetention: .lingering
            )
        )
    }

    deinit {
        guard let reservedLocale else { return }
        Task {
            await AppleSpeechAssetReservations.shared.release(reservedLocale)
        }
    }

    private func releaseAssets() async {
        guard let reservedLocale else { return }
        self.reservedLocale = nil
        await AppleSpeechAssetReservations.shared.release(reservedLocale)
    }

    private static func requestAuthorization() async -> SFSpeechRecognizerAuthorizationStatus {
        let status = SFSpeechRecognizer.authorizationStatus()
        guard status == .notDetermined else { return status }
        return await withCheckedContinuation { continuation in
            SFSpeechRecognizer.requestAuthorization { status in
                continuation.resume(returning: status)
            }
        }
    }

    nonisolated private static func displayName(for locale: Locale) -> String {
        Locale.current.localizedString(forIdentifier: locale.identifier)
            ?? locale.localizedString(forIdentifier: locale.identifier)
            ?? locale.identifier
    }
}
