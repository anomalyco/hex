import CoreGraphics

/// The kind of text field, as far as the key rows care.
nonisolated enum KeyboardFieldKind: Equatable, Sendable {
    case text
    /// Adds `.` and `/` right of space.
    case url
    /// Adds `@` and `.` right of space, matching Apple's iOS 26 email layout.
    case email
    /// A `.webSearch` field (Safari's address bar): text, with Apple's `.` right of space.
    case webSearch
}

nonisolated extension KeyboardFieldKind {
    /// Fields typed in words, where autocorrect, swipe, suggestions, smart punctuation, emoji
    /// and Learn how I type work as the field's traits allow: text, and web search, whose only
    /// difference is its `.` key. URL and email fields type addresses.
    var takesWords: Bool { self == .text || self == .webSearch }

    /// The keys right of the space bar, each `width` wide: Apple's `.` and `/` in a URL field,
    /// `@` and `.` in an email field, and `.` in a web search field.
    func keysRightOfSpace(width: CGFloat) -> [(KeyKind, CGFloat, String)] {
        switch self {
        case .text: []
        case .url: [(.character("."), width, "."), (.character("/"), width, "/")]
        case .email: [(.character("@"), width, "@"), (.character("."), width, ".")]
        case .webSearch: [(.character("."), width, ".")]
        }
    }
}
