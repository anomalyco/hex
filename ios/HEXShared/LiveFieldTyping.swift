import UIKit

/// Settings › Dictation › Type as I speak: the live words typed into the field while the
/// keyboard records, their tail revised as the recogniser changes its mind, and taken back for
/// the dictation's own text when it stops, so that goes in exactly where and as it would without
/// them. It keeps exactly what it typed (`typed`) and touches the field only while the text
/// before the cursor still ends with it. Once it doesn't (the person or the app changed the
/// text, moved the cursor or selected something, or the app reports no text), it stops for this
/// dictation and leaves the field alone: the dictation then goes in at the cursor as without it.
/// One change is different: the app emptying the field while it ends with the words is a send
/// (`wasSent`), and the keyboard goes on listening as a fresh dictation.
@MainActor
final class LiveFieldTyping {
    private let target: KeyboardTextTarget
    /// The text before the cursor when it began: the field as the dictation goes in.
    let base: String?
    /// Exactly what it typed, the space it put before the words included.
    private(set) var typed = ""
    private(set) var stopped = false

    /// Fields it never types in: what goes in there is kept from view.
    static let privateContent: Set<UITextContentType> = [.password, .newPassword, .oneTimeCode, .creditCardNumber]

    /// Nil where it never types: secure, password, one-time code and card number fields, with
    /// text selected, or a field with text whose app reports none (nothing could be checked).
    init?(target: KeyboardTextTarget) {
        guard Self.types(in: target) else { return nil }
        self.target = target
        base = target.documentContextBeforeInput
    }

    /// Whether it would type in `target` as it is now (`init?(target:)`).
    static func types(in target: KeyboardTextTarget) -> Bool {
        mayType(secure: target.isSecureTextEntry, content: target.textContentType, selected: target.selectedText)
            && (target.documentContextBeforeInput != nil || !target.hasText)
    }

    /// Words typed in an earlier keyboard that hid before the dictation went in, resumed while
    /// the text before the cursor still ends with them, for `takeBack`.
    init?(resuming typed: String, in target: KeyboardTextTarget) {
        guard !typed.isEmpty, (target.selectedText ?? "").isEmpty, let before = target.documentContextBeforeInput,
              SmartPunctuation.plain(before).hasSuffix(SmartPunctuation.plain(typed)) else { return nil }
        self.target = target
        base = String(before.dropLast(typed.count))
        self.typed = typed
    }

    /// The app emptied the field while it still ended with the words (a chat's Send, or select
    /// all and delete): they went out as they were. An edit that leaves other text isn't one.
    var wasSent: Bool {
        guard !stopped, !typed.isEmpty, !target.hasText else { return false }
        return (target.documentContextBeforeInput ?? "").isEmpty && (target.documentContextAfterInput ?? "").isEmpty
    }

    static func mayType(secure: Bool?, content: UITextContentType?, selected: String?) -> Bool {
        secure != true && !(content.map(privateContent.contains) ?? false) && (selected ?? "").isEmpty
    }

    /// Types `words` in place of what it typed so far, removing only the part that changed.
    func show(_ words: String) {
        guard !stopped else { return }
        let next = TranscriptFormatter.insertionText(for: words, beforeContext: base, afterContext: nil)
        let old = Array(typed)
        let new = Array(next)
        var same = 0
        while same < min(old.count, new.count), old[same] == new[same] { same += 1 }
        let remove = old.count - same
        guard remove > 0 || same < new.count else { return }
        guard fieldEndsWithTyped(removing: remove) else {
            stopped = true
            return
        }
        for _ in 0..<remove { target.deleteBackward() }
        if same < new.count { target.insertText(String(new[same...])) }
        typed = next
    }

    /// Removes what it typed, while the field still ends with it, and stops. True when the
    /// field is as it was before the dictation (nothing typed, or all of it taken back).
    @discardableResult
    func takeBack() -> Bool {
        defer { stopped = true }
        guard !typed.isEmpty else { return true }
        guard !stopped, fieldEndsWithTyped(removing: typed.count) else { return false }
        for _ in typed { target.deleteBackward() }
        typed = ""
        return true
    }

    /// Whether the text before the cursor still ends with what it typed (quotes as a host's
    /// smart punctuation leaves them), nothing selected. A host that shows only the end of a
    /// long dictation passes when all it shows is the end of what was typed, and holds every
    /// character to remove. Before anything is typed, the text must be as it was.
    private func fieldEndsWithTyped(removing remove: Int) -> Bool {
        guard (target.selectedText ?? "").isEmpty else { return false }
        let reported = target.documentContextBeforeInput
        let before = SmartPunctuation.plain(reported ?? "")
        guard !typed.isEmpty else { return before == SmartPunctuation.plain(base ?? "") }
        guard reported != nil else { return false }
        let mine = SmartPunctuation.plain(typed)
        if before.hasSuffix(mine) { return true }
        return !before.isEmpty && mine.hasSuffix(before) && before.count >= remove
    }
}
