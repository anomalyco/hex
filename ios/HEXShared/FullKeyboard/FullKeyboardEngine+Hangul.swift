import UIKit

/// Settings › Experiments › Korean keyboard (두벌식): while 한국어 types, the letter keys feed
/// `HangulComposer` and the field shows the composing run by deleting the part that changed
/// and inserting what it became. Nothing is marked text: a commit only forgets the run, which
/// already sits in the host, so space, return, punctuation, a page or language switch, emoji,
/// dictation, a cursor move or the keyboard hiding all leave the syllable in place.
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
        // A space the strip held for the next word goes in before its first jamo, as before
        // any other key.
        if spacePending {
            spaceBeforeNextWord(character)
        }
        // A host that changed under the run (a paste, another field's text) starts a new one
        // rather than deleting text that is no longer the run.
        let run = korean.composer.text
        if !run.isEmpty,
           !(textBefore ?? "").hasSuffix(run), !(typingContext ?? "").hasSuffix(run) {
            korean.composer.reset()
        }
        let old = korean.composer.text
        let jamo = isUppercase ? HangulComposer.shifted(character) : character
        let new = korean.composer.type(jamo)
        replaceHangul(old, new)
        logState("typed")
        return true
    }

    /// Delete while composing takes the last jamo back one step (한 → 하 → ㅎ → nothing, and a
    /// compound vowel or final splits). False with nothing composing, so delete acts as usual.
    func deleteHangulBackward() -> Bool {
        guard composesHangul, !korean.composer.isEmpty else { return false }
        let run = korean.composer.text
        guard (textBefore ?? "").hasSuffix(run) || (typingContext ?? "").hasSuffix(run) else {
            korean.composer.reset()
            return false
        }
        let old = korean.composer.text
        guard let new = korean.composer.backspace() else { return false }
        replaceHangul(old, new)
        return true
    }

    /// Space, return, punctuation, a page or language switch, emoji or dictation: the run is
    /// already in the field, so a commit only forgets it.
    func commitHangulComposition() {
        guard !korean.composer.isEmpty else { return }
        korean.composer.reset()
    }

    /// A selection or cursor move: a run that is no longer right before the cursor (or is part
    /// of a selection) is committed, as the text is already in the field.
    func commitHangulIfHostMoved() {
        guard !korean.composer.isEmpty, !korean.isUpdating else { return }
        guard (target.selectedText ?? "").isEmpty,
              (typingContext ?? "").hasSuffix(korean.composer.text) else {
            korean.composer.reset()
            return
        }
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
            korean.composer.reset()
            return
        }
        forgetTextAroundKeys()
        tokenTaps.removeAll()
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
        typingLog?.edit("insert", [
            TypingLogField("deleted", .string(String(oldCharacters[common...]))),
            TypingLogField("inserted", .string(inserted)),
            TypingLogField("src", .string("hangul")),
        ])
        didEdit()
    }
}
