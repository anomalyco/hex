import Foundation

/// An emoji picked from the strip (`KeyboardSuggestion.Kind.emoji`).
@MainActor
extension FullKeyboardEngine {
    /// Types `suggestion`'s emoji with its own space, as any suggestion: in place of the word
    /// being typed ("smile" → 😊, `FullKeyboardView.withTypedWordEmoji`), or after a finished
    /// word ("pizza " → "pizza 🍕 "). A word it took the place of comes back as an
    /// autocorrection's does: delete takes the space, and the strip's left slot offers the word
    /// (`offeringRevert`). Learn how I type learns nothing from it, neither a typing fix nor
    /// the taps (`TypingJournal`, `AdaptiveTouch`).
    func acceptEmoji(_ suggestion: KeyboardSuggestion) {
        let token = AutocorrectEngine.currentToken(in: textBefore)
        typingLog?.edit("suggestion", [
            TypingLogField("slot", suggestions.firstIndex(of: suggestion).map { .int($0) } ?? .null),
            TypingLogField("text", .string(suggestion.displayText)),
            TypingLogField("deleted", .string(token ?? "")),
            TypingLogField("inserted", .string(suggestion.replacement + " ")),
            TypingLogField("candidates", TypingLogger.candidates(suggestions)),
        ])
        journal?.didFinish(word: nil)
        adaptiveTouch?.forget()
        if let token {
            for _ in 0..<token.count {
                target.deleteBackward()
            }
        }
        insertPicked(suggestion.replacement)
        if let token {
            recordCorrection(original: token, corrected: suggestion.replacement, ending: " ", changes: [])
        }
    }
}
