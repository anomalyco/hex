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
    /// marks first; none when there are more than `limit` (a long word of marked vowels), or
    /// with `partial` the `limit` with the fewest marks. Made in that order, never more.
    static func markedSpellings(of lower: String, language: KeyboardLanguage, limit: Int = 256, partial: Bool = false) -> [String] {
        let marks = markedLetters(language)
        let letters = Array(lower)
        let slots = letters.indices.filter { marks[letters[$0]] != nil }
        guard !slots.isEmpty, partial || markedSpellingCount(of: lower, language: language) <= limit else { return [] }
        var spellings: [String] = []
        var word = letters
        // Every spelling with exactly `left` more marks, on slots from `from` on.
        func add(from: Int, left: Int) {
            guard spellings.count < limit else { return }
            if left == 0 { spellings.append(String(word)); return }
            guard slots.count - from >= left else { return }
            for i in from..<slots.count {
                let slot = slots[i]
                for option in marks[letters[slot]]! {
                    word[slot] = option
                    add(from: i + 1, left: left - 1)
                    word[slot] = letters[slot]
                    if spellings.count >= limit { return }
                }
            }
        }
        for count in 1...slots.count where spellings.count < limit { add(from: 0, left: count) }
        return spellings
    }

    /// How many spellings `markedSpellings` would make of `lower` without a limit.
    static func markedSpellingCount(of lower: String, language: KeyboardLanguage) -> Int {
        let marks = markedLetters(language)
        return lower.reduce(1) { $0 * ((marks[$1]?.count ?? 0) + 1) } - 1
    }

    /// The words of this language's dictionary that `token` spells once its own letters are put
    /// back (`markedSpellings`), lowercase: those the dictionary's guesses list first, in their
    /// order, then the rest, fewest marks first. Empty without such letters, or when the
    /// dictionary takes any letters for a word (iOS lists Polish, Czech, Hungarian, Romanian and
    /// Norwegian with no word data until their keyboard's data is on the iPhone: the iOS 27.2
    /// simulator, with or without Apple's keyboard for them), so a missing dictionary guesses
    /// nothing. Space's fix (`checkingWholeWord`) tries up to 256 spellings, 48 for a partner;
    /// each letter typed tries only the 8 with the fewest marks and leaves the ranking to space, as
    /// French took 2.9 ms -> 7.5 ms a letter at the 95th percentile (iPhone 17 simulator) and up to
    /// 252 lookups trying them all.
    /// `ranked`: how many come from the guesses.
    func markedWords(for token: String) -> (words: [String], ranked: Int) {
        let lower = token.lowercased()
        guard token.count >= 2, token.count <= 24, !token.contains("'"), !Self.markedLetters(language).isEmpty else { return ([], 0) }
        let whole = Self.checksWholeWord
        // One keystroke asks for the same word up to three times (space's fix, the strip, a
        // partner's turn): the last few answers are kept for the dictionary that gave them. A
        // letter's answer serves space only when it tried every spelling.
        if let hit = Self.markedCache.first(where: {
            $0.checker === checker && $0.language == language && $0.lower == lower && (!whole || $0.whole)
        }) {
            return hit.result
        }
        let letterLimit = 8
        let covers = whole || Self.markedSpellingCount(of: lower, language: language) <= letterLimit
        var result: (words: [String], ranked: Int) = ([], 0)
        if !dictionaryTakesAnything {
            let limit = whole ? (layoutLanguage == language ? 256 : 48) : letterLimit
            let words = Self.markedSpellings(of: lower, language: language, limit: limit, partial: !whole).filter { checker.isWordValid($0) }
            // Ranking by the guesses, a slow call, waits for space's fix.
            if words.count > 1, whole {
                let ranked = checker.guesses(for: lower).map { $0.lowercased() }.filter(words.contains)
                result = (ranked + words.filter { !ranked.contains($0) }, ranked.count)
            } else {
                result = (words, 0)
            }
        }
        let answer = MarkedAnswer(checker: checker, language: language, lower: lower, whole: covers, result: result)
        Self.markedCache.insert(answer, at: 0)
        if Self.markedCache.count > 8 { Self.markedCache.removeLast() }
        return result
    }

    /// Runs `body` (space's fix) trying every marked spelling of the word, not only a letter's few.
    func checkingWholeWord<T>(_ body: () -> T) -> T {
        Self.checksWholeWord = true
        defer { Self.checksWholeWord = false }
        return body()
    }

    private static var checksWholeWord = false

    private struct MarkedAnswer {
        weak var checker: TextChecking?
        let language: KeyboardLanguage
        let lower: String
        /// Every spelling space would try was tried.
        let whole: Bool
        let result: (words: [String], ranked: Int)
    }

    private static var markedCache: [MarkedAnswer] = []

    /// Whether this language's dictionary takes a made-up word: then it is no dictionary at all.
    /// Asked once per dictionary and language.
    var dictionaryTakesAnything: Bool {
        if let known = Self.takesAnything.first(where: { $0.checker === checker && $0.language == language }) { return known.answer }
        let answer = checker.isWordValid("qzxjvkq")
        Self.takesAnything = [(checker, language, answer)] + Self.takesAnything.filter { $0.checker != nil }.prefix(3)
        return answer
    }

    private static var takesAnything: [(checker: TextChecking?, language: KeyboardLanguage, answer: Bool)] = []

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
    /// put one in: one of several that fit, or for a two-letter word that is one too without
    /// them, its marked twin ("ze" offers "że", "ou" "où"); longer words the dictionary knows as
    /// typed aren't looked at, which kept each letter's lookups down. Never put in by space. Not
    /// while typing on another language's keys (a bilingual partner): checking every prefix there
    /// doubled English's per-letter time beside French, and space still puts the partner's
    /// letters back.
    func suggestedMarkedWord(for token: String) -> String? {
        guard layoutLanguage == language, token.count <= 2 || !knowsAsTyped(token) else { return nil }
        return markedWords(for: token).words.first
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
