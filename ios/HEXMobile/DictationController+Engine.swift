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
            engineName = "Parakeet V2"
            prepareModel()
            return
        }
        let fits = SpeechEngineChoice.current.parakeetFitsBesideAppleSpeech
        Task { [weak self] in
            guard let self else { return }
            if await Self.appleSpeechLanguage(picked: AppleSpeechTranscriber.preferredLanguage(), parakeetFits: fits) != nil {
                self.engineName = "Apple Speech"
                self.prepareModel()
            } else {
                self.engineName = "Parakeet V2"
                if hasCachedModel {
                    self.phase = .failed(ParakeetLoadGuard.message)
                }
            }
        }
    }

    /// Experiments › Multilingual model (Parakeet) at launch: true when Parakeet Ultra takes
    /// the dictation language and is getting ready. With the experiment off, it only deletes a
    /// download turning it off left mid-way.
    private func prepareMultilingualParakeetAtLaunch() -> Bool {
        ParakeetLoadGuard(model: ParakeetUltraTranscriber.loadGuardModel).noteLaunch()
        guard MultilingualParakeetSettings().isEnabled else {
            ParakeetUltraModel.remove()
            return false
        }
        guard ParakeetUltraTranscriber.chosenLanguage() != nil else { return false }
        engineName = ParakeetUltraTranscriber.name
        prepareModel()
        return true
    }

    /// Before iOS 26: Parakeet loads at launch once downloaded, as before. The recognizer gets
    /// ready at launch only once Speech Recognition is allowed; otherwise setup asks for it
    /// (onboarding's first page, or Set up on home), never a launch by itself.
    private func prepareBeforeAppleSpeechAtLaunch(hasCachedModel: Bool) {
        let choice = SpeechEngineChoice.current
        let wanted = Self.wantedLanguage()
        switch choice.engine(for: wanted, appleSpeechPicks: false) {
        case .parakeet:
            engineName = "Parakeet V2"
            if hasCachedModel { prepareModel() }
        case .onDeviceRecognizer, .appleSpeech:
            engineName = "Apple Speech"
            languageNotice = choice.notice(for: wanted)
            if SFSpeechRecognizer.authorizationStatus() == .authorized {
                // The keyboard's Hex key starts recording in Hex rather than only opening it.
                keyboardBridge.setModelInstalled(true)
                prepareModel()
            }
        }
    }

    /// Whether getting the engine ready downloads a model in Hex: not Apple's on-device
    /// recognizer, the only "Apple Speech" before iOS 26, whose model iOS keeps.
    var engineDownloads: Bool {
        if #available(iOS 26.0, *) { return true }
        return engineName != "Apple Speech"
    }

    /// The language dictation is meant to be in: the one set, else the keyboard's Automatic
    /// follows, else the iPhone's.
    nonisolated static func wantedLanguage(settings: LanguageSettings = LanguageSettings()) -> String? {
        settings.dictationLanguage ?? settings.keyboardDictationLanguage() ?? Locale.preferredLanguages.first
    }

    static func defaultTranscriber() async -> any LocalTranscribing {
        // Experiments › Multilingual model (Parakeet), for its languages; off, nothing changes.
        if let language = ParakeetUltraTranscriber.chosenLanguage() {
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
    /// to Parakeet and any other language (`wanted`: the one set, else the keyboard's
    /// Automatic follows, else the iPhone's) stays on Apple Speech, which says it can't
    /// dictate it rather than Parakeet hearing Spanish as English words.
    @available(iOS 26.0, *)
    nonisolated static func appleSpeechLanguage(
        picked: SpeechLanguage?,
        wanted: String? = wantedLanguage(),
        parakeetFits: Bool = ParakeetTranscriber.fitsInMemory
    ) -> SpeechLanguage? {
        if let picked {
            return AppleSpeechTranscriber.shouldUse(picked, parakeetFits: parakeetFits) ? picked : nil
        }
        guard let wanted, !SpeechLanguagePicker.isEnglish(wanted) else { return nil }
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
            return String(localized: "Parakeet needs more memory than Hex has here. Hex uses Apple's on-device speech instead.")
        }
        if (error as? ParakeetUltraTranscriber.TranscriberError) == .notEnoughMemory {
            return String(localized: "Parakeet Ultra needs more memory than Hex has here. Hex uses its usual speech model instead.")
        }
        return nil
    }
}
