import Foundation

/// The single derivation of whether and how the keyboard uses Parakeet, shared by:
/// - `KeyboardParakeet.choice()` (which model the keyboard loads and transcribes with)
/// - `KeyboardParakeetDownload.hold` (whether Hex auto-downloads Parakeet V2 for the keyboard)
/// - `KeyboardParakeetNotice.modelAwaitsDownload` (whether the keyboard shows the download notice)
nonisolated enum KeyboardParakeetPlan: Equatable, Sendable {
    enum Model: Equatable, Sendable {
        case parakeetV2
        case parakeetUltra

        var isMultilingual: Bool { self == .parakeetUltra }
    }

    /// Switch is off or this iPhone lacks the physical memory (`fitsDevice == false`).
    case off
    /// Two-language dictation is on; Parakeet does not take the keyboard's dictation.
    case twoLanguage
    /// Multilingual Parakeet takes this language.
    /// `fitsKeyboardLimit` is false only when the keyboard measured a limit below `requiredLimit`.
    case multilingual(language: String, downloaded: Bool, fitsKeyboardLimit: Bool)
    /// Dictation language is not English (and not taken by Multilingual Parakeet).
    case notEnglish
    /// English dictation, switch on and iPhone fits, but the keyboard hasn't measured its
    /// memory limit on this iPhone yet.
    case unmeasured(language: String, v2Downloaded: Bool)
    /// English dictation, switch on and iPhone fits, but the keyboard's measured memory limit
    /// is below `KeyboardParakeetBudget.requiredLimit`.
    case tooSmall
    /// English dictation, keyboard fits, waiting for Parakeet V2 to finish downloading.
    case awaitingV2Download(language: String)
    /// English dictation, keyboard fits, Parakeet V2 is complete in the App Group.
    case readyV2(language: String)

    static func resolve(
        isEnabled: Bool,
        fitsDevice: Bool,
        twoLanguageEnabled: Bool,
        multilingualLanguage: String?,
        wantedLanguage: String?,
        keyboardLimit: Int?,
        v2Downloaded: Bool,
        ultraDownloaded: Bool
    ) -> KeyboardParakeetPlan {
        guard isEnabled, fitsDevice else { return .off }
        if twoLanguageEnabled { return .twoLanguage }
        let fitsLimit = keyboardLimit.map { $0 >= KeyboardParakeetBudget.requiredLimit } ?? true
        if let multilingualLanguage {
            return .multilingual(
                language: multilingualLanguage,
                downloaded: ultraDownloaded,
                fitsKeyboardLimit: fitsLimit
            )
        }
        guard let wantedLanguage, SpeechLanguagePicker.isEnglish(wantedLanguage) else {
            return .notEnglish
        }
        guard let keyboardLimit else {
            return .unmeasured(language: wantedLanguage, v2Downloaded: v2Downloaded)
        }
        guard keyboardLimit >= KeyboardParakeetBudget.requiredLimit else {
            return .tooSmall
        }
        return v2Downloaded ? .readyV2(language: wantedLanguage) : .awaitingV2Download(language: wantedLanguage)
    }

    static func current(
        defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup),
        preferred: [String] = Locale.preferredLanguages,
        physicalMemory: UInt64 = ProcessInfo.processInfo.physicalMemory,
        v2Downloaded: Bool = SpeechModelStore.isComplete(SpeechModelStore.parakeetV2),
        ultraDownloaded: Bool = SpeechModelStore.isComplete(SpeechModelStore.parakeetUltra)
    ) -> KeyboardParakeetPlan {
        let parakeet = KeyboardParakeetSettings(defaults: defaults)
        let languages = LanguageSettings(defaults: defaults)
        let wanted = languages.wantedDictationLanguage(preferred: preferred)
        let multilingual = MultilingualParakeetSettings(defaults: defaults).language(
            set: languages.dictationLanguage,
            keyboard: languages.keyboardDictationLanguage(preferred: preferred),
            preferred: preferred,
            physicalMemory: physicalMemory
        )
        return resolve(
            isEnabled: parakeet.isEnabled,
            fitsDevice: KeyboardParakeetSettings.fits(physicalMemory: physicalMemory),
            twoLanguageEnabled: TwoLanguageDictationSettings(defaults: defaults).isEnabled,
            multilingualLanguage: multilingual,
            wantedLanguage: wanted,
            keyboardLimit: parakeet.keyboardLimit,
            v2Downloaded: v2Downloaded,
            ultraDownloaded: ultraDownloaded
        )
    }

    /// Which model and language the keyboard itself should load/transcribe with, or nil for Apple Speech.
    var modelChoice: (model: Model, language: String)? {
        switch self {
        case .multilingual(let language, downloaded: true, fitsKeyboardLimit: true):
            (.parakeetUltra, language)
        case .readyV2(let language):
            (.parakeetV2, language)
        case .unmeasured(let language, v2Downloaded: true):
            // Before a limit is measured (e.g. simulator where available == 0), allow V2 if complete;
            // on device warmUp records the real limit before checking choice().
            (.parakeetV2, language)
        case .off, .twoLanguage, .multilingual, .notEnglish, .unmeasured, .tooSmall, .awaitingV2Download:
            nil
        }
    }

    /// Whether the keyboard should show its "New speech model" download notice.
    var awaitsV2Download: Bool {
        if case .awaitingV2Download = self { return true }
        return false
    }
}
