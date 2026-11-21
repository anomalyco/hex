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
        /// A word that can follow the one just finished. Only an explicit pick inserts it.
        case nextWord
        /// An emoji for the word just finished (Settings › Keyboard › Emoji), offered after a space,
        /// when the strip has no words to show.
        case emoji
        /// Another reading of the word just swiped (Settings › Keyboard › Swipe typing): tapping it
        /// puts it in that word's place.
        case swiped
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
/// Delete what Hex learned removes them, and a keyboard that loaded them earlier drops its copy.
@MainActor
final class LearnedWordsStore {
    nonisolated static let defaultsKey = "keyboard.learnedWords"
    static let maxWords = 2_000
    static let maxBytes = 64 * 1024

    private let defaults: UserDefaults?
    private var storage: [String] = []
    private var wordSet: Set<String> = []
    private var sessionKeepCounts: [String: Int] = [:]
    private var loadedAt = Date().timeIntervalSince1970

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
        load()
    }

    var words: [String] {
        dropIfErased()
        return storage
    }

    func contains(_ word: String) -> Bool {
        dropIfErased()
        return wordSet.contains(word.lowercased())
    }

    /// Records an explicit keep (via `"quoted"` chip or Backspace-revert). Two keeps persist the word.
    func recordExplicitKeep(_ raw: String) {
        dropIfErased()
        let key = raw.lowercased()
        guard key.count >= 2, key.count <= 40 else { return }
        let count = (sessionKeepCounts[key] ?? 0) + 1
        sessionKeepCounts[key] = count
        if count >= 2 {
            learn(key)
        }
    }

    func learn(_ raw: String) {
        dropIfErased()
        let key = raw.lowercased()
        guard !key.isEmpty, !wordSet.contains(key) else { return }
        storage.append(key)
        wordSet.insert(key)
        while storage.count > Self.maxWords {
            let evicted = storage.removeFirst()
            wordSet.remove(evicted)
        }
        save()
    }

    private func dropIfErased() {
        let erasedAt = TypingJournalSettings(defaults: defaults).erasedAt
        guard erasedAt > loadedAt else { return }
        storage = []
        wordSet = []
        sessionKeepCounts = [:]
        loadedAt = erasedAt
    }

    private func load() {
        guard let data = defaults?.data(forKey: Self.defaultsKey),
              data.count <= Self.maxBytes,
              let decoded = try? JSONDecoder().decode([String].self, from: data) else { return }
        storage = Array(decoded.suffix(Self.maxWords))
        wordSet = Set(storage)
    }

    private func save() {
        guard let defaults else { return }
        while !storage.isEmpty {
            if let data = try? JSONEncoder().encode(storage), data.count <= Self.maxBytes {
                defaults.set(data, forKey: Self.defaultsKey)
                return
            }
            let evicted = storage.removeFirst()
            wordSet.remove(evicted)
        }
    }
}

/// Cautious on-device suggestions and replace-on-space engine combining `UILexicon`,
/// deterministic contractions/transpositions, QWERTY-ranked `UITextChecker`, and `LearnedWordsStore`.
@MainActor
final class AutocorrectEngine {
    struct AppliedCorrection: Equatable {
        /// One word backspace puts back as typed (`from`), and what autocorrect made it.
        struct Change: Equatable {
            let from: String
            let to: String
        }

        let original: String
        let insertedText: String
        /// The words in `original` that autocorrect changed; backspace keeps each as typed.
        let changes: [Change]
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
    /// Bilingual autocorrect: the other Latin-script language in Settings › Keyboard ›
    /// Languages, with its own dictionary (and Spatial autocorrect's model) on these keys, so
    /// typos are fixed in either language and a word of either is never "corrected" into the
    /// other, as Apple's "Français et English" keyboard does. Nil with one language.
    private(set) var partner: AutocorrectEngine?
    /// The partner, unless its dictionary takes any letters for a word: iOS can list a
    /// language whose word data isn't there (the iOS 27.2 simulator's `pl_PL`), and such a
    /// partner would keep every typo as "a word in the other language". Checked once per
    /// language, on first use.
    private var workingPartner: AutocorrectEngine? {
        guard let partner else { return nil }
        if partner.acceptsAnyLetters == nil {
            partner.acceptsAnyLetters = partner.checker.isWordValid("qzxjvkq")
        }
        return partner.acceptsAnyLetters == true ? nil : partner
    }
    private var acceptsAnyLetters: Bool?
    /// The keys you type on, whose neighbours price a slip: `language`'s layout, or a
    /// partner's primary's.
    private(set) var layoutLanguage: KeyboardLanguage = .english
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
    /// The layout language's word list and word pairs (the typing model, whether or not
    /// Spatial autocorrect is on): English chooses between words the same letters spell with
    /// and without an apostrophe by the words around them (`contextChoice`). Nil without one.
    var contextModel: TypingDecoderModel?
    /// The letter keys for `decoder`, from the current layout.
    var decoderKeys: TypingDecoder.Keys? {
        didSet { partner?.decoderKeys = decoderKeys }
    }
    /// Where each letter of the word being typed was tapped, one point per letter; nil when
    /// the keys don't know (the cursor moved, the text came from elsewhere).
    var decoderTaps: [CGPoint]? {
        didSet { partner?.decoderTaps = decoderTaps }
    }
    private var beforeInput: String?
    /// Bumped whenever a source of your own words changes, so the decoder's copy is rebuilt.
    private var userWordsRevision = 0
    private var userWordsBuilt: (revision: Int, model: ObjectIdentifier)?
    /// Bumped whenever your own words, the language or Suggest swear words change (Swipe
    /// typing rebuilds its lists then).
    var ownWordsRevision: Int { userWordsRevision }
    /// Words never suggested or corrected to in `language` (`OffensiveWords`).
    private(set) var offensiveWords: OffensiveWords
    /// Settings › Keyboard › Suggest swear words: swear words may be suggested and corrected
    /// to; slurs never are. Off by default.
    var allowsSwearing = false {
        didSet {
            if oldValue != allowsSwearing { userWordsRevision += 1; decoderCache = nil }
            partner?.allowsSwearing = allowsSwearing
        }
    }
    /// Settings › Keyboard › Auto-capitalization. Off, a fix from the dictionary or Spatial
    /// autocorrect's model adds no capital you didn't type, as Apple's keyboard with
    /// Auto-Capitalization off: "i" stays "i", "im" becomes "i'm", "mary" stays "mary" and
    /// "mondya" becomes "monday". Your own spellings keep theirs (`isOwnSpelling`). On by default.
    var addsCapitals = true {
        didSet { partner?.addsCapitals = addsCapitals }
    }
    private var decoderCache: (before: String, taps: [CGPoint], decision: TypingDecoder.Decision?)?

    init(
        checker: TextChecking? = nil,
        learnedWords: LearnedWordsStore? = nil
    ) {
        self.checker = checker ?? SystemTextChecker()
        self.learnedWords = learnedWords ?? LearnedWordsStore()
        self.offensiveWords = .bundled(KeyboardLanguage.english.rawValue)
    }

    /// Whether `candidate` may be suggested or corrected to: not an offensive word, nor an
    /// offensive two-word expression with `previous` (slurs never, swear words only with
    /// `allowsSwearing`), unless it is your own from `UILexicon` (a Contacts name, a text
    /// replacement), as Apple's keyboard offers those.
    func mayOffer(_ candidate: String, after previous: String? = nil) -> Bool {
        guard offensiveWords.blocks(candidate, allowSwearing: allowsSwearing, after: previous) else { return true }
        return lexiconTerms.contains { $0.caseInsensitiveCompare(candidate) == .orderedSame }
            || lexiconShortcuts.values.contains { $0.caseInsensitiveCompare(candidate) == .orderedSame }
    }

    /// `mayOffer` for a candidate in place of `token`, after the word before it.
    private func mayOffer(_ candidate: String, replacing token: String) -> Bool {
        mayOffer(candidate, after: Self.previousWord(beforeInput ?? "", token: token))
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
        partner?.loadLexiconEntries(entries)
    }

    func clearPendingUndo() {
        lastCorrection = nil
    }

    func recordAppliedCorrection(original: String, insertedText: String, changes: [AppliedCorrection.Change]) {
        lastCorrection = AppliedCorrection(original: original, insertedText: insertedText, changes: changes)
    }

    func consumePendingUndo() -> AppliedCorrection? {
        defer { lastCorrection = nil }
        return lastCorrection
    }

    /// Follows the keyboard layout. `available` is `UITextChecker.availableLanguages`.
    func setLanguage(_ language: KeyboardLanguage, available: [String] = UITextChecker.availableLanguages) {
        self.language = language
        layoutLanguage = language
        acceptsAnyLetters = nil
        if partner?.language == language {
            partner = nil
        } else {
            partner?.layoutLanguage = language
            partner?.decoderCache = nil
        }
        offensiveWords = .bundled(language.rawValue)
        userWordsRevision += 1
        decoderCache = nil
        let code = language.spellCheckLanguage(available: available)
        hasDictionary = code != nil
        if let code {
            checker.useLanguage(code)
        } else {
            partner = nil
        }
        suppressedInSession.removeAll()
        lastCorrection = nil
    }

    /// Bilingual autocorrect's second language (`KeyboardLanguageList.partner`), or nil. It
    /// shares your words (`learnedWords`, text replacements, Learn how I type) and types on
    /// this engine's keys. None without a dictionary for either language. `checker` is for
    /// tests.
    func setPartner(
        _ other: KeyboardLanguage?,
        available: [String] = UITextChecker.availableLanguages,
        checker: TextChecking? = nil
    ) {
        guard let other, other != language, hasDictionary, other.spellCheckLanguage(available: available) != nil else {
            partner = nil
            return
        }
        guard partner?.language != other else { return }
        let engine = AutocorrectEngine(checker: checker ?? SystemTextChecker(), learnedWords: learnedWords)
        engine.setLanguage(other, available: available)
        engine.layoutLanguage = language
        engine.lexiconShortcuts = lexiconShortcuts
        engine.lexiconTerms = lexiconTerms
        engine.learning = learning
        engine.allowsSwearing = allowsSwearing
        engine.addsCapitals = addsCapitals
        engine.decoderKeys = decoderKeys
        engine.decoderTaps = decoderTaps
        partner = engine
    }

    func applyLearning(_ learning: TypingLearning) {
        if self.learning != learning {
            self.learning = learning
            userWordsRevision += 1
        }
        partner?.applyLearning(learning)
    }

    func suppressAndKeep(_ word: String) {
        suppressedInSession.insert(word.lowercased())
        learnedWords.recordExplicitKeep(word)
        userWordsRevision += 1
        // The store is shared, so the partner only drops its copy of your words.
        partner?.suppressedInSession.insert(word.lowercased())
        partner?.userWordsRevision += 1
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

    // MARK: - Predictions between words

    /// The empty field, or after a sentence's end and a space, or a new line.
    static func isSentenceStart(_ beforeInput: String?) -> Bool {
        guard let beforeInput else { return true }
        guard let last = beforeInput.last else { return true }
        if last.isNewline { return true }
        guard last == " " else { return false }
        var trimmed = Substring(beforeInput)
        while trimmed.last == " " { trimmed = trimmed.dropLast() }
        guard let written = trimmed.last else { return true }
        return written.isNewline || written == "." || written == "!" || written == "?"
    }

    /// Common sentence-initial words per language (Hex's own short lists), lowercase except
    /// words that always take a capital.
    static func sentenceStarters(_ language: KeyboardLanguage) -> [String] {
        switch language {
        case .english: ["I", "the", "I'm"]
        case .spanish: ["yo", "el", "la"]
        case .german: ["ich", "das", "die"]
        case .french, .frenchCanadian, .frenchSwiss: ["je", "le", "la"]
        case .italian: ["io", "il", "la"]
        case .portuguese: ["eu", "o", "a"]
        case .polish: ["ja", "to", "nie"]
        case .norwegian: ["jeg", "det", "vi"]
        case .russian: ["я", "это", "мы"]
        case .ukrainian: ["я", "це", "ми"]
        case .serbianLatin: ["ja", "to", "da"]
        }
    }

    /// Marks after which a sentence goes on.
    static let midSentenceMarks: Set<Character> = [",", ";", ":", ")", "\"", "\u{201D}", "\u{2019}", "\u{2014}"]

    /// Common first words of a clause, after a comma (Hex's own short lists).
    static func clauseStarters(_ language: KeyboardLanguage) -> [String] {
        switch language {
        case .english: ["I", "but", "and"]
        case .spanish: ["que", "pero", "y"]
        case .german: ["und", "aber", "ich"]
        case .french, .frenchCanadian, .frenchSwiss: ["et", "mais", "je"]
        case .italian: ["e", "ma", "che"]
        case .portuguese: ["e", "mas", "que"]
        case .polish: ["a", "ale", "że"]
        case .norwegian: ["og", "men", "jeg"]
        case .russian: ["и", "но", "что"]
        case .ukrainian: ["і", "але", "що"]
        case .serbianLatin: ["i", "ali", "da"]
        }
    }

    /// The language's commonest words, for next-word slots the model can't fill.
    static func commonWords(_ language: KeyboardLanguage) -> [String] {
        switch language {
        case .english: ["the", "to", "and", "a", "I", "of"]
        case .spanish: ["de", "que", "y", "la", "el", "en"]
        case .german: ["und", "die", "der", "ich", "ist", "das"]
        case .french, .frenchCanadian, .frenchSwiss: ["de", "la", "et", "le", "je", "à"]
        case .italian: ["di", "e", "che", "la", "il", "a"]
        case .portuguese: ["de", "que", "e", "o", "a", "do"]
        case .polish: ["nie", "i", "w", "się", "na", "to"]
        case .norwegian: ["og", "i", "det", "er", "på", "som"]
        case .russian: ["и", "в", "не", "на", "я", "что"]
        case .ukrainian: ["і", "в", "не", "на", "що", "я"]
        case .serbianLatin: ["i", "je", "da", "u", "se", "na"]
        }
    }

    /// Up to three distinct, allowed words as tap-only predictions, capitalized when shift
    /// would be on (never with Auto-capitalization off).
    private func predictions(_ words: [String], previous: String?, capitalized: Bool) -> [KeyboardSuggestion] {
        var out: [KeyboardSuggestion] = []
        for word in words where out.count < 3 {
            guard previous.map({ word.lowercased() != $0 }) ?? true, mayOffer(word, after: previous) else { continue }
            let text = capitalized && addsCapitals ? word.prefix(1).uppercased() + word.dropFirst() : word
            guard !out.contains(where: { $0.replacement.caseInsensitiveCompare(text) == .orderedSame }) else { continue }
            out.append(KeyboardSuggestion(displayText: text, replacement: text, kind: .nextWord))
        }
        return out
    }

    // MARK: - Suggestions & Replace-on-Space

    /// `capitalizesNext`: shift would turn itself on for the next letter (the start of a
    /// sentence, or every word in a Words field), so predictions start with a capital.
    func suggestions(
        beforeInput: String?,
        field: KeyboardFieldKind,
        autocorrectionType: UITextAutocorrectionType?,
        predictionModel: TypingDecoderModel? = nil,
        capitalizesNext: Bool = false
    ) -> [KeyboardSuggestion] {
        guard field == .text, autocorrectionType != .no, hasDictionary else { return [] }
        // An empty field, or a new sentence (". ", "! ", "? ", a new line): three common
        // first words, as Apple's keyboard offers.
        if Self.isSentenceStart(beforeInput) {
            return predictions(Self.sentenceStarters(language), previous: nil, capitalized: capitalizesNext)
        }
        if let beforeInput, beforeInput.last == " " {
            let before = beforeInput.dropLast()
            // After a comma or other mid-sentence mark and a space: words that start a clause, as
            // Apple's bar offers there (not after a handle, a path, a number or a second space).
            if let mark = before.last, Self.midSentenceMarks.contains(mark) {
                return predictions(Self.clauseStarters(language), previous: nil, capitalized: capitalizesNext)
            }
            guard let finished = Self.currentToken(in: String(before)) else { return [] }
            let previous = finished.lowercased()
            var words: [String] = []
            if let model = predictionModel, model.language == language.rawValue {
                words = model.nextWords(after: previous, allowing: { mayOffer($0, after: previous) })
            }
            // Fewer than three from the model: the language's commonest words fill the rest,
            // after the model's, so no slot stays empty.
            return predictions(words + Self.commonWords(language), previous: previous, capitalized: capitalizesNext)
        }
        guard let token = Self.currentToken(in: beforeInput) else { return [] }
        self.beforeInput = beforeInput

        let lower = token.lowercased()
        let allowAuto = isEnabled
            && !suppressedInSession.contains(lower)
            && !learning.rejected.contains(lower)
            && !learnedWords.contains(lower)
            && !Self.shorthandAllowlist.contains(lower)

        partner?.beforeInput = beforeInput
        let autoFix = allowAuto ? cautiousReplacement(for: token) : nil
        var pool: [String] = autoFix.map { [$0] } ?? []

        if let partner = workingPartner {
            // Both languages' candidates, those of the language the words before are in
            // first; spelling guesses only for a word neither knows.
            let mine = candidates(for: token, autoFix: autoFix, guesses: !partner.knowsAsTyped(token), limit: 2, model: predictionModel)
            let theirs = partner.candidates(for: token, autoFix: autoFix, guesses: !knowsAsTyped(token), limit: 2, model: partner.decoder?.model)
            let leansPartner = languageLean(partner, token: token) > 0
            let (first, second) = leansPartner ? (theirs, mine) : (mine, theirs)
            for index in 0..<max(first.count, second.count) {
                for item in [first, second].compactMap({ index < $0.count ? $0[index] : nil })
                where pool.count < 2 && !pool.contains(where: { $0.caseInsensitiveCompare(item) == .orderedSame }) {
                    pool.append(item)
                }
            }
        } else {
            pool += candidates(for: token, autoFix: autoFix, guesses: true, limit: 2 - pool.count, model: predictionModel)
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

    /// The strip's candidates for `token` in this language after `autoFix` (already in the
    /// strip), at most `limit`: a learned fix or contraction space won't apply, Spatial
    /// autocorrect's other readings of your taps, spelling guesses for a word the dictionary
    /// doesn't know (only with `guesses`), then completions.
    private func candidates(for token: String, autoFix: String?, guesses: Bool, limit: Int, model: TypingDecoderModel?) -> [String] {
        let lower = token.lowercased()
        var pool: [String] = autoFix.map { [$0] } ?? []
        let start = pool.count
        let end = start + max(0, limit)
        func has(_ item: String) -> Bool {
            pool.contains { $0.caseInsensitiveCompare(item) == .orderedSame }
        }

        if autoFix == nil {
            if let learned = learning.typos[lower], mayOffer(learned, replacing: token) {
                // A learned fix space won't apply (the typo is a real word, or you kept it) is still offered.
                pool.append(Self.matchCase(of: token, onto: learned))
            } else if language.usesEnglishFixes, let suggested = Self.suggestOnlyContractions[lower] {
                pool.append(Self.matchCase(of: token, onto: suggested))
            }
        }

        // A short prefix is usually an unfinished word. Reserve a slot for its most likely
        // completion in this phrase without changing what space does to the literal.
        var predictions: [String] = []
        if pool.count < end, token.count <= 3, let model, model.language == language.rawValue {
            let previous = Self.previousWord(beforeInput ?? "", token: token)
            predictions = model.completions(of: lower, after: previous, allowing: { mayOffer($0, after: previous) })
                .map { Self.matchCase(of: token, onto: $0) }
            if autoFix == nil, let first = predictions.first(where: { !has($0) }) { pool.append(first) }
        }

        // Spatial autocorrect's other readings of your taps.
        if let decision = decoderDecision(for: token) {
            for reading in decision.readings where pool.count < end && mayOffer(reading.text, replacing: token) {
                let cased = Self.matchCase(of: token, onto: reading.text)
                if !has(cased) {
                    pool.append(cased)
                }
            }
        }

        // Spelling guesses for a word the dictionary doesn't know.
        if guesses && pool.count < end && token.count >= 2 {
            let valid = Self.shorthandAllowlist.contains(lower) || learnedWords.contains(lower) || checker.isWordValid(token)
            if !valid {
                for guess in rankedGuesses(for: token) {
                    let cased = Self.matchCase(of: token, onto: guess)
                    if cased.caseInsensitiveCompare(token) != .orderedSame, mayOffer(guess, replacing: token), !has(cased) {
                        pool.append(cased)
                    }
                    if pool.count >= end { break }
                }
            }
        }

        // Completions of what you're typing, the likeliest first.
        for item in predictions where pool.count < end && !has(item) { pool.append(item) }
        if pool.count < end {
            for item in rankedCompletions(for: token) where item != token && !has(item) {
                pool.append(item)
                if pool.count >= end { break }
            }
        }
        return Array(pool.dropFirst(start).prefix(max(0, limit)))
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
        where item.count > token.count && item.count <= token.count + 8 && mayOffer(item, replacing: token) && seen.insert(item.lowercased()).inserted {
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
        partner?.beforeInput = beforeInput
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

    /// What space puts in place of `token`, if anything. With a partner (bilingual
    /// autocorrect), a word either language knows as typed stays as it is (your text
    /// replacements still apply); a typo either can fix gets the fix, and when both can, the
    /// fix in the language of the words before it, else the cheaper slip on these keys, else
    /// the keyboard's own language's.
    private func cautiousReplacement(for token: String) -> String? {
        guard let partner = workingPartner else { return ownReplacement(for: token) }
        if let shortcut = lexiconShortcuts[token.lowercased()] {
            return shortcut
        }
        // One-letter words are left to the keyboard's own language: `UITextChecker` takes
        // every letter as a word, so English "i" would never become "I" beside French.
        if token.count >= 2, partner.knowsAsTyped(token) { return nil }
        let own = ownReplacement(for: token)
        guard token.count >= 2, !knowsAsTyped(token), let theirs = partner.ownReplacement(for: token) else { return own }
        guard let own, own.caseInsensitiveCompare(theirs) != .orderedSame else { return own ?? theirs }
        return prefersPartner(partner, token: token, own: own, theirs: theirs) ? theirs : own
    }

    /// Whether `token` is a word in this language as typed: the dictionary's, one of your
    /// own (`UILexicon`, kept twice) or chat shorthand. A name it knows only capitalised
    /// doesn't count ("paris").
    private func knowsAsTyped(_ token: String) -> Bool {
        let lower = token.lowercased()
        return Self.shorthandAllowlist.contains(lower) || learnedWords.contains(lower)
            || lexiconTerms.contains { $0.lowercased() == lower } || checker.isWordValid(token)
    }

    /// Which language the words before `token` are in: above zero the partner's, below this
    /// one's, counting the last four that only one of the dictionaries knows.
    private func languageLean(_ partner: AutocorrectEngine, token: String) -> Int {
        let before = (beforeInput ?? "").suffix(120)
        let rest = before.hasSuffix(token) ? before.dropLast(token.count) : before
        var lean = 0
        for word in rest.split(whereSeparator: { !$0.isLetter && $0 != "'" }).suffix(4) where word.count >= 2 {
            let mine = checker.isWordValid(String(word))
            let theirs = partner.checker.isWordValid(String(word))
            if theirs && !mine { lean += 1 } else if mine && !theirs { lean -= 1 }
        }
        return lean
    }

    /// Both languages can fix the typo: the partner's fix when the words before are in its
    /// language, or, without a lean, when its fix is clearly the cheaper slip on these keys.
    private func prefersPartner(_ partner: AutocorrectEngine, token: String, own: String, theirs: String) -> Bool {
        let lean = languageLean(partner, token: token)
        if lean != 0 { return lean > 0 }
        let lower = token.lowercased()
        func cost(_ fix: String) -> Double {
            let normalized = fix.replacingOccurrences(of: "'", with: "").lowercased()
            return Self.singleEditKeyboardCost(from: lower, to: normalized, language: layoutLanguage) ?? 1.0
        }
        return cost(theirs) + 0.2 < cost(own)
    }

    /// `cautiousReplacement` in this language alone.
    private func ownReplacement(for token: String) -> String? {
        let lower = token.lowercased()
        // 1. Explicit iOS Text Replacement (`omw` -> `On my way!`).
        if let shortcut = lexiconShortcuts[lower] {
            return shortcut
        }
        // A word that may not be offered, typed in full, is a word you meant: never corrected
        // into another (as on Android: blocked by the current setting, or after its expression's
        // first word).
        if offensiveWords.blocks(lower, allowSwearing: allowsSwearing, after: Self.previousWord(beforeInput ?? "", token: token)) {
            return nil
        }
        // A typo you fixed the same way at least twice (Learn how I type), unless what you
        // typed is a real word: then it is only suggested.
        if let learned = learning.typos[lower], mayOffer(learned, replacing: token), !checker.isWordValid(token) {
            return Self.matchCase(of: token, onto: learned)
        }
        // Leave ALL-CAPS acronyms (`API`, `PR`, `NYC`) alone unless matched in `UILexicon` above,
        // except unambiguous all-caps contractions/typos of 3+ letters (`DONT` -> `DON'T`, `TEH` -> `THE`).
        let isAllCaps = token.count >= 2 && token == token.uppercased() && token != token.lowercased()
        // Leave camelCase / PascalCase identifiers (`HexKey`, `iOS`) alone.
        let hasInternalCaps = token.dropFirst().contains(where: \.isUppercase) && !isAllCaps
        if hasInternalCaps { return nil }

        // A word the same letters spell with an apostrophe ("id", "I'd"), chosen by the word
        // before when it says so clearly ("yes id" -> "yes I'd", "the id" stays). The next
        // word may still revise it (`revision(of:before:next:)`).
        if !isAllCaps, !token.contains("'"), let choice = contextChoice(
            for: token, previous: Self.previousWord(beforeInput ?? "", token: token), next: nil,
            startsSentence: Self.startsSentence(before: beforeInput ?? "", token: token)
        ), mayOffer(choice, replacing: token) {
            return withoutAddedCapitals(choice, typed: token)
        }

        // 2. Deterministic contractions and common QWERTY transpositions (English only:
        // Norwegian "i" means "in" and must not become "I").
        if language.usesEnglishFixes, let fixed = Self.unambiguousFixes[lower] {
            if isAllCaps && token.count < 3 { return nil }
            return withoutAddedCapitals(Self.matchCase(of: token, onto: fixed), typed: token)
        }

        if isAllCaps { return nil }

        // Spatial autocorrect, when on, decides alone: your taps, the word before, and a margin.
        if let decision = decoderDecision(for: token) {
            if let replacement = decision.replacement, mayOffer(replacement, replacing: token) {
                var cased = Self.matchCase(of: token, onto: replacement)
                // A missed space after English "i" ("iwant") gives "I want", as "i" alone does.
                if language.usesEnglishFixes, cased.contains(" ") {
                    cased = cased.split(separator: " ").map { $0 == "i" ? "I" : String($0) }.joined(separator: " ")
                }
                return withoutAddedCapitals(cased, typed: token)
            }
            // A name, day or place its lowercase word list lacks, typed in lowercase, is
            // capitalised as the dictionary path does ("mary" -> "Mary"), unless
            // Auto-capitalization is off.
            return addsCapitals ? properNoun(for: token) : nil
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
            guard let rawGuess = usableGuess(checkerGuess, typed: token, among: guesses), mayOffer(rawGuess, replacing: token) else { continue }
            let normalizedGuess = rawGuess.replacingOccurrences(of: "'", with: "").lowercased()
            guard let editCost = Self.singleEditKeyboardCost(from: lower, to: normalizedGuess, language: layoutLanguage) else { continue }
            let totalScore = editCost + Double(rank) * 0.08
            if totalScore < bestScore {
                bestScore = totalScore
                bestGuess = rawGuess
            }
        }
        guard let bestGuess, bestScore <= 1.35 else { return nil }
        return withoutAddedCapitals(Self.matchCase(of: token, onto: bestGuess), typed: token)
    }

    /// `fix` for `token` as autocorrect puts it in: with `addsCapitals` off and nothing
    /// typed in capitals, all lowercase unless it is your own spelling; nil when that leaves
    /// just what you typed ("i").
    private func withoutAddedCapitals(_ fix: String, typed token: String) -> String? {
        guard !addsCapitals, !token.contains(where: \.isUppercase), !isOwnSpelling(fix) else { return fix }
        let lowered = fix.lowercased()
        return lowered == token ? nil : lowered
    }

    /// Whether `word`, capitals and all, is one of your own spellings: a Contacts name or
    /// other `UILexicon` term, a text replacement's output, a word you kept twice, or a fix
    /// Hex learned from you. Auto-capitalization off leaves their capitals alone.
    func isOwnSpelling(_ word: String) -> Bool {
        lexiconTerms.contains(word) || lexiconShortcuts.values.contains(word)
            || learning.typos.values.contains(word) || learnedWords.words.contains(word)
    }

    /// The very common two-letter word (the language's `commonTwoLetterWords`, commonest
    /// first) one neighbouring key or one swap away from `lower`, if any.
    private func commonTwoLetterFix(for lower: String) -> String? {
        var best: (word: String, cost: Double)?
        for word in language.commonTwoLetterWords {
            guard let cost = Self.singleEditKeyboardCost(from: lower, to: word, language: layoutLanguage),
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

    // MARK: - Words the context decides

    /// English words chosen by the words around them rather than as typos: the same letters
    /// with and without an apostrophe ("id" or "I'd", "its" or "it's"), and words that sound
    /// alike ("your", "there", "then"). "were" and "we're" aren't here: the word pairs can't
    /// tell them apart, so the strip offers "we're" and space never changes it.
    static let contextGroups: [[String]] = [
        ["id", "I'd"], ["ill", "I'll"], ["hell", "he'll"], ["shell", "she'll"],
        ["wed", "we'd"], ["lets", "let's"], ["well", "we'll"],
        ["its", "it's"], ["your", "you're"], ["there", "their", "they're"], ["then", "than"],
    ]

    /// The typing model's word pairs barely spell contractions (its books split "I'd" into
    /// "I" and "'d"), so a contraction is scored as the words it stands for.
    static let contractionWords: [String: [[String]]] = [
        "i'd": [["i", "would"], ["i", "had"]], "i'll": [["i", "will"]], "he'll": [["he", "will"]],
        "she'll": [["she", "will"]], "we'd": [["we", "would"], ["we", "had"]], "let's": [["let", "us"]],
        "we'll": [["we", "will"]], "it's": [["it", "is"], ["it", "has"]], "you're": [["you", "are"]],
        "they're": [["they", "are"]],
    ]

    private static let contextGroupOf: [String: [String]] = {
        var out: [String: [String]] = [:]
        for group in contextGroups { for word in group { out[word.lowercased()] = group } }
        return out
    }()

    /// Nats the better word must win by: with the word before only, and once the next word
    /// is known too. Changing only an apostrophe needs less than changing letters ("your"
    /// and "you're"). Tuned on Tatoeba sentences outside the benchmark's held-out ones.
    static let contextMargin = (previous: 3.0, next: 1.5)
    static let contextLetterMargin = (previous: 6.0, next: 3.0)
    /// -ln P of a word the model lacks.
    private static let contextUnknownCost = 15.0
    /// How much likelier a sentence starts with a contraction's pronoun ("I'd", "It's") than
    /// the word list alone says: the pairs have no sentence starts.
    private static let contextSentenceStart = 3.0
    /// "will", "would", "had" and "us", which thousands of words follow in the pairs: a next
    /// word that never follows a contraction's last one of them is evidence against it
    /// ("till hell freezes", not "till he'll freezes"). "are" and "is" are left out: the
    /// adjectives after them are too rare for the pairs ("you're funny").
    private static let contextMissingFollower = 5.0
    private static let contextRichFollowers: Set<String> = ["will", "would", "had", "us"]

    /// Whether `token`, at the end of `before`, starts a sentence.
    static func startsSentence(before: String, token: String) -> Bool {
        let rest = before.hasSuffix(token) ? String(before.dropLast(token.count)) : before
        return AutoCapitalization.shouldCapitalize(mode: .sentences, before: rest, hasText: true)
    }

    /// -ln P(`word` | `previous`) + -ln P(`next` | `word`) by the model's pairs, a
    /// contraction as its words ("I'd like" as "I would like").
    private func contextCost(_ word: String, previous: String?, next: String?, startsSentence: Bool, model: TypingDecoderModel) -> Double {
        let unknown = Self.contextUnknownCost
        let expanded = Self.contractionWords[word.lowercased()]
        var best = Double.infinity
        for words in expanded ?? [[word.lowercased()]] {
            var cost = model.cost(of: words[0], after: previous) ?? unknown
            for k in words.indices.dropFirst() { cost += model.cost(of: words[k], after: words[k - 1]) ?? unknown }
            if let next {
                let last = words[words.count - 1]
                cost += model.cost(of: next, after: last) ?? unknown
                if Self.contextRichFollowers.contains(last), !model.hasPair(last, next) {
                    cost += Self.contextMissingFollower
                }
            }
            best = min(best, cost)
        }
        return expanded != nil && startsSentence ? best - Self.contextSentenceStart : best
    }

    /// The word of `word`'s group (`contextGroups`) that the words around it make clearly
    /// likelier, cased as `word` is, or nil to keep it. English only, with the layout's
    /// model, and never a word you kept or Hex learned not to change.
    func contextChoice(for word: String, previous: String?, next: String?, startsSentence: Bool = false) -> String? {
        guard isEnabled, language.usesEnglishFixes, let model = contextModel, model.language == language.rawValue,
              let group = Self.contextGroupOf[word.lowercased()] else { return nil }
        let lower = word.lowercased()
        guard !suppressedInSession.contains(lower), !learning.rejected.contains(lower), !learnedWords.contains(lower),
              !lexiconTerms.contains(where: { $0.lowercased() == lower }) else { return nil }
        let own = contextCost(word, previous: previous, next: next, startsSentence: startsSentence, model: model)
        var best: (word: String, cost: Double)?
        for other in group where other.lowercased() != lower {
            let cost = contextCost(other, previous: previous, next: next, startsSentence: startsSentence, model: model)
            if cost < best?.cost ?? .infinity { best = (other, cost) }
        }
        guard let best else { return nil }
        let sameLetters = best.word.replacingOccurrences(of: "'", with: "").lowercased() == lower.replacingOccurrences(of: "'", with: "")
        let margins = sameLetters ? Self.contextMargin : Self.contextLetterMargin
        guard own - best.cost >= (next == nil ? margins.previous : margins.next) else { return nil }
        return Self.matchContextCase(of: word, onto: best.word)
    }

    /// `target` cased as `word` reads: a capital first letter carries over (a sentence's
    /// start), except the "I" of "I'd" or "I'll", which comes off ("I'd" to "id").
    private static func matchContextCase(of word: String, onto target: String) -> String {
        let startsWithI = word.hasPrefix("I'") && word.dropFirst(2).allSatisfy(\.isLowercase)
        guard let first = word.first, first.isUppercase, !startsWithI, let t = target.first else { return target }
        return t.uppercased() + target.dropFirst()
    }

    /// The word before the one being finished, revised now that `next` follows it: "well
    /// see" becomes "we'll see", as Apple's keyboard decides at the next word. `before` is
    /// the text up to the end of `word`. Nil to leave it.
    func revision(of word: String, before: String, next: String, field: KeyboardFieldKind, autocorrectionType: UITextAutocorrectionType?) -> String? {
        guard field == .text, autocorrectionType != .no, hasDictionary, word.count >= 2 else { return nil }
        let isAllCaps = word == word.uppercased() && word != word.lowercased() && !word.hasPrefix("I'")
        guard !isAllCaps, let nextWord = next.split(separator: " ").first.map(String.init) else { return nil }
        let previous = Self.previousWord(before, token: word)
        guard let choice = contextChoice(
            for: word, previous: previous, next: nextWord.lowercased(),
            startsSentence: Self.startsSentence(before: before, token: word)
        ), mayOffer(choice, after: previous) else { return nil }
        return addsCapitals || word.first?.isUppercase == true ? choice : choice.lowercased()
    }

    /// One word in place of `first`, a space tapped near the letter row's edge (`space`) and
    /// the word being finished (`decoderTaps`), when Spatial autocorrect reads the space as a
    /// letter ("deli ate" -> "delicate"); nil to keep both. `before` is the text up to the
    /// end of `first`. `first` and `firstTaps` are as you typed them.
    func merge(first: String, firstTaps: [CGPoint], space: CGPoint, second: String, before: String) -> String? {
        guard isEnabled, hasDictionary, let decoder, let keys = decoderKeys, let taps = decoderTaps,
              decoder.model.language == language.rawValue, !first.contains("'"), !second.contains("'") else { return nil }
        for word in [first, second] {
            let lower = word.lowercased()
            if suppressedInSession.contains(lower) || learning.rejected.contains(lower) || learnedWords.contains(lower)
                || Self.shorthandAllowlist.contains(lower) || lexiconTerms.contains(where: { $0.lowercased() == lower }) {
                return nil
            }
        }
        let previous = Self.previousWord(before, token: first)
        guard let merged = decoder.merge(
            first: first, firstTaps: firstTaps, space: space, second: second, secondTaps: taps,
            previous: previous, firstIsValid: knowsAsTyped(first), secondIsValid: knowsAsTyped(second), keys: keys
        ), mayOffer(merged, after: previous) else { return nil }
        return withoutAddedCapitals(Self.matchCase(of: first, onto: merged), typed: first + second)
    }

    /// Your own words for Swipe typing: Contacts names and other `UILexicon` terms, one-word
    /// text replacements, and words you kept twice that may be offered.
    var ownWords: [String] {
        lexiconTerms + lexiconShortcuts.values.filter { !$0.contains(" ") } + learnedWords.words.filter { mayOffer($0) }
    }

    /// The English contractions Hex corrects to or offers ("I'm", "don't", "we'll"), which
    /// Swipe typing reads as common words.
    static let englishContractions: [String] = Set(
        unambiguousFixes.values.filter { $0.contains("'") } + suggestOnlyContractions.values
    ).sorted()

    /// Gives the decoder your own words: Contacts names and other `UILexicon` terms, the
    /// one-word outputs of text replacements, words you kept twice, and typo fixes Hex
    /// learned. Rebuilt only when one of them, or the model, changed.
    ///
    /// Also hands it the words never to offer: the language's offensive words, less the swear
    /// words while Suggest swear words is on. Words Hex learned are left out when offensive;
    /// your `UILexicon` words never are.
    private func syncUserWords() {
        guard let model = decoder?.model else { return }
        let id = ObjectIdentifier(model)
        if let built = userWordsBuilt, built.revision == userWordsRevision, built.model == id { return }
        let learned = (learnedWords.words + Array(learning.typos.values)).filter { mayOffer($0) }
        let words = lexiconTerms + Array(lexiconShortcuts.values) + learned
        decoder?.userWords = TypingDecoder.UserWords(words, model: model)
        decoder?.neverSuggest = TypingDecoder.NeverSuggest(offensiveWords.blocked(allowSwearing: allowsSwearing), model: model)
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
        let language = layoutLanguage
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
        // Polish: diacritics you didn't add, however many ("zolw" -> "żółw", "maly" -> "mały"),
        // are as cheap as one missing Spanish accent, a little more for each further one.
        if language == .polish, s.count == t.count {
            let missed = zip(s, t).filter { $0 != $1 }
            if !missed.isEmpty, missed.allSatisfy({ samePolishLetterIgnoringDiacritics($0, $1) }) {
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

    /// Whether two lowercase letters are one Polish letter with or without its diacritic (a/ą,
    /// z/ż, l/ł: ł has no decomposition, so it is named).
    static func samePolishLetterIgnoringDiacritics(_ a: Character, _ b: Character) -> Bool {
        sameLetterIgnoringAccents(a, b) || Set([a, b]) == ["l", "ł"]
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

    /// Words a period after them makes an abbreviation ("approx.", "etc."): a period never
    /// corrects them.
    static let abbreviations: Set<String> = [
        "approx", "appt", "apt", "assn", "asst", "ave", "blvd", "cf", "co", "corp", "dept", "dr",
        "eg", "est", "etc", "fig", "govt", "ie", "inc", "jr", "lb", "lbs", "ltd", "misc", "mr",
        "mrs", "ms", "mt", "no", "nr", "oz", "pp", "prof", "pt", "rd", "sr", "st", "vol", "vs", "wk", "yr",
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
