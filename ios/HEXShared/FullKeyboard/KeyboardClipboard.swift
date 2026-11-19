import UIKit

/// What the strip offers from the clipboard. Deciding to offer reads only whether
/// something is there (`hasStrings`), never its contents; the clipboard is read when the
/// Paste chip is tapped.
enum KeyboardClipboardOffer: Equatable {
    /// Something was copied recently and hasn't been pasted from the keyboard yet.
    case paste
    /// A one-time-code field: paste a copied code, or switch keyboards so iOS can offer
    /// a code from Messages or Mail (only Apple's keyboard receives those).
    case code(canPaste: Bool)
}

/// Remembers which copy the keyboard first noticed and which one it already pasted, in
/// the App Group, so the offer survives the keyboard process restarting between apps.
@MainActor
struct KeyboardClipboardTracker {
    static let freshness: TimeInterval = 3 * 60

    private enum Key {
        static let seenChange = "keyboard.clipboard.seenChange"
        static let seenAt = "keyboard.clipboard.seenAt"
        static let usedChange = "keyboard.clipboard.usedChange"
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

