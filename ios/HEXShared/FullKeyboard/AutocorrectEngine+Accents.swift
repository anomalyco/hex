import Foundation

/// Bilingual autocorrect's second look at the word before, once the next word says which
/// language it was in, and a language's own letters put back on a word typed without them.
/// Kept beside the engine so its own file stays under the cap.
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
        return withoutAddedCapitals(Self.matchCase(of: word, onto: accented), typed: word).map { ($0, into.language) }
    }
}

@MainActor
extension AutocorrectEngine {
    /// The letters a language writes with a mark on a plain key, by that key: Polish's ą ć ę ł
    /// ń ó ś ź ż, Czech's háčky and čárky, Hungarian's accented vowels, Romanian's ă â î ș ț
    /// (comma below, as its keys type them), French's and Portuguese's accents and Norwegian's
    /// å and ø. Words of these languages often need two or three of them, which
    /// `UITextChecker`'s guesses rarely restore ("zolw" gets no "żółw", "deja" no "déjà",
    /// "amanha" no "amanhã"). Empty for English, Spanish, German, Italian and the rest, whose
    /// guesses carry their one accent.
    static func markedLetters(_ language: KeyboardLanguage) -> [Character: [Character]] {
        switch language {
        case .polish:
            ["a": ["ą"], "c": ["ć"], "e": ["ę"], "l": ["ł"], "n": ["ń"], "o": ["ó"], "s": ["ś"], "z": ["ż", "ź"]]
        case .czech:
            ["a": ["á"], "c": ["č"], "d": ["ď"], "e": ["é", "ě"], "i": ["í"], "n": ["ň"], "o": ["ó"], "r": ["ř"],
             "s": ["š"], "t": ["ť"], "u": ["ú", "ů"], "y": ["ý"], "z": ["ž"]]
        case .hungarian:
            ["a": ["á"], "e": ["é"], "i": ["í"], "o": ["ó", "ö", "ő"], "u": ["ú", "ü", "ű"]]
        case .romanian:
            ["a": ["ă", "â"], "i": ["î"], "s": ["ș"], "t": ["ț"]]
        case .french, .frenchCanadian, .frenchSwiss:
            ["a": ["à", "â"], "c": ["ç"], "e": ["é", "è", "ê", "ë"], "i": ["î", "ï"], "o": ["ô"], "u": ["ù", "û", "ü"]]
        case .portuguese:
            ["a": ["á", "â", "ã", "à"], "c": ["ç"], "e": ["é", "ê"], "i": ["í"], "o": ["ó", "ô", "õ"], "u": ["ú"]]
        case .norwegian:
            ["a": ["å"], "o": ["ø"]]
        default:
            [:]
        }
    }

    /// Spellings of `lower` with marks added to letters typed plain (`markedLetters`), fewest
    /// marks first; none when there are more than `limit` (a long word of marked vowels).
    static func markedSpellings(of lower: String, language: KeyboardLanguage, limit: Int = 256) -> [String] {
        let marks = markedLetters(language)
        let letters = Array(lower)
        let slots = letters.indices.filter { marks[letters[$0]] != nil }
        let count = slots.reduce(1) { $0 * (marks[letters[$1]]!.count + 1) }
        guard !slots.isEmpty, count - 1 <= limit else { return [] }
        var spellings: [(word: String, marks: Int)] = []
        for n in 1..<count {
            var rest = n
            var word = letters
            var added = 0
            for slot in slots {
                let options = marks[letters[slot]]!
                let pick = rest % (options.count + 1)
                rest /= options.count + 1
                if pick > 0 { word[slot] = options[pick - 1]; added += 1 }
            }
            spellings.append((String(word), added))
        }
        return spellings.sorted { $0.marks < $1.marks }.map(\.word)
    }

    /// The words of this language's dictionary that `token` spells once its own letters are put
    /// back (`markedSpellings`, at most 48 tried for a partner), lowercase: those the dictionary's guesses list first, in their
    /// order, then the rest, fewest marks first. Empty without such letters, or when the
    /// dictionary takes any letters for a word (iOS lists Polish, Czech, Hungarian, Romanian and
    /// Norwegian with no word data until their keyboard's data is on the iPhone: the iOS 27.2 simulator,
    /// with or without Apple's keyboard for them), so a missing dictionary guesses nothing.
    /// `ranked`: how many come from the guesses.
    func markedWords(for token: String) -> (words: [String], ranked: Int) {
        let lower = token.lowercased()
        guard token.count >= 2, token.count <= 24, !token.contains("'"), !Self.markedLetters(language).isEmpty else { return ([], 0) }
        // One keystroke asks for the same word up to three times (space's fix, the strip, a
        // partner's turn): the last few answers are kept for the dictionary that gave them.
        if let hit = Self.markedCache.first(where: { $0.checker === checker && $0.language == language && $0.lower == lower }) {
            return hit.result
        }
        var result: (words: [String], ranked: Int) = ([], 0)
        if !dictionaryTakesAnything {
            // On another language's keys (a bilingual partner) every word typed is tried, so fewer.
            let limit = layoutLanguage == language ? 256 : 48
            let words = Self.markedSpellings(of: lower, language: language, limit: limit).filter { checker.isWordValid($0) }
            if words.count > 1 {
                let ranked = checker.guesses(for: lower).map { $0.lowercased() }.filter(words.contains)
                result = (ranked + words.filter { !ranked.contains($0) }, ranked.count)
            } else {
                result = (words, 0)
            }
        }
        Self.markedCache.insert(MarkedAnswer(checker: checker, language: language, lower: lower, result: result), at: 0)
        if Self.markedCache.count > 8 { Self.markedCache.removeLast() }
        return result
    }

    private struct MarkedAnswer {
        weak var checker: TextChecking?
        let language: KeyboardLanguage
        let lower: String
        let result: (words: [String], ranked: Int)
    }

    private static var markedCache: [MarkedAnswer] = []

    /// Whether this language's dictionary takes a made-up word: then it is no dictionary at all.
    var dictionaryTakesAnything: Bool {
        checker.isWordValid("qzxjvkq")
    }

    /// What space puts in for `token` typed without its language's own letters ("zolw" ->
    /// "żółw", "prosze" -> "proszę", "sa" -> "są"), from the dictionary alone: only when the
    /// plain spelling is no word of yours or of either language, and only one word fits or the
    /// dictionary's guesses rank one first. Several unranked ("dziekuje": dziękuje, dziękuję)
    /// are offered in the strip instead (`suggestedMarkedWord`). Nil otherwise.
    func restoredMarkedWord(for token: String) -> String? {
        guard !Self.markedLetters(language).isEmpty, !knowsAsTyped(token), workingPartner?.knowsAsTyped(token) != true else { return nil }
        let (words, ranked) = markedWords(for: token)
        return words.count == 1 || ranked > 0 ? words.first : nil
    }

    /// The strip's word with this language's own letters put back on `token` when space doesn't
    /// put one in: a word that is one too without them ("ze" offers "że", "sad" "sąd"), or one
    /// of several that fit. Never put in by space. Not while typing on another language's keys
    /// (a bilingual partner): checking every prefix there doubled English's per-letter time
    /// beside French, and space still puts the partner's letters back.
    func suggestedMarkedWord(for token: String) -> String? {
        layoutLanguage == language ? markedWords(for: token).words.first : nil
    }

    /// `ownReplacement` with bilingual autocorrect's partner chosen but without a working
    /// dictionary (`workingPartner` nil: its keyboard's data isn't on the iPhone): a word typed
    /// with the partner's own letters ("żółw", "łódź" beside English) is the partner's, never
    /// this language's typo, so it stays as typed. Plain letters can't be told apart, and this
    /// language's typos are still fixed ("teh").
    func replacementBesideSilentPartner(for token: String) -> String? {
        if let partner, partner.language != language, !knowsAsTyped(token),
           token.lowercased().contains(where: Set(Self.markedLetters(partner.language).values.joined()).contains) {
            return nil
        }
        return ownReplacement(for: token)
    }
}
