import Foundation

/// Bilingual autocorrect's second look at the word before, once the next word says which
/// language it was in. Kept beside the engine so its own file stays under the cap.
@MainActor
extension AutocorrectEngine {
    /// `word`, kept as typed because only one of the two languages knows it ("voce" and "ate"
    /// are English words), respelled with the other language's accents once `next` leans that
    /// one ("voce esta" -> "você esta", "ate amanha" -> "até amanhã"), as Apple's keyboard fixes
    /// the word before at the next one. Only a guess of that dictionary's that adds accents
    /// alone, and not when the words before `word` lean the other way. With the language it
    /// is in, or nil to leave it.
    func accentRevision(of word: String, before: String, next: String) -> (String, KeyboardLanguage)? {
        let lower = word.lowercased()
        guard let partner = workingPartner, word.count >= 2, next.count >= 2, !word.contains("'"),
              !suppressedInSession.contains(lower), !learning.rejected.contains(lower) else { return nil }
        let lean = languageLean(partner, before: next, commonFrom: 2)
        let rest = before.hasSuffix(word) ? String(before.dropLast(word.count)) : before
        guard lean != 0, languageLean(partner, before: rest) * lean >= 0 else { return nil }
        let (into, other) = lean > 0 ? (partner, self) : (self, partner)
        guard other.knowsAsTyped(word), !into.knowsAsTyped(word) else { return nil }
        // Typed without accents, and a lowercase guess that only adds them (never a name's capital).
        func plain(_ s: String) -> String { s.folding(options: .diacriticInsensitive, locale: nil) }
        guard plain(lower) == lower, let accented = into.checker.guesses(for: lower).first(where: {
            $0 != lower && $0 == $0.lowercased() && plain($0) == lower
        }), into.mayOffer(accented, after: Self.previousWord(before, token: word)) else { return nil }
        return withoutAddedCapitals(matchCase(of: word, onto: accented), typed: word).map { ($0, into.language) }
    }
}
