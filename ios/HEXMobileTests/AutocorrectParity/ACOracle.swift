// The autocorrect oracle for the Rust port's parity fixture (`AutocorrectParityExport`):
// the engine's state, settings and strip, a copy of `AutocorrectEngine.swift` (up to `replacementOnSpace`) as of dev commit d6efeddb, renamed (`ACOracle`,
// `TapOracle`, `ACO…`) so it runs on its own copies, independent of the app's. Changed from
// the copy: the dictionary is `ACOChecking` (no `SystemTextChecker`), learned words live in
// `ACOLearnedWords` (an in-memory copy of `LearnedWordsStore`), the bundled lists are read
// from the repository, and the two orders the owner fixed in the port go through
// `ACOracle.orderedValues` and `ACOOffensiveWords.bundled`. Delete with the exporter once iOS
// runs the Rust autocorrect.

import CoreGraphics
import Foundation
import UIKit
@testable import HEX

/// Cautious on-device suggestions and replace-on-space engine combining `UILexicon`,
/// deterministic contractions/transpositions, QWERTY-ranked `UITextChecker`, and `ACOLearnedWords`.
@MainActor
final class ACOracle {
    let checker: ACOChecking
    /// The words kept in this engine's language: the one shared list, or with Learn each
    /// language separately on, this language's own (`learnsPerLanguage`).
    var learnedWords: ACOLearnedWords
    var sharedLearnedWords: ACOLearnedWords
    /// Experiment (Learn each language separately): each keyboard language keeps its own
    /// words, seeded from the shared list, and the partner its own too. Off, one shared list.
    var learnsPerLanguage = false {
        didSet { if learnsPerLanguage != oldValue { scopeLearnedWords() } }
    }
    /// Each language's list once made, so switching back keeps what it holds in memory.
    var scopedLearnedWords: [String: ACOLearnedWords] = [:]
    /// Settings › Keyboard › Autocorrect. Off, only your text replacements still show in the
    /// strip and apply on space or a closing mark, as Apple's keyboard does with Auto-Correction
    /// off; a field that turns autocorrection off gets none, as there.
    var isEnabled = true
    /// The keyboard layout's language: its dictionary, and whether English fixes apply.
    var language: KeyboardLanguage = .english
    /// False when the iPhone has no spelling dictionary for `language`: then nothing is
    /// suggested or corrected but your own text replacements, rather than "correcting"
    /// the language into another.
    var hasDictionary = true
    /// Bilingual autocorrect: the other Latin-script language in Settings › Keyboard ›
    /// Languages, with its own dictionary (and Spatial autocorrect's model) on these keys, so
    /// typos are fixed in either language and a word of either is never "corrected" into the
    /// other, as Apple's "Français et English" keyboard does. Nil with one language.
    var partner: ACOracle?
    /// The partner, unless its dictionary takes any letters for a word: iOS can list a
    /// language whose word data isn't there (the iOS 27.2 simulator's `pl_PL`), and such a
    /// partner would keep every typo as "a word in the other language". Checked once per
    /// language, on first use.
    var workingPartner: ACOracle? {
        guard let partner else { return nil }
        if partner.acceptsAnyLetters == nil {
            partner.acceptsAnyLetters = partner.checker.isWordValid("qzxjvkq")
        }
        return partner.acceptsAnyLetters == true ? nil : partner
    }
    var acceptsAnyLetters: Bool?
    /// The keys you type on, whose neighbours price a slip: `language`'s layout, or a
    /// partner's primary's, in `layoutLetters`'s arrangement (`setKeys`).
    var layoutLanguage: KeyboardLanguage = .english
    var layoutLetters: KeyboardLetterLayout?
    var lexiconShortcuts: [String: String] = [:]
    /// Your `UILexicon` terms, then your vocabulary's words (`ACOracle+Vocabulary`).
    var lexiconTerms: [String] = []
    var systemLexiconTerms: [String] = []
    var vocabulary: [String] = []
    var suppressedInSession: Set<String> = []
    /// Learn how I type: typos you fixed the same way twice and words you turned down twice.
    var learning = ACOLearning()
    var lastCorrection: AppliedCorrection?
    /// The language of the fix space would apply to the word it was asked about: the
    /// partner's when its fix won, else this engine's; nil for a text replacement.
    var lastFixLanguage: KeyboardLanguage?
    /// The language of the last revision `revision(of:)` returned: the partner's for its accents.
    var lastRevisionLanguage: KeyboardLanguage?
    /// Experiment (Spatial autocorrect): reads the word's taps with a language model in place
    /// of `UITextChecker`'s one-edit guesses, for the language its model was built for. Nil
    /// while off or without a model.
    var decoder: TapOracle? {
        didSet { if oldValue?.model !== decoder?.model { userWordsBuilt = nil } }
    }
    /// The layout language's word list and word pairs (the typing model, whether or not
    /// Spatial autocorrect is on): English chooses between words the same letters spell with
    /// and without an apostrophe by the words around them (`contextChoice`). Nil without one.
    var contextModel: TapOracleModel?
    /// The letter keys for `decoder`, from the current layout.
    var decoderKeys: TapOracle.Keys? {
        didSet { partner?.decoderKeys = decoderKeys }
    }
    /// The centres Spatial autocorrect reads with Adaptive touch off (the stock ones, or
    /// moved by Learn how I type), while `decoderKeys` has its personal ones: what space puts
    /// in is a reading these also clear by the margin, else their own decision. Personal
    /// centres rank readings; they never make a correction the keyboard wouldn't without them.
    var decoderBaselineKeys: TapOracle.Keys? {
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
    var germanCapitals: ACOGermanCapitals? {
        didSet { germanCapitalsRead = true }
    }
    var germanCapitalsRead = false
    var germanNouns: ACOGermanCapitals? {
        if !germanCapitalsRead { germanCapitals = ACOGermanCapitals.bundled() }
        return germanCapitals
    }
    var beforeInput: String?
    /// Bumped whenever a source of your own words changes, so the decoder's copy is rebuilt.
    var userWordsRevision = 0
    var userWordsBuilt: (revision: Int, model: ObjectIdentifier)?
    /// Bumped whenever your own words, the language or Suggest swear words change (Swipe
    /// typing rebuilds its lists then).
    var ownWordsRevision: Int { userWordsRevision }
    /// Words never suggested or corrected to in `language` (`ACOOffensiveWords`).
    var offensiveWords: ACOOffensiveWords
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
    var decoderCache: (before: String, taps: [CGPoint], partnerWord: Bool, decision: TapOracle.Decision?)?

    init(
        checker: ACOChecking,
        learnedWords: ACOLearnedWords
    ) {
        let words = learnedWords
        self.checker = checker
        sharedLearnedWords = words
        self.learnedWords = words
        self.offensiveWords = .bundled(KeyboardLanguage.english)
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
    func mayOffer(_ candidate: String, replacing token: String) -> Bool {
        mayOffer(candidate, after: Self.previousWord(beforeInput ?? "", token: token))
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

    /// Hex's vocabulary list for typing (`ACOracle+Vocabulary`).
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
    func shortcut(for token: String) -> String? {
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
    func setLanguage(_ language: KeyboardLanguage, available: [String]) {
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
        offensiveWords = .bundled(language)
        userWordsRevision += 1
        decoderCache = nil
        let code = language.acoSpellCheckLanguage(available: available)
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
        available: [String],
        checker: ACOChecking
    ) {
        guard let other, other != language, hasDictionary, other.acoSpellCheckLanguage(available: available) != nil else {
            partner = nil
            return
        }
        guard partner?.language != other else { return }
        let engine = ACOracle(
            checker: checker, learnedWords: learnsPerLanguage ? scopedWords(other) : sharedLearnedWords
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
    func scopeLearnedWords() {
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

    func scopedWords(_ language: KeyboardLanguage) -> ACOLearnedWords {
        if let store = scopedLearnedWords[language.rawValue] { return store }
        let store = sharedLearnedWords.scoped(to: language.rawValue)
        scopedLearnedWords[language.rawValue] = store
        return store
    }

    /// Hands over what the journal learned. With Learn each language separately on, the
    /// partner gets `partnerLearning` (its own language's) instead of these.
    func applyLearning(_ learning: ACOLearning, partner partnerLearning: ACOLearning? = nil) {
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
        case .romanian: ["eu", "nu", "am"]
        case .norwegian: ["jeg", "det", "vi"]
        case .russian: ["я", "это", "мы"]
        case .ukrainian: ["я", "це", "ми"]
        case .greek: ["θα", "δεν", "το"]
        case .serbianLatin: ["ja", "to", "da"]
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
        case .romanian: ["și", "dar", "că"]
        case .norwegian: ["og", "men", "jeg"]
        case .russian: ["и", "но", "что"]
        case .ukrainian: ["і", "але", "що"]
        case .greek: ["και", "αλλά", "που"]
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
        case .czech: ["a", "se", "je", "v", "na", "to"]
        case .romanian: ["de", "și", "în", "nu", "să", "la"]
        case .norwegian: ["og", "i", "det", "er", "på", "som"]
        case .russian: ["и", "в", "не", "на", "я", "что"]
        case .ukrainian: ["і", "в", "не", "на", "що", "я"]
        case .greek: ["και", "να", "το", "θα", "δεν", "η"]
        case .serbianLatin: ["i", "je", "da", "u", "se", "na"]
        }
    }

    /// Up to three distinct, allowed words as tap-only predictions, capitalized when shift
    /// would be on (never with Auto-capitalization off).
    func predictions(_ words: [String], previous: String?, capitalized: Bool) -> [KeyboardSuggestion] {
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
        predictionModel: TapOracleModel? = nil,
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
        guard hasDictionary else { return replacementOnly ?? [] }
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
    func candidates(
        for token: String, autoFix: String?, guesses: Bool, limit: Int, model: TapOracleModel?, corrects: Bool = true
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
            } else if language.acoUsesEnglishFixes, let suggested = Self.suggestOnlyContractions[lower] {
                pool.append(Self.matchCase(of: token, onto: suggested))
            } else if !suppressedInSession.contains(lower), !learning.rejected.contains(lower),
                      !learnedWords.contains(lower), let accented = language.acoAccentSuggestions[lower] {
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
    func rankedCompletions(for token: String) -> [String] {
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
}
