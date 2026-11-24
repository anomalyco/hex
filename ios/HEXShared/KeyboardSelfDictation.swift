import Foundation

/// Whether the keyboard dictates by itself, in its own process with Hex closed (iOS 26,
/// `KeyboardLocalDictation`), so Hex needn't keep the mic warm for it: there Keep mic warm
/// defaults to Off and its row hides (`KeyboardBridge.warmDuration`). Not before iOS 26, and
/// not while the keyboard can't dictate by itself: it couldn't get its speech model
/// (`LanguageSettings.keyboardNeedsHex`), or Hex was opened for a dictation in the last
/// `recentWindow` (recorded by the keyboard, and by Hex whatever the reason), until the
/// keyboard next finishes a dictation by itself. Which speech model is picked never stops it:
/// the keyboard dictates the multilingual model's languages too, with Apple's speech
/// recognition until it has that model loaded.
nonisolated struct KeyboardSelfDictation {
    enum Reason: String {
        case memory
        case speechModel
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

    /// Written by the keyboard as it opens Hex because it can't dictate by itself: Open Hex in
    /// the line that says why, or before iOS 26 Dictate itself.
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

    /// Why the keyboard can't dictate in its own process, or nil when it dictates by itself.
    /// From iOS 26, with `includeRecentHandoff` false (the keyboard's own question), only
    /// `speechModel` or `memory`.
    func handoffReason(
        osMajorVersion: Int = ProcessInfo.processInfo.operatingSystemVersion.majorVersion,
        now: Date = Date(),
        hasMemoryToStart: Bool = true,
        includeRecentHandoff: Bool = true
    ) -> Reason? {
        guard osMajorVersion >= Self.firstVersion else { return .openedHex }
        if includeRecentHandoff,
           let handed = handedToHex,
           now.timeIntervalSince(handed.at) < Self.recentWindow {
            return handed.reason
        }
        if LanguageSettings(defaults: defaults).keyboardNeedsHex(now: now) {
            return .speechModel
        }
        if !hasMemoryToStart || Self.simulatesShortMemory(defaults) {
            return .memory
        }
        return nil
    }

    /// Simulator builds only: the App Group's `simulator.keyboardMemoryShort` stands in for an
    /// iPhone short of keyboard memory, which a simulator never is, for screenshots and recordings.
    static func simulatesShortMemory(_ defaults: UserDefaults?) -> Bool {
        #if targetEnvironment(simulator)
        defaults?.bool(forKey: "simulator.keyboardMemoryShort") == true
        #else
        false
        #endif
    }

    func dictatesItself(
        osMajorVersion: Int = ProcessInfo.processInfo.operatingSystemVersion.majorVersion,
        now: Date = Date()
    ) -> Bool {
        handoffReason(osMajorVersion: osMajorVersion, now: now) == nil
    }
}
