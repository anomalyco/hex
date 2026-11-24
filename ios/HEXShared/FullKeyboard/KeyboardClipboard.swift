import UIKit

/// What the strip offers from the clipboard. Deciding to offer reads only whether
/// something is there (`hasStrings`), never its contents; the clipboard is read when the
/// Paste chip is tapped.
///
/// iOS asks "Allow Paste" for every read while Settings › Apps › Hex › Paste from Other Apps
/// is Ask, its default. The keyboard's reads count as Hex's own: on the simulator, pasted
/// looks up the keyboard's identity as `anomaly.hex.mobile`, a keyboard read alone makes the
/// row appear under Hex, and with Allow the chip pastes without asking. `UIPasteControl`
/// (which never asks) draws blank in a keyboard, and `detectPatterns` can't give the text,
/// so the strip points to that setting after a paste that asked or was refused.
enum KeyboardClipboardOffer: Equatable {
    /// Something was copied recently and hasn't been pasted from the keyboard yet, and the
    /// field is empty.
    case paste
    /// An empty field that expects a code (`KeyboardClipboardTracker.expectsCode`) with Apple's
    /// keyboard among the user's keyboards: paste a copied code, or switch to it so iOS can
    /// offer a code from Messages or Mail (only Apple's keyboard receives those; a third-party
    /// keyboard gets nothing, not even the system's suggestion above it). `opensList`: the chip
    /// opens iOS's keyboard list, where a switch to the next keyboard could land elsewhere
    /// (`KeyboardInputModes.route`).
    case code(canPaste: Bool, opensList: Bool = false)
    /// An empty field iOS marks as a username or password (`KeyboardClipboardTracker.expectsLogin`)
    /// with Apple's keyboard among the user's keyboards: the Passwords chip takes the person
    /// there, by Code from Messages' route, since iOS offers saved logins (Passwords, and
    /// managers' AutoFill extensions) only in Apple's keyboard's bar.
    case passwords(canPaste: Bool, opensList: Bool = false)
    /// A finished dictation the keyboard didn't insert by itself
    /// (`KeyboardViewModel.offeredDictation`); it never touches the clipboard.
    case dictation

    /// Code from Messages or Passwords: a chip that takes the person to Apple's keyboard.
    var leadsToAppleKeyboard: Bool { opensList != nil }

    /// Whether that chip opens iOS's keyboard list rather than switching; nil without the chip.
    var opensList: Bool? {
        switch self {
        case .code(_, let list), .passwords(_, let list): list
        case .paste, .dictation: nil
        }
    }

    /// Whether Paste shows: beside Code from Messages or Passwords only with a fresh copy.
    var canPaste: Bool {
        switch self {
        case .code(let paste, _), .passwords(let paste, _): paste
        case .paste, .dictation: true
        }
    }
}

/// The user's keyboards, from `UITextInputMode.activeInputModes`, which a keyboard extension
/// sees in Settings › General › Keyboard › Keyboards order. On iOS 27.2 a third-party keyboard is
/// a `UIKeyboardExtensionInputMode` and Apple's are `UIKeyboardInputMode`; Emoji's language is
/// `emoji`. `advanceToNextInputMode` from Hex on a number pad lands on Apple's keyboard whenever
/// one is on: Emoji and other third-party keyboards don't take number pads. On any other field it
/// lands on Emoji or another third-party keyboard about half the time when one is in the list, in
/// any order (simulator, black box, TestFlight build 126); iOS's keyboard list
/// (`handleInputModeList`) lands where the person picks.
enum KeyboardInputModes {
    struct Mode: Equatable {
        var primaryLanguage: String?
        var className: String
    }

    @MainActor
    static func hasAppleKeyboard(_ modes: [UITextInputMode] = UITextInputMode.activeInputModes) -> Bool {
        hasAppleKeyboard(modes.map { Mode(primaryLanguage: $0.primaryLanguage, className: NSStringFromClass(type(of: $0))) })
    }

    /// Apple's own keyboard for a language: not Emoji or dictation, and not an extension.
    static func hasAppleKeyboard(_ modes: [Mode]) -> Bool {
        modes.contains { mode in
            !["emoji", "dictation"].contains(mode.primaryLanguage) && !mode.className.contains("Extension")
        }
    }

    /// How Code from Messages reaches Apple's keyboard.
    enum Route: Equatable {
        /// A switch to the next keyboard always lands on Apple's.
        case next
        /// It could land on Emoji or another third-party keyboard: iOS's keyboard list instead,
        /// where the person picks Apple's.
        case list
    }

    @MainActor
    static func route(numberPad: Bool, _ modes: [UITextInputMode] = UITextInputMode.activeInputModes) -> Route? {
        route(modes.map { Mode(primaryLanguage: $0.primaryLanguage, className: NSStringFromClass(type(of: $0))) }, numberPad: numberPad)
    }

    /// Nil without Apple's keyboard. The next keyboard is always Apple's on a number pad, and
    /// elsewhere only when Emoji and other third-party keyboards are off.
    static func route(_ modes: [Mode], numberPad: Bool) -> Route? {
        guard hasAppleKeyboard(modes) else { return nil }
        if numberPad { return .next }
        let onlyApple = !modes.contains { $0.primaryLanguage == "emoji" } && modes.filter { $0.className.contains("Extension") }.count <= 1
        return onlyApple ? .next : .list
    }
}

/// Remembers which copy the keyboard first noticed and which one it already pasted, in
/// the App Group, so the offer survives the keyboard process restarting between apps.
@MainActor
struct KeyboardClipboardTracker {
    static let freshness: TimeInterval = 3 * 60

    /// A read that took this long waited for someone to answer Allow Paste.
    static let askedReadDuration: TimeInterval = 0.3
    /// How many pastes that asked point to the setting; one refused always does.
    static let allowPasteHintLimit = 3

    /// The strip's pointer to the setting that stops Allow Paste.
    static var allowPasteHint: String {
        String(localized: "Settings › Apps › Hex › Paste from Other Apps › Allow")
    }

    /// The two-line tappable strip notice after iOS asks Allow Paste or refuses a read:
    /// names the exact row in Settings and offers a button that jumps straight there.
    static var allowPasteNotice: KeyboardStripNotice {
        KeyboardStripNotice(
            title: String(localized: "Paste without asking"),
            detail: String(localized: "Paste from Other Apps › Allow"),
            button: String(localized: "Settings")
        )
    }

    static var allowPasteHintAnnouncement: String {
        String(localized: "To paste without being asked, go to Settings, Apps, Hex, Paste from Other Apps, and choose Allow.")
    }

    private enum Key {
        static let seenChange = "keyboard.clipboard.seenChange"
        static let seenAt = "keyboard.clipboard.seenAt"
        static let usedChange = "keyboard.clipboard.usedChange"
        static let ownedChange = "keyboard.clipboard.ownedChange"
        static let allowPasteHints = "keyboard.clipboard.allowPasteHints"
        static let allowPastePrompted = "keyboard.clipboard.allowPastePrompted"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Only in an empty field, as Apple's keyboard offers a recent copy (iOS 27.2): once
    /// something is typed the strip goes back to suggestions. `appleKeyboardRoute` is asked only
    /// where a code is expected, given whether the field is a number pad: Code from Messages shows
    /// only with Apple's keyboard on (Emoji or another third-party keyboard gets no code,
    /// TestFlight build 56), and opens the keyboard list where the next keyboard could be one of
    /// those (TestFlight build 126). It needs no Full Access, which only Paste does.
    /// `canSwitchKeyboard` is false where Apple's keyboard is
    /// split (the Duo unfolded and upright, or not known yet): iOS 27.2's switch to the next
    /// keyboard can end the app being typed in there (`InputModeListTouch`), so there the chip
    /// opens iOS's keyboard list, whose pick doesn't, rather than leaving only the globe.
    func offer(
        textContentType: UITextContentType?,
        keyboardType: UIKeyboardType?,
        autocorrectionType: UITextAutocorrectionType?,
        fieldIsEmpty: Bool,
        hasFullAccess: Bool,
        canSwitchKeyboard: Bool = true,
        appleKeyboardRoute: (_ numberPad: Bool) -> KeyboardInputModes.Route? = { KeyboardInputModes.route(numberPad: $0) },
        pasteboard: UIPasteboard = .general,
        now: Date = Date()
    ) -> KeyboardClipboardOffer? {
        guard fieldIsEmpty else { return nil }
        // Without Full Access a keyboard can't use the clipboard at all.
        let fresh = hasFullAccess && hasFreshCopy(pasteboard: pasteboard, now: now)
        if Self.expectsCode(textContentType: textContentType, keyboardType: keyboardType, autocorrectionType: autocorrectionType),
           let route = appleKeyboardRoute(Self.isNumberPad(keyboardType)) {
            return .code(canPaste: fresh, opensList: route == .list || !canSwitchKeyboard)
        }
        if Self.expectsLogin(textContentType: textContentType), let route = appleKeyboardRoute(false) {
            return .passwords(canPaste: fresh, opensList: route == .list || !canSwitchKeyboard)
        }
        return fresh ? .paste : nil
    }

    /// A username, password or new-password field. In practice Hex meets only usernames, and
    /// only on the web (Safari's `autocomplete="username"`, iOS 26.5 and 27.2): iOS brings its own
    /// keyboard to a native `.username` field and to every secure one. An email field can't be
    /// told apart from one outside a login form (a keyboard sees only its own field's traits),
    /// so it gets no chip.
    static func expectsLogin(textContentType: UITextContentType?) -> Bool {
        guard let textContentType else { return false }
        return [.username, .password, .newPassword].contains(textContentType)
    }

    /// A `.oneTimeCode` field with a number pad, or with autocorrection set either way: WebKit
    /// always sets it, so Safari's `autocomplete="one-time-code"` fields count (iOS 27.2).
    /// Some chat apps mark their message box `.oneTimeCode` too, with letters and autocorrection
    /// left at default: that box gets suggestions, as it does with Apple's keyboard (TestFlight
    /// build 56, ChatGPT and Capy).
    static func expectsCode(
        textContentType: UITextContentType?,
        keyboardType: UIKeyboardType?,
        autocorrectionType: UITextAutocorrectionType?
    ) -> Bool {
        guard textContentType == .oneTimeCode else { return false }
        if isNumberPad(keyboardType) || keyboardType == .numbersAndPunctuation { return true }
        return autocorrectionType == .yes || autocorrectionType == .no
    }

    /// The pads where the switch to the next keyboard passes over Emoji and other third-party
    /// keyboards (iOS 27.2 simulator).
    static func isNumberPad(_ keyboardType: UIKeyboardType?) -> Bool {
        [.numberPad, .asciiCapableNumberPad, .decimalPad, .phonePad].contains(keyboardType)
    }

    /// What the Paste chip inserts, in one read: text, else a link. Reading `string` and then
    /// `url` would ask Allow Paste twice for a link.
    static func read(_ pasteboard: UIPasteboard = .general) -> String? {
        if pasteboard.hasStrings { return pasteboard.string }
        if pasteboard.hasURLs { return pasteboard.url?.absoluteString }
        return nil
    }

    /// Whether the keyboard has seen iOS prompt or refuse a cross-app paste and has not yet
    /// seen a fast cross-app paste since (or been dismissed in Hex). Because iOS creates the
    /// Paste from Other Apps row only after a cross-app read, true also means the row exists.
    var needsAllowPasteSetting: Bool {
        defaults?.bool(forKey: Key.allowPastePrompted) ?? false
    }

    /// After the chip's read: whether to show the strip's Settings notice. A refused read
    /// (Don't Allow, or Deny) always shows it, since the chip did nothing; one that waited
    /// `askedReadDuration` for Allow Paste shows it the first `allowPasteHintLimit` times,
    /// then leaves the strip alone while keeping `needsAllowPasteSetting` true for Hex.
    func shouldPointToAllowPaste(readDuration: TimeInterval, refused: Bool) -> Bool {
        if refused {
            defaults?.set(true, forKey: Key.allowPastePrompted)
            return true
        }
        guard readDuration >= Self.askedReadDuration else { return false }
        defaults?.set(true, forKey: Key.allowPastePrompted)
        let shown = defaults?.integer(forKey: Key.allowPasteHints) ?? 0
        guard shown < Self.allowPasteHintLimit else { return false }
        defaults?.set(shown + 1, forKey: Key.allowPasteHints)
        return true
    }

    /// Records a copy written by Hex itself (keyboard Copy/Cut or the app's transcript copy)
    /// so a later fast paste of our own text does not clear `needsAllowPasteSetting`.
    func markOwnedCopy(pasteboard: UIPasteboard = .general) {
        defaults?.set(pasteboard.changeCount, forKey: Key.ownedChange)
    }

    /// A paste that returned immediately without prompting: if the clipboard came from another
    /// app, Paste from Other Apps is Allow, so clear the app-side banner and reset the counter.
    func recordSilentPaste(pasteboard: UIPasteboard = .general) {
        guard let defaults else { return }
        if let owned = defaults.object(forKey: Key.ownedChange) as? Int, owned == pasteboard.changeCount {
            return
        }
        defaults.set(false, forKey: Key.allowPastePrompted)
        defaults.set(0, forKey: Key.allowPasteHints)
    }

    /// Dismisses the app-side banner until iOS prompts again.
    func dismissAllowPasteSetting() {
        defaults?.set(false, forKey: Key.allowPastePrompted)
    }

    func markPasted(pasteboard: UIPasteboard = .general) {
        defaults?.set(pasteboard.changeCount, forKey: Key.usedChange)
    }

    private func hasFreshCopy(pasteboard: UIPasteboard, now: Date) -> Bool {
        guard pasteboard.hasStrings || pasteboard.hasURLs else { return false }
        let change = pasteboard.changeCount
        guard let defaults else { return true }
        guard defaults.integer(forKey: Key.usedChange) != change else { return false }
        if defaults.integer(forKey: Key.seenChange) != change {
            defaults.set(change, forKey: Key.seenChange)
            defaults.set(now.timeIntervalSince1970, forKey: Key.seenAt)
            return true
        }
        return now.timeIntervalSince1970 - defaults.double(forKey: Key.seenAt) < Self.freshness
    }
}

