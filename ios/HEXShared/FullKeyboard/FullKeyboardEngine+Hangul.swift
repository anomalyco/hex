import UIKit

/// Settings › Experiments › Korean keyboard (두벌식): while 한국어 types, the letter keys feed
/// `HangulComposer` and the field shows the composing run by deleting the part that changed
/// and inserting what it became. Nothing is marked text: a commit only forgets the run, which
/// already sits in the host, so space, return, punctuation, a page or language switch, emoji,
/// dictation, paste, undo, a selection action, a word delete, a cursor move or the keyboard
/// hiding all leave the syllable in place.
@MainActor
extension FullKeyboardEngine {
    /// Whether the experiment is on and 한국어 is what types now.
    var composesHangul: Bool { language == .korean && korean.isEnabled }

    /// A letter key's character through the composer. True when it typed a jamo; false when it
    /// isn't one (punctuation commits the run and types as usual) or composition is off.
    func typeHangul(_ character: String) -> Bool {
        guard composesHangul, layerState.layer == .letters else { return false }
        guard HangulComposer.isJamo(character) else {
            commitHangulComposition()
            return false
        }
        // A host that moved under the run (a cursor move, a paste, another field's text)
        // commits it; this jamo starts a new run where the cursor is now.
        if !korean.composer.isEmpty, !hangulRunIsBeforeCursor() {
            korean.reset()
        }
        // A space the strip held for the next word goes in before its first jamo, as before
        // any other key.
        if spacePending {
            spaceBeforeNextWord(character)
        }
        let old = korean.composer.text
        let jamo = isUppercase ? HangulComposer.shifted(character) : character
        let new = korean.composer.type(jamo)
        replaceHangul(old, new)
        logState("typed")
        return true
    }

    /// Delete while composing takes the last jamo back one step (한 → 하 → ㅎ → nothing, and a
    /// compound vowel or final splits). False with nothing composing, or when the cursor moved
    /// away from the run, so delete acts as usual.
    func deleteHangulBackward() -> Bool {
        guard composesHangul, !korean.composer.isEmpty else { return false }
        guard hangulRunIsBeforeCursor() else {
            korean.reset()
            return false
        }
        let old = korean.composer.text
        guard let new = korean.composer.backspace() else { return false }
        replaceHangul(old, new)
        return true
    }

    /// Space, return, punctuation, a page or language switch, emoji or any path that writes
    /// the field directly: the run is already in the field, so a commit only forgets it.
    func commitHangulComposition() {
        guard !korean.composer.isEmpty else { return }
        korean.reset()
    }

    /// A selection or cursor move: a run that is no longer right before the cursor is
    /// committed, as the text is already in the field.
    func commitHangulIfHostMoved() {
        guard !korean.composer.isEmpty, !korean.isUpdating else { return }
        guard hangulRunIsBeforeCursor() else {
            korean.reset()
            return
        }
    }

    /// Whether the run is still the field's at the cursor: no selection, the same field and
    /// text after the cursor as when it was last shown, and its own text right before it. The
    /// text after the cursor is what tells a move to another place whose text before it ends
    /// alike from no move at all.
    private func hangulRunIsBeforeCursor() -> Bool {
        guard !korean.composer.isEmpty else { return false }
        guard (target.selectedText ?? "").isEmpty else { return false }
        // A field UIKit hasn't named yet matches any: losing or gaining a name is no move.
        if let document = korean.documentIdentifier, let current = target.documentIdentifier,
           document != current {
            return false
        }
        guard target.documentContextAfterInput == korean.afterText else { return false }
        let run = korean.composer.text
        return (textBefore ?? "").hasSuffix(run) || (typingContext ?? "").hasSuffix(run)
    }

    /// Replaces the run's text in the field: only what changed since `old` is deleted, then
    /// what `new` adds is inserted. The engine's own change notifications are fenced by
    /// `korean.isUpdating`.
    private func replaceHangul(_ old: String, _ new: String) {
        // The host's own report when it still shows the run, else what Hex knows it typed
        // while the host lags behind it.
        let reported = textBefore ?? ""
        let context = typingContext ?? ""
        let before: String
        if old.isEmpty || reported.hasSuffix(old) {
            before = reported
        } else if context.hasSuffix(old) {
            before = context
        } else {
            korean.reset()
            return
        }
        // Composing only edits before the cursor, so the text after it is the run's to keep.
        let after = target.documentContextAfterInput
        let document = target.documentIdentifier
        forgetTextAroundKeys()
        tokenTaps.removeAll()
        lastSpaceAt = nil
        let oldCharacters = Array(old)
        let newCharacters = Array(new)
        var common = 0
        while common < oldCharacters.count, common < newCharacters.count,
              oldCharacters[common] == newCharacters[common] {
            common += 1
        }
        let deleted = oldCharacters.count - common
        let inserted = String(newCharacters[common...])
        let kept = String(oldCharacters[..<common])
        rememberTypingContext(before: String(before.dropLast(oldCharacters.count)) + kept, inserted: inserted)
        korean.isUpdating = true
        defer { korean.isUpdating = false }
        for _ in 0..<deleted {
            target.deleteBackward()
        }
        if !inserted.isEmpty {
            target.insertText(inserted)
        }
        if korean.composer.isEmpty {
            korean.afterText = nil
            korean.documentIdentifier = nil
        } else {
            korean.afterText = after
            korean.documentIdentifier = document
        }
        typingLog?.edit("insert", [
            TypingLogField("deleted", .string(String(oldCharacters[common...]))),
            TypingLogField("inserted", .string(inserted)),
            TypingLogField("src", .string("hangul")),
        ])
        didEdit()
    }
}
