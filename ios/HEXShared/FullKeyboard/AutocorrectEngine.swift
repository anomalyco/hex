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
        /// What you typed before autocorrect changed it, offered once the cursor is back at the
        /// end of the corrected words (delete takes only their space): tapping it puts it back.
        case revert
    }

    let displayText: String
    let replacement: String
    let kind: Kind

    var willAutoReplace: Bool {
        if case .primary(let auto) = kind { return auto }
        return false
    }
}

/// Persists words the user explicitly kept twice, bounded to 2,000 words / 64 KB in the App Group.
/// Delete what Hex learned removes them, and a keyboard that loaded them earlier drops its copy.
@MainActor
final class LearnedWordsStore {
    nonisolated static let defaultsKey = "keyboard.learnedWords"
    nonisolated static let scopedPrefix = defaultsKey + "."
    static let maxWords = 2_000
    static let maxBytes = 64 * 1024

    private let defaults: UserDefaults?
    /// The language this list belongs to (`Learn each language separately`); nil is the
    /// shared list every language uses while the experiment is off.
    private let language: String?
    private var storage: [String] = []
    private var wordSet: Set<String> = []
    private var sessionKeepCounts: [String: Int] = [:]
    private var loadedAt = Date().timeIntervalSince1970

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup), language: String? = nil) {
        self.defaults = defaults
        self.language = language
        load()
    }

    /// This list's words for one language, made on first use and seeded from the shared list.
    func scoped(to language: String) -> LearnedWordsStore {
        LearnedWordsStore(defaults: defaults, language: language)
    }

    nonisolated static func key(for language: String) -> String {
        defaultsKey + "." + language
    }

    /// Whether `key` is a language's own list (`keyboard.learnedWords.<code>`).
    nonisolated static func isScopedKey(_ key: String) -> Bool {
        key.hasPrefix(scopedPrefix)
    }

    private var storageKey: String {
        language.map { Self.key(for: $0) } ?? Self.defaultsKey
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
        if let data = defaults?.data(forKey: storageKey), data.count <= Self.maxBytes,
           let decoded = try? JSONDecoder().decode([String].self, from: data) {
            storage = Array(decoded.suffix(Self.maxWords))
            wordSet = Set(storage)
            return
        }
        guard language != nil else { return }
        // A language's list starts with the shared words, and remembers that it was made so a
        // later shared keep doesn't leak into it.
        if let data = defaults?.data(forKey: Self.defaultsKey), data.count <= Self.maxBytes,
           let decoded = try? JSONDecoder().decode([String].self, from: data) {
            storage = Array(decoded.suffix(Self.maxWords))
            wordSet = Set(storage)
        }
        save()
    }

    private func save() {
        guard let defaults else { return }
        while true {
            guard let data = try? JSONEncoder().encode(storage) else { return }
            if data.count <= Self.maxBytes || storage.isEmpty {
                defaults.set(data, forKey: storageKey)
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
    let checker: TextChecking
    /// The words kept in this engine's language: the one shared list, or with Learn each
    /// language separately on, this language's own (`learnsPerLanguage`).
    private(set) var learnedWords: LearnedWordsStore
    private var sharedLearnedWords: LearnedWordsStore
    /// Experiment (Learn each language separately): each keyboard language keeps its own
    /// words, seeded from the shared list, and the partner its own too. Off, one shared list.
    var learnsPerLanguage = false {
        didSet { if learnsPerLanguage != oldValue { scopeLearnedWords() } }
    }
    /// Each language's list once made, so switching back keeps what it holds in memory.
    private var scopedLearnedWords: [String: LearnedWordsStore] = [:]
    /// Settings › Keyboard › Autocorrect. Off, only your text replacements still show in the
    /// strip and apply on space or a closing mark, as Apple's keyboard does with Auto-Correction
    /// off; a field that turns autocorrection off gets none, as there.
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
    var workingPartner: AutocorrectEngine? {
        guard let partner else { return nil }
        if partner.acceptsAnyLetters == nil {
            partner.acceptsAnyLetters = partner.checker.isWordValid("qzxjvkq")
        }
        return partner.acceptsAnyLetters == true ? nil : partner
    }
    private var acceptsAnyLetters: Bool?
    /// The keys you type on, whose neighbours price a slip: `language`'s layout, or a
    /// partner's primary's, in `layoutLetters`'s arrangement (`setKeys`).
    private(set) var layoutLanguage: KeyboardLanguage = .english
    private(set) var layoutLetters: KeyboardLetterLayout?
    private(set) var lexiconShortcuts: [String: String] = [:]
    /// Your `UILexicon` terms, then your vocabulary's words (`AutocorrectEngine+Vocabulary`).
    private(set) var lexiconTerms: [String] = []
    private var systemLexiconTerms: [String] = []
    private(set) var vocabulary: [String] = []
    private(set) var suppressedInSession: Set<String> = []
    /// Learn how I type: typos you fixed the same way twice and words you turned down twice.
    private(set) var learning = TypingLearning()
    private(set) var lastCorrection: AppliedCorrection?
    /// The language of the fix space would apply to the word it was asked about: the
    /// partner's when its fix won, else this engine's; nil for a text replacement.
    private(set) var lastFixLanguage: KeyboardLanguage?
    /// The language of the last revision `revision(of:)` returned: the partner's for its accents.
    private(set) var lastRevisionLanguage: KeyboardLanguage?
    /// Experiment (Spatial autocorrect): reads the word's taps with a language model in place
    /// of `UITextChecker`'s one-edit guesses, for the language its model was built for. Nil
    /// while off or without a model.
    var decoder: TypingDecoder? {
        didSet { if oldValue?.model !== decoder?.model { userWordsBuilt = nil; useOwnWords(decoder?.model) } }
    }
    /// The layout language's word list and word pairs (the typing model, whether or not
    /// Spatial autocorrect is on): English chooses between words the same letters spell with
    /// and without an apostrophe by the words around them (`contextChoice`). Nil without one.
    var contextModel: TypingDecoderModel?
    /// The letter keys for `decoder`, from the current layout.
    var decoderKeys: TypingDecoder.Keys? {
        didSet { partner?.decoderKeys = decoderKeys }
    }
    /// The centres Spatial autocorrect reads with Adaptive touch off (the stock ones, or
    /// moved by Learn how I type), while `decoderKeys` has its personal ones: what space puts
    /// in is a reading these also clear by the margin, else their own decision. Personal
    /// centres rank readings; they never make a correction the keyboard wouldn't without them.
    var decoderBaselineKeys: TypingDecoder.Keys? {
        didSet { partner?.decoderBaselineKeys = decoderBaselineKeys }
    }
    /// Where each letter of the word being typed was tapped, one point per letter; nil when
    /// the keys don't know (the cursor moved, the text came from elsewhere).
    var decoderTaps: [CGPoint]? {
        didSet { partner?.decoderTaps = decoderTaps }
    }
    /// German nouns' shares of capitalised uses (`de.capitals`, beside the German model): the
    /// keyboard reads it with the German model, and a German word before that reads it itself;
    /// never for another language. Tests set their own list.
    var germanCapitals: GermanCapitals? {
        didSet { germanCapitalsRead = true }
    }
    private var germanCapitalsRead = false
    private var germanNouns: GermanCapitals? {
        if !germanCapitalsRead { germanCapitals = GermanCapitals.bundled() }
        return germanCapitals
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
    private var decoderCache: (before: String, taps: [CGPoint], partnerWord: Bool, decision: TypingDecoder.Decision?)?

    init(
        checker: TextChecking? = nil,
        learnedWords: LearnedWordsStore? = nil
    ) {
        let words = learnedWords ?? LearnedWordsStore()
        self.checker = checker ?? SystemTextChecker()
        sharedLearnedWords = words
        self.learnedWords = words
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
        systemLexiconTerms = terms
        lexiconTerms = terms + Self.vocabularyWords(vocabulary)
        userWordsRevision += 1
        partner?.loadLexiconEntries(entries)
    }

    /// Hex's vocabulary list for typing (`AutocorrectEngine+Vocabulary`).
    func setVocabulary(_ terms: [String]) {
        guard terms != vocabulary else { return }
        vocabulary = terms
        lexiconTerms = systemLexiconTerms + Self.vocabularyWords(terms)
        userWordsRevision += 1
        decoderCache = nil
        partner?.setVocabulary(terms)
    }

    /// Your text replacement for `token`, cased as Apple's keyboard writes it (black box, iOS
    /// 27.2): a shortcut typed with a leading capital, yours or auto-capitalisation's ("Brb"),
    /// capitalises the replacement's first letter ("Be right back"); one typed in lowercase or
    /// all caps ("BRB") gets the replacement as stored, and stored capitals always stay
    /// ("mip" and "MIP" both give "My iPhone").
    private func shortcut(for token: String) -> String? {
        guard let stored = lexiconShortcuts[token.lowercased()] else { return nil }
        let letters = token.filter(\.isLetter)
        let leadingCapital = token.first?.isUppercase == true && (letters.count < 2 || letters != letters.uppercased())
        guard leadingCapital, let first = stored.first, first.isLowercase else { return stored }
        return first.uppercased() + stored.dropFirst()
    }

    /// Your text replacement whose shortcut ends `before`, as Apple's keyboard finds one
    /// (black box, iOS 27.2): a shortcut may hold digits and punctuation ("ad2", "s.o"), and may
    /// follow an opening bracket or a comma ("(myem", "hi,myem"), but never the end of a longer
    /// word ("xmyem"). The longest one wins. `typed` is the shortcut as typed, `replacement`
    /// cased as `shortcut(for:)` writes it. Nothing turns one off: Apple's keyboard applies a
    /// text replacement every time, however often you kept the shortcut.
    func textReplacement(before: String?) -> (typed: String, replacement: String)? {
        guard !lexiconShortcuts.isEmpty, let before, before.last.map({ !$0.isWhitespace }) == true else { return nil }
        let chunk = before.suffix(64).reversed().prefix { !$0.isWhitespace }.reversed()
        let joins: (Character) -> Bool = { $0.isLetter || $0.isNumber || $0 == "'" || $0 == "\u{2019}" }
        var start = chunk.startIndex
        while start < chunk.endIndex {
            let atBoundary = start == chunk.startIndex || !joins(chunk[chunk.index(before: start)]) || !joins(chunk[start])
            let typed = String(chunk[start...])
            if atBoundary, let replacement = shortcut(for: typed) {
                return (typed, replacement)
            }
            start = chunk.index(after: start)
        }
        return nil
    }

    func forgetCorrection() {
        lastCorrection = nil
    }

    func recordAppliedCorrection(original: String, corrected: String, anchor: String, changes: [AppliedCorrection.Change]) {
        lastCorrection = AppliedCorrection(original: original, corrected: corrected, changes: changes, anchor: anchor)
    }

    /// The latest autocorrection when the cursor is right after it (`AppliedCorrection.isAtEnd`).
    func revertibleCorrection(before: String?) -> AppliedCorrection? {
        guard let lastCorrection, lastCorrection.isAtEnd(of: before) else { return nil }
        return lastCorrection
    }

    /// Follows the keyboard layout. `available` is `UITextChecker.availableLanguages`.
    func setLanguage(_ language: KeyboardLanguage, available: [String] = UITextChecker.availableLanguages) {
        self.language = language
        scopeLearnedWords()
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

    /// The keys the letters are typed on now (`KeyboardLayout.language` and `letters`), for the
    /// neighbouring-key cost; the partner types on the same ones.
    func setKeys(_ language: KeyboardLanguage, letters: KeyboardLetterLayout) {
        layoutLanguage = language
        layoutLetters = letters
        partner?.layoutLanguage = language
        partner?.layoutLetters = letters
    }

    /// Bilingual autocorrect's second language (`KeyboardLanguageList.partner`), or nil. It
    /// shares your words (`learnedWords`, text replacements, Learn how I type) and types on
    /// this engine's keys, except with Learn each language separately: then it keeps its own
    /// language's words. None without a dictionary for either language. `checker` is for
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
        let engine = AutocorrectEngine(
            checker: checker ?? SystemTextChecker(), learnedWords: learnsPerLanguage ? scopedWords(other) : sharedLearnedWords
        )
        engine.setLanguage(other, available: available)
        engine.layoutLanguage = layoutLanguage
        engine.layoutLetters = layoutLetters
        engine.lexiconShortcuts = lexiconShortcuts
        engine.systemLexiconTerms = systemLexiconTerms
        engine.vocabulary = vocabulary
        engine.lexiconTerms = lexiconTerms
        if !learnsPerLanguage { engine.learning = learning }
        engine.allowsSwearing = allowsSwearing
        engine.addsCapitals = addsCapitals
        engine.decoderKeys = decoderKeys
        engine.decoderBaselineKeys = decoderBaselineKeys
        engine.decoderTaps = decoderTaps
        partner = engine
    }

    /// Gives this engine and its partner the list for their languages while Learn each
    /// language separately is on, or the one shared list while it is off.
    private func scopeLearnedWords() {
        let store = learnsPerLanguage ? scopedWords(language) : sharedLearnedWords
        if store !== learnedWords {
            learnedWords = store
            userWordsRevision += 1
        }
        guard let partner else { return }
        let theirs = learnsPerLanguage ? scopedWords(partner.language) : sharedLearnedWords
        partner.sharedLearnedWords = theirs
        if theirs !== partner.learnedWords {
            partner.learnedWords = theirs
            partner.userWordsRevision += 1
        }
    }

    private func scopedWords(_ language: KeyboardLanguage) -> LearnedWordsStore {
        if let store = scopedLearnedWords[language.rawValue] { return store }
        let store = sharedLearnedWords.scoped(to: language.rawValue)
        scopedLearnedWords[language.rawValue] = store
        return store
    }

    /// Hands over what the journal learned. With Learn each language separately on, the
    /// partner gets `partnerLearning` (its own language's) instead of these.
    func applyLearning(_ learning: TypingLearning, partner partnerLearning: TypingLearning? = nil) {
        if self.learning != learning {
            self.learning = learning
            userWordsRevision += 1
        }
        if learnsPerLanguage, let partnerLearning {
            if let partner, partner.learning != partnerLearning {
                partner.learning = partnerLearning
                partner.userWordsRevision += 1
            }
            return
        }
        partner?.applyLearning(learning)
    }

    func suppressAndKeep(_ word: String) {
        suppressedInSession.insert(word.lowercased())
        learnedWords.recordExplicitKeep(word)
        userWordsRevision += 1
        // The partner never learns this keep itself; it only drops its copy of your words.
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

    /// The empty field, or after a sentence's end and a space, or a new line. In Greek, ; is
    /// the question mark and ends a sentence too.
    static func isSentenceStart(_ beforeInput: String?, language: KeyboardLanguage = .english) -> Bool {
        guard let beforeInput else { return true }
        guard let last = beforeInput.last else { return true }
        if last.isNewline { return true }
        guard last == " " else { return false }
        var trimmed = Substring(beforeInput)
        while trimmed.last == " " { trimmed = trimmed.dropLast() }
        guard let written = trimmed.last else { return true }
        return written.isNewline || written == "." || written == "!" || written == "?"
            || (language == .greek && (written == ";" || written == "\u{037E}"))
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
        case .czech: ["to", "já", "ale"]
        case .hungarian: ["ez", "én", "a"]
        case .romanian: ["eu", "nu", "am"]
        case .norwegian: ["jeg", "det", "vi"]
        case .russian: ["я", "это", "мы"]
        case .ukrainian: ["я", "це", "ми"]
        case .greek: ["θα", "δεν", "το"]
        case .serbianLatin: ["ja", "to", "da"]
        case .korean: []
        }
    }

    /// Marks after which a sentence goes on.
    static let midSentenceMarks: Set<Character> = [",", ";", ":", ")", "\"", "'", "\u{201D}", "\u{2019}", "\u{2014}"]

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
        case .czech: ["a", "ale", "že"]
        case .hungarian: ["és", "de", "hogy"]
        case .romanian: ["și", "dar", "că"]
        case .norwegian: ["og", "men", "jeg"]
        case .russian: ["и", "но", "что"]
        case .ukrainian: ["і", "але", "що"]
        case .greek: ["και", "αλλά", "που"]
        case .serbianLatin: ["i", "ali", "da"]
        case .korean: []
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
        case .czech: ["a", "se", "je", "v", "na", "to"]
        case .hungarian: ["a", "az", "és", "hogy", "nem", "egy"]
        case .romanian: ["de", "și", "în", "nu", "să", "la"]
        case .norwegian: ["og", "i", "det", "er", "på", "som"]
        case .russian: ["и", "в", "не", "на", "я", "что"]
        case .ukrainian: ["і", "в", "не", "на", "що", "я"]
        case .greek: ["και", "να", "το", "θα", "δεν", "η"]
        case .serbianLatin: ["i", "je", "da", "u", "se", "na"]
        case .korean: []
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
        lastFixLanguage = nil
        guard field == .text, autocorrectionType != .no else { return [] }
        // Your text replacement shows as Apple's keyboard shows it ("myem": "myem", then the
        // address Space puts in), in any language and whatever the word's history.
        let replaced = textReplacement(before: beforeInput)
        let replacementOnly = replaced.map { typed, replacement in
            [
                KeyboardSuggestion(displayText: "“\(typed)”", replacement: typed, kind: .keepLiteral),
                KeyboardSuggestion(displayText: replacement, replacement: replacement, kind: .primary(autoReplace: true)),
            ]
        }
        guard hasWords else { return replacementOnly ?? [] }
        // An empty field, or a new sentence (". ", "! ", "? ", a new line): three common
        // first words, as Apple's keyboard offers.
        if Self.isSentenceStart(beforeInput, language: language) {
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
        guard let token = Self.currentToken(in: beforeInput) else { return replacementOnly ?? [] }
        // A shortcut with digits or punctuation ("ad2", "s.o"): only it.
        if let replaced, replaced.typed.count != token.count { return replacementOnly ?? [] }
        self.beforeInput = beforeInput

        let lower = token.lowercased()
        // Chat shorthand ("brb") is left alone.
        let allowAuto = !suppressedInSession.contains(lower)
            && !learning.rejected.contains(lower)
            && !learnedWords.contains(lower)
            && !Self.shorthandAllowlist.contains(lower)

        partner?.beforeInput = beforeInput
        // Autocorrect off: your text replacements only.
        let autoFix = replaced?.replacement ?? (allowAuto && isEnabled ? cautiousReplacement(for: token) : nil)
        var pool: [String] = autoFix.map { [$0] } ?? []

        if let partner = workingPartner {
            // Both languages' candidates, those of the language the words before are in
            // first; spelling guesses only for a word neither knows.
            let mine = candidates(for: token, autoFix: autoFix, guesses: !partner.knowsAsTyped(token), limit: 2, model: predictionModel, corrects: isEnabled)
            let theirs = partner.candidates(for: token, autoFix: autoFix, guesses: !knowsAsTyped(token), limit: 2, model: partner.decoder?.model, corrects: isEnabled)
            // A sentence in one language (`languageLean`) gets only that language's words, even
            // when it has none (TestFlight build 56: "autocomplete suggests English words in
            // Czech too"); without one, both, in turn.
            let lean = languageLean(partner, token: token)
            let ordered = lean < 0 ? [mine] : lean > 0 ? [theirs]
                : (0..<max(mine.count, theirs.count)).map { i in [mine, theirs].compactMap { i < $0.count ? $0[i] : nil } }
            for item in ordered.joined()
            where pool.count < 2 && !pool.contains(where: { $0.caseInsensitiveCompare(item) == .orderedSame }) {
                pool.append(item)
            }
        } else {
            pool += candidates(for: token, autoFix: autoFix, guesses: true, limit: 2 - pool.count, model: predictionModel, corrects: isEnabled)
        }

        pool = withoutAddedCapitals(pool, typed: token)
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
    ///
    /// `corrects` false (Autocorrect off): completions only, as Apple's keyboard with
    /// Auto-Correction off offers "Imagine" for "im" but not "I'm", and nothing for "teh" or
    /// "dont" (black box, iOS 27.2).
    private func candidates(
        for token: String, autoFix: String?, guesses: Bool, limit: Int, model: TypingDecoderModel?, corrects: Bool = true
    ) -> [String] {
        let lower = token.lowercased()
        var pool: [String] = autoFix.map { [$0] } ?? []
        let start = pool.count
        let end = start + max(0, limit)
        func has(_ item: String) -> Bool {
            pool.contains { $0.caseInsensitiveCompare(item) == .orderedSame }
        }

        if corrects, autoFix == nil {
            if let learned = learning.typos[lower], mayOffer(learned, replacing: token) {
                // A learned fix space won't apply (the typo is a real word, or you kept it) is still offered.
                pool.append(cased(learned, typed: token))
            } else if language.usesEnglishFixes, let suggested = Self.suggestOnlyContractions[lower] {
                pool.append(Self.matchCase(of: token, onto: suggested))
            } else if !suppressedInSession.contains(lower), !learning.rejected.contains(lower),
                      !learnedWords.contains(lower), let accented = language.accentSuggestions[lower] {
                // Offered, never put in by space: the plain spelling is a word of its own
                // ("e" is "and", "ce" is "to us"), as the contractions above are.
                pool.append(cased(accented, typed: token))
            }
        }
        // Your vocabulary's word that starts with what you typed, before the dictionary's.
        if pool.count < end, let term = vocabularyCompletion(for: token), !has(term) { pool.append(term) }

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
        if corrects, let decision = decoderDecision(for: token) {
            for reading in decision.readings where pool.count < end && mayOffer(reading.text, replacing: token) {
                let text = cased(reading.text, typed: token)
                if !has(text) {
                    pool.append(text)
                }
            }
        }

        // Spelling guesses for a word the dictionary doesn't know.
        if corrects && guesses && pool.count < end && token.count >= 2 {
            if !knowsAsTyped(token) {
                for guess in rankedGuesses(for: token) {
                    let fixed = cased(guess, typed: token)
                    if fixed.caseInsensitiveCompare(token) != .orderedSame, mayOffer(guess, replacing: token), !has(fixed) {
                        pool.append(fixed)
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
        return (common + yours + names).map { cased($0, typed: token) }
    }

    /// Returns `(tokenToDelete, replacement)` only when cautious autocorrect is confident, or
    /// for a text replacement (with Autocorrect off too).
    func replacementOnSpace(
        beforeInput: String?,
        field: KeyboardFieldKind,
        autocorrectionType: UITextAutocorrectionType?
    ) -> (original: String, replacement: String)? {
        lastFixLanguage = nil
        guard field == .text, autocorrectionType != .no else { return nil }
        // Your own text replacement comes first, as on Apple's keyboard: before the chat
        // shorthand Hex leaves alone ("brb"), with Autocorrect off or no dictionary, and
        // however often you kept the shortcut.
        if let replaced = textReplacement(before: beforeInput) ?? pronounCapital(before: beforeInput) { return (replaced.typed, replaced.replacement) }
        guard let token = Self.currentToken(in: beforeInput) else { return nil }
        self.beforeInput = beforeInput
        partner?.beforeInput = beforeInput
        let lower = token.lowercased()
        guard !suppressedInSession.contains(lower),
              !learning.rejected.contains(lower),
              !learnedWords.contains(lower) else { return nil }
        guard isEnabled, hasWords, !Self.shorthandAllowlist.contains(lower) else { return nil }
        guard let fix = cautiousReplacement(for: token) else { return nil }
        return (token, fix)
    }

    /// What space puts in place of `token`, if anything. With a partner (bilingual
    /// autocorrect), a word either language knows as typed stays as it is (your text
    /// replacements still apply), except a sure fix of this language's (`isSureFix`) once
    /// the words before are in it: the other dictionary takes many of its typos for words
    /// ("dont", "im", "dk" in French, "im", "tkm" in German), and kept them. A typo either can
    /// fix gets the fix, though never the other's guess of a name ("Zuri" for "Ziri"); when both
    /// can, the fix in the language of the words before it, else the cheaper slip on these
    /// keys, else the keyboard's own language's. Held-out English benchmark, French beside it:
    /// 7.56% WER before, 5.79% now, against 5.75% alone.
    private func cautiousReplacement(for token: String) -> String? {
        if let shortcut = shortcut(for: token) {
            lastFixLanguage = nil
            return shortcut
        }
        if let term = vocabularyTerm(for: token) {
            lastFixLanguage = nil
            return Self.vocabularySpelling(of: token, as: term)
        }
        // This language's own accented spelling decides before any partner logic: the table
        // only holds plain spellings that are no word of this language ("perche" -> "perché"),
        // so an English lean must not hand it to the other dictionary's guess.
        if let accented = accentRestoration(for: token) {
            lastFixLanguage = language
            return accented
        }
        guard let partner = workingPartner else {
            lastFixLanguage = language
            return ownReplacement(for: token)
        }
        let lean = languageLean(partner, token: token)
        // One-letter words are left to the keyboard's own language: `UITextChecker` takes
        // every letter as a word, so English "i" would never become "I" beside French.
        if token.count >= 2, partner.knowsAsTyped(token), Self.hasVowel(token) {
            if lean < 0, isSureFix(token), let own = ownReplacement(for: token) {
                lastFixLanguage = language
                return own
            }
            // Spatial autocorrect may still read the taps as another word, on a clear margin, as
            // it does its own: "tkm" (German) becomes "Tom", "merci" stays. Not a sure fix's word,
            // which the next word decides ("Im" stays for "I'm going", not "In"), and never just
            // its accents: the other's word is spelled right ("these" stays, not French "thèse").
            guard lean <= 0, !isAllCapsOrMixed(token),
                  !(language.usesEnglishFixes && Self.unambiguousFixes[token.lowercased()] != nil),
                  let replacement = decoderDecision(for: token, partnerWord: true)?.replacement,
                  TypingDecoder.withoutAccents(replacement.lowercased()) != TypingDecoder.withoutAccents(token.lowercased()), mayOffer(replacement, replacing: token) else {
                lastFixLanguage = nil
                return nil
            }
            lastFixLanguage = language
            return withoutAddedCapitals(cased(replacement, typed: token), typed: token)
        }
        let own = ownReplacement(for: token)
        // A name the other dictionary guesses ("Zuri" for "Ziri") is no fix of yours.
        guard token.count >= 2, !knowsAsTyped(token), let theirs = partner.ownReplacement(for: token),
              !partner.isName(theirs) else {
            lastFixLanguage = own == nil ? nil : language
            return own
        }
        guard let own, own.caseInsensitiveCompare(theirs) != .orderedSame else {
            lastFixLanguage = own == nil ? partner.language : language
            return own ?? theirs
        }
        if prefersPartner(lean, token: token, own: own, theirs: theirs) {
            lastFixLanguage = partner.language
            return theirs
        }
        lastFixLanguage = language
        return own
    }

    /// Whether this language's dictionary knows `word` only with a capital, and German's noun
    /// list doesn't call it a noun: a name.
    private func isName(_ word: String) -> Bool {
        let lower = word.lowercased()
        guard lower.count >= 2, let first = lower.first else { return false }
        guard checker.isWordValid(first.uppercased() + lower.dropFirst()), !checker.isWordValid(lower) else { return false }
        return !(language == .german && germanNouns?.capitalises(lower, after: nil) == true)
    }

    /// Whether `token` has a vowel. iOS's French and German dictionaries take abbreviations
    /// without one ("dk", "dl", "tkm"), which beside English kept its slips ("Tkm" for "Tom"),
    /// so a partner's word without a vowel doesn't count as a word. Serbian's vowelless words
    /// ("prst") never meet this: it has no dictionary, so it is never a partner.
    static func hasVowel(_ token: String) -> Bool {
        token.lowercased().folding(options: .diacriticInsensitive, locale: nil).contains { "aeiouy".contains($0) }
    }

    /// ALL-CAPS ("API") or with capitals inside ("iOS"), which Spatial autocorrect leaves alone.
    private func isAllCapsOrMixed(_ token: String) -> Bool {
        token.dropFirst().contains(where: \.isUppercase)
    }

    /// A fix this language makes of `token` even where the other one knows the word: a
    /// contraction typed without its apostrophe or a common slip (`unambiguousFixes`: "dont",
    /// "im"), or a two-letter word ("dk" -> "do"), which in the other dictionary is mostly an
    /// abbreviation. Longer words of the other language ("est", "merci") stay as typed.
    private func isSureFix(_ token: String) -> Bool {
        token.count == 2 || (language.usesEnglishFixes && Self.unambiguousFixes[token.lowercased()] != nil)
    }

    /// Bilingual autocorrect: `word`, which the partner's dictionary knew, was left as typed
    /// at a sentence's start (no words before to tell the languages apart), and `next` now
    /// leans this language (`languageLean`): its sure fix (`unambiguousFixes`), as "Im
    /// going" becomes "I'm going" and "Dont do" "Don't do", while German "Im Haus" stays.
    private func bilingualRevision(of word: String, before: String, next: String) -> String? {
        guard let partner = workingPartner, language.usesEnglishFixes, !word.contains("'"),
              let fixed = Self.unambiguousFixes[word.lowercased()], word.count >= 2 else { return nil }
        let lower = word.lowercased()
        guard !suppressedInSession.contains(lower), !learning.rejected.contains(lower), !learnedWords.contains(lower),
              partner.knowsAsTyped(word), next.count >= 2, languageLean(partner, before: next, commonFrom: 2) < 0 else { return nil }
        let rest = before.hasSuffix(word) ? String(before.dropLast(word.count)) : before
        guard languageLean(partner, before: rest) == 0, mayOffer(fixed, after: Self.previousWord(before, token: word)) else { return nil }
        return withoutAddedCapitals(Self.matchCase(of: word, onto: fixed), typed: word)
    }

    /// Whether `token` is a word in this language as typed: the dictionary's, one of your
    /// own (`UILexicon`, kept twice) or chat shorthand. A name it knows only capitalised
    /// doesn't count ("paris").
    func knowsAsTyped(_ token: String) -> Bool {
        let lower = token.lowercased()
        return Self.shorthandAllowlist.contains(lower) || learnedWords.contains(lower)
            || lexiconTerms.contains { $0.lowercased() == lower } || checker.isWordValid(token)
    }

    /// Which language the words before `token` are in: above zero the partner's, below this
    /// one's, counting the last four that only one of the dictionaries knows, or, of three
    /// letters or more, that both know but only one language's typing model has among its
    /// common words: iOS's French and German dictionaries take "the", "you", "your", "not"
    /// and "please" (iOS 27.2 simulator), so English sentences rarely leaned English.
    private func languageLean(_ partner: AutocorrectEngine, token: String) -> Int {
        let before = (beforeInput ?? "").suffix(120)
        return languageLean(partner, before: String(before.hasSuffix(token) ? before.dropLast(token.count) : before))
    }

    /// `commonFrom`: the fewest letters a word both dictionaries know needs to count by the
    /// typing models. Three, as French "de", "la" and "et" are common in the English model's
    /// books, except for the word after a sentence's first (`bilingualRevision`: "Im in").
    func languageLean(_ partner: AutocorrectEngine, before: String, commonFrom: Int = 3) -> Int {
        var lean = 0
        for word in before.suffix(120).split(whereSeparator: { !$0.isLetter && $0 != "'" }).suffix(4) where word.count >= 2 {
            let mine = checker.isWordValid(String(word))
            let theirs = partner.checker.isWordValid(String(word))
            if theirs && !mine {
                lean += 1
            } else if mine && !theirs {
                lean -= 1
            } else if mine, word.count >= commonFrom {
                let myCommon = isCommon(String(word))
                if myCommon != partner.isCommon(String(word)) { lean += myCommon ? -1 : 1 }
            }
        }
        return lean
    }

    /// Whether this language's typing model (`contextModel`, else Spatial autocorrect's)
    /// has `word` among its commonest, about the top 2,000 (-ln P at most 10 nats: English
    /// "please" 9.25, "capable" 9.9). False without a model for the language.
    private func isCommon(_ word: String) -> Bool {
        guard let model = contextModel ?? decoder?.model, model.language == language.rawValue,
              let index = model.index(of: word.lowercased()) else { return false }
        return model.unigramCost(index) <= Self.commonWordCost
    }

    static let commonWordCost = 10.0

    /// Both languages can fix the typo: the partner's fix when the words before are in its
    /// language, or, without a lean, when its fix is clearly the cheaper slip on these keys.
    private func prefersPartner(_ lean: Int, token: String, own: String, theirs: String) -> Bool {
        if lean != 0 { return lean > 0 }
        let lower = token.lowercased()
        func cost(_ fix: String) -> Double {
            let normalized = fix.replacingOccurrences(of: "'", with: "").lowercased()
            return Self.singleEditKeyboardCost(from: lower, to: normalized, language: layoutLanguage, letters: layoutLetters) ?? 1.0
        }
        return cost(theirs) + 0.2 < cost(own)
    }

    /// `cautiousReplacement` in this language alone.
    func ownReplacement(for token: String) -> String? {
        let lower = token.lowercased()
        // 1. Explicit iOS Text Replacement (`omw` -> `On my way!`).
        if let shortcut = shortcut(for: token) {
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
            return cased(learned, typed: token)
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
                var cased = self.cased(replacement, typed: token)
                // A missed space after English "i" ("iwant") gives "I want", as "i" alone does.
                if language.usesEnglishFixes, cased.contains(" ") {
                    cased = cased.split(separator: " ").map { $0 == "i" ? "I" : String($0) }.joined(separator: " ")
                }
                // German spells every noun with a capital, and the German model is lowercase
                // throughout like the English one: a corrected word the dictionary knows only
                // capitalised, or `de.capitals` calls a noun, takes its capital ("hsus" -> "haus"
                // -> "Haus"), as the no-replacement path does below. One word only: a split
                // reading keeps its second word as read.
                if addsCapitals, !cased.contains(" "), decoder?.model.language == KeyboardLanguage.german.rawValue {
                    cased = nounCapitalised(for: cased, typed: token) ?? cased
                }
                return withoutAddedCapitals(cased, typed: token)
            }
            // A word the model knows lowercase that the dictionary knows only capitalised
            // (a name, day or place), or that German's `de.capitals` calls a noun, typed in
            // lowercase, is capitalised as the dictionary path does ("mary" -> "Mary",
            // "haus" -> "Haus"), unless Auto-capitalization is off.
            return addsCapitals ? nounCapitalised(for: token, typed: token) : nil
        }

        // 3. A two-letter non-word one neighbouring key from a very common two-letter word
        // ("tk" -> "to", Spanish "nl" -> "no"); `UITextChecker` guesses skip two letters.
        if token.count == 2, !token.contains("'"), let fix = commonTwoLetterFix(for: lower),
           !checker.isWordValid(token) {
            return Self.matchCase(of: token, onto: fix)
        }

        // 4. A word typed without its accent where the plain spelling is no word of the
        // language ("perche" -> "perché", "citta" -> "città"): the dictionary path below
        // needs three letters and never rewrites a word it knows, and a language's own list
        // is deterministic. Ambiguous pairs ("e"/"è", "ce"/"c'è") are suggestion-only.
        if let accented = accentRestoration(for: token) {
            return accented
        }

        // 5. Single-edit `UITextChecker` typo on words of 3+ characters, never rewriting valid words
        // (a French elision's sides each as a word, `frenchElisionFix`).
        if token.contains("'") { return frenchElisionFix(for: token) }
        guard token.count >= 3 else { return nil }
        guard !checker.isWordValid(token) else { return nil }

        var bestGuess: String?
        var bestScore = Double.greatestFiniteMagnitude
        let guesses = checker.guesses(for: token)
        for (rank, checkerGuess) in guesses.prefix(8).enumerated() {
            guard let rawGuess = usableGuess(checkerGuess, typed: token, among: guesses), mayOffer(rawGuess, replacing: token) else { continue }
            let normalizedGuess = rawGuess.replacingOccurrences(of: "'", with: "").lowercased()
            guard let editCost = Self.singleEditKeyboardCost(from: lower, to: normalizedGuess, language: layoutLanguage, letters: layoutLetters) else { continue }
            let totalScore = editCost + Double(rank) * 0.08
            if totalScore < bestScore {
                bestScore = totalScore
                bestGuess = rawGuess
            }
        }
        guard let bestGuess, bestScore <= 1.35 else { return nil }
        return withoutAddedCapitals(cased(bestGuess, typed: token), typed: token)
    }

    /// The language's own accented spelling of `token`, where the plain spelling is no word
    /// of it ("perche" -> "perché"), or nil. All-caps and mixed-case words are left alone,
    /// as the dictionary path leaves them, and the table only holds non-words of this
    /// language, so it may decide before a bilingual partner's dictionary does.
    private func accentRestoration(for token: String) -> String? {
        let isAllCaps = token.count >= 2 && token == token.uppercased() && token != token.lowercased()
        guard !isAllCaps, !token.dropFirst().contains(where: \.isUppercase) else { return nil }
        let lower = token.lowercased()
        guard let accented = language.accentRestorations[lower], mayOffer(accented, replacing: token) else {
            return nil
        }
        return withoutAddedCapitals(cased(accented, typed: token), typed: token)
    }

    /// `candidate` cased as `token` was typed (`matchCase`), less a capital it only has
    /// from where it was learned (`withoutStrayCapital`).
    private func cased(_ candidate: String, typed token: String) -> String {
        withoutStrayCapital(Self.matchCase(of: token, onto: candidate), typed: token)
    }

    /// `fix` for `token` as autocorrect puts it in: with `addsCapitals` off and nothing
    /// typed in capitals, all lowercase unless it is your own spelling; nil when that leaves
    /// just what you typed ("i").
    func withoutAddedCapitals(_ fix: String, typed token: String) -> String? {
        guard !addsCapitals, !token.contains(where: \.isUppercase), !isOwnSpelling(fix) else { return fix }
        let lowered = fix.lowercased()
        return lowered == token ? nil : lowered
    }

    /// Whether `word`, capitals and all, is one of your own spellings: a Contacts name or
    /// other `UILexicon` term, a text replacement's output, a word you kept twice, or a fix
    /// Hex learned from you. Auto-capitalization off leaves their capitals alone, except a
    /// capital that `withoutStrayCapital` drops: a Contacts name or learned fix that is also a
    /// common word ("Will", "World") stays lowercase mid-sentence either way.
    func isOwnSpelling(_ word: String) -> Bool {
        lexiconShortcuts.values.contains(word)
            || ((lexiconTerms.contains(word) || learning.typos.values.contains(word) || learnedWords.words.contains(word))
                && withoutStrayCapital(word, typed: "") == word)
    }

    /// The very common two-letter word (the language's `commonTwoLetterWords`, commonest
    /// first) one neighbouring key or one swap away from `lower`, if any.
    private func commonTwoLetterFix(for lower: String) -> String? {
        var best: (word: String, cost: Double)?
        for word in language.commonTwoLetterWords {
            guard let cost = Self.singleEditKeyboardCost(from: lower, to: word, language: layoutLanguage, letters: layoutLetters),
                  cost > 0, cost <= 0.65 else { continue }
            if best == nil || cost < best!.cost { best = (word, cost) }
        }
        return best?.word
    }

    /// What spatial autocorrect makes of `token`'s taps, or nil when it doesn't apply: off,
    /// another language's model, an apostrophe or one letter typed, a letter the model lacks,
    /// or taps that don't match the word.
    /// `partnerWord`: the bilingual partner knows `token` as typed, so it counts as a word.
    private func decoderDecision(for token: String, partnerWord: Bool = false) -> TypingDecoder.Decision? {
        syncUserWords()
        guard let decoder, let keys = decoderKeys, let taps = decoderTaps, decoder.model.language == language.rawValue,
              token.count >= 2, taps.count == token.count, !token.contains("'") else { return nil }
        let before = beforeInput ?? ""
        if let cache = decoderCache, cache.before == before, cache.taps == taps, cache.partnerWord == partnerWord { return cache.decision }
        let lower = token.lowercased()
        // Whether what you typed is a word at all, which `properNoun` already answers for a noun
        // the dictionary knows only capitalised. German's list only decides the capital its
        // callers apply to the reading; it never makes a word the dictionary lacks untouchable
        // here.
        let valid = Self.shorthandAllowlist.contains(lower) || learnedWords.contains(lower)
            || lexiconTerms.contains { $0.lowercased() == lower } || checker.isWordValid(token)
            || properNoun(for: token) != nil || partnerWord
        let previous = Self.previousWord(before, token: token)
        var decision = decisionOfferingOwnCapitals(decoder, taps: taps, typed: token, previous: previous, valid: valid, keys: keys)
        if let baseline = decoderBaselineKeys, let replacement = decision?.replacement {
            func clears(_ reading: String) -> Bool? {
                decoder.replaces(with: reading, taps: taps, typed: lower, previous: previous, literalIsValid: valid, keys: baseline)
            }
            switch clears(replacement) {
            case true?:
                break
            case false?:
                // The next reading the baseline would also put in, if any: never a new rewrite,
                // and no second search on the letter.
                let readings = decision?.readings ?? []
                decision?.replacement = readings.lazy.map(\.text).first { $0 != replacement && !isOwnCapitals($0, typed: token) && clears($0) == true }
            case nil:
                decision?.replacement = decisionOfferingOwnCapitals(decoder, taps: taps, typed: token, previous: previous, valid: valid, keys: baseline)?.replacement
            }
        }
        decoderCache = (before, taps, partnerWord, decision)
        return decision
    }

    /// `token` capitalised when it is all lowercase and the dictionary knows it only that way
    /// (a name, day or place: "mary" -> "Mary", "monday" -> "Monday"; a German noun:
    /// "haus" -> "Haus"), else nil.
    private func properNoun(for token: String) -> String? {
        guard token.count >= 2, token == token.lowercased(), let first = token.first else { return nil }
        let capitalised = first.uppercased() + token.dropFirst()
        guard checker.isWordValid(capitalised), !checker.isWordValid(token) else { return nil }
        return capitalised
    }

    /// `properNoun`, with German's noun list in front of the dictionary: a word the books write
    /// with a capital almost always takes one anywhere ("haus" -> "Haus"), and one they write
    /// that way half the time takes one after an article or determiner ("das leben" -> "das
    /// Leben"; "zu leben" stays). Without a determiner a 50-94% word still takes a capital
    /// where only the capital is a word ("abbas" -> "Abbas", "heute abend" -> "heute Abend").
    /// `typed` is what you typed, which finds the word before it when a correction changed
    /// `word`. Words the list doesn't know, and every other language, keep the dictionary's
    /// rule.
    private func nounCapitalised(for word: String, typed token: String) -> String? {
        guard language == .german, let capitals = germanNouns,
              let capitalise = capitals.capitalises(word, after: Self.previousWord(beforeInput ?? "", token: token)),
              capitalise, let first = word.first
        else { return properNoun(for: word) }
        return first.uppercased() + word.dropFirst()
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
        guard field == .text, autocorrectionType != .no, hasWords, word.count >= 2 else { return nil }
        let isAllCaps = word == word.uppercased() && word != word.lowercased() && !word.hasPrefix("I'")
        guard !isAllCaps, let nextWord = next.split(separator: " ").first.map(String.init) else { return nil }
        let previous = Self.previousWord(before, token: word)
        lastRevisionLanguage = language
        if let fixed = bilingualRevision(of: word, before: before, next: nextWord) { return fixed }
        if let (fixed, into) = accentRevision(of: word, before: before, next: nextWord) { lastRevisionLanguage = into; return fixed }
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
        // Adaptive touch's personal centres never join words: the centres read without it decide.
        guard isEnabled, hasWords, let decoder, let keys = decoderBaselineKeys ?? decoderKeys, let taps = decoderTaps,
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
        let letters = layoutLetters
        let guesses = checker.guesses(for: token)
        return guesses
            .compactMap { usableGuess($0, typed: token, among: guesses) }
            .prefix(8)
            .enumerated()
            .sorted { a, b in
                let costA = Self.singleEditKeyboardCost(
                    from: lower,
                    to: a.element.replacingOccurrences(of: "'", with: "").lowercased(),
                    language: language,
                    letters: letters
                ) ?? (2.0 + Double(abs(a.element.count - token.count)) * 0.3)
                let costB = Self.singleEditKeyboardCost(
                    from: lower,
                    to: b.element.replacingOccurrences(of: "'", with: "").lowercased(),
                    language: language,
                    letters: letters
                ) ?? (2.0 + Double(abs(b.element.count - token.count)) * 0.3)
                return (costA + Double(a.offset) * 0.08) < (costB + Double(b.offset) * 0.08)
            }
            .map(\.element)
    }
}
