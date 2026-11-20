import UIKit
import XCTest
@testable import HEX

@MainActor
final class AutocorrectEngineTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!
    private var target: FixtureTextTarget!
    private var engine: FullKeyboardEngine!
    private var now: TimeInterval = 200

    override func setUp() async throws {
        suiteName = "AutocorrectEngineTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
        target = FixtureTextTarget()
        target.autocapitalizationType = UITextAutocapitalizationType.none
        let store = LearnedWordsStore(defaults: defaults)
        let autocorrect = AutocorrectEngine(checker: SystemTextChecker(), learnedWords: store)
        engine = FullKeyboardEngine(target: target, autocorrect: autocorrect)
        engine.width = 393
        engine.clock = { [unowned self] in self.now }
        engine.configureForField()
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    private func tap(_ kind: KeyKind) {
        let key = engine.layout.key(kind)!
        let center = CGPoint(x: key.frame.midX, y: key.frame.midY)
        engine.touchBegan(id: 1, at: center)
        engine.touchEnded(id: 1, at: center)
        now += 0.45
    }

    private func typeWord(_ word: String) {
        for ch in word {
            if ch.isUppercase {
                tap(.shift)
                tap(.character(ch.lowercased()))
            } else if ch == " " {
                tap(.space)
            } else {
                tap(.character(String(ch)))
            }
        }
    }

    func testFiftyWordAndContextCases() {
        engine.autocorrect.loadLexiconEntries([
            ("omw", "On my way!"),
            ("addr", "123 Market St"),
            ("Kit", "Kit"),
        ])

        // 1–25: Contractions, transpositions, shortcuts, and casing that MUST auto-replace on space.
        let replacements: [(String, String)] = [
            ("i", "I"),
            ("im", "I'm"),
            ("ive", "I've"),
            ("dont", "don't"),
            ("Dont", "Don't"),
            ("DONT", "DON'T"),
            ("cant", "can't"),
            ("wont", "won't"),
            ("didnt", "didn't"),
            ("doesnt", "doesn't"),
            ("isnt", "isn't"),
            ("youre", "you're"),
            ("theyre", "they're"),
            ("thats", "that's"),
            ("oclock", "o'clock"),
            ("teh", "the"),
            ("Teh", "The"),
            ("TEH", "THE"),
            ("taht", "that"),
            ("adn", "and"),
            ("wiht", "with"),
            ("recieve", "receive"),
            ("keybaord", "keyboard"),
            ("iphnoe", "iPhone"),
            ("omw", "On my way!"),
        ]
        for (typed, expected) in replacements {
            let actual = engine.autocorrect.replacementOnSpace(
                beforeInput: typed,
                field: .text,
                autocorrectionType: .default
            )
            XCTAssertEqual(actual?.replacement, expected, "expected \(typed) -> \(expected)")
        }

        // 26–52: Valid English words, ambiguous words, acronyms, PascalCase, shorthand, and non-prose contexts that MUST stay untouched.
        let untouched: [(String, KeyboardFieldKind, UITextAutocorrectionType)] = [
            ("the", .text, .default),
            ("from", .text, .default),
            ("form", .text, .default),
            ("their", .text, .default),
            ("there", .text, .default),
            ("were", .text, .default),
            ("well", .text, .default),
            ("ill", .text, .default),
            ("its", .text, .default),
            ("lets", .text, .default),
            ("running", .text, .default),
            ("late", .text, .default),
            ("tonight", .text, .default),
            ("thx", .text, .default),
            ("pls", .text, .default),
            ("tmrw", .text, .default),
            ("tbh", .text, .default),
            ("idk", .text, .default),
            ("lmao", .text, .default),
            ("ios", .text, .default),
            ("hex", .text, .default),
            ("API", .text, .default),
            ("NYC", .text, .default),
            ("HexKey", .text, .default),
            ("@teh", .text, .default),
            ("https://anoma.ly/teh", .text, .default),
            ("teh", .email, .default),
            ("teh", .url, .default),
            ("teh", .text, .no),
        ]
        for (context, field, mode) in untouched {
            let actual = engine.autocorrect.replacementOnSpace(
                beforeInput: context,
                field: field,
                autocorrectionType: mode
            )
            XCTAssertNil(actual, "expected \(context) (\(field)) to stay untouched, got \(String(describing: actual))")
        }
    }

    func testAmbiguousContractionsSuggestWithoutAutoReplacing() {
        let slots = engine.autocorrect.suggestions(
            beforeInput: "well",
            field: .text,
            autocorrectionType: .default
        )
        XCTAssertEqual(slots.first?.displayText, "“well”")
        XCTAssertEqual(slots.dropFirst().first?.replacement, "we'll")
        XCTAssertEqual(slots.dropFirst().first?.willAutoReplace, false)
    }

    func testSpaceDeleteSpaceKeepLoopAndLearnedWordsPersistence() {
        typeWord("teh")
        XCTAssertEqual(engine.suggestions.count >= 2, true)
        XCTAssertEqual(engine.suggestions[0].kind, .keepLiteral)
        XCTAssertEqual(engine.suggestions[1].replacement, "the")
        XCTAssertTrue(engine.suggestions[1].willAutoReplace)

        // 1. Space applies cautious autocorrect (`teh` -> `the `).
        tap(.space)
        XCTAssertEqual(target.text, "the ")

        // 2. Single Backspace reverts `the ` -> `teh` and suppresses `"teh"` for this session.
        tap(.delete)
        XCTAssertEqual(target.text, "teh")

        // 3. Pressing Space again keeps `teh ` untouched!
        tap(.space)
        XCTAssertEqual(target.text, "teh ")

        // 4. Keeping `"teh"` a second time via Slot 0 persists it in App Group `learnedWords`.
        typeWord("teh")
        engine.acceptSuggestion(engine.suggestions[0])
        XCTAssertEqual(target.text, "teh teh ")
        XCTAssertTrue(LearnedWordsStore(defaults: defaults).contains("teh"))
    }

    func testQwertyAdjacencyPrefersPhysicalNeighbor() {
        let near = AutocorrectEngine.singleEditKeyboardCost(from: "tge", to: "the")!
        let far = AutocorrectEngine.singleEditKeyboardCost(from: "tpe", to: "the")!
        XCTAssertLessThan(near, far, "g is adjacent to h; p is across the keyboard")
        XCTAssertNil(AutocorrectEngine.singleEditKeyboardCost(from: "abcdef", to: "abxyef"), "two substitutions are rejected")
    }

    func testALayoutWithoutADictionaryNeverCorrectsIntoEnglish() {
        engine.autocorrect.loadLexiconEntries([("omw", "On my way!")])
        engine.autocorrect.setLanguage(.norwegian, available: ["en_US", "en"])
        XCTAssertFalse(engine.autocorrect.hasDictionary)
        typeWord("teh")
        XCTAssertTrue(engine.suggestions.isEmpty, "no English suggestions for Norwegian")
        tap(.space)
        XCTAssertEqual(target.text, "teh ")
        typeWord("omw")
        tap(.space)
        XCTAssertEqual(target.text, "teh On my way! ", "your own text replacements still apply")
    }

    func testEnglishFixesStayOffInOtherLanguages() {
        engine.autocorrect.setLanguage(.norwegian, available: ["nb", "en_US"])
        XCTAssertTrue(engine.autocorrect.hasDictionary)
        typeWord("i")
        tap(.space)
        XCTAssertEqual(target.text, "i ", "Norwegian \"i\" means \"in\"")
        engine.autocorrect.setLanguage(.english, available: ["en_US"])
        typeWord("i")
        tap(.space)
        XCTAssertEqual(target.text, "i I ")
    }

    /// An Italian iPhone types in Italian: its words stay ("Domani" never becomes "Domain")
    /// and its typos get Italian fixes, accents included.
    func testItalianCorrectsInItalianNeverEnglish() {
        engine.language = .italian
        XCTAssertEqual(engine.autocorrect.language, .italian)
        XCTAssertTrue(engine.autocorrect.hasDictionary)
        for word in ["Domani", "andiano", "al", "mare", "con", "i", "bambibi", "perche", "Sono", "molto", "stai"] {
            typeWord(word)
            tap(.space)
        }
        XCTAssertEqual(target.text, "Domani andiamo al mare con i bambini perché Sono molto stai ")
    }

    /// Russian types in Russian: its words stay and its typos get Russian fixes, never English.
    func testRussianCorrectsInRussianNeverEnglish() {
        engine.language = .russian
        XCTAssertEqual(engine.autocorrect.language, .russian)
        XCTAssertTrue(engine.autocorrect.hasDictionary)
        for word in ["Привет", "опаздываю", "на", "десчть", "минут", "буду", "дома", "в", "восемь", "хорошл", "спасибл"] {
            typeWord(word)
            tap(.space)
        }
        XCTAssertEqual(target.text, "Привет опаздываю на десять минут буду дома в восемь хорошо спасибо ")
    }

    /// Serbian (Latin) has no dictionary on iOS: nothing is "corrected", into English or anything else.
    func testSerbianLatinWithoutADictionaryCorrectsNothing() {
        engine.language = .serbianLatin
        XCTAssertEqual(engine.autocorrect.language, .serbianLatin)
        XCTAssertFalse(engine.autocorrect.hasDictionary)
        for word in ["Kasnim", "deset", "minuta", "hvala", "sta", "radis", "Sutra", "dolazim", "the", "teh"] {
            typeWord(word)
            tap(.space)
        }
        XCTAssertEqual(target.text, "Kasnim deset minuta hvala sta radis Sutra dolazim the teh ")
    }

    func testAdjacencyFollowsTheLayout() {
        // German swaps Y and Z: on QWERTZ, Z sits beside T.
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("z", "t", language: .german))
        XCTAssertFalse(AutocorrectEngine.areAdjacentKeys("z", "t", language: .english))
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("ø", "l", language: .norwegian))
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("ñ", "l", language: .spanish))
    }

    // MARK: - Split and punctuated guesses

    /// A deterministic dictionary: `UITextChecker`'s answers vary between iOS versions.
    private final class StubChecker: TextChecking {
        var valid: Set<String> = []
        var guessTable: [String: [String]] = [:]
        var completionTable: [String: [String]] = [:]
        func isWordValid(_ word: String) -> Bool { valid.contains(word.lowercased()) }
        func guesses(for word: String) -> [String] { guessTable[word.lowercased()] ?? [] }
        func completions(for prefix: String) -> [String] { completionTable[prefix.lowercased()] ?? [] }
    }

    private func stubbed(_ configure: (StubChecker) -> Void) -> AutocorrectEngine {
        let checker = StubChecker()
        configure(checker)
        return AutocorrectEngine(checker: checker, learnedWords: LearnedWordsStore(defaults: defaults))
    }

    private func fix(_ engine: AutocorrectEngine, _ typed: String) -> String? {
        engine.replacementOnSpace(beforeInput: typed, field: .text, autocorrectionType: .default)?.replacement
    }

    func testAMissedSpaceBecomesTwoWordsNeverAHyphen() {
        let engine = stubbed {
            $0.valid = ["up", "to", "a", "lot"]
            // UITextChecker lists the hyphenated form first and the spaced one later.
            $0.guessTable = ["upto": ["up-to", "updo", "unto", "up to"], "alot": ["a-lot", "lot", "a lot"]]
        }
        XCTAssertEqual(fix(engine, "upto"), "up to")
        XCTAssertEqual(fix(engine, "Upto"), "Up to")
        XCTAssertEqual(fix(engine, "alot"), "a lot", "English \"a\" is a word")
        let slots = engine.suggestions(beforeInput: "upto", field: .text, autocorrectionType: .default)
        XCTAssertEqual(slots.dropFirst().first?.replacement, "up to")
        XCTAssertFalse(slots.contains { $0.replacement.contains("-") }, "never offers a hyphen you didn't type")
    }

    func testASplitNeedsTwoWordsYourLettersAndTheSpacedForm() {
        let engine = stubbed {
            $0.valid = ["ab", "cd", "e", "mail", "xy"]
            $0.guessTable = [
                // Only the hyphenated form: the checker doesn't offer the two words.
                "abcd": ["ab-cd"],
                // "zq" isn't a word.
                "xyzq": ["xy-zq", "xy zq"],
                // One letter that isn't "a" or "I" (UITextChecker accepts every letter).
                "bmail": ["b-mail", "b mail"],
                // Not a missed space: the letters changed.
                "emial": ["e-mail", "e mail"],
            ]
        }
        for typed in ["abcd", "xyzq", "bmail", "emial"] {
            XCTAssertNil(fix(engine, typed), "\(typed) stays as typed")
        }
    }

    func testAGuessNeverAddsPunctuationYouDidntType() {
        let engine = stubbed {
            $0.valid = ["ok"]
            $0.guessTable = ["tbq": ["t.b.q", "tb-q", "tbq."], "wnot": ["w.not", "won't"]]
        }
        XCTAssertNil(fix(engine, "tbq"))
        XCTAssertEqual(fix(engine, "wnot"), "won't", "a contraction's apostrophe is part of the word")
        let slots = engine.suggestions(beforeInput: "tbq", field: .text, autocorrectionType: .default)
        XCTAssertFalse(slots.contains { $0.replacement.contains(".") || $0.replacement.contains("-") })
    }

    func testTheSystemDictionarySplitsAMissedSpace() {
        // iOS 27's UITextChecker guesses "never mind" and "never-mind" for this.
        XCTAssertEqual(fix(engine.autocorrect, "nevermind"), "never mind")
        XCTAssertNotEqual(fix(engine.autocorrect, "eachother")?.contains("-"), true)
    }

    // MARK: - Completions

    private func strip(_ engine: AutocorrectEngine, _ typed: String) -> [String] {
        engine.suggestions(beforeInput: typed, field: .text, autocorrectionType: .default).map(\.replacement)
    }

    func testCompletionsKeepTheDictionarysFrequencyOrderAheadOfYourContacts() {
        let engine = stubbed {
            $0.valid = ["zo"]
            // Most probable first, as UITextChecker lists them; not by length or alphabet.
            $0.completionTable = ["zo": ["zone", "zoo", "zombie", "zones"]]
        }
        engine.loadLexiconEntries([("Zorbin", "Zorbin"), ("Zoltrax", "Zoltrax"), ("zoeticket", "zoeticket")])
        XCTAssertEqual(strip(engine, "zo"), ["zo", "zone", "zoo"])
        XCTAssertEqual(strip(engine, "Zo"), ["Zo", "Zone", "Zoo"], "a capital at the start of a sentence still means words first")
    }

    func testNamesComeLastUnlessYouTypedACapital() {
        let engine = stubbed {
            $0.valid = ["qu"]
            $0.completionTable = ["qu": ["Quilla", "quip", "quiz"]]
        }
        engine.loadLexiconEntries([("Quorvent", "Quorvent"), ("qubitly", "qubitly")])
        XCTAssertEqual(strip(engine, "qu"), ["qu", "quip", "quiz"])
        XCTAssertEqual(strip(engine, "Qu"), ["Qu", "Quilla", "Quip"], "you typed a capital: names keep their place")

        // With fewer dictionary words, your own lowercase word comes before a name.
        let few = stubbed {
            $0.valid = ["qu"]
            $0.completionTable = ["qu": ["Quilla", "quip"]]
        }
        few.loadLexiconEntries([("Quorvent", "Quorvent"), ("qubitly", "qubitly")])
        XCTAssertEqual(strip(few, "qu"), ["qu", "quip", "qubitly"])
        XCTAssertEqual(strip(few, "Qu"), ["Qu", "Quilla", "Quip"])
        // Nothing else: a contact's name is still offered.
        let names = stubbed { $0.valid = ["qu"] }
        names.loadLexiconEntries([("Quorvent", "Quorvent")])
        XCTAssertEqual(strip(names, "qu"), ["qu", "Quorvent"])
    }

    func testTheStripNeverRepeatsWhatYouTyped() {
        let engine = stubbed {
            $0.valid = ["k", "kay"]
            $0.completionTable = ["k": ["k", "know", "keep"], "kay": ["kay", "kayak"]]
        }
        XCTAssertEqual(strip(engine, "k"), ["k", "know", "keep"], "one letter gets completions too")
        XCTAssertEqual(strip(engine, "kay"), ["kay", "kayak"])
        XCTAssertEqual(strip(engine, "Kay"), ["Kay", "Kayak"])
        XCTAssertEqual(strip(engine, "kq"), ["kq"], "nothing to offer: only the left slot, not a second copy")
        XCTAssertEqual(
            engine.suggestions(beforeInput: "kq", field: .text, autocorrectionType: .default).map(\.kind),
            [.keepLiteral]
        )
    }

    func testTheSystemDictionaryRanksCommonWordsFirst() {
        // iOS 27's UITextChecker lists "b" as but, be, because… and "th" as the, they, that's….
        XCTAssertEqual(Array(strip(engine.autocorrect, "b").dropFirst()), ["but", "be"])
        XCTAssertEqual(strip(engine.autocorrect, "th").dropFirst().first, "the")
        XCTAssertEqual(strip(engine.autocorrect, "Th").dropFirst().first, "The")
    }

    // MARK: - Spatial autocorrect (the bundled models)

    private func useDecoder(_ language: KeyboardLanguage = .english) throws {
        if language != engine.language { engine.language = language }
        let model = try XCTUnwrap(TypingDecoderModel.bundled(language.rawValue), "the \(language) model is bundled")
        engine.autocorrect.decoder = TypingDecoder(model: model)
    }

    private func tap(at point: CGPoint) {
        engine.touchBegan(id: 1, at: point)
        engine.touchEnded(id: 1, at: point)
        now += 0.2
    }

    /// Types `word` at the keys' centres, except an "o" written as "^" lands low, just inside
    /// the k key (where these logs' o taps go astray), then a space.
    private func typeTaps(_ word: String) {
        for ch in word {
            if ch == "^" {
                let o = engine.layout.key(.character("o"))!.frame
                let k = engine.layout.key(.character("k"))!.frame
                let point = CGPoint(x: o.midX - 8, y: k.minY + 4)
                XCTAssertEqual(engine.layout.hitTest(point)?.kind, .character("k"))
                tap(at: point)
            } else {
                let key = engine.layout.key(.character(String(ch)))!.frame
                tap(at: CGPoint(x: key.midX, y: key.midY))
            }
        }
        tap(.space)
    }

    func testSpatialAutocorrectReadsTapsWithTheWordBefore() throws {
        try useDecoder()
        for word in ["we", "need", "t^"] { typeTaps(word) }
        XCTAssertEqual(target.text, "we need to ", "a two-letter slip after \"need\"")
        for word in ["and", "seeh^w"] { typeTaps(word) }
        XCTAssertEqual(target.text, "we need to and see how ", "a missed space with a slip inside")
    }

    func testSpatialAutocorrectLeavesWordsTypedRight() throws {
        try useDecoder()
        for word in ["come", "and", "miss", "the", "quiz"] { typeTaps(word) }
        XCTAssertEqual(target.text, "come and miss the quiz ")
    }

    /// The models' word lists are lowercase: a name or day typed in lowercase is still a
    /// word, capitalised as the dictionary path does, never "may" or "money".
    func testSpatialAutocorrectCapitalisesANameTypedInLowercase() throws {
        try useDecoder()
        for word in ["see", "mary", "on", "monday", "in", "boston"] { typeTaps(word) }
        XCTAssertEqual(target.text, "see Mary on Monday in Boston ")
    }

    func testSpatialAutocorrectAddsSpanishAccents() throws {
        try useDecoder(.spanish)
        for word in ["esta", "aqui", "manana"] { typeTaps(word) }
        XCTAssertEqual(target.text, "esta aquí mañana ", "\"esta\" is a word too, so it stays")
    }

    func testSpatialAutocorrectKnowsYourOwnWords() throws {
        try useDecoder()
        engine.autocorrect.loadLexiconEntries([("Zorbix", "Zorbix")])
        typeTaps("zorbiz")
        typeTaps("zorbix")
        XCTAssertEqual(target.text, "Zorbix zorbix ", "a slip in a name from Contacts, and the name typed right")
    }

    func testTurningAutocorrectSettingOffKeepsSuggestionsWithoutAutoReplacing() {
        engine.autocorrect.isEnabled = false
        typeWord("dont")
        XCTAssertFalse(engine.suggestions.isEmpty)
        XCTAssertFalse(engine.suggestions[1].willAutoReplace)
        tap(.space)
        XCTAssertEqual(target.text, "dont ")
    }
}
