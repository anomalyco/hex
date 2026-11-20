import UIKit

/// One slot in the 3-column suggestion strip above the keys.
nonisolated struct KeyboardSuggestion: Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        /// Left slot (`"typed"`): keeps the exact characters and teaches Hex not to rewrite them.
        case keepLiteral
        /// Center slot: best replacement or completion (`autoReplace` highlights when Space will apply it).
        case primary(autoReplace: Bool)
        /// Right slot: runner-up completion or spelling guess.
        case secondary
        /// An emoji for the word just finished (Experiments › Emoji), offered after a space,
        /// when the strip has no words to show.
        case emoji
    }

    let displayText: String
    let replacement: String
    let kind: Kind

    var willAutoReplace: Bool {
        if case .primary(let auto) = kind { return auto }
        return false
    }
}

/// Narrow seam over `UITextChecker` so unit tests can run deterministically or against UIKit's dictionary.
@MainActor
protocol TextChecking: AnyObject {
    func isWordValid(_ word: String) -> Bool
    func guesses(for word: String) -> [String]
    func completions(for prefix: String) -> [String]
    /// Switches the dictionary (a `UITextChecker` language code such as `nb`).
    func useLanguage(_ code: String)
}

extension TextChecking {
    func useLanguage(_ code: String) {}
}

@MainActor
final class SystemTextChecker: TextChecking {
    private var checker: UITextChecker?
    private var language: String

    init(language: String = "en_US") {
        self.language = language
    }

    func useLanguage(_ code: String) {
        language = code
    }

    /// Warms `UITextChecker` after the keyboard's first frame commits so cold open stays < 12 ms.
    func warmUp() {
        guard checker == nil else { return }
        let instance = UITextChecker()
        _ = instance.rangeOfMisspelledWord(
            in: "hello",
            range: NSRange(location: 0, length: 5),
            startingAt: 0,
            wrap: false,
            language: language
        )
        checker = instance
    }

    private var activeChecker: UITextChecker {
        if let checker { return checker }
        let instance = UITextChecker()
        checker = instance
        return instance
    }

    func isWordValid(_ word: String) -> Bool {
        let ns = word as NSString
        let range = NSRange(location: 0, length: ns.length)
        guard range.length > 0 else { return true }
        let misspelled = activeChecker.rangeOfMisspelledWord(
            in: word,
            range: range,
            startingAt: 0,
            wrap: false,
            language: language
        )
        return misspelled.location == NSNotFound
    }

    func guesses(for word: String) -> [String] {
        let ns = word as NSString
        let range = NSRange(location: 0, length: ns.length)
        guard range.length > 0 else { return [] }
        return activeChecker.guesses(forWordRange: range, in: word, language: language) ?? []
    }

    func completions(for prefix: String) -> [String] {
        let ns = prefix as NSString
        let range = NSRange(location: 0, length: ns.length)
        guard range.length > 0 else { return [] }
        return activeChecker.completions(forPartialWordRange: range, in: prefix, language: language) ?? []
    }
}

/// Persists words the user explicitly kept twice, bounded to 2,000 words / 64 KB in the App Group.
@MainActor
final class LearnedWordsStore {
    static let defaultsKey = "keyboard.learnedWords"
    static let maxWords = 2_000
    static let maxBytes = 64 * 1024

    private let defaults: UserDefaults?
    private(set) var words: [String] = []
    private var wordSet: Set<String> = []
    private var sessionKeepCounts: [String: Int] = [:]

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
        load()
    }

    func contains(_ word: String) -> Bool {
        wordSet.contains(word.lowercased())
    }

    /// Records an explicit keep (via `"quoted"` chip or Backspace-revert). Two keeps persist the word.
    func recordExplicitKeep(_ raw: String) {
        let key = raw.lowercased()
        guard key.count >= 2, key.count <= 40 else { return }
        let count = (sessionKeepCounts[key] ?? 0) + 1
        sessionKeepCounts[key] = count
        if count >= 2 {
            learn(key)
        }
    }

    func learn(_ raw: String) {
        let key = raw.lowercased()
        guard !key.isEmpty, !wordSet.contains(key) else { return }
        words.append(key)
        wordSet.insert(key)
        while words.count > Self.maxWords {
            let evicted = words.removeFirst()
            wordSet.remove(evicted)
        }
        save()
    }

    private func load() {
        guard let data = defaults?.data(forKey: Self.defaultsKey),
              data.count <= Self.maxBytes,
              let decoded = try? JSONDecoder().decode([String].self, from: data) else { return }
        words = Array(decoded.suffix(Self.maxWords))
        wordSet = Set(words)
    }

    private func save() {
        guard let defaults else { return }
        while !words.isEmpty {
            if let data = try? JSONEncoder().encode(words), data.count <= Self.maxBytes {
                defaults.set(data, forKey: Self.defaultsKey)
                return
            }
            let evicted = words.removeFirst()
            wordSet.remove(evicted)
        }
    }
}

/// Cautious on-device suggestions and replace-on-space engine combining `UILexicon`,
/// deterministic contractions/transpositions, QWERTY-ranked `UITextChecker`, and `LearnedWordsStore`.
@MainActor
final class AutocorrectEngine {
    struct AppliedCorrection: Equatable {
        let original: String
        let insertedText: String
    }

    let checker: TextChecking
    let learnedWords: LearnedWordsStore
    var isEnabled = true
    /// The keyboard layout's language: its dictionary, and whether English fixes apply.
    private(set) var language: KeyboardLanguage = .english
    /// False when the iPhone has no spelling dictionary for `language`: then nothing is
    /// suggested or corrected but your own text replacements, rather than "correcting"
    /// the language into another.
    private(set) var hasDictionary = true
    private var lexiconShortcuts: [String: String] = [:]
    private var lexiconTerms: [String] = []
    private var suppressedInSession: Set<String> = []
    /// Learn how I type: typos you fixed the same way twice and words you turned down twice.
    private(set) var learning = TypingLearning()
    private(set) var lastCorrection: AppliedCorrection?
    /// Experiment (Spatial autocorrect): reads the word's taps with a language model in place
    /// of `UITextChecker`'s one-edit guesses, for the language its model was built for. Nil
    /// while off or without a model.
    var decoder: TypingDecoder? {
        didSet { if oldValue?.model !== decoder?.model { userWordsBuilt = nil } }
    }
    /// The letter keys for `decoder`, from the current layout.
    var decoderKeys: TypingDecoder.Keys?
    /// Where each letter of the word being typed was tapped, one point per letter; nil when
    /// the keys don't know (the cursor moved, the text came from elsewhere).
    var decoderTaps: [CGPoint]?
    private var beforeInput: String?
    /// Bumped whenever a source of your own words changes, so the decoder's copy is rebuilt.
    private var userWordsRevision = 0
    private var userWordsBuilt: (revision: Int, model: ObjectIdentifier)?
    private var decoderCache: (before: String, taps: [CGPoint], decision: TypingDecoder.Decision?)?

    init(
        checker: TextChecking? = nil,
        learnedWords: LearnedWordsStore? = nil
    ) {
        self.checker = checker ?? SystemTextChecker()
        self.learnedWords = learnedWords ?? LearnedWordsStore()
    }

    func loadLexicon(_ lexicon: UILexicon) {
        loadLexiconEntries(lexicon.entries.map { ($0.userInput, $0.documentText) })
    }

    func loadLexiconEntries(_ entries: [(userInput: String, documentText: String)]) {
        var shortcuts: [String: String] = [:]
        var terms: [String] = []
        for (input, output) in entries {
            let trimmedInput = input.trimmingCharacters(in: .whitespacesAndNewlines)
            let trimmedOutput = output.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmedInput.isEmpty, !trimmedOutput.isEmpty else { continue }
            if trimmedInput.lowercased() != trimmedOutput.lowercased() {
                shortcuts[trimmedInput.lowercased()] = trimmedOutput
            } else {
                terms.append(trimmedOutput)
            }
        }
        lexiconShortcuts = shortcuts
        lexiconTerms = terms
        userWordsRevision += 1
    }

    func clearPendingUndo() {
        lastCorrection = nil
    }

    func recordAppliedCorrection(original: String, insertedText: String) {
        lastCorrection = AppliedCorrection(original: original, insertedText: insertedText)
    }

    func consumePendingUndo() -> AppliedCorrection? {
        defer { lastCorrection = nil }
        return lastCorrection
    }

    /// Follows the keyboard layout. `available` is `UITextChecker.availableLanguages`.
    func setLanguage(_ language: KeyboardLanguage, available: [String] = UITextChecker.availableLanguages) {
        self.language = language
        let code = language.spellCheckLanguage(available: available)
        hasDictionary = code != nil
        if let code {
            checker.useLanguage(code)
        }
        suppressedInSession.removeAll()
        lastCorrection = nil
    }

    func applyLearning(_ learning: TypingLearning) {
        if self.learning != learning {
            self.learning = learning
            userWordsRevision += 1
        }
    }

    func suppressAndKeep(_ word: String) {
        suppressedInSession.insert(word.lowercased())
        learnedWords.recordExplicitKeep(word)
        userWordsRevision += 1
    }

    // MARK: - Token inspection

    /// Extracts the active word right before the caret, returning `nil` inside URLs, handles, paths, or numbers.
    static func currentToken(in beforeInput: String?) -> String? {
        guard let beforeInput, !beforeInput.isEmpty else { return nil }
        var end = beforeInput.endIndex
        var start = end
        while start > beforeInput.startIndex {
            let prev = beforeInput.index(before: start)
            let ch = beforeInput[prev]
            if ch.isLetter || (ch == "'" && start < end && prev > beforeInput.startIndex && beforeInput[beforeInput.index(before: prev)].isLetter) {
                start = prev
            } else {
                break
            }
        }
        guard start < end else { return nil }
        // Guard against tokens attached to `@`, `/`, `.`, `#`, `_`, `-`, or digits.
        if start > beforeInput.startIndex {
            let boundary = beforeInput[beforeInput.index(before: start)]
            if boundary == "@" || boundary == "/" || boundary == "." || boundary == "#"
                || boundary == "_" || boundary == "-" || boundary == ":" || boundary.isNumber {
                return nil
            }
        }
        let token = String(beforeInput[start..<end])
        guard token.first?.isLetter == true else { return nil }
        return token
    }

    // MARK: - Suggestions & Replace-on-Space

    func suggestions(
        beforeInput: String?,
        field: KeyboardFieldKind,
        autocorrectionType: UITextAutocorrectionType?
    ) -> [KeyboardSuggestion] {
        guard field == .text, autocorrectionType != .no, hasDictionary else { return [] }
        guard let token = Self.currentToken(in: beforeInput) else { return [] }
        self.beforeInput = beforeInput

        let lower = token.lowercased()
        let allowAuto = isEnabled
            && !suppressedInSession.contains(lower)
            && !learning.rejected.contains(lower)
            && !learnedWords.contains(lower)
            && !Self.shorthandAllowlist.contains(lower)

        let autoFix = allowAuto ? cautiousReplacement(for: token) : nil
        var pool: [String] = []

        if let autoFix {
            pool.append(autoFix)
        } else if let learned = learning.typos[lower] {
            // A learned fix space won't apply (the typo is a real word, or you kept it) is still offered.
            pool.append(Self.matchCase(of: token, onto: learned))
        } else if language.usesEnglishFixes, let suggested = Self.suggestOnlyContractions[lower] {
            pool.append(Self.matchCase(of: token, onto: suggested))
        }

        // Spatial autocorrect's other readings of your taps.
        if let decision = decoderDecision(for: token) {
            for reading in decision.readings where pool.count < 2 {
                let cased = Self.matchCase(of: token, onto: reading.text)
                if !pool.contains(where: { $0.caseInsensitiveCompare(cased) == .orderedSame }) {
                    pool.append(cased)
                }
            }
        }

        // Spelling guesses for a word the dictionary doesn't know.
        if pool.count < 2 && token.count >= 2 {
            let valid = Self.shorthandAllowlist.contains(lower) || learnedWords.contains(lower) || checker.isWordValid(token)
            if !valid {
                for guess in rankedGuesses(for: token) {
                    let cased = Self.matchCase(of: token, onto: guess)
                    if cased.caseInsensitiveCompare(token) != .orderedSame,
                       !pool.contains(where: { $0.caseInsensitiveCompare(cased) == .orderedSame }) {
                        pool.append(cased)
                    }
                    if pool.count >= 2 { break }
                }
            }
        }

        // Completions of what you're typing, the likeliest first.
        if pool.count < 2 {
            for item in rankedCompletions(for: token)
            where item != token && !pool.contains(where: { $0.caseInsensitiveCompare(item) == .orderedSame }) {
                pool.append(item)
                if pool.count >= 2 { break }
            }
        }

        let literal = KeyboardSuggestion(displayText: "“\(token)”", replacement: token, kind: .keepLiteral)
        // Nothing better than what you typed: the strip shows it once, not again in the centre.
        guard let first = pool.first else { return [literal] }
        let primary = KeyboardSuggestion(
            displayText: first,
            replacement: first,
            kind: .primary(autoReplace: autoFix != nil)
        )
        let secondary: KeyboardSuggestion? = if pool.count >= 2 {
            KeyboardSuggestion(displayText: pool[1], replacement: pool[1], kind: .secondary)
        } else {
            nil
        }
        return [literal, primary] + (secondary.map { [$0] } ?? [])
    }

    /// Words that start with `token`, the likeliest first, cased like `token`.
    ///
    /// `UITextChecker` already lists completions most probable first (its documented order,
    /// and on iOS 27 plainly by frequency: "b" gives but, be, because), so that order is
    /// kept. Your own words from `UILexicon` (Contacts names, text replacements) come
    /// after the dictionary's, and a capitalised name comes last of all unless you typed a
    /// capital, so "th" offers "the" and "they", not a contact called Thompson.
    private func rankedCompletions(for token: String) -> [String] {
        let lower = token.lowercased()
        let typedCapital = token.first?.isUppercase == true
        let isName: (String) -> Bool = { $0.first?.isUppercase == true }
        var seen: Set<String> = [lower]
        var common: [String] = []
        var yours: [String] = []
        var names: [String] = []
        for item in checker.completions(for: lower)
        where item.count > token.count && item.count <= token.count + 8 && seen.insert(item.lowercased()).inserted {
            if isName(item) && !typedCapital { names.append(item) } else { common.append(item) }
        }
        for term in lexiconTerms
        where term.count > token.count && term.lowercased().hasPrefix(lower) && seen.insert(term.lowercased()).inserted {
            if isName(term) && !typedCapital { names.append(term) } else { yours.append(term) }
        }
        return (common + yours + names).map { Self.matchCase(of: token, onto: $0) }
    }

    /// Returns `(tokenToDelete, replacement)` only when cautious autocorrect is confident.
    func replacementOnSpace(
        beforeInput: String?,
        field: KeyboardFieldKind,
        autocorrectionType: UITextAutocorrectionType?
    ) -> (original: String, replacement: String)? {
        guard isEnabled, field == .text, autocorrectionType != .no else { return nil }
        guard let token = Self.currentToken(in: beforeInput) else { return nil }
        self.beforeInput = beforeInput
        let lower = token.lowercased()
        guard !suppressedInSession.contains(lower),
              !learning.rejected.contains(lower),
              !learnedWords.contains(lower),
              !Self.shorthandAllowlist.contains(lower) else { return nil }
        guard hasDictionary else {
            return lexiconShortcuts[lower].map { (token, $0) }
        }
        guard let fix = cautiousReplacement(for: token) else { return nil }
        return (token, fix)
    }

    private func cautiousReplacement(for token: String) -> String? {
        let lower = token.lowercased()
        // 1. Explicit iOS Text Replacement (`omw` -> `On my way!`).
        if let shortcut = lexiconShortcuts[lower] {
            return shortcut
        }
        // A typo you fixed the same way at least twice (Learn how I type), unless what you
        // typed is a real word: then it is only suggested.
        if let learned = learning.typos[lower], !checker.isWordValid(token) {
            return Self.matchCase(of: token, onto: learned)
        }
        // Leave ALL-CAPS acronyms (`API`, `PR`, `NYC`) alone unless matched in `UILexicon` above,
        // except unambiguous all-caps contractions/typos of 3+ letters (`DONT` -> `DON'T`, `TEH` -> `THE`).
        let isAllCaps = token.count >= 2 && token == token.uppercased() && token != token.lowercased()
        // Leave camelCase / PascalCase identifiers (`HexKey`, `iOS`) alone.
        let hasInternalCaps = token.dropFirst().contains(where: \.isUppercase) && !isAllCaps
        if hasInternalCaps { return nil }

        // 2. Deterministic contractions and common QWERTY transpositions (English only:
        // Norwegian "i" means "in" and must not become "I").
        if language.usesEnglishFixes, let fixed = Self.unambiguousFixes[lower] {
            if isAllCaps && token.count < 3 { return nil }
            return Self.matchCase(of: token, onto: fixed)
        }

        if isAllCaps { return nil }

        // Spatial autocorrect, when on, decides alone: your taps, the word before, and a margin.
        if let decision = decoderDecision(for: token) {
            if let replacement = decision.replacement {
                return Self.matchCase(of: token, onto: replacement)
            }
            // A name, day or place its lowercase word list lacks, typed in lowercase, is
            // capitalised as the dictionary path does ("mary" -> "Mary").
            return properNoun(for: token)
        }

        // 3. A two-letter non-word one neighbouring key from a very common two-letter word
        // ("tk" -> "to", Spanish "nl" -> "no"); `UITextChecker` guesses skip two letters.
        if token.count == 2, !token.contains("'"), let fix = commonTwoLetterFix(for: lower),
           !checker.isWordValid(token) {
            return Self.matchCase(of: token, onto: fix)
        }

        // 4. Single-edit `UITextChecker` typo on words of 3+ characters, never rewriting valid words.
        guard token.count >= 3, !token.contains("'") else { return nil }
        guard !checker.isWordValid(token) else { return nil }

        var bestGuess: String?
        var bestScore = Double.greatestFiniteMagnitude
        let guesses = checker.guesses(for: token)
        for (rank, checkerGuess) in guesses.prefix(8).enumerated() {
            guard let rawGuess = usableGuess(checkerGuess, typed: token, among: guesses) else { continue }
            let normalizedGuess = rawGuess.replacingOccurrences(of: "'", with: "").lowercased()
            guard let editCost = Self.singleEditKeyboardCost(from: lower, to: normalizedGuess, language: language) else { continue }
            let totalScore = editCost + Double(rank) * 0.08
            if totalScore < bestScore {
                bestScore = totalScore
                bestGuess = rawGuess
            }
        }
        guard let bestGuess, bestScore <= 1.35 else { return nil }
        return Self.matchCase(of: token, onto: bestGuess)
    }

    /// The very common two-letter word (the language's `commonTwoLetterWords`, commonest
    /// first) one neighbouring key or one swap away from `lower`, if any.
    private func commonTwoLetterFix(for lower: String) -> String? {
        var best: (word: String, cost: Double)?
        for word in language.commonTwoLetterWords {
            guard let cost = Self.singleEditKeyboardCost(from: lower, to: word, language: language),
                  cost > 0, cost <= 0.65 else { continue }
            if best == nil || cost < best!.cost { best = (word, cost) }
        }
        return best?.word
    }

    /// What spatial autocorrect makes of `token`'s taps, or nil when it doesn't apply: off,
    /// another language's model, an apostrophe or one letter typed, a letter the model lacks,
    /// or taps that don't match the word.
    private func decoderDecision(for token: String) -> TypingDecoder.Decision? {
        syncUserWords()
        guard let decoder, let keys = decoderKeys, let taps = decoderTaps, decoder.model.language == language.rawValue,
              token.count >= 2, taps.count == token.count, !token.contains("'") else { return nil }
        let before = beforeInput ?? ""
        if let cache = decoderCache, cache.before == before, cache.taps == taps { return cache.decision }
        let lower = token.lowercased()
        let valid = Self.shorthandAllowlist.contains(lower) || learnedWords.contains(lower)
            || lexiconTerms.contains { $0.lowercased() == lower } || checker.isWordValid(token)
            || properNoun(for: token) != nil
        let decision = decoder.decide(
            taps: taps, typed: lower, previous: Self.previousWord(before, token: token),
            literalIsValid: valid, keys: keys
        )
        decoderCache = (before, taps, decision)
        return decision
    }

    /// `token` capitalised when it is all lowercase and the dictionary knows it only that way
    /// (a name, day or place: "mary" -> "Mary", "monday" -> "Monday"), else nil.
    private func properNoun(for token: String) -> String? {
        guard token.count >= 2, token == token.lowercased(), let first = token.first else { return nil }
        let capitalised = first.uppercased() + token.dropFirst()
        guard checker.isWordValid(capitalised), !checker.isWordValid(token) else { return nil }
        return capitalised
    }

    /// Gives the decoder your own words: Contacts names and other `UILexicon` terms, the
    /// one-word outputs of text replacements, words you kept twice, and typo fixes Hex
    /// learned. Rebuilt only when one of them, or the model, changed.
    private func syncUserWords() {
        guard let model = decoder?.model else { return }
        let id = ObjectIdentifier(model)
        if let built = userWordsBuilt, built.revision == userWordsRevision, built.model == id { return }
        let words = lexiconTerms + Array(lexiconShortcuts.values) + learnedWords.words + Array(learning.typos.values)
        decoder?.userWords = TypingDecoder.UserWords(words, model: model)
        userWordsBuilt = (userWordsRevision, id)
        decoderCache = nil
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

    /// A `UITextChecker` guess as autocorrect and the strip may use it, or nil. A guess adds
    /// no character you didn't type but letters and a contraction's apostrophe (never "t.b."
    /// for "tb"). One that splits what you typed into two words ("up-to" or "up to" for
    /// "upto") is used only as two words with a space, never hyphenated, and only when its
    /// letters are exactly yours (a missed space), both parts are words, and the checker
    /// offers the spaced form itself.
    private func usableGuess(_ guess: String, typed token: String, among guesses: [String]) -> String? {
        let isSeparator: (Character) -> Bool = { $0 == " " || $0 == "-" }
        guard guess.contains(where: isSeparator) else {
            let typed = Set(token)
            let addsOnlyLetters = guess.allSatisfy { $0.isLetter || $0 == "'" || typed.contains($0) }
            return addsOnlyLetters ? guess : nil
        }
        let parts = guess.split(separator: " ", omittingEmptySubsequences: false)
            .flatMap { $0.split(separator: "-", omittingEmptySubsequences: false) }
            .map(String.init)
        guard parts.count == 2,
              parts.joined().lowercased() == token.lowercased(),
              parts.allSatisfy({ isSplitWord($0) })
        else { return nil }
        let spaced = parts.joined(separator: " ")
        guard guesses.contains(where: { $0.caseInsensitiveCompare(spaced) == .orderedSame }) else { return nil }
        return spaced
    }

    /// One half of a split guess: a word of two or more letters the dictionary knows, or
    /// English "a" and "I" (`UITextChecker` accepts every single letter as a word).
    private func isSplitWord(_ part: String) -> Bool {
        guard !part.isEmpty, part.allSatisfy(\.isLetter) else { return false }
        if part.count == 1 {
            return language.usesEnglishFixes && (part.lowercased() == "a" || part.lowercased() == "i")
        }
        return checker.isWordValid(part)
    }

    private func rankedGuesses(for token: String) -> [String] {
        let lower = token.lowercased()
        let language = language
        let guesses = checker.guesses(for: token)
        return guesses
            .compactMap { usableGuess($0, typed: token, among: guesses) }
            .prefix(8)
            .enumerated()
            .sorted { a, b in
                let costA = Self.singleEditKeyboardCost(
                    from: lower,
                    to: a.element.replacingOccurrences(of: "'", with: "").lowercased(),
                    language: language
                ) ?? (2.0 + Double(abs(a.element.count - token.count)) * 0.3)
                let costB = Self.singleEditKeyboardCost(
                    from: lower,
                    to: b.element.replacingOccurrences(of: "'", with: "").lowercased(),
                    language: language
                ) ?? (2.0 + Double(abs(b.element.count - token.count)) * 0.3)
                return (costA + Double(a.offset) * 0.08) < (costB + Double(b.offset) * 0.08)
            }
            .map(\.element)
    }

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

    /// Returns a weighted cost if `source` and `target` are within 1 Damerau-Levenshtein edit; `nil` otherwise.
    static func singleEditKeyboardCost(
        from source: String,
        to target: String,
        language: KeyboardLanguage = .english
    ) -> Double? {
        if source == target { return 0 }
        let s = Array(source)
        let t = Array(target)
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
                return areAdjacentKeys(s[i], t[i], language: language) ? 0.65 : 1.15
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
        let leftAdj = extraIdx > 0 && areAdjacentKeys(extraChar, longer[extraIdx - 1], language: language)
        let rightAdj = extraIdx + 1 < longer.count && areAdjacentKeys(extraChar, longer[extraIdx + 1], language: language)
        return (leftAdj || rightAdj) ? 0.70 : 0.90
    }

    /// Whether two letters differ only by an accent, diaeresis or tilde (e/é, u/ü, n/ñ).
    static func sameLetterIgnoringAccents(_ a: Character, _ b: Character) -> Bool {
        guard a != b else { return false }
        let base: (Character) -> Unicode.Scalar? = { String($0).decomposedStringWithCanonicalMapping.unicodeScalars.first }
        return base(a) == base(b)
    }

    private static let keyCoordinates: [KeyboardLanguage: [Character: (Double, Double)]] = Dictionary(
        uniqueKeysWithValues: KeyboardLanguage.allCases.map { ($0, $0.keyCoordinates) }
    )

    static func areAdjacentKeys(_ a: Character, _ b: Character, language: KeyboardLanguage = .english) -> Bool {
        guard let keys = keyCoordinates[language], let pa = keys[a], let pb = keys[b] else { return false }
        let dx = pa.1 - pb.1
        let dy = pa.0 - pb.0
        return (dx * dx + dy * dy).squareRoot() <= 1.6
    }

    // MARK: - Built-in dictionaries

    /// Valid words where an apostrophe form exists, offered as a suggestion without clobbering the word.
    static let suggestOnlyContractions: [String: String] = [
        "ill": "I'll",
        "id": "I'd",
        "were": "we're",
        "wed": "we'd",
        "well": "we'll",
        "its": "it's",
        "lets": "let's",
        "hell": "he'll",
        "shell": "she'll",
    ]

    /// Common chat & engineering shorthand that cautious autocorrect never rewrites.
    static let shorthandAllowlist: Set<String> = [
        "ok", "okay", "ty", "thx", "pls", "plz", "tmrw", "tbh", "idk", "imo", "imho",
        "lol", "lmao", "omg", "brb", "btw", "afaik", "fyi", "nvm", "rn", "irl", "jk",
        "np", "yw", "smh", "ikr", "ngl", "fr", "gg", "dm", "pr", "prs", "ci", "ui",
        "ux", "ios", "macos", "api", "apis", "url", "urls", "json", "sql", "ssh", "cli",
        "gpu", "cpu", "ram", "sdk", "hex", "wisp", "seer", "rune", "kit", "gonna",
        "wanna", "gotta", "kinda", "sorta", "yeah", "yep", "nope", "nah", "hey", "yo",
    ]

    /// ~125 unambiguous missing-apostrophe contractions and high-frequency QWERTY transpositions.
    static let unambiguousFixes: [String: String] = [
        // Pronoun & missing-apostrophe contractions
        "i": "I",
        "im": "I'm",
        "ive": "I've",
        "dont": "don't",
        "cant": "can't",
        "wont": "won't",
        "didnt": "didn't",
        "doesnt": "doesn't",
        "isnt": "isn't",
        "arent": "aren't",
        "wasnt": "wasn't",
        "werent": "weren't",
        "hasnt": "hasn't",
        "havent": "haven't",
        "hadnt": "hadn't",
        "wouldnt": "wouldn't",
        "couldnt": "couldn't",
        "shouldnt": "shouldn't",
        "mustnt": "mustn't",
        "neednt": "needn't",
        "youre": "you're",
        "youve": "you've",
        "youd": "you'd",
        "youll": "you'll",
        "theyre": "they're",
        "theyve": "they've",
        "theyd": "they'd",
        "theyll": "they'll",
        "weve": "we've",
        "shes": "she's",
        "hes": "he's",
        "itll": "it'll",
        "thats": "that's",
        "theres": "there's",
        "heres": "here's",
        "wheres": "where's",
        "whos": "who's",
        "whats": "what's",
        "hows": "how's",
        "whens": "when's",
        "whys": "why's",
        "aint": "ain't",
        "yall": "y'all",
        "oclock": "o'clock",
        "maam": "ma'am",
        "cmon": "c'mon",
        "wouldve": "would've",
        "couldve": "could've",
        "shouldve": "should've",
        "mightve": "might've",
        "mustve": "must've",
        // High-frequency QWERTY transpositions & fat-finger typos
        "teh": "the",
        "hte": "the",
        "taht": "that",
        "thta": "that",
        "adn": "and",
        "nad": "and",
        "wiht": "with",
        "iwth": "with",
        "waht": "what",
        "hwat": "what",
        "wich": "which",
        "whcih": "which",
        "thier": "their",
        "theri": "their",
        "recieve": "receive",
        "definately": "definitely",
        "seperate": "separate",
        "occured": "occurred",
        "untill": "until",
        "abotu": "about",
        "baout": "about",
        "becuase": "because",
        "beacuse": "because",
        "jsut": "just",
        "jstu": "just",
        "knwo": "know",
        "konw": "know",
        "tiem": "time",
        "tmie": "time",
        "shoudl": "should",
        "hsould": "should",
        "woudl": "would",
        "owuld": "would",
        "coudl": "could",
        "oculd": "could",
        "ahve": "have",
        "hvae": "have",
        "maek": "make",
        "mkae": "make",
        "thign": "thing",
        "hting": "thing",
        "peopel": "people",
        "poeple": "people",
        "workign": "working",
        "goign": "going",
        "doign": "doing",
        "runnign": "running",
        "comign": "coming",
        "thnaks": "thanks",
        "htanks": "thanks",
        "plase": "please",
        "pleaes": "please",
        "yuor": "your",
        "yoru": "your",
        "yuo": "you",
        "oyu": "you",
        "fomr": "from",
        "verry": "very",
        "evrey": "every",
        "realyl": "really",
        "realy": "really",
        "actualyl": "actually",
        "probalby": "probably",
        "tomorow": "tomorrow",
        "tommorow": "tomorrow",
        "tonihgt": "tonight",
        "tongiht": "tonight",
        "mesasge": "message",
        "messgae": "message",
        "keybaord": "keyboard",
        "keboard": "keyboard",
        "iphnoe": "iPhone",
        "ipohne": "iPhone",
    ]
}
