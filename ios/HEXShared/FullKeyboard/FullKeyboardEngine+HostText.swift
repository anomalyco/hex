import Foundation
import UIKit

/// The text field the keys type into. `UITextDocumentProxy` in the keyboard; a fake in tests.
@MainActor
protocol KeyboardTextTarget: AnyObject {
    var documentContextBeforeInput: String? { get }
    var documentContextAfterInput: String? { get }
    var hasText: Bool { get }
    var autocapitalizationType: UITextAutocapitalizationType? { get }
    var autocorrectionType: UITextAutocorrectionType? { get }
    var keyboardType: UIKeyboardType? { get }
    var returnKeyType: UIReturnKeyType? { get }
    var enablesReturnKeyAutomatically: Bool? { get }
    var keyboardAppearance: UIKeyboardAppearance? { get }
    var textContentType: UITextContentType? { get }
    var isSecureTextEntry: Bool? { get }
    var smartQuotesType: UITextSmartQuotesType? { get }
    var smartDashesType: UITextSmartDashesType? { get }
    var selectedText: String? { get }
    /// `UITextDocumentProxy.documentIdentifier`: the field, where UIKit names it.
    var documentIdentifier: UUID? { get }
    func insertText(_ text: String)
    func deleteBackward()
    func adjustTextPosition(byCharacterOffset offset: Int)
}

extension KeyboardTextTarget {
    var autocorrectionType: UITextAutocorrectionType? { .default }
    var enablesReturnKeyAutomatically: Bool? { false }
    var textContentType: UITextContentType? { nil }
    var isSecureTextEntry: Bool? { nil }
    var selectedText: String? { nil }
    var documentIdentifier: UUID? { nil }
    func adjustTextPosition(byCharacterOffset offset: Int) {}
}

@MainActor
final class DocumentProxyTextTarget: KeyboardTextTarget {
    let proxy: () -> UITextDocumentProxy?

    init(_ proxy: @escaping () -> UITextDocumentProxy?) {
        self.proxy = proxy
    }

    var documentContextBeforeInput: String? { proxy()?.documentContextBeforeInput }
    var hasText: Bool { proxy()?.hasText ?? false }
    var autocapitalizationType: UITextAutocapitalizationType? { proxy()?.autocapitalizationType }
    var autocorrectionType: UITextAutocorrectionType? { proxy()?.autocorrectionType }
    var keyboardType: UIKeyboardType? { proxy()?.keyboardType }
    var returnKeyType: UIReturnKeyType? { proxy()?.returnKeyType }
    var enablesReturnKeyAutomatically: Bool? { proxy()?.enablesReturnKeyAutomatically }
    var keyboardAppearance: UIKeyboardAppearance? { proxy()?.keyboardAppearance }
    var textContentType: UITextContentType? { proxy()?.textContentType ?? nil }
    var isSecureTextEntry: Bool? { proxy()?.isSecureTextEntry }
    var selectedText: String? { proxy()?.selectedText }
    var documentIdentifier: UUID? {
        guard let proxy = proxy() else { return nil }
        return TypingLogTraits.documentIdentifier(of: proxy)
    }
    func insertText(_ text: String) { proxy()?.insertText(text) }
    func deleteBackward() { proxy()?.deleteBackward() }
    func adjustTextPosition(byCharacterOffset offset: Int) { proxy()?.adjustTextPosition(byCharacterOffset: offset) }
}


/// What a host reports around Hex's own insert. A proxy can lag `insertText` — a web view's
/// keeps returning the text from before it, or none at all — and then deliver `textDidChange`
/// after the keys have moved on. That echo is not a change to the text, and until the host
/// catches up its report is not the text either: word boundaries and casing read
/// `typingContext` (what Hex typed), while anything that deletes or replaces text keeps
/// reading `textBefore` (what the host reports).
@MainActor
extension FullKeyboardEngine {
    /// How long Hex takes a host that still reports the text from before its insert at its
    /// word: long enough for an echo on the next runloop, short enough that a change made
    /// elsewhere is never read as one. `clock` keeps it deterministic in tests.
    static let typingContextLifetime: TimeInterval = 0.5

    /// The text before the cursor Hex's own insert left, while the host may not report it yet.
    struct ExpectedTypingContext {
        /// What the host reported before the insert went in.
        var reported: String?
        /// What Hex expects the host to report after it.
        var expected: String
        /// The text after the cursor then: an insert at the cursor leaves it, a move changes it.
        var after: String?
        /// When Hex stops taking a host that still reports `reported` for merely behind.
        var until: TimeInterval
    }

    /// Whether the host is behind Hex's insert: same text after the cursor, the field still
    /// has text, nothing selected, and a report that is what the host had before the insert or
    /// a step of the way to what Hex typed (a proxy catches up one insert at a time). Anything
    /// else is a change made elsewhere: a cleared field, another field, a cursor move.
    private var hostIsBehind: Bool {
        guard let expected = expectedTypingContext, clock() <= expected.until,
              target.hasText, (target.selectedText ?? "").isEmpty,
              target.documentContextAfterInput == expected.after else { return false }
        guard let reported = textBefore else { return true }
        if reported == expected.expected { return false }
        if reported == expected.reported { return true }
        return reported.count >= (expected.reported?.count ?? 0) && expected.expected.hasPrefix(reported)
    }

    /// The text before the cursor as word boundaries and casing read it: the host's, unless it
    /// still reports what it had before Hex's last insert, then what Hex typed.
    var typingContext: String? {
        guard hostIsBehind, let expected = expectedTypingContext else { return textBefore }
        return expected.expected
    }

    /// Records what Hex's own insert of `inserted` after `before` will leave, read before it
    /// goes in: the text a host that has caught up reports next, for `typingContext` while
    /// it hasn't. Both are as `textBefore` reads them: quotes straight.
    func rememberTypingContext(before: String?, inserted: String) {
        expectedTypingContext = ExpectedTypingContext(
            reported: textBefore,
            expected: (before ?? "") + SmartPunctuation.plain(inserted),
            after: target.documentContextAfterInput,
            until: clock() + Self.typingContextLifetime
        )
    }

    /// A delete, a cursor move, a pick or a field change: what Hex typed is no longer what the
    /// host should report.
    func forgetTypingContext() {
        expectedTypingContext = nil
    }

    /// Whether the host's update is Hex's own edit (an echo, or a host still behind it) rather
    /// than a change made elsewhere: a swipe's pending space then still belongs to the text.
    func hostEchoed(_ swipe: SwipedWord) -> Bool {
        if let selected = target.selectedText, !selected.isEmpty { return false }
        return swipe.before == typingContext && swipe.after == target.documentContextAfterInput
    }

    /// The host caught up, or changed the text: either way Hex's record has done its job. A
    /// host still reporting the text from before the insert keeps it for the next key.
    func settleTypingContext() {
        guard expectedTypingContext != nil, !hostIsBehind else { return }
        expectedTypingContext = nil
    }
}
