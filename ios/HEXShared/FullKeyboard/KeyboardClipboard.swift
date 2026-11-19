import UIKit
import UniformTypeIdentifiers

/// What the strip offers from the clipboard. Nothing here reads the clipboard's
/// contents, so the keyboard never triggers iOS's paste prompt.
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

/// The target of the strip's system Paste button. `UIPasteControl` hands it the
/// clipboard only when the person taps the button, without a prompt.
@MainActor
final class KeyboardPasteReceiver: UIView {
    var onPaste: ((String) -> Void)?

    override init(frame: CGRect) {
        super.init(frame: frame)
        isHidden = true
        pasteConfiguration = UIPasteConfiguration(acceptableTypeIdentifiers: [
            UTType.utf8PlainText.identifier,
            UTType.plainText.identifier,
            UTType.url.identifier,
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override func canPaste(_ itemProviders: [NSItemProvider]) -> Bool {
        itemProviders.contains { $0.canLoadObject(ofClass: String.self) || $0.canLoadObject(ofClass: URL.self) }
    }

    override func paste(itemProviders: [NSItemProvider]) {
        if let provider = itemProviders.first(where: { $0.canLoadObject(ofClass: String.self) }) {
            _ = provider.loadObject(ofClass: String.self) { [weak self] text, _ in
                guard let text else { return }
                Task { @MainActor in self?.deliver(text) }
            }
        } else if let provider = itemProviders.first(where: { $0.canLoadObject(ofClass: URL.self) }) {
            _ = provider.loadObject(ofClass: URL.self) { [weak self] url, _ in
                guard let url else { return }
                Task { @MainActor in self?.deliver(url.absoluteString) }
            }
        }
    }

    private func deliver(_ text: String) {
        guard !text.isEmpty else { return }
        onPaste?(text)
    }
}
