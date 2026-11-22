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
    /// keyboard gets nothing, not even the system's suggestion above it).
    case code(canPaste: Bool)
    /// A finished dictation the keyboard didn't insert by itself
    /// (`KeyboardViewModel.offeredDictation`); it never touches the clipboard.
    case dictation
}

/// The user's keyboards, from `UITextInputMode.activeInputModes`, which a keyboard extension
/// sees in Settings › General › Keyboard › Keyboards order. On iOS 27.2 a third-party keyboard is
/// a `UIKeyboardExtensionInputMode` and Apple's are `UIKeyboardInputMode`; Emoji's language is
/// `emoji`. `advanceToNextInputMode` from Hex on a number pad lands on Apple's keyboard whenever
/// one is on: Emoji and other third-party keyboards don't take number pads. On any other field it
/// lands on Emoji or another third-party keyboard about half the time when one is in the list, in
/// any order (simulator, black box, TestFlight build 126).
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

    @MainActor
    static func switchReachesAppleKeyboard(numberPad: Bool, _ modes: [UITextInputMode] = UITextInputMode.activeInputModes) -> Bool {
        switchReachesAppleKeyboard(
            modes.map { Mode(primaryLanguage: $0.primaryLanguage, className: NSStringFromClass(type(of: $0))) },
            numberPad: numberPad
        )
    }

    /// Whether one switch to the next keyboard always lands on Apple's: on a number pad whenever
    /// Apple's keyboard is on; elsewhere only when Emoji and other third-party keyboards are off.
    static func switchReachesAppleKeyboard(_ modes: [Mode], numberPad: Bool) -> Bool {
        guard hasAppleKeyboard(modes) else { return false }
        if numberPad { return true }
        return !modes.contains { $0.primaryLanguage == "emoji" } && modes.filter { $0.className.contains("Extension") }.count <= 1
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

    static var allowPasteHintAnnouncement: String {
        String(localized: "To paste without being asked, go to Settings, Apps, Hex, Paste from Other Apps, and choose Allow.")
    }

    private enum Key {
        static let seenChange = "keyboard.clipboard.seenChange"
        static let seenAt = "keyboard.clipboard.seenAt"
        static let usedChange = "keyboard.clipboard.usedChange"
        static let allowPasteHints = "keyboard.clipboard.allowPasteHints"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Only in an empty field, as Apple's keyboard offers a recent copy (iOS 27.2): once
    /// something is typed the strip goes back to suggestions. `reachesAppleKeyboard` is asked only
    /// where a code is expected, given whether the field is a number pad: Code from Messages shows
    /// only where its switch always lands on Apple's keyboard, as Emoji or another third-party
    /// keyboard gets no code and the chip would do nothing (TestFlight builds 56 and 126). It needs
    /// no Full Access, which only Paste does. `canSwitchKeyboard` is false where Apple's keyboard is
    /// split (the Duo unfolded and upright, or not known yet): iOS 27.2's switch to the next
    /// keyboard can end the app being typed in there (`FullKeyboardView.passesGlobeTouch`), so
    /// only Paste is offered and the globe's list leads to Apple's keyboard.
    func offer(
        textContentType: UITextContentType?,
        keyboardType: UIKeyboardType?,
        autocorrectionType: UITextAutocorrectionType?,
        fieldIsEmpty: Bool,
        hasFullAccess: Bool,
        canSwitchKeyboard: Bool = true,
        reachesAppleKeyboard: (_ numberPad: Bool) -> Bool = { KeyboardInputModes.switchReachesAppleKeyboard(numberPad: $0) },
        pasteboard: UIPasteboard = .general,
        now: Date = Date()
    ) -> KeyboardClipboardOffer? {
        guard fieldIsEmpty else { return nil }
        // Without Full Access a keyboard can't use the clipboard at all.
        let fresh = hasFullAccess && hasFreshCopy(pasteboard: pasteboard, now: now)
        if canSwitchKeyboard,
           Self.expectsCode(textContentType: textContentType, keyboardType: keyboardType, autocorrectionType: autocorrectionType),
           reachesAppleKeyboard(Self.isNumberPad(keyboardType)) {
            return .code(canPaste: fresh)
        }
        return fresh ? .paste : nil
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

    /// After the chip's read: whether to point to Settings › Apps › Hex › Paste from Other
    /// Apps. A refused read (the clipboard has text but none came back: Don't Allow, or the
    /// setting is Deny) always does, since the chip did nothing; one that waited
    /// `askedReadDuration` for Allow Paste does the first `allowPasteHintLimit` times, then
    /// iOS's prompt is left alone.
    func shouldPointToAllowPaste(readDuration: TimeInterval, refused: Bool) -> Bool {
        if refused { return true }
        guard readDuration >= Self.askedReadDuration else { return false }
        let shown = defaults?.integer(forKey: Key.allowPasteHints) ?? 0
        guard shown < Self.allowPasteHintLimit else { return false }
        defaults?.set(shown + 1, forKey: Key.allowPasteHints)
        return true
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

