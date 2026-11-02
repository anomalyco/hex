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
}

@MainActor
final class SystemTextChecker: TextChecking {
    private var checker: UITextChecker?
    private let language: String

    init(language: String = "en_US") {
        self.language = language
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
    private var lexiconShortcuts: [String: String] = [:]
    private var lexiconTerms: [String] = []
    private var suppressedInSession: Set<String> = []
    private(set) var lastCorrection: AppliedCorrection?

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

    func suppressAndKeep(_ word: String) {
        suppressedInSession.insert(word.lowercased())
        learnedWords.recordExplicitKeep(word)
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
        guard field == .text, autocorrectionType != .no else { return [] }
        guard let token = Self.currentToken(in: beforeInput) else { return [] }

        let lower = token.lowercased()
        let allowAuto = isEnabled
            && !suppressedInSession.contains(lower)
            && !learnedWords.contains(lower)
            && !Self.shorthandAllowlist.contains(lower)

        let autoFix = allowAuto ? cautiousReplacement(for: token) : nil
        var pool: [String] = []

        if let autoFix {
            pool.append(autoFix)
        } else if let suggested = Self.suggestOnlyContractions[lower] {
            pool.append(Self.matchCase(of: token, onto: suggested))
        }

        // Prefix matches from Contacts / lexicon terms.
        if lower.count >= 2 {
            for term in lexiconTerms where term.lowercased().hasPrefix(lower) && term.lowercased() != lower {
                let candidate = Self.matchCase(of: token, onto: term)
                if !pool.contains(where: { $0.caseInsensitiveCompare(candidate) == .orderedSame }) {
                    pool.append(candidate)
                }
                if pool.count >= 3 { break }
            }
        }

        // Spelling guesses or prefix completions from `UITextChecker`.
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
            if pool.count < 2 {
                let completions = checker.completions(for: token)
                    .filter { $0.count > token.count && $0.count <= token.count + 8 }
                    .sorted { a, b in
                        if a.count != b.count { return a.count < b.count }
                        return a.localizedCaseInsensitiveCompare(b) == .orderedAscending
                    }
                for item in completions {
                    let cased = Self.matchCase(of: token, onto: item)
                    if !pool.contains(where: { $0.caseInsensitiveCompare(cased) == .orderedSame }) {
                        pool.append(cased)
                    }
                    if pool.count >= 2 { break }
                }
            }
        }

        let literal = KeyboardSuggestion(displayText: "“\(token)”", replacement: token, kind: .keepLiteral)
        let primary: KeyboardSuggestion = if let first = pool.first {
            KeyboardSuggestion(
                displayText: first,
                replacement: first,
                kind: .primary(autoReplace: autoFix != nil)
            )
        } else {
            KeyboardSuggestion(displayText: token, replacement: token, kind: .primary(autoReplace: false))
        }
        let secondary: KeyboardSuggestion? = if pool.count >= 2 {
            KeyboardSuggestion(displayText: pool[1], replacement: pool[1], kind: .secondary)
        } else {
            nil
        }
        return [literal, primary] + (secondary.map { [$0] } ?? [])
    }

    /// Returns `(tokenToDelete, replacement)` only when cautious autocorrect is confident.
    func replacementOnSpace(
        beforeInput: String?,
        field: KeyboardFieldKind,
        autocorrectionType: UITextAutocorrectionType?
    ) -> (original: String, replacement: String)? {
        guard isEnabled, field == .text, autocorrectionType != .no else { return nil }
        guard let token = Self.currentToken(in: beforeInput) else { return nil }
        let lower = token.lowercased()
        guard !suppressedInSession.contains(lower),
              !learnedWords.contains(lower),
              !Self.shorthandAllowlist.contains(lower) else { return nil }
        guard let fix = cautiousReplacement(for: token) else { return nil }
        return (token, fix)
    }

    private func cautiousReplacement(for token: String) -> String? {
        let lower = token.lowercased()
        // 1. Explicit iOS Text Replacement (`omw` -> `On my way!`).
        if let shortcut = lexiconShortcuts[lower] {
            return shortcut
        }
        // Leave ALL-CAPS acronyms (`API`, `PR`, `NYC`) alone unless matched in `UILexicon` above,
        // except unambiguous all-caps contractions/typos of 3+ letters (`DONT` -> `DON'T`, `TEH` -> `THE`).
        let isAllCaps = token.count >= 2 && token == token.uppercased() && token != token.lowercased()
        // Leave camelCase / PascalCase identifiers (`HexKey`, `iOS`) alone.
        let hasInternalCaps = token.dropFirst().contains(where: \.isUppercase) && !isAllCaps
        if hasInternalCaps { return nil }

        // 2. Deterministic contractions and common QWERTY transpositions.
        if let fixed = Self.unambiguousFixes[lower] {
            if isAllCaps && token.count < 3 { return nil }
            return Self.matchCase(of: token, onto: fixed)
        }

        if isAllCaps { return nil }

        // 3. Single-edit `UITextChecker` typo on words of 3+ characters, never rewriting valid words.
        guard token.count >= 3, !token.contains("'") else { return nil }
        guard !checker.isWordValid(token) else { return nil }

        var bestGuess: String?
        var bestScore = Double.greatestFiniteMagnitude
        for (rank, rawGuess) in checker.guesses(for: token).prefix(8).enumerated() {
            // Skip multi-word splits (`alot` -> `a lot` is fine only if single space, keep v1 single-token + apostrophe).
            guard !rawGuess.contains(" ") else { continue }
            let normalizedGuess = rawGuess.replacingOccurrences(of: "'", with: "").lowercased()
            guard let editCost = Self.singleEditKeyboardCost(from: lower, to: normalizedGuess) else { continue }
            let totalScore = editCost + Double(rank) * 0.08
            if totalScore < bestScore {
                bestScore = totalScore
                bestGuess = rawGuess
            }
        }
        guard let bestGuess, bestScore <= 1.35 else { return nil }
        return Self.matchCase(of: token, onto: bestGuess)
    }

    private func rankedGuesses(for token: String) -> [String] {
        let lower = token.lowercased()
        return checker.guesses(for: token)
            .prefix(8)
            .enumerated()
            .sorted { a, b in
                let costA = Self.singleEditKeyboardCost(
                    from: lower,
                    to: a.element.replacingOccurrences(of: "'", with: "").lowercased()
                ) ?? (2.0 + Double(abs(a.element.count - token.count)) * 0.3)
                let costB = Self.singleEditKeyboardCost(
                    from: lower,
                    to: b.element.replacingOccurrences(of: "'", with: "").lowercased()
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
    static func singleEditKeyboardCost(from source: String, to target: String) -> Double? {
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
                return areAdjacentKeys(s[i], t[i]) ? 0.65 : 1.15
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
        let leftAdj = extraIdx > 0 && areAdjacentKeys(extraChar, longer[extraIdx - 1])
        let rightAdj = extraIdx + 1 < longer.count && areAdjacentKeys(extraChar, longer[extraIdx + 1])
        return (leftAdj || rightAdj) ? 0.70 : 0.90
    }

    private static let keyCoordinates: [Character: (Double, Double)] = {
        var map: [Character: (Double, Double)] = [:]
        for (col, ch) in "qwertyuiop".enumerated() { map[ch] = (0, Double(col)) }
        for (col, ch) in "asdfghjkl".enumerated() { map[ch] = (1, Double(col) + 0.5) }
        for (col, ch) in "zxcvbnm".enumerated() { map[ch] = (2, Double(col) + 1.5) }
        return map
    }()

    static func areAdjacentKeys(_ a: Character, _ b: Character) -> Bool {
        guard let pa = keyCoordinates[a], let pb = keyCoordinates[b] else { return false }
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
