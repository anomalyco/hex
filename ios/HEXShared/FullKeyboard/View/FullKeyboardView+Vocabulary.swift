import UIKit

/// Add to vocabulary from the strip: hold the word you typed (the strip's left slot) and the
/// strip offers it; the chip adds it to Hex's vocabulary list (`CustomVocabularySettings`,
/// the one Settings › Keyboard › Vocabulary and Settings › Dictation edit), which the keys
/// then never autocorrect away, complete and swipe.
@MainActor
extension FullKeyboardView {
    func offerVocabulary(_ suggestion: KeyboardSuggestion) {
        var word = suggestion.replacement
        // A capital only the sentence's start gave it ("Kubectl" opening a message) isn't its own.
        if let first = word.first, first.isUppercase, !word.dropFirst().contains(where: \.isUppercase),
           AutocorrectEngine.startsSentence(before: engine.target.documentContextBeforeInput ?? "", token: word) {
            word = first.lowercased() + word.dropFirst()
        }
        guard suggestion.kind == .keepLiteral, engine.autocorrect.vocabularyTerm(for: word) == nil,
              VocabularySpelling.normalizedKey(word).count >= 2 else { return }
        typingLog?.ui("vocabulary_offer")
        selectionFeedback()
        vocabularyOffer = word
        refreshStrip()
    }

    var vocabularyNotice: KeyboardStripNotice? {
        guard let word = vocabularyOffer else { return nil }
        return KeyboardStripNotice(
            title: "“\(word)”",
            detail: String(localized: "Never autocorrected"),
            button: String(localized: "Add to vocabulary")
        )
    }

    /// Saves `word` in the App Group (kept for this session too where the keyboard can't
    /// write there, without Full Access) and uses it at once.
    func addToVocabulary(_ word: String) {
        typingLog?.ui("vocabulary_add")
        let settings = CustomVocabularySettings()
        let terms = CustomVocabularySettings.sanitize(settings.terms + engine.autocorrect.vocabulary + [word])
        settings.terms = terms
        engine.autocorrect.setVocabulary(terms)
        vocabularyOffer = nil
        engine.refreshSuggestions()
        refreshStrip()
    }
}
