import Foundation

/// Whether the keyboard dictates by itself, in its own process with Hex closed (iOS 26,
/// `KeyboardLocalDictation`), so Hex needn't keep the mic warm for it: there Keep mic warm
/// defaults to Off and its row hides (`KeyboardBridge.warmDuration`). Not before iOS 26, and
/// not while the keyboard hands its dictations to Hex: it couldn't get its speech model
/// (`LanguageSettings.keyboardNeedsHex`), Experiments › Multilingual model takes the language
/// and only Hex loads it, or the keyboard opened Hex for a dictation in the last
/// `recentWindow` (recorded by the keyboard, and by Hex whatever the reason), until the
/// keyboard next finishes a dictation by itself.
nonisolated struct KeyboardSelfDictation {
    enum Reason: String {
        case memory
        case speechModel
        case multilingual
        /// Hex saw the keyboard open it for a dictation, whatever the keyboard's reason.
        case openedHex
    }

    static let handedToHexKey = "keyboard.handedDictationToHex"
    static let handedToHexAtKey = "keyboard.handedDictationToHexAt"
    /// How long one dictation the keyboard handed to Hex keeps the warm mic's default, unless
    /// the keyboard dictates by itself sooner: a week, as memory runs short with how the iPhone
    /// is used (older 4 GB models, many apps open), which comes back from one day to the next.
    static let recentWindow: TimeInterval = 7 * 24 * 60 * 60
    static let firstVersion = 26

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Written by the keyboard as Dictate opens Hex because it can't dictate by itself.
    func recordHandedToHex(_ reason: Reason, at now: Date = Date()) {
        defaults?.set(reason.rawValue, forKey: Self.handedToHexKey)
        defaults?.set(now.timeIntervalSince1970, forKey: Self.handedToHexAtKey)
    }

    /// Written by Hex as the keyboard opens it for a dictation, for any reason (one the keyboard
    /// didn't record, such as a dictation it couldn't start), keeping the keyboard's reason.
    func recordOpenedHex(at now: Date = Date()) {
        recordHandedToHex(handedToHex?.reason ?? .openedHex, at: now)
    }

    /// The keyboard finished a dictation by itself again.
    func clearHandedToHex() {
        guard defaults?.object(forKey: Self.handedToHexKey) != nil else { return }
        defaults?.removeObject(forKey: Self.handedToHexKey)
        defaults?.removeObject(forKey: Self.handedToHexAtKey)
    }

    var handedToHex: (reason: Reason, at: Date)? {
        guard let raw = defaults?.string(forKey: Self.handedToHexKey), let reason = Reason(rawValue: raw),
              let at = defaults?.object(forKey: Self.handedToHexAtKey) as? Double else { return nil }
        return (reason, Date(timeIntervalSince1970: at))
    }

    func dictatesItself(
        osMajorVersion: Int = ProcessInfo.processInfo.operatingSystemVersion.majorVersion,
        now: Date = Date()
    ) -> Bool {
        guard osMajorVersion >= Self.firstVersion else { return false }
        if let handed = handedToHex, now.timeIntervalSince(handed.at) < Self.recentWindow { return false }
        let languages = LanguageSettings(defaults: defaults)
        if languages.keyboardNeedsHex(now: now) { return false }
        // Speech model in keyboard (on unless turned off) dictates the multilingual model's
        // language in the keyboard only once the keyboard has that model loaded; until then
        // Dictate opens Hex for it.
        let multilingual = MultilingualParakeetSettings(defaults: defaults)
        let language = multilingual.language(set: languages.dictationLanguage, keyboard: languages.keyboardDictationLanguage())
        return !multilingual.keyboardOpensHex(language: language)
            || KeyboardParakeetSettings(defaults: defaults).keyboardHasMultilingualModel
    }
}
