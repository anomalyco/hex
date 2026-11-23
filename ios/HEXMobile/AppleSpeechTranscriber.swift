import AVFAudio
import Foundation
import Speech

@available(iOS 26.0, *)
actor AppleSpeechAssetReservations {
    static let shared = AppleSpeechAssetReservations()

    private enum State {
        case acquiring(Int, Task<Void, Error>, Int)
        /// The generation of the acquisition this counts, and how many callers hold it.
        case reserved(Int, Int)
        case releasing(Int, Task<Void, Never>)
    }

    private let reserver: @Sendable (Locale) async -> Void
    private let releaser: @Sendable (Locale) async -> Void
    private var generation = 0
    private var states: [String: State] = [:]

    /// `reserver` and `releaser` are Apple's calls, swapped out in tests.
    init(
        reserver: @escaping @Sendable (Locale) async -> Void = { await AppleSpeechAssets.reserve($0) },
        releaser: @escaping @Sendable (Locale) async -> Void = { _ = await AssetInventory.release(reservedLocale: $0) }
    ) {
        self.reserver = reserver
        self.releaser = releaser
    }

    /// Reserves `locale`'s assets, joining a call already running for it. A call that never
    /// answers is forgotten after `limit`, so the next preparation asks Apple again instead of
    /// joining it forever; reservations are best effort, so the caller may simply skip it.
    func acquire(_ locale: Locale, limit: Duration) async throws {
        let key = locale.identifier(.bcp47)
        while true {
            switch states[key] {
            case nil:
                generation += 1
                let currentGeneration = generation
                let acquisition = Task<Void, Error> {
                    await reserver(locale)
                }
                states[key] = .acquiring(currentGeneration, acquisition, 1)
                if try await finishAcquisition(
                    acquisition,
                    generation: currentGeneration,
                    key: key,
                    limit: limit
                ) {
                    return
                }
            case .reserved(let reservedGeneration, let references):
                states[key] = .reserved(reservedGeneration, references + 1)
                return
            case .acquiring(let currentGeneration, let acquisition, let references):
                states[key] = .acquiring(currentGeneration, acquisition, references + 1)
                if try await finishAcquisition(
                    acquisition,
                    generation: currentGeneration,
                    key: key,
                    limit: limit
                ) {
                    return
                }
            case .releasing(let currentGeneration, let release):
                // A release that never answers must not hold the next reservation: clear it
                // however the wait ends, and only then let a cancellation through. The release
                // itself can't be cancelled: if it answers after a fresh reservation starts,
                // Apple may drop the asset just reserved again. Reservations are best effort,
                // and the install still runs without one.
                let cancelled: Bool
                do {
                    try await SpeechModelDeadline.run(limit) { await release.value }
                    cancelled = false
                } catch is CancellationError {
                    cancelled = true
                } catch {
                    cancelled = false
                }
                if case .releasing(let generation, _) = states[key],
                   generation == currentGeneration {
                    states[key] = nil
                }
                if cancelled { throw CancellationError() }
            }
        }
    }

    /// True when this call's success was counted. False when another caller cleared the call
    /// first (a timeout), or a newer one replaced it: the caller asks again rather than
    /// believing in a reservation the actor does not count.
    private func finishAcquisition(
        _ acquisition: Task<Void, Error>,
        generation currentGeneration: Int,
        key: String,
        limit: Duration
    ) async throws -> Bool {
        do {
            // The call must answer: one that never does would otherwise be joined by every
            // later preparation.
            try await SpeechModelDeadline.run(limit) {
                try await acquisition.value
            }
            if case .acquiring(let generation, _, let references) = states[key],
               generation == currentGeneration {
                states[key] = .reserved(generation, references)
                return true
            }
            if case .reserved(let generation, _) = states[key],
               generation == currentGeneration {
                // The first caller to resume counted every reference on this call, including
                // this one, when it joined.
                return true
            }
            return false
        } catch {
            // Any one caller's timeout or cancellation clears and cancels the call for every
            // other joiner too; each of them fails on its own wait and tries again.
            if case .acquiring(let generation, _, _) = states[key],
               generation == currentGeneration {
                states[key] = nil
                acquisition.cancel()
            }
            throw error
        }
    }

    func release(_ locale: Locale) async {
        let key = locale.identifier(.bcp47)
        guard case .reserved(let reservedGeneration, let references) = states[key] else { return }
        if references > 1 {
            states[key] = .reserved(reservedGeneration, references - 1)
        } else {
            generation += 1
            let currentGeneration = generation
            let release = Task {
                await releaser(locale)
            }
            states[key] = .releasing(currentGeneration, release)
            await release.value
            if case .releasing(let generation, _) = states[key],
               generation == currentGeneration {
                states[key] = nil
            }
        }
    }

    #if DEBUG
    /// What the tests need: how many callers are joined on a reservation, or that a release is
    /// pending before they wait on it.
    enum DebugState: Equatable {
        case acquiring(Int)
        case reserved(Int)
        case releasing
    }

    func debugState(_ locale: Locale) -> DebugState? {
        let state = states[locale.identifier(.bcp47)]
        if case .acquiring(_, _, let references) = state { return .acquiring(references) }
        if case .reserved(_, let references) = state { return .reserved(references) }
        if case .releasing = state { return .releasing }
        return nil
    }
    #endif
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

    nonisolated let engine: DictationEngine = .appleSpeech

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
    /// on an iPhone without `SpeechTranscriber` stays on Parakeet where it fits; every other
    /// language `DictationTranscriber` has beats English-only Parakeet. The iPhones without
    /// `SpeechTranscriber` (A13: iPhone 11, SE 2nd generation) have 3–4 GB, where loading
    /// Parakeet ended Hex at "50%" and again at every launch, so they dictate English with
    /// `DictationTranscriber`. An iPhone that has `SpeechTranscriber` keeps English on Apple
    /// Speech even on `DictationTranscriber` (its `SpeechTranscriber` model set aside after
    /// failed installs): Parakeet would need a download for something Apple already hears
    /// (TestFlight build 126, iPhone 15 Pro Max, iOS 26.6). Only once neither Apple module's
    /// English would install, twice in a row (`appleSetAside`), is Parakeet the way left.
    nonisolated static func shouldUse(
        _ language: SpeechLanguage,
        parakeetFits: Bool = ParakeetTranscriber.fitsInMemory,
        hasSpeechTranscriber: Bool = SpeechTranscriber.isAvailable,
        appleSetAside: Bool? = nil
    ) -> Bool {
        if language.module == .speech || !SpeechLanguagePicker.isEnglish(language.identifier) || !parakeetFits {
            return true
        }
        let setAside = appleSetAside ?? LanguageSettings().isUnavailable(language.identifier)
        return hasSpeechTranscriber && !setAside
    }

    /// How long one Apple Speech step may wait before Hex gives up on it: the speech daemon can
    /// stop answering, and no step may hold the screen on "Checking the speech model".
    static let stepStallLimit: Duration = .seconds(60)

    func prepare(
        progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void
    ) async throws {
        guard !isPrepared else { return }
        let locale = language.locale

        // Best effort: a reservation Hex can't get (over Apple's limit) doesn't stop the download.
        // One that never answers is forgotten by `acquire` and skipped, so the next preparation
        // asks again instead of joining it.
        do {
            try await AppleSpeechAssetReservations.shared.acquire(locale, limit: Self.stepStallLimit)
            reservedLocale = locale
        } catch is CancellationError {
            throw CancellationError()
        } catch {
            // Without a reservation the install still runs, or fails with its own reason.
        }
        do {
            let detail = String(localized: "Downloading the speech model")
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
                    detail: String(localized: "Loading the speech model")
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
                    detail: String(localized: "Ready")
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

        // Experiments › Two-language dictation: the same recording read by a second installed
        // language too, merged word by word. Off, or without a usable second language, the
        // single-language path below runs exactly as before.
        let twoLanguage = TwoLanguageDictationSettings()
        if twoLanguage.isEnabled,
           let pair = await AppleSpeechTwoLanguage.usablePair(for: language),
           let merged = await AppleSpeechTwoLanguage.transcribe(
               audioURL,
               pair: pair,
               vocabulary: vocabulary,
               lingeringModels: true
           ) {
            guard !merged.text.isEmpty else { throw TranscriberError.emptyTranscript }
            return merged.text
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
        // Best effort, and watched: a release that never answers must not keep the failure
        // from reaching the screen.
        try? await SpeechModelDeadline.run(Self.stepStallLimit) {
            await AppleSpeechAssetReservations.shared.release(reservedLocale)
        }
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
