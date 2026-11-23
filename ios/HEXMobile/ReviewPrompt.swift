import Foundation

/// When Hex asks for an App Store review, always through iOS's own prompt (`requestReview`):
/// App Review rejects a custom one that only sends happy people to the store. iOS shows it at
/// most three times in 365 days and may skip any ask (and never shows it in TestFlight); Hex
/// asks less often than that: once per app version, `spacing` apart, `yearlyLimit` a year, and
/// only after a good moment. The keyboard can't show the prompt, and most people stay in it, so
/// the moments are in the app: the practice's first dictation, a changed setting, and an open
/// after the keyboard dictated or the `opensToAsk`th open.
struct ReviewPrompt {
    enum Moment: Equatable {
        /// Onboarding's Hex is ready, after the practice's dictation arrived.
        case practiceDictated
        /// Back on Home from Settings with a setting changed.
        case settingChanged
        /// Hex came to the front by itself, not from the keyboard, past onboarding.
        case opened
    }

    static let spacing: TimeInterval = 120 * 24 * 60 * 60
    static let yearlyLimit = 3
    static let opensToAsk = 3
    private static let year: TimeInterval = 365 * 24 * 60 * 60

    enum Key {
        static let askedAt = "reviewPrompt.askedAt"
        static let askedVersion = "reviewPrompt.askedVersion"
        static let practiceDictated = "reviewPrompt.practiceDictated"
        static let opens = "reviewPrompt.opens"
        static let keyboardResultSeen = "reviewPrompt.keyboardResultSeen"
    }

    private let defaults: UserDefaults
    private let version: String
    private let now: () -> Date

    init(
        defaults: UserDefaults = .standard,
        version: String = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "",
        now: @escaping () -> Date = Date.init
    ) {
        self.defaults = defaults
        self.version = version
        self.now = now
    }

    /// The practice's first dictation arrived: Hex is ready may ask.
    func notePracticeDictated() {
        defaults.set(true, forKey: Key.practiceDictated)
    }

    /// Whether to ask at `moment`. True records the ask, so call it only right before
    /// `requestReview`. `keyboardResultID` is the keyboard's last inserted dictation
    /// (`KeyboardBridge.lastInsertedResultID`), for `.opened`.
    func shouldAsk(at moment: Moment, keyboardResultID: String? = nil) -> Bool {
        guard isGood(moment, keyboardResultID: keyboardResultID), isAllowed else { return false }
        let at = now().timeIntervalSince1970
        defaults.set(askTimes.filter { at - $0 < Self.year } + [at], forKey: Key.askedAt)
        defaults.set(version, forKey: Key.askedVersion)
        defaults.set(0, forKey: Key.opens)
        return true
    }

    private func isGood(_ moment: Moment, keyboardResultID: String?) -> Bool {
        switch moment {
        case .practiceDictated:
            let dictated = defaults.bool(forKey: Key.practiceDictated)
            defaults.removeObject(forKey: Key.practiceDictated)
            return dictated
        case .settingChanged:
            return true
        case .opened:
            let opens = defaults.integer(forKey: Key.opens) + 1
            defaults.set(opens, forKey: Key.opens)
            let seen = defaults.string(forKey: Key.keyboardResultSeen)
            if let keyboardResultID { defaults.set(keyboardResultID, forKey: Key.keyboardResultSeen) }
            let keyboardDictated = keyboardResultID != nil && keyboardResultID != seen
            return keyboardDictated || opens >= Self.opensToAsk
        }
    }

    private var isAllowed: Bool {
        guard defaults.string(forKey: Key.askedVersion) != version else { return false }
        let at = now().timeIntervalSince1970
        let thisYear = askTimes.filter { at - $0 < Self.year }
        if let last = thisYear.max(), at - last < Self.spacing { return false }
        return thisYear.count < Self.yearlyLimit
    }

    private var askTimes: [Double] {
        defaults.array(forKey: Key.askedAt) as? [Double] ?? []
    }
}
