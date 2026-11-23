import HexKeyboardCore
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
    /// Bumped whenever `words` changes, so autocorrect's core gets the new list.
    private(set) var revision = 0

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
        revision += 1
        while storage.count > Self.maxWords {
            let evicted = storage.removeFirst()
            wordSet.remove(evicted)
        }
        save()
    }

    /// Drops the words if Delete what Hex learned erased them since this store read them.
    func refresh() {
        dropIfErased()
    }

    private func dropIfErased() {
        let erasedAt = TypingJournalSettings(defaults: defaults).erasedAt
        guard erasedAt > loadedAt else { return }
        revision += 1
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
            revision += 1
        }
    }
}


/// Cautious on-device suggestions and replace-on-space: the keyboard core's autocorrect
/// (`crates/keyboard/core/src/autocorrect`, `hk_autocorrect_*`), which decides everything,
/// with Apple's spelling dictionary behind a callback (`TextChecking`), and your `UILexicon`
/// terms, vocabulary, kept words (`LearnedWordsStore`, kept here on disk) and settings handed
/// over as data. This class keeps the Swift API the keys and tests call.
@MainActor
final class AutocorrectEngine {
    let checker: TextChecking
    /// The core's handle.
    let core: KeyboardCoreAutocorrect
    /// The words kept in this engine's language: the one shared list, or with Learn each
    /// language separately on, this language's own (`learnsPerLanguage`).
    private(set) var learnedWords: LearnedWordsStore
    private var sharedLearnedWords: LearnedWordsStore
    /// Experiment (Learn each language separately): each keyboard language keeps its own
    /// words, seeded from the shared list, and the partner its own too. Off, one shared list.
    var learnsPerLanguage = false {
        didSet { if learnsPerLanguage != oldValue { pushSettings(); scopeLearnedWords() } }
    }
    /// Each language's list once made, so switching back keeps what it holds in memory.
    private var scopedLearnedWords: [String: LearnedWordsStore] = [:]
    /// Settings › Keyboard › Autocorrect. Off, only your text replacements still show in the
    /// strip and apply on space or a closing mark, as Apple's keyboard does with Auto-Correction
    /// off; a field that turns autocorrection off gets none, as there.
    var isEnabled = true {
        didSet { pushSettings() }
    }
    /// The keyboard layout's language: its dictionary, and the core's manifest for it.
    private(set) var language: KeyboardLanguage = .english
    /// False when the iPhone has no spelling dictionary for `language`.
    var hasDictionary: Bool { core.ask(4) }
    /// Bilingual autocorrect: the other Latin-script language in Settings › Keyboard ›
    /// Languages, with its own dictionary (and Spatial autocorrect's model) on these keys. Nil
    /// with one language.
    private(set) var partner: Partner?
    /// The keys you type on, whose neighbours price a slip: `language`'s layout, or a
    /// partner's primary's, in `layoutLetters`'s arrangement (`setKeys`).
    private(set) var layoutLanguage: KeyboardLanguage = .english
    private(set) var layoutLetters: KeyboardLetterLayout?
    /// Your `UILexicon` terms (`loadLexiconEntries`) and Hex's vocabulary list.
    private(set) var vocabulary: [String] = []
    /// Learn how I type: typos you fixed the same way twice and words you turned down twice.
    private(set) var learning = TypingLearning()
    private(set) var lastCorrection: AppliedCorrection?
    /// The language of the fix space would apply to the word it was asked about: the
    /// partner's when its fix won, else this engine's; nil for a text replacement.
    var lastFixLanguage: KeyboardLanguage? { core.text(5).flatMap(KeyboardLanguage.init(rawValue:)) }
    /// The language of the last revision `revision(of:)` returned: the partner's for its accents.
    var lastRevisionLanguage: KeyboardLanguage? { core.text(6).flatMap(KeyboardLanguage.init(rawValue:)) }
    /// Experiment (Spatial autocorrect): reads the word's taps with a language model in place
    /// of `UITextChecker`'s one-edit guesses, for the language its model was built for. Nil
    /// while off or without a model.
    var decoder: TypingDecoder? {
        didSet {
            core.model(HK_AUTOCORRECT_DECODER, decoder?.model)
            if oldValue?.model !== decoder?.model { useOwnWords(decoder?.model) }
        }
    }
    /// The layout language's word list and word pairs (the typing model, whether or not
    /// Spatial autocorrect is on), for the words the context decides. Nil without one.
    var contextModel: TypingDecoderModel? {
        didSet { core.model(HK_AUTOCORRECT_CONTEXT, contextModel) }
    }
    /// The letter keys for `decoder`, from the current layout.
    var decoderKeys: TypingDecoder.Keys? {
        didSet { core.keys(0, decoderKeys) }
    }
    /// The centres Spatial autocorrect reads with Adaptive touch off, while `decoderKeys` has
    /// its personal ones: what space puts in is a reading these also clear by the margin.
    var decoderBaselineKeys: TypingDecoder.Keys? {
        didSet { core.keys(1, decoderBaselineKeys) }
    }
    /// Where each letter of the word being typed was tapped, one point per letter; nil when
    /// the keys don't know (the cursor moved, the text came from elsewhere).
    var decoderTaps: [CGPoint]? {
        didSet { core.taps(decoderTaps) }
    }
    /// German nouns' shares of capitalised uses (`de.capitals`): the bundled list unless set.
    /// Tests set their own list.
    var germanCapitals: GermanCapitals? {
        didSet { _ = hk_autocorrect_set_german_capitals(core.pointer, germanCapitals?.core.pointer) }
    }
    /// Bumped whenever your own words, the language or Suggest swear words change (Swipe
    /// typing rebuilds its lists then).
    var ownWordsRevision: Int { Int(hk_autocorrect_own_words_revision(core.pointer)) }
    /// Words never suggested or corrected to in `language` (`OffensiveWords`).
    var offensiveWords: OffensiveWords { .bundled(language.rawValue) }
    /// Settings › Keyboard › Suggest swear words: swear words may be suggested and corrected
    /// to; slurs never are. Off by default.
    var allowsSwearing = false {
        didSet { pushSettings() }
    }
    /// Settings › Keyboard › Auto-capitalization. Off, a fix adds no capital you didn't type.
    /// On by default.
    var addsCapitals = true {
        didSet { pushSettings() }
    }

    init(
        checker: TextChecking? = nil,
        learnedWords: LearnedWordsStore? = nil
    ) {
        let words = learnedWords ?? LearnedWordsStore()
        let checker = checker ?? SystemTextChecker()
        self.checker = checker
        core = KeyboardCoreAutocorrect(
            neverSuggest: OffensiveWords.bundledList, germanCapitals: GermanCapitals.bundled()?.core,
            checker: KeyboardCoreChecker.make(checker)
        )
        sharedLearnedWords = words
        self.learnedWords = words
        core.grid(layoutLanguage, layoutLetters)
    }

    /// The settings, whenever one changes.
    private func pushSettings() {
        _ = hk_autocorrect_set_settings(
            core.pointer, isEnabled ? 1 : 0, allowsSwearing ? 1 : 0, addsCapitals ? 1 : 0, learnsPerLanguage ? 1 : 0
        )
    }

    /// The bilingual partner's language and what the keys set for it.
    @MainActor
    final class Partner {
        let language: KeyboardLanguage
        let checker: TextChecking
        fileprivate(set) var learnedWords: LearnedWordsStore
        fileprivate(set) var learning: TypingLearning
        fileprivate(set) var vocabulary: [String]
        fileprivate(set) var layoutLetters: KeyboardLetterLayout?
        private let core: KeyboardCoreAutocorrect
        /// Spatial autocorrect's model for this language, read with the same taps.
        var decoder: TypingDecoder? {
            didSet {
                core.model(HK_AUTOCORRECT_PARTNER_DECODER, decoder?.model)
                if oldValue?.model !== decoder?.model { AutocorrectEngine.useOwnWords(decoder?.model, in: checker) }
            }
        }

        fileprivate init(
            language: KeyboardLanguage, checker: TextChecking, learnedWords: LearnedWordsStore,
            learning: TypingLearning, vocabulary: [String], layoutLetters: KeyboardLetterLayout?, core: KeyboardCoreAutocorrect
        ) {
            self.language = language
            self.checker = checker
            self.learnedWords = learnedWords
            self.learning = learning
            self.vocabulary = vocabulary
            self.layoutLetters = layoutLetters
            self.core = core
        }
    }

    /// Hands the core the kept words when a list changed, before it decides anything.
    private func prepare() {
        core.learnedWords(learnedWords, partner: partner?.learnedWords)
    }

    /// Whether `candidate` may be suggested or corrected to: not an offensive word, nor an
    /// offensive two-word expression with `previous`, unless it is your own from `UILexicon`.
    func mayOffer(_ candidate: String, after previous: String? = nil) -> Bool {
        prepare()
        return core.ask(0, candidate, previous)
    }

    func loadLexicon(_ lexicon: UILexicon) {
        loadLexiconEntries(lexicon.entries.map { ($0.userInput, $0.documentText) })
    }

    func loadLexiconEntries(_ entries: [(userInput: String, documentText: String)]) {
        KeyboardCoreText.with(entries.map(\.userInput)) { inputs, count in
            KeyboardCoreText.with(entries.map(\.documentText)) { outputs, _ in
                _ = hk_autocorrect_load_lexicon(core.pointer, inputs, outputs, count)
            }
        }
    }

    /// Hex's vocabulary list for typing.
    func setVocabulary(_ terms: [String]) {
        guard terms != vocabulary else { return }
        vocabulary = terms
        partner?.vocabulary = terms
        core.words(UInt8(HK_AUTOCORRECT_VOCABULARY), terms)
    }

    /// Your text replacement whose shortcut ends `before`, cased as Apple's keyboard writes it.
    func textReplacement(before: String?) -> (typed: String, replacement: String)? {
        prepare()
        var typed: UnsafeMutablePointer<CChar>?
        var replacement: UnsafeMutablePointer<CChar>?
        _ = hk_autocorrect_text_replacement(core.pointer, before, &typed, &replacement)
        return KeyboardCoreText.pair(typed, replacement)
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
        layoutLanguage = language
        KeyboardCoreText.with(available) { list, count in
            _ = hk_autocorrect_set_language(core.pointer, language.rawValue, list, count)
        }
        if core.text(4) == nil { partner = nil }
        scopeLearnedWords()
        lastCorrection = nil
        core.grid(layoutLanguage, layoutLetters)
    }

    /// The keys the letters are typed on now (`KeyboardLayout.language` and `letters`), for the
    /// neighbouring-key cost; the partner types on the same ones.
    func setKeys(_ language: KeyboardLanguage, letters: KeyboardLetterLayout) {
        layoutLanguage = language
        layoutLetters = letters
        partner?.layoutLetters = letters
        core.grid(language, letters)
    }

    /// Bilingual autocorrect's second language (`KeyboardLanguageList.partner`), or nil. It
    /// shares your words and types on this engine's keys, except with Learn each language
    /// separately: then it keeps its own language's words. `checker` is for tests.
    func setPartner(
        _ other: KeyboardLanguage?,
        available: [String] = UITextChecker.availableLanguages,
        checker: TextChecking? = nil
    ) {
        let theirs = checker ?? SystemTextChecker()
        KeyboardCoreText.with(available) { list, count in
            _ = hk_autocorrect_set_partner(core.pointer, other?.rawValue, list, count, KeyboardCoreChecker.make(theirs))
        }
        guard let tag = core.text(4) else {
            partner = nil
            return
        }
        guard partner?.language.rawValue != tag, let other else { return }
        core.forgetPartnerWords()
        partner = Partner(
            language: other, checker: theirs, learnedWords: learnsPerLanguage ? scopedWords(other) : sharedLearnedWords,
            learning: learnsPerLanguage ? TypingLearning() : learning, vocabulary: vocabulary, layoutLetters: layoutLetters,
            core: core
        )
    }

    /// Gives this engine and its partner the list for their languages while Learn each
    /// language separately is on, or the one shared list while it is off.
    private func scopeLearnedWords() {
        learnedWords = learnsPerLanguage ? scopedWords(language) : sharedLearnedWords
        if let partner {
            partner.learnedWords = learnsPerLanguage ? scopedWords(partner.language) : sharedLearnedWords
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
        self.learning = learning
        let theirs = learnsPerLanguage ? partnerLearning : nil
        partner?.learning = theirs ?? learning
        KeyboardCoreText.learning(learning) { mine in
            if let theirs {
                KeyboardCoreText.learning(theirs) { _ = hk_autocorrect_apply_learning(core.pointer, mine, $0) }
            } else {
                _ = hk_autocorrect_apply_learning(core.pointer, mine, nil)
            }
        }
    }

    func suppressAndKeep(_ word: String) {
        learnedWords.recordExplicitKeep(word)
        _ = hk_autocorrect_suppress_and_keep(core.pointer, word)
    }

    // MARK: - Token inspection and predictions between words (the core's)

    /// The active word right before the caret; nil inside URLs, handles, paths, or numbers.
    static func currentToken(in beforeInput: String?) -> String? {
        KeyboardCoreText.text(0, beforeInput, nil)
    }

    /// The empty field, or after a sentence's end and a space, or a new line.
    static func isSentenceStart(_ beforeInput: String?, language: KeyboardLanguage = .english) -> Bool {
        hk_text_is(0, beforeInput, nil, language.rawValue) == 1
    }

    /// Common sentence-initial words per language (its manifest's).
    static func sentenceStarters(_ language: KeyboardLanguage) -> [String] {
        KeyboardCoreText.languageWords(language.rawValue, 0)
    }

    /// Common first words of a clause, after a comma.
    static func clauseStarters(_ language: KeyboardLanguage) -> [String] {
        KeyboardCoreText.languageWords(language.rawValue, 1)
    }

    /// The language's commonest words, for next-word slots the model can't fill.
    static func commonWords(_ language: KeyboardLanguage) -> [String] {
        KeyboardCoreText.languageWords(language.rawValue, 2)
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
        prepare()
        var out = HkSuggestions()
        defer { hk_suggestions_free(&out) }
        let status = hk_autocorrect_suggestions(
            core.pointer, beforeInput, Self.field(field), autocorrectionType == .no ? 1 : 0,
            predictionModel?.pointer, capitalizesNext ? 1 : 0, &out
        )
        guard status == 0 else { return [] }
        return (0..<out.count).map { i in
            let item = out.items[i]
            let kind: KeyboardSuggestion.Kind = switch item.kind {
            case 0: .keepLiteral
            case 1: .primary(autoReplace: false)
            case 2: .primary(autoReplace: true)
            case 3: .secondary
            default: .nextWord
            }
            return KeyboardSuggestion(displayText: String(cString: item.display_text), replacement: String(cString: item.replacement), kind: kind)
        }
    }

    static func field(_ field: KeyboardFieldKind) -> UInt8 {
        switch field {
        case .text: 0
        case .url: 1
        case .email: 2
        }
    }

    /// Returns `(tokenToDelete, replacement)` only when cautious autocorrect is confident, or
    /// for a text replacement (with Autocorrect off too).
    func replacementOnSpace(
        beforeInput: String?,
        field: KeyboardFieldKind,
        autocorrectionType: UITextAutocorrectionType?
    ) -> (original: String, replacement: String)? {
        prepare()
        var original: UnsafeMutablePointer<CChar>?
        var replacement: UnsafeMutablePointer<CChar>?
        _ = hk_autocorrect_replacement_on_space(
            core.pointer, beforeInput, Self.field(field), autocorrectionType == .no ? 1 : 0, &original, &replacement
        )
        return KeyboardCoreText.pair(original, replacement).map { (original: $0.0, replacement: $0.1) }
    }

    /// Whether `token` is a word in this language as typed: the dictionary's, one of your
    /// own (`UILexicon`, kept twice) or chat shorthand.
    func knowsAsTyped(_ token: String) -> Bool {
        prepare()
        return core.ask(1, token)
    }

    /// The word of `word`'s group that the words around it make clearly likelier, cased as
    /// `word` is, or nil to keep it.
    func contextChoice(for word: String, previous: String?, next: String?, startsSentence: Bool = false) -> String? {
        prepare()
        var out: UnsafeMutablePointer<CChar>?
        _ = hk_autocorrect_context_choice(core.pointer, word, previous, next, startsSentence ? 1 : 0, &out)
        return KeyboardCoreText.take(out)
    }

    /// Whether `token`, at the end of `before`, starts a sentence.
    static func startsSentence(before: String, token: String) -> Bool {
        hk_text_is(1, before, token, nil) == 1
    }

    /// The word before the one being finished, revised now that `next` follows it: "well
    /// see" becomes "we'll see". `before` is the text up to the end of `word`. Nil to leave it.
    func revision(of word: String, before: String, next: String, field: KeyboardFieldKind, autocorrectionType: UITextAutocorrectionType?) -> String? {
        prepare()
        var out: UnsafeMutablePointer<CChar>?
        _ = hk_autocorrect_revision(core.pointer, word, before, next, Self.field(field), autocorrectionType == .no ? 1 : 0, &out)
        return KeyboardCoreText.take(out)
    }

    /// One word in place of `first`, a space tapped near the letter row's edge (`space`) and
    /// the word being finished (`decoderTaps`), when Spatial autocorrect reads the space as a
    /// letter ("deli ate" -> "delicate"); nil to keep both.
    func merge(first: String, firstTaps: [CGPoint], space: CGPoint, second: String, before: String) -> String? {
        prepare()
        let points = firstTaps.flatMap { [Double($0.x), Double($0.y)] }
        var out: UnsafeMutablePointer<CChar>?
        _ = hk_autocorrect_merge(core.pointer, first, points, firstTaps.count, Double(space.x), Double(space.y), second, before, &out)
        return KeyboardCoreText.take(out)
    }

    /// Your own words for Swipe typing: Contacts names and other `UILexicon` terms, one-word
    /// text replacements, and words you kept twice that may be offered.
    var ownWords: [String] {
        prepare()
        return core.list(0)
    }

    /// `fix` for `token` as autocorrect puts it in: with `addsCapitals` off and nothing typed
    /// in capitals, all lowercase unless it is your own spelling; nil when that leaves just
    /// what you typed ("i").
    func withoutAddedCapitals(_ fix: String, typed token: String) -> String? {
        prepare()
        return core.text(1, fix, token)
    }

    /// The strip's words for `token` with Auto-capitalization off: no capital you didn't type.
    func withoutAddedCapitals(_ pool: [String], typed token: String) -> [String] {
        prepare()
        return core.list(2, pool, token)
    }

    /// Whether `word`, capitals and all, is one of your own spellings.
    func isOwnSpelling(_ word: String) -> Bool {
        prepare()
        return core.ask(2, word)
    }

    /// `word` without a capital first letter it only has from where it was learned.
    func withoutStrayCapital(_ word: String, typed: String) -> String {
        prepare()
        return core.text(0, word, typed) ?? word
    }

    /// Whether `word`, one of your own words with capitals after its first letter, is read for
    /// `typed`, which has none: the strip offers it, but neither space nor a swipe puts it in.
    func isOwnCapitals(_ word: String, typed: String) -> Bool {
        prepare()
        return core.ask(3, word, typed)
    }

    /// A swipe's readings, best first, putting in no `isOwnCapitals` one unless caps lock is on.
    func readingsOfferingOwnCapitals(_ readings: [String], capsLock: Bool, model: TypingDecoderModel) -> [String] {
        prepare()
        return core.list(3, readings, nil, flag: capsLock, model: model.pointer)
    }

    /// The vocabulary word `token` spells, whatever its capitals; nil when it isn't one.
    func vocabularyTerm(for token: String) -> String? {
        prepare()
        return core.text(2, token)
    }
}
