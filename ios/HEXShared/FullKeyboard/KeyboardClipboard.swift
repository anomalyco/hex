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
    /// Something was copied recently and hasn't been pasted from the keyboard yet.
    case paste
    /// A one-time-code field: paste a copied code, or switch keyboards so iOS can offer
    /// a code from Messages or Mail (only Apple's keyboard receives those).
    case code(canPaste: Bool)
    /// A finished dictation the keyboard didn't insert by itself
    /// (`KeyboardViewModel.offeredDictation`); it never touches the clipboard.
    case dictation
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

    func offer(
        textContentType: UITextContentType?,
        hasFullAccess: Bool,
        pasteboard: UIPasteboard = .general,
        now: Date = Date()
    ) -> KeyboardClipboardOffer? {
        // Without Full Access a keyboard can't use the clipboard at all.
        guard hasFullAccess else { return nil }
        let fresh = hasFreshCopy(pasteboard: pasteboard, now: now)
        if textContentType == .oneTimeCode {
            return .code(canPaste: fresh)
        }
        return fresh ? .paste : nil
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

