import Foundation

/// The Greek layout's own typing rules, as the system Greek keyboard was observed to apply them
/// (iOS 27.2 simulator, real touches in Messages, black box). The tonos key ´ accents the vowel
/// right before it (α´ -> ά, Α´ -> Ά, and ϊ´ -> ΐ, ϋ´ -> ΰ); after anything else, an accented
/// vowel included, it types ´ (U+00B4) itself, and it never adds a diaeresis (ϊ ϋ ΐ ΰ are held on
/// ι and υ). σ types ς while it ends the word, and turns back into σ when another letter
/// follows ("ας", then α: "ασα").
nonisolated enum GreekTyping {
    /// The tonos key: the character it types when there is no vowel before it to accent.
    static let tonosKey = "\u{00B4}"

    private static let accented: [Character: Character] = [
        "α": "ά", "ε": "έ", "η": "ή", "ι": "ί", "ο": "ό", "υ": "ύ", "ω": "ώ",
        "Α": "Ά", "Ε": "Έ", "Η": "Ή", "Ι": "Ί", "Ο": "Ό", "Υ": "Ύ", "Ω": "Ώ",
        "ϊ": "ΐ", "ϋ": "ΰ",
    ]

    /// The vowel `before` ends with, accented, when the tonos key is typed after it; nil when
    /// the key types ´ itself.
    static func accenting(_ text: String, after before: String?) -> String? {
        guard text == tonosKey, let last = before?.last, let vowel = accented[last] else { return nil }
        return String(vowel)
    }

    /// What a letter key types: ς for σ, which ends the word until another letter follows.
    static func letter(_ text: String) -> String {
        text == "σ" ? "ς" : text
    }

    /// Whether typing `text` after `before` turns a final ς back into σ: another letter
    /// follows it.
    static func reopensFinalSigma(typing text: String, after before: String?) -> Bool {
        before?.last == "ς" && text.first?.isLetter == true
    }
}

@MainActor
extension FullKeyboardEngine {
    /// The Greek layout's tonos key and final sigma (`GreekTyping`): true when the tonos key
    /// accented the vowel before it, which is all it does; else `text` is what to type (ς for
    /// σ), with a final ς before it already back to σ.
    func typeGreek(_ text: inout String) -> Bool {
        let before = target.documentContextBeforeInput
        if let vowel = GreekTyping.accenting(text, after: before) {
            target.deleteBackward()
            target.insertText(vowel)
            didEdit()
            logState("typed")
            return true
        }
        if GreekTyping.reopensFinalSigma(typing: text, after: before) {
            target.deleteBackward()
            target.insertText("σ")
        }
        text = GreekTyping.letter(text)
        return false
    }
}
