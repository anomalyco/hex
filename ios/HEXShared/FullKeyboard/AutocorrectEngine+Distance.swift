import Foundation

/// Case matching (with the strip's and a fix's capitals, and the word before a token) and the
/// keyboard-distance cost of a one-edit typo: what cautious autocorrect, bilingual choices,
/// and the strip's spelling guesses measure a slip by. Kept beside the engine so its own file
/// stays under the cap.
@MainActor
extension AutocorrectEngine {
    // MARK: - Case matching & QWERTY distance

    static func matchCase(of typed: String, onto target: String) -> String {
        guard !typed.isEmpty, !target.isEmpty else { return target }
        if typed.count >= 2 && typed == typed.uppercased() && typed != typed.lowercased() {
            return target.uppercased()
        }
        // Preserve intentional casing in `I`, `I'm`, `iPhone`, etc. unless `typed` started capitalized.
        if target != target.lowercased() {
            if typed.first?.isUppercase == true, let first = target.first, first.isLowercase {
                return first.uppercased() + target.dropFirst()
            }
            return target
        }
        if typed.first?.isUppercase == true, let first = target.first {
            return first.uppercased() + target.dropFirst()
        }
        return target
    }

    /// `word` without a capital first letter it only has from where it was learned: a fix
    /// you picked at the start of a sentence ("Helli" to "Hello"), a guess iOS knows
    /// capitalised, a Contacts name that is also a common word ("Bell", "Will"). When `typed`
    /// starts in lowercase and the dictionary knows the word in lowercase, it is written in
    /// lowercase, as Apple's keyboard writes "bell" mid-sentence beside a contact Kate Bell
    /// (black box, iOS 27.2). Names the dictionary knows only capitalised ("Taylor"), "I" and
    /// its contractions, words with more capitals ("iPhone"), a word in your vocabulary spelled
    /// as you added it ("Swift") and German, whose nouns take capitals, keep theirs.
    func withoutStrayCapital(_ word: String, typed: String) -> String {
        guard language != .german, hasDictionary, typed.first?.isUppercase != true, word.count >= 2,
              let first = word.first, first.isUppercase, !word.dropFirst().contains(where: \.isUppercase),
              !word.contains(" "), !isVocabularySpelling(word) else { return word }
        let lower = first.lowercased() + word.dropFirst()
        if language.usesEnglishFixes, lower.hasPrefix("i'") || lower.hasPrefix("i\u{2019}") { return word }
        return checker.isWordValid(lower) ? lower : word
    }

    /// The word before `token` in `before`, lowercased, when only spaces separate them
    /// (nil at the start of a sentence or after punctuation).
    static func previousWord(_ before: String, token: String) -> String? {
        var rest = before.hasSuffix(token) ? before.dropLast(token.count) : Substring(before)
        guard rest.last == " " else { return nil }
        while rest.last == " " { rest = rest.dropLast() }
        var word = ""
        while let c = rest.last, c.isLetter || c == "'" {
            word.insert(c, at: word.startIndex)
            rest = rest.dropLast()
        }
        return word.isEmpty ? nil : word.lowercased()
    }

    /// The strip's words for `token` with Auto-capitalization off: no capital you didn't type,
    /// as space puts none in ("Mandy" and "I'd" come "mandy" and "i'd"), except your own
    /// spellings; a word that is then what you typed, or twice there, goes.
    func withoutAddedCapitals(_ pool: [String], typed token: String) -> [String] {
        guard !addsCapitals, !token.contains(where: \.isUppercase) else { return pool }
        return pool.map { isOwnSpelling($0) ? $0 : $0.lowercased() }.reduce(into: []) { kept, item in
            if item != token, !kept.contains(item) { kept.append(item) }
        }
    }

    /// Returns a weighted cost if `source` and `target` are within 1 Damerau-Levenshtein edit; `nil` otherwise.
    static func singleEditKeyboardCost(
        from source: String,
        to target: String,
        language: KeyboardLanguage = .english,
        letters: KeyboardLetterLayout? = nil
    ) -> Double? {
        if source == target { return 0 }
        let s = Array(source)
        let t = Array(target)
        // Polish: diacritics you didn't add, however many ("zolw" -> "żółw", "maly" -> "mały"),
        // are as cheap as one missing Spanish accent, a little more for each further one. Greek
        // likewise: a tonos or diaeresis left out ("καλημερα" -> "καλημέρα", "αυπνια" ->
        // "αϋπνία"), or σ where the word ends in ς ("λογοσ" -> "λόγος"). Czech too: háčky and
        // čárky left out ("cesky" -> "český", "prilis" -> "příliš"), and Hungarian: its accented
        // vowels left out ("koszonom" -> "köszönöm", "kesobb" -> "később", "szivesen" ->
        // "szívesen"), ő and ű included, as o and u with the combining double acute.
        if language == .polish || language == .greek || language == .czech || language == .hungarian,
           s.count == t.count {
            func same(_ a: Character, _ b: Character) -> Bool {
                switch language {
                case .greek: Self.sameGreekLetterIgnoringAccents(a, b)
                case .hungarian: Self.sameLetterIgnoringAccents(a, b)
                default: Self.samePolishLetterIgnoringDiacritics(a, b)
                }
            }
            let missed = zip(s, t).filter { $0 != $1 }
            if !missed.isEmpty, missed.allSatisfy({ same($0, $1) }) {
                return 0.15 + 0.05 * Double(missed.count - 1)
            }
        }
        let ds = s.count - t.count
        guard abs(ds) <= 1 else { return nil }

        if ds == 0 {
            var diffs: [Int] = []
            for i in 0..<s.count where s[i] != t[i] {
                diffs.append(i)
                if diffs.count > 2 { return nil }
            }
            if diffs.count == 1 {
                let i = diffs[0]
                // An accent or tilde you didn't add ("aqui" -> "aquí", "manana" -> "mañana")
                // is the cheapest fix there is, not a far-away key.
                if sameLetterIgnoringAccents(s[i], t[i]) { return 0.15 }
                return areAdjacentKeys(s[i], t[i], language: language, letters: letters) ? 0.65 : 1.15
            }
            if diffs.count == 2, diffs[1] == diffs[0] + 1,
               s[diffs[0]] == t[diffs[1]], s[diffs[1]] == t[diffs[0]] {
                return 0.60
            }
            return nil
        }

        // Single insertion or deletion.
        let (shorter, longer) = s.count < t.count ? (s, t) : (t, s)
        var i = 0
        var j = 0
        var skippedIndex: Int?
        while i < shorter.count && j < longer.count {
            if shorter[i] == longer[j] {
                i += 1
                j += 1
            } else if skippedIndex == nil {
                skippedIndex = j
                j += 1
            } else {
                return nil
            }
        }
        let extraIdx = skippedIndex ?? (longer.count - 1)
        let extraChar = longer[extraIdx]
        let leftAdj = extraIdx > 0 && areAdjacentKeys(extraChar, longer[extraIdx - 1], language: language, letters: letters)
        let rightAdj = extraIdx + 1 < longer.count && areAdjacentKeys(extraChar, longer[extraIdx + 1], language: language, letters: letters)
        return (leftAdj || rightAdj) ? 0.70 : 0.90
    }

    /// Whether two letters differ only by an accent, diaeresis or tilde (e/é, u/ü, n/ñ).
    static func sameLetterIgnoringAccents(_ a: Character, _ b: Character) -> Bool {
        guard a != b else { return false }
        let base: (Character) -> Unicode.Scalar? = { String($0).decomposedStringWithCanonicalMapping.unicodeScalars.first }
        return base(a) == base(b)
    }

    /// Whether two lowercase letters are one Polish letter with or without its diacritic (a/ą,
    /// z/ż, l/ł: ł has no decomposition, so it is named).
    static func samePolishLetterIgnoringDiacritics(_ a: Character, _ b: Character) -> Bool {
        sameLetterIgnoringAccents(a, b) || Set([a, b]) == ["l", "ł"]
    }

    /// Whether two lowercase letters are one Greek letter with or without its tonos or
    /// diaeresis (α/ά, ι/ϊ/ΐ), or σ and its word-final form ς.
    static func sameGreekLetterIgnoringAccents(_ a: Character, _ b: Character) -> Bool {
        sameLetterIgnoringAccents(a, b) || Set([a, b]) == ["σ", "ς"]
    }

    private struct LayoutKey: Hashable {
        let language: KeyboardLanguage
        let letters: KeyboardLetterLayout
    }

    private static let keyCoordinates: [LayoutKey: [Character: (Double, Double)]] = Dictionary(
        uniqueKeysWithValues: KeyboardLanguage.allCases.flatMap { language in
            language.letterLayouts.map { (LayoutKey(language: language, letters: $0), language.keyCoordinates($0)) }
        }
    )

    static func areAdjacentKeys(
        _ a: Character,
        _ b: Character,
        language: KeyboardLanguage = .english,
        letters: KeyboardLetterLayout? = nil
    ) -> Bool {
        let key = LayoutKey(language: language, letters: language.offered(letters))
        guard let keys = keyCoordinates[key], let pa = keys[a], let pb = keys[b] else { return false }
        let dx = pa.1 - pb.1
        let dy = pa.0 - pb.0
        return (dx * dx + dy * dy).squareRoot() <= 1.6
    }
}
