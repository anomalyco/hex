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
                let acquisition = Task<Void, Error> {
                    await AppleSpeechAssets.reserve(locale)
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
    enum TranscriberError: HexDescribedError {
        case emptyTranscript
        case unavailable

        var errorDescription: String? {
            switch self {
            case .emptyTranscript:
                String(localized: "No speech heard.")
            case .unavailable:
                String(localized: "Apple's speech recognition can't dictate this language on this iPhone.")
            }
        }
    }

    nonisolated let displayName = "Apple Speech"

    /// The language it dictates; the same language on `DictationTranscriber` once
    /// `SpeechTranscriber`'s model wouldn't install.
    private var language: SpeechLanguage

    private var preparedAnalyzer: SpeechAnalyzer?
    private var preparedModule: AppleSpeechModule?
    private var isPrepared = false
    private var reservedLocale: Locale?

    var languageName: String {
        Self.displayName(for: language.locale)
    }

    var languageIdentifier: String {
        language.identifier
    }

    /// Never asks for Speech Recognition access: that permission is for `SFSpeechRecognizer`,
    /// which Hex uses only before iOS 26 (`OnDeviceSpeechTranscriber`). `SpeechAnalyzer` runs
    /// on the iPhone without it, in the app as in the keyboard, for `SpeechTranscriber` and
    /// `DictationTranscriber` alike, so a person who turned it off still dictates (TestFlight
    /// build 51, iOS 27.0.1).
    init(language: SpeechLanguage) {
        self.language = language
    }

    /// Settings › Dictation language, else the first preferred language either module has.
    static func preferredLanguage() async -> SpeechLanguage? {
        await AppleSpeechLanguages.resolve()
    }

    /// Whether Hex dictates this language with Apple Speech rather than Parakeet. English
    /// without `SpeechTranscriber` stays on Parakeet where it fits; every other language
    /// `DictationTranscriber` has beats English-only Parakeet. The iPhones without
    /// `SpeechTranscriber` (A13: iPhone 11, SE 2nd generation) have 3–4 GB, where loading
    /// Parakeet ended Hex at "50%" and again at every launch, so they dictate English with
    /// `DictationTranscriber`.
    nonisolated static func shouldUse(
        _ language: SpeechLanguage,
        parakeetFits: Bool = ParakeetTranscriber.fitsInMemory
    ) -> Bool {
        language.module == .speech || !SpeechLanguagePicker.isEnglish(language.identifier) || !parakeetFits
    }

    func prepare(
        progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void
    ) async throws {
        guard !isPrepared else { return }
        let locale = language.locale

        // Best effort: a reservation Hex can't get (over Apple's limit) doesn't stop the download.
        if (try? await AppleSpeechAssetReservations.shared.acquire(locale)) != nil {
            reservedLocale = locale
        }
        do {
            let detail = "Downloading Apple \(Self.displayName(for: locale)) speech"
            // `SpeechTranscriber`'s model, else the same language's on `DictationTranscriber`;
            // a download that stops moving ends instead of holding the progress bar forever.
            language = try await AppleSpeechAssets.install(language, progress: { fraction in
                progressHandler(
                    ModelPreparationProgress(fractionCompleted: min(0.85, fraction * 0.85), detail: detail)
                )
            })
            let speech = AppleSpeechModule(language)
            progressHandler(
                ModelPreparationProgress(
                    fractionCompleted: 0.9,
                    detail: "Preheating Apple Speech"
                )
            )

            let analyzer = makeAnalyzer(module: speech)
            try await analyzer.prepareToAnalyze(in: nil)

            isPrepared = true
            preparedModule = speech
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
        try await transcribe(audioURL, vocabulary: [])
    }

    func transcribe(_ audioURL: URL, vocabulary: [String]) async throws -> String {
        guard isPrepared else {
            throw TranscriberError.unavailable
        }

        let speech: AppleSpeechModule
        let analyzer: SpeechAnalyzer
        if let preparedModule, let preparedAnalyzer {
            speech = preparedModule
            analyzer = preparedAnalyzer
            self.preparedModule = nil
            self.preparedAnalyzer = nil
        } else {
            speech = AppleSpeechModule(language)
            analyzer = makeAnalyzer(module: speech)
        }

        let audioFile = try AVAudioFile(forReading: audioURL)

        // Each analyzer handles one dictation and is then discarded, so a context
        // cannot leak into a later capture with an empty list.
        if !vocabulary.isEmpty {
            let context = AnalysisContext()
            context.contextualStrings[.general] = vocabulary
            try? await analyzer.setContext(context)
        }

        try await analyzer.start(inputAudioFile: audioFile, finishAfterFile: true)

        let text = try await speech.finalText()
        guard !text.isEmpty else {
            throw TranscriberError.emptyTranscript
        }
        return text
    }

    private func makeAnalyzer(module: AppleSpeechModule) -> SpeechAnalyzer {
        SpeechAnalyzer(
            modules: [module.module],
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

    nonisolated private static func displayName(for locale: Locale) -> String {
        Locale.current.localizedString(forIdentifier: locale.identifier)
            ?? locale.localizedString(forIdentifier: locale.identifier)
            ?? locale.identifier
    }
}

@available(iOS 26.0, *)
extension AppleSpeechTranscriber.TranscriberError: NoSpeechError {
    var meansNoSpeech: Bool { self == .emptyTranscript }
}
