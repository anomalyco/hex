import UIKit

/// The return key for the field's `returnKeyType`, as Apple's keyboard draws it (iOS 27.2, seen
/// in fields of every type: screenshots and the accessibility tree): a symbol on the blue key
/// for the types that act (send ↑, go, join and route →, search 🔍, done ✓, emergency call
/// 📞) and on a plain key for return ↵, next and continue ›. Both the full keyboard and the
/// dictation bar draw it.
nonisolated struct ReturnKeyStyle: Equatable, Sendable {
    let symbol: String
    let isAction: Bool
    let accessibilityLabel: String

    init(_ type: UIReturnKeyType?) {
        switch type ?? .default {
        case .send:
            (symbol, isAction, accessibilityLabel) = ("arrow.up", true, String(localized: "Send"))
        case .go:
            (symbol, isAction, accessibilityLabel) = ("arrow.right", true, String(localized: "Go"))
        case .join:
            (symbol, isAction, accessibilityLabel) = ("arrow.right", true, String(localized: "Join"))
        case .route:
            (symbol, isAction, accessibilityLabel) = ("arrow.right", true, String(localized: "Route"))
        case .google, .yahoo, .search:
            (symbol, isAction, accessibilityLabel) = ("magnifyingglass", true, String(localized: "Search"))
        case .done:
            (symbol, isAction, accessibilityLabel) = ("checkmark", true, String(localized: "Done"))
        case .emergencyCall:
            (symbol, isAction, accessibilityLabel) = ("phone", true, String(localized: "Emergency call"))
        case .next:
            (symbol, isAction, accessibilityLabel) = ("chevron.right", false, String(localized: "Next"))
        case .continue:
            (symbol, isAction, accessibilityLabel) = ("chevron.right", false, String(localized: "Continue"))
        default:
            (symbol, isAction, accessibilityLabel) = ("return", false, String(localized: "Return"))
        }
    }

    /// Off while the field is empty when it asks for that (`enablesReturnKeyAutomatically`):
    /// Apple's key turns grey with a faint glyph and a tap does nothing.
    static func isEnabled(automatically: Bool?, hasText: Bool) -> Bool {
        !(automatically == true && !hasText)
    }
}
