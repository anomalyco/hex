import AVFAudio
import Foundation
import Speech

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
        guard preparedAnalyzer == nil else { return }
        guard await Self.requestAuthorization() == .authorized else {
            throw TranscriberError.authorizationDenied
        }
        guard let locale = await SpeechTranscriber.supportedLocale(
            equivalentTo: requestedLocale
        ) else {
            throw TranscriberError.unavailable
        }

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

        _ = try? await AssetInventory.reserve(locale: locale)
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
                modelRetention: .processLifetime
            )
        )
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
