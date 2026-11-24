import Foundation
import Speech

extension DictationController {
    // MARK: - Which engine dictates (`SpeechEngineChoice`)

    /// At launch. A launch after Hex ended while loading Parakeet doesn't load it again by
    /// itself, or it would end the same way at every launch: from iOS 26 Apple Speech takes
    /// over where it has the language, before it the on-device recognizer always does.
    func prepareAtLaunch(hasCachedModel: Bool) {
        let parakeetLoads = ParakeetLoadGuard()
        parakeetLoads.noteLaunch()
        let parakeetInterrupted = parakeetLoads.lastLoadWasInterrupted
        if prepareMultilingualParakeetAtLaunch() { return }
        guard #available(iOS 26.0, *) else {
            prepareBeforeAppleSpeechAtLaunch(hasCachedModel: hasCachedModel)
            return
        }
        if hasCachedModel && !parakeetInterrupted {
            engine = .parakeetV2
            prepareModel()
            return
        }
        let fits = SpeechEngineChoice.current.parakeetFitsBesideAppleSpeech
        Task { [weak self] in
            guard let self else { return }
            if await Self.appleSpeechLanguage(picked: AppleSpeechTranscriber.preferredLanguage(), parakeetFits: fits) != nil {
                self.engine = .appleSpeech
                self.prepareModel()
            } else {
                self.engine = .parakeetV2
                if hasCachedModel {
                    self.failModel(ParakeetLoadGuard.message)
                }
            }
        }
    }

    /// Settings › Dictation › Model: Multilingual at launch: true when Parakeet Ultra takes
    /// the dictation language and loads now, downloaded and compiled for this iOS (seconds).
    /// Otherwise Hex gets ready with the engine it uses without it, and the download or a
    /// first load runs beside it (`syncMultilingualParakeet`). With the experiment turned
    /// off, it only deletes a download turning it off left mid-way.
    private func prepareMultilingualParakeetAtLaunch() -> Bool {
        let loads = ParakeetLoadGuard(model: ParakeetUltraTranscriber.loadGuardModel)
        loads.noteLaunch()
        let settings = MultilingualParakeetSettings()
        guard settings.isEnabled else {
            // English picked: a download left part-way goes; a complete one stays for a switch back.
            if settings.isTurnedOff {
                ParakeetUltraModel.removeUnlessComplete()
            }
            return false
        }
        if loads.lastLoadWasInterrupted, settings.language() != nil {
            multilingualDownload.fail(ParakeetUltraTranscriber.setAsideMessage)
        }
        guard Self.multilingualStep(
            language: ParakeetUltraTranscriber.chosenLanguage(),
            downloaded: ParakeetUltraModel.isDownloaded(),
            compiled: ParakeetUltraTranscriber.isCompiled
        ) == .dictate else {
            // A first load waits for the engine Hex gets ready meanwhile (its end syncs).
            startMultilingualDownloadIfWanted()
            return false
        }
        engine = .parakeetUltra
        prepareModel()
        return true
    }

    /// What Hex does about Parakeet Ultra: nothing (off, a language it doesn't take, set
    /// aside), download it, load it beside the engine that dictates meanwhile (a first load
    /// compiles for a minute or more), or dictate with it.
    enum MultilingualStep: Equatable {
        case none
        case download
        case loadBeside
        case dictate
    }

    nonisolated static func multilingualStep(language: String?, downloaded: Bool, compiled: Bool) -> MultilingualStep {
        guard language != nil else { return .none }
        guard downloaded else { return .download }
        return compiled ? .dictate : .loadBeside
    }

    /// Starts Parakeet Ultra's download when the experiment wants it and it isn't on the
    /// iPhone, whatever engine is getting ready meanwhile.
    func startMultilingualDownloadIfWanted() {
        // Memory left only matters for a load, not a download.
        guard ParakeetUltraTranscriber.chosenLanguage(availableMemory: 0) != nil,
              !ParakeetUltraModel.isDownloaded() else { return }
        multilingualDownload.start()
    }

    /// Once an engine is ready, and once the download finishes: downloads Parakeet Ultra, or
    /// loads it beside the engine that dictates, when the experiment wants it and it isn't the
    /// one dictating already.
    func syncMultilingualParakeet() {
        guard MultilingualParakeetSettings().isEnabled else {
            multilingualDownload.settle()
            return
        }
        let language = ParakeetUltraTranscriber.chosenLanguage()
        let wanted = ParakeetUltraTranscriber.chosenLanguage(availableMemory: 0)
        switch Self.multilingualStep(
            language: wanted,
            downloaded: ParakeetUltraModel.isDownloaded(),
            compiled: true
        ) {
        case .none:
            if !multilingualDownload.isFailed { multilingualDownload.settle() }
        case .download:
            startMultilingualDownloadIfWanted()
        case .loadBeside, .dictate:
            if engine == .parakeetUltra, isModelReady {
                multilingualDownload.settle()
            } else if let language {
                loadMultilingualParakeetBeside(language: language)
            } else if let wanted, replaceParakeetWithMultilingual(language: wanted) {
                multilingualDownload.loading()
            } else {
                // Too little memory left to load it now: said, with Try again.
                multilingualDownload.fail(
                    String(localized: "Not enough memory for the multilingual model. Hex is using its usual model.")
                )
            }
        }
    }

    /// Parakeet Ultra loaded: Hex dictates its languages with it from now on.
    func multilingualParakeetLoaded() {
        multilingualDownload.settle()
    }

    /// Parakeet Ultra didn't load. Missing files download again; anything else sets it aside
    /// (`ParakeetLoadGuard`) until Try again, so Hex doesn't try at every launch.
    func multilingualParakeetFailedToLoad(_ error: Error) {
        guard MultilingualParakeetSettings().isEnabled, !(error is CancellationError) else {
            multilingualDownload.settle()
            return
        }
        if (error as? ParakeetUltraTranscriber.TranscriberError) == .notDownloaded {
            multilingualDownload.settle()
            startMultilingualDownloadIfWanted()
            return
        }
        ParakeetLoadGuard(model: ParakeetUltraTranscriber.loadGuardModel).setAside()
        multilingualDownload.fail(
            Self.switchNotice(for: error) ?? UserFacingError.message(for: error, context: .model)
        )
    }

    /// Try again beside the multilingual model's status: lets a set-aside load try again, and
    /// picks up the download from what's on disk.
    func retryMultilingualParakeet() {
        ParakeetLoadGuard(model: ParakeetUltraTranscriber.loadGuardModel).forget()
        multilingualDownload.settle()
        if ParakeetUltraModel.isDownloaded() {
            syncMultilingualParakeet()
        } else if ParakeetUltraTranscriber.chosenLanguage(availableMemory: 0) != nil {
            multilingualDownload.restart()
        }
    }

    /// Use English beside the multilingual model's status, as Settings › Dictation › Model does.
    func turnOffMultilingualParakeet() {
        MultilingualParakeetSettings().isEnabled = false
        multilingualParakeetChanged()
    }

    /// Before iOS 26: Parakeet loads at launch once downloaded, as before. The recognizer gets
    /// ready at launch only once Speech Recognition is allowed; otherwise setup asks for it
    /// (onboarding's first page, or Set up on home), never a launch by itself.
    private func prepareBeforeAppleSpeechAtLaunch(hasCachedModel: Bool) {
        let choice = SpeechEngineChoice.current
        let wanted = Self.wantedLanguage()
        switch choice.engine(for: wanted, appleSpeechPicks: false) {
        case .parakeet:
            engine = .parakeetV2
            if hasCachedModel { prepareModel() }
        case .onDeviceRecognizer, .appleSpeech:
            engine = .onDeviceRecognizer
            languageNotice = choice.notice(for: wanted)
            if SFSpeechRecognizer.authorizationStatus() == .authorized {
                // The keyboard's Hex key starts recording in Hex rather than only opening it.
                keyboardBridge.setModelInstalled(true)
                prepareModel()
            }
        }
    }

    /// Whether getting the engine ready downloads a model in Hex: not Apple's on-device
    /// recognizer, whose model iOS keeps.
    var engineDownloads: Bool {
        engine.downloads
    }

    /// The language dictation is meant to be in: delegates to `LanguageSettings.wantedDictationLanguage`.
    nonisolated static func wantedLanguage(settings: LanguageSettings = LanguageSettings()) -> String? {
        settings.wantedDictationLanguage()
    }

    static func defaultTranscriber() async -> any LocalTranscribing {
        // Settings › Dictation › Model: Multilingual, for its languages once it's downloaded and
        // compiled; until then (and off) the engine Hex uses without it.
        if let language = ParakeetUltraTranscriber.chosenLanguage(),
           multilingualStep(
               language: language,
               downloaded: ParakeetUltraModel.isDownloaded(),
               compiled: ParakeetUltraTranscriber.isCompiled
           ) == .dictate {
            return ParakeetUltraTranscriber(language: language)
        }
        let choice = SpeechEngineChoice.current
        guard #available(iOS 26.0, *) else {
            return transcriberBeforeAppleSpeech(choice: choice)
        }
        if let language = await appleSpeechLanguage(
            picked: AppleSpeechTranscriber.preferredLanguage(),
            parakeetFits: choice.parakeetFitsBesideAppleSpeech
        ) {
            return AppleSpeechTranscriber(language: language)
        }
        return ParakeetTranscriber()
    }

    /// Before iOS 26: Parakeet or the on-device recognizer (`SpeechEngineChoice`), in the
    /// language the recognizer has on this iPhone, else the one wanted, which then says it
    /// isn't available here.
    nonisolated static func transcriberBeforeAppleSpeech(
        choice: SpeechEngineChoice,
        suite: String = KeyboardBridge.appGroup,
        access: SpeechRecognitionAccess = .system,
        resolve: (LanguageSettings) -> String? = { OnDeviceSpeechLanguages.resolve(settings: $0) }
    ) -> any LocalTranscribing {
        let settings = LanguageSettings(defaults: UserDefaults(suiteName: suite))
        let wanted = wantedLanguage(settings: settings)
        guard choice.engine(for: wanted, appleSpeechPicks: false) == .onDeviceRecognizer else {
            return ParakeetTranscriber()
        }
        let language = resolve(settings) ?? wanted ?? "en-US"
        return OnDeviceSpeechTranscriber(language: language, access: access, suite: suite)
    }

    /// The language Apple Speech dictates in, or nil for Parakeet V2, which knows only
    /// English: Apple Speech's pick, unless it is English that Parakeet takes
    /// (`AppleSpeechTranscriber.shouldUse`). With no pick (Speech not answering), English goes
    /// to Parakeet only on an iPhone without `SpeechTranscriber`, and any other language
    /// (`wanted`: the one set, else the keyboard's Automatic follows, else the iPhone's) stays
    /// on Apple Speech, which says it can't dictate it rather than Parakeet hearing Spanish as
    /// English words.
    @available(iOS 26.0, *)
    nonisolated static func appleSpeechLanguage(
        picked: SpeechLanguage?,
        wanted: String? = wantedLanguage(),
        parakeetFits: Bool = ParakeetTranscriber.fitsInMemory,
        hasSpeechTranscriber: Bool = SpeechTranscriber.isAvailable,
        appleSetAside: Bool? = nil
    ) -> SpeechLanguage? {
        if let picked {
            return AppleSpeechTranscriber.shouldUse(
                picked,
                parakeetFits: parakeetFits,
                hasSpeechTranscriber: hasSpeechTranscriber,
                appleSetAside: appleSetAside
            ) ? picked : nil
        }
        guard let wanted else { return nil }
        if SpeechLanguagePicker.isEnglish(wanted) {
            let english = SpeechLanguage(locale: Locale(identifier: wanted), module: .speech)
            let setAside = appleSetAside ?? LanguageSettings().isUnavailable(english.identifier)
            guard hasSpeechTranscriber, !setAside else { return nil }
            return english
        }
        return SpeechLanguage(locale: Locale(identifier: wanted), module: .dictation)
    }

    /// What home says when a preparation failed in a way the next pick gets past: Automatic
    /// set a language aside, or Parakeet found too little memory to load (it set itself aside,
    /// so the next pick is Apple's).
    nonisolated static func switchNotice(for error: Error) -> String? {
        if let unavailable = error as? SpeechModelUnavailable, unavailable.switchedAutomatically {
            return UserFacingError.message(for: unavailable, context: .model)
        }
        if (error as? ParakeetTranscriber.TranscriberError) == .notEnoughMemory {
            return String(localized: "Not enough memory for the speech model. Hex is using Apple's on-device speech recognition.")
        }
        if (error as? ParakeetUltraTranscriber.TranscriberError) == .notEnoughMemory {
            return String(localized: "Not enough memory for the multilingual model. Hex is using its usual model.")
        }
        return nil
    }
}
