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

    /// Settings › Keyboard › Auto-capitalization off, as Apple's keyboard with it off (iOS 27.2
    /// simulator: "i" stays "i", "im" becomes "i'm", "mondya" "monday"): a fix adds no capital
    /// you didn't type, but keeps those you typed and your own spellings'.
    func testAutoCapitalizationOffAddsNoCapitalsToFixes() {
        engine.autoCapitalizes = false
        engine.autocorrect.loadLexiconEntries([("omw", "On my way!"), ("Kit", "Kit")])
        let replacements: [(String, String?)] = [
            ("i", nil),
            ("im", "i'm"),
            ("Im", "I'm"),
            ("ive", "i've"),
            ("dont", "don't"),
            ("Teh", "The"),
            ("mondya", "monday"),
            ("omw", "On my way!"),
        ]
        for (typed, expected) in replacements {
            let actual = engine.autocorrect.replacementOnSpace(beforeInput: typed, field: .text, autocorrectionType: .default)
            XCTAssertEqual(actual?.replacement, expected, "expected \(typed) -> \(String(describing: expected))")
        }
        target.autocapitalizationType = .sentences
        engine.configureForField()
        typeWord("i think im here ")
        XCTAssertEqual(target.text, "i think i'm here ")
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

    /// Types `text` at the keys' centres into a new field, with the typing model's word pairs,
    /// and returns the field's text.
    private func typedWithContext(_ text: String, then: ((FullKeyboardEngine) -> Void)? = nil) throws -> String {
        let field = FixtureTextTarget()
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let keys = FullKeyboardEngine(
            target: field,
            autocorrect: AutocorrectEngine(checker: SystemTextChecker(), learnedWords: LearnedWordsStore(defaults: defaults))
        )
        keys.width = 393
        keys.clock = { [unowned self] in self.now }
        keys.letterOdds = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        keys.configureForField()
        let saved = engine
        engine = keys
        defer { engine = saved }
        typeWord(text)
        then?(keys)
        return field.text
    }

    /// X (Erick): "the id in this didn't autocorrect to I'd". A word the same letters spell
    /// with an apostrophe follows the words around it, as on Apple's keyboard: the word
    /// before decides when it is clear ("yes id" -> "yes I'd", "the id" stays), else the next
    /// word ("well see" -> "we'll see", "well done" stays).
    func testContractionsWithoutTheApostropheFollowTheWordsAroundThem() throws {
        let cases: [(String, String)] = [
            ("id like to go ", "I'd like to go "),
            ("yes id love to ", "yes I'd love to "),
            ("i think id help ", "I think I'd help "),
            ("the id in this ", "the id in this "),
            ("my id is here ", "my id is here "),
            ("ill be there ", "I'll be there "),
            ("shell be here ", "she'll be here "),
            ("the shell is ", "the shell is "),
            ("wed love to ", "we'd love to "),
            ("lets go ", "let's go "),
            ("well see ", "we'll see "),
            ("well done ", "well done "),
            ("as well as ", "as well as "),
            ("till hell freezes over ", "till hell freezes over "),
            ("hell be here ", "he'll be here "),
            ("were going home ", "were going home "),
        ]
        for (typed, expected) in cases {
            XCTAssertEqual(try typedWithContext(typed), expected, typed)
        }
        // Without the typing model nothing changes ("ill" stays a word).
        typeWord("ill be there ")
        XCTAssertEqual(target.text, "ill be there ")
    }

    /// X (Justin): "It doesn't seem to understand grammar. Its vs it's for instance." The words
    /// around its/it's, your/you're, there/their/they're and then/than choose between them when
    /// they're clear, at the word's own space or the next word's.
    func testSoundAlikeWordsFollowTheWordsAroundThem() throws {
        let cases: [(String, String)] = [
            ("its a nice day ", "it's a nice day "),
            ("i think its broken ", "I think it's broken "),
            ("the dog wagged its tail ", "the dog wagged its tail "),
            ("winter is on its way ", "winter is on its way "),
            ("your welcome ", "you're welcome "),
            ("i like your car ", "I like your car "),
            ("there going home ", "they're going home "),
            ("their is a dog ", "there is a dog "),
            ("they lost there keys ", "they lost their keys "),
            ("more then that ", "more than that "),
            ("and than we left ", "and then we left "),
            ("youre mother loves you ", "your mother loves you "),
        ]
        for (typed, expected) in cases {
            XCTAssertEqual(try typedWithContext(typed), expected, typed)
        }
        let own = try typedWithContext("you") { _ in
            self.tap(.layerSwitch(.numbers))
            self.tap(.character("'"))
            self.typeWord("re mother ")
        }
        XCTAssertEqual(own, "you're mother ", "an apostrophe you typed yourself stays")
    }

    /// The next word's space revised the word before: one backspace puts back both as typed,
    /// and the word you kept stays.
    func testOneBackspaceTakesARevisionBack() throws {
        let text = try typedWithContext("well see ") { keys in
            XCTAssertEqual(keys.target.documentContextBeforeInput, "we'll see ")
            keys.deleteBackward()
            XCTAssertEqual(keys.target.documentContextBeforeInput, "well see")
            let space = keys.layout.key(.space)!.frame
            keys.touchBegan(id: 1, at: CGPoint(x: space.midX, y: space.midY))
            keys.touchEnded(id: 1, at: CGPoint(x: space.midX, y: space.midY))
        }
        XCTAssertEqual(text, "well see ")
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

    func testDeleteWhatHexLearnedForgetsLearnedWords() {
        let store = LearnedWordsStore(defaults: defaults)
        store.learn("teh")
        XCTAssertTrue(store.contains("teh"))

        TypingJournal.delete(in: nil, settings: TypingJournalSettings(defaults: defaults))

        XCTAssertFalse(store.contains("teh"), "a keyboard that loaded the word drops it")
        XCTAssertNil(defaults.data(forKey: LearnedWordsStore.defaultsKey))
        XCTAssertFalse(LearnedWordsStore(defaults: defaults).contains("teh"))
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

    /// Ukrainian types with iOS's uk_UA dictionary, never Russian's or English's: its words
    /// stay, ʼ (Apple's apostrophe key) included. The iOS 27.2 simulator lists uk_UA but
    /// flags no word and offers no guess, so its typo fixes are checked on an iPhone only.
    func testUkrainianTypesInUkrainianNeverRussianOrEnglish() {
        engine.language = .ukrainian
        XCTAssertEqual(engine.autocorrect.language, .ukrainian)
        XCTAssertTrue(engine.autocorrect.hasDictionary)
        XCTAssertNil(engine.partnerLanguage)
        for word in ["Привіт", "як", "справи", "мʼясо", "їжак", "єнот", "ґанок", "дякую", "сімʼя", "на", "і"] {
            typeWord(word)
            tap(.space)
        }
        XCTAssertEqual(target.text, "Привіт як справи мʼясо їжак єнот ґанок дякую сімʼя на і ")
    }

    /// Polish types with iOS's pl dictionary, and missing diacritics are the cheapest fix there
    /// is, however many a word lacks ("zolw" -> "żółw", "maly" -> "mały": ł has no
    /// decomposition), as a missing Spanish accent is; other languages keep one edit. The
    /// iOS 27.2 simulator lists pl_PL but its checker flags no word and offers no Polish
    /// guess, so the fixes run against a stub dictionary answering as an iPhone's would.
    func testPolishRestoresMissingDiacritics() {
        XCTAssertEqual(AutocorrectEngine.singleEditKeyboardCost(from: "maly", to: "mały", language: .polish)!, 0.15, accuracy: 0.001)
        XCTAssertEqual(AutocorrectEngine.singleEditKeyboardCost(from: "zolw", to: "żółw", language: .polish)!, 0.25, accuracy: 0.001)
        XCTAssertEqual(AutocorrectEngine.singleEditKeyboardCost(from: "dzien", to: "dzień", language: .polish)!, 0.15, accuracy: 0.001)
        XCTAssertNil(AutocorrectEngine.singleEditKeyboardCost(from: "zolw", to: "żółw", language: .english), "three edits elsewhere")
        XCTAssertNil(AutocorrectEngine.singleEditKeyboardCost(from: "zolw", to: "żółt", language: .polish), "w and t aren't one letter")
        engine.language = .polish
        XCTAssertEqual(engine.autocorrect.language, .polish)
        XCTAssertTrue(engine.autocorrect.hasDictionary)
        XCTAssertNil(engine.autocorrect.decoder, "no Polish typing model")

        let polish = stubbed {
            $0.valid = ["żółw", "mały", "mamy", "dzień", "jest", "dobry"]
            // "mamy" is one far key from "maly" and listed first; "mały" only lacks its ł.
            $0.guessTable = ["zolw": ["żółw"], "maly": ["mamy", "mały"], "dzien": ["dzień"]]
        }
        polish.setLanguage(.polish, available: ["pl_PL", "en_US"])
        XCTAssertEqual(fix(polish, "zolw"), "żółw")
        XCTAssertEqual(fix(polish, "Zolw"), "Żółw")
        XCTAssertEqual(fix(polish, "maly"), "mały")
        XCTAssertEqual(fix(polish, "dzien"), "dzień")
        XCTAssertNil(fix(polish, "jest"), "a word stays")
        polish.setLanguage(.spanish, available: ["pl_PL", "es_ES"])
        XCTAssertNil(fix(polish, "zolw"), "three diacritics are three edits on another layout")
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
        // AZERTY: M sits under P, A beside Z, and W under D.
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("m", "p", language: .french))
        XCTAssertFalse(AutocorrectEngine.areAdjacentKeys("m", "p", language: .english))
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("a", "z", language: .french))
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("w", "d", language: .french))
        XCTAssertFalse(AutocorrectEngine.areAdjacentKeys("w", "q", language: .french))
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("a", "q", language: .frenchCanadian))
    }

    // MARK: - Split and punctuated guesses

    /// A deterministic dictionary: `UITextChecker`'s answers vary between iOS versions.
    private final class StubChecker: TextChecking {
        var valid: Set<String> = []
        /// Words the dictionary knows only with their capitals, as German nouns are ("Haus",
        /// never "haus"): `valid` lowercases, so it cannot say this.
        var capitalised: Set<String> = []
        var acceptsEverything = false
        var guessTable: [String: [String]] = [:]
        var completionTable: [String: [String]] = [:]
        func isWordValid(_ word: String) -> Bool {
            acceptsEverything || valid.contains(word.lowercased()) || capitalised.contains(word)
        }
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

    // MARK: - Bilingual autocorrect

    private func fix(_ engine: AutocorrectEngine, before: String) -> String? {
        engine.replacementOnSpace(beforeInput: before, field: .text, autocorrectionType: .default)?.replacement
    }

    /// French and English on one keyboard (Apple's "Français et English"): a word of either is
    /// never "corrected" into the other, a typo only one can fix gets its fix, and when both
    /// can, the language of the words before wins, else the keyboard's own.
    func testBilingualAutocorrectNeverCorrectsOneLanguageIntoTheOther() {
        let available = ["fr_FR", "en_US"]
        let english = StubChecker()
        english.valid = ["the", "see", "you", "meeting", "at", "noon"]
        english.guessTable = ["mercu": ["mercy"], "est": ["east", "set"], "meetign": ["meeting"], "chst": ["chat"]]
        english.completionTable = ["mer": ["merry", "mercy"]]
        let french = stubbed {
            $0.valid = ["je", "pense", "que", "dont", "est", "le", "chat", "merci"]
            $0.guessTable = ["mercu": ["merci"], "the": ["thé"], "chst": ["chat"], "dont": ["don't"]]
            $0.completionTable = ["mer": ["merci", "mercredi"]]
        }
        french.setLanguage(.french, available: available)
        french.setPartner(.english, available: available, checker: english)
        XCTAssertEqual(french.partner?.language, .english)

        XCTAssertNil(fix(french, before: "the"), "an English word stays on the French keyboard")
        XCTAssertNil(fix(french, before: "dont"), "French \"dont\" never becomes \"don't\"")
        XCTAssertEqual(fix(french, before: "teh"), "the", "an English typo is fixed while French types")
        XCTAssertEqual(fix(french, before: "meetign"), "meeting")
        XCTAssertEqual(fix(french, before: "chst"), "chat")
        XCTAssertEqual(fix(french, before: "mercu"), "merci", "both can fix it: the keyboard's own language")
        XCTAssertEqual(fix(french, before: "je pense que mercu"), "merci")
        XCTAssertEqual(fix(french, before: "see you mercu"), "mercy", "the words before are English")
        XCTAssertEqual(Array(strip(french, "mercu").dropFirst()), ["merci", "mercy"], "both languages' fixes in the strip")
        XCTAssertEqual(Array(strip(french, "mer").dropFirst()), ["merci", "merry"], "and both languages' completions")

        // English typing with French beside it: French words stay, "i" is still "I".
        let englishFirst = stubbed { $0.valid = english.valid; $0.guessTable = english.guessTable }
        englishFirst.setLanguage(.english, available: available)
        let frenchChecker = StubChecker()
        frenchChecker.valid = ["dont", "est", "merci"]
        englishFirst.setPartner(.french, available: available, checker: frenchChecker)
        XCTAssertNil(fix(englishFirst, before: "est"))
        XCTAssertNil(fix(englishFirst, before: "dont"), "French \"dont\" stays")
        XCTAssertEqual(fix(englishFirst, before: "i"), "I")

        // No partner without a dictionary for it, or for the keyboard's own language.
        french.setPartner(.german, available: available)
        XCTAssertNil(french.partner)
        french.setPartner(.english, available: available, checker: english)
        french.setLanguage(.serbianLatin, available: available)
        XCTAssertNil(french.partner)
    }

    /// A partner whose dictionary takes any letters for a word (the iOS 27.2 simulator's pl_PL
    /// has no word data) is left out: English beside it still fixes "teh", where it kept every
    /// typo as "a Polish word"; a partner whose dictionary works still keeps its words.
    func testAPartnerDictionaryThatTakesEveryWordIsIgnored() {
        let available = ["en_US", "pl_PL"]
        let english = stubbed {
            $0.valid = ["the", "and", "just"]
            $0.guessTable = ["tge": ["the"], "jest": ["just"]]
        }
        english.setLanguage(.english, available: available)
        let empty = StubChecker()
        empty.acceptsEverything = true
        english.setPartner(.polish, available: available, checker: empty)
        XCTAssertEqual(english.partner?.language, .polish)
        XCTAssertEqual(fix(english, before: "teh"), "the")
        XCTAssertEqual(fix(english, before: "tge"), "the")
        XCTAssertTrue(strip(english, "tge").contains("the"))
        XCTAssertEqual(fix(english, before: "jest"), "just", "no working Polish dictionary to keep it")

        english.setPartner(nil, available: available)
        let polish = StubChecker()
        polish.valid = ["jest", "tak"]
        english.setPartner(.polish, available: available, checker: polish)
        XCTAssertNil(fix(english, before: "jest"), "a Polish word stays beside a working Polish dictionary")
        XCTAssertEqual(fix(english, before: "teh"), "the")
    }

    /// How often one language's keyboard changes the other's words, against iOS's own
    /// dictionaries and English's Spatial autocorrect model: common French words typed on the
    /// English keyboard and common English words on the French one (taps where the layout has
    /// the letters, else the word put in as it is), each in an empty field. With both
    /// languages chosen, none changes. Prints `BILINGUAL_BREAKS` lines.
    func testABilingualKeyboardKeepsBothLanguagesWords() throws {
        let frenchWords = [
            "bonjour", "merci", "demain", "toujours", "dont", "est", "pour", "sur", "pas", "plus",
            "tout", "mais", "elle", "les", "des", "une", "que", "qui", "dans", "avec", "fait",
            "être", "très", "où", "ça", "même", "bien", "aussi", "comme", "nous", "vous", "ils",
            "sont", "été", "après", "encore", "chez", "parce", "rien", "peu", "beaucoup",
            "jamais", "cette", "notre", "votre", "leur", "quand", "alors", "deux", "trois",
            "jour", "temps", "voir", "faire", "dire", "aller", "veux", "peut", "suis", "oui",
            "non", "soir", "matin", "maison", "travail", "ami", "amis", "envie", "vraiment",
            "bisous", "salut", "réunion", "semaine", "rendez", "arrive", "attends", "train",
        ]
        let englishWords = [
            "the", "and", "you", "that", "was", "for", "are", "with", "his", "they", "this",
            "have", "from", "one", "had", "word", "but", "not", "what", "all", "were", "when",
            "your", "can", "said", "there", "use", "each", "which", "she", "how", "their",
            "will", "other", "about", "out", "many", "then", "them", "these", "some", "her",
            "would", "make", "like", "him", "into", "time", "has", "look", "two", "more",
            "write", "see", "number", "way", "could", "people", "than", "first", "water",
            "been", "call", "who", "now", "find", "long", "down", "day", "did", "get", "come",
            "made", "may", "part", "tomorrow", "meeting", "thanks", "weekend", "tonight",
        ]
        func typed(_ words: [String], on language: KeyboardLanguage, partner: KeyboardLanguage?) -> [String] {
            engine.language = language
            engine.partnerLanguage = partner
            engine.autocorrect.decoder = TypingDecoderModel.bundled(language.rawValue).map { TypingDecoder(model: $0) }
            engine.autocorrect.partner?.decoder = partner.flatMap { TypingDecoderModel.bundled($0.rawValue) }.map { TypingDecoder(model: $0) }
            var changed: [String] = []
            for word in words {
                while target.hasText { target.deleteBackward() }
                engine.textDidChange()
                let keys = word.map { engine.layout.key(.character(String($0))) }
                if keys.allSatisfy({ $0 != nil }) {
                    for ch in word { tap(.character(String(ch))) }
                } else {
                    target.insertText(word)
                    engine.textDidChange()
                }
                tap(.space)
                if target.text != word + " " { changed.append("\(word)→\(target.text.trimmingCharacters(in: .whitespaces))") }
            }
            return changed
        }
        let englishAlone = typed(frenchWords, on: .english, partner: nil)
        let frenchAlone = typed(englishWords, on: .french, partner: nil)
        let englishWithFrench = typed(frenchWords, on: .english, partner: .french)
        let frenchWithEnglish = typed(englishWords, on: .french, partner: .english)
        let englishKeepsEnglish = typed(englishWords, on: .english, partner: .french)
        let frenchKeepsFrench = typed(frenchWords, on: .french, partner: .english)
        print("BILINGUAL_BREAKS English alone changed \(englishAlone.count) of \(frenchWords.count) French words: \(englishAlone)")
        print("BILINGUAL_BREAKS Français alone changed \(frenchAlone.count) of \(englishWords.count) English words: \(frenchAlone)")
        print("BILINGUAL_BREAKS English + Français, English typing, changed \(englishWithFrench.count) French words: \(englishWithFrench)")
        print("BILINGUAL_BREAKS English + Français, Français typing, changed \(frenchWithEnglish.count) English words: \(frenchWithEnglish)")
        print("BILINGUAL_BREAKS own words changed: English \(englishKeepsEnglish), Français \(frenchKeepsFrench)")
        XCTAssertEqual(englishWithFrench, [])
        XCTAssertEqual(frenchWithEnglish, [])
        XCTAssertEqual(englishKeepsEnglish, [])
        XCTAssertEqual(frenchKeepsFrench, [])
    }

    /// The iPhone's own dictionaries: English typos are fixed on the French keyboard while
    /// French and English words stay as typed.
    func testTheFrenchKeyboardFixesEnglishTyposWithEnglishBesideIt() {
        engine.language = .french
        engine.partnerLanguage = .english
        XCTAssertEqual(engine.autocorrect.partner?.language, .english)
        for word in ["Bonjour", "merci", "the", "meeting", "est", "dont", "teh", "tomorow", "demian"] {
            typeWord(word)
            tap(.space)
        }
        XCTAssertEqual(target.text, "Bonjour merci the meeting est dont the tomorrow demain ")
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

    func testShortCompletionsFollowThePhraseAndKeepTheTypedCase() throws {
        engine.autoCapitalizes = false
        engine.autocorrect.isEnabled = false
        for (language, prefix, expected) in [(KeyboardLanguage.english, "I would li", "like"),
                                               (.english, "I would Li", "Like"),
                                               (.spanish, "muchas gr", "gracias")] {
            engine.language = language
            let model = try XCTUnwrap(TypingDecoderModel.bundled(language.rawValue))
            let strip = engine.autocorrect.suggestions(
                beforeInput: prefix, field: .text, autocorrectionType: .default, predictionModel: model
            )
            XCTAssertEqual(strip.dropFirst().first?.replacement, expected)
            XCTAssertFalse(strip.contains { $0.willAutoReplace })
        }
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

    func testACommonBigramDoesNotSwallowADistinctWellAimedTap() throws {
        engine.width = 402
        try useDecoder()
        engine.letterOdds = engine.autocorrect.decoder?.model
        target.insertText("I promised ")
        // These three taps read "tom". "Promised to" is common, but the m was neither a
        // repeat nor a touch between the neighbouring keys.
        for (letter, dx, dy) in [("t", -7.2, 6.6), ("o", 14.5, 12.5), ("m", -10.0, 3.9)] {
            let key = try XCTUnwrap(engine.layout.key(.character(letter)))
            tap(at: CGPoint(x: key.frame.midX + dx, y: key.frame.midY + dy))
        }
        tap(.space)
        XCTAssertEqual(target.text.lowercased(), "i promised tom ")
    }

    /// The models' word lists are lowercase: a name or day typed in lowercase is still a
    /// word, capitalised as the dictionary path does, never "may" or "money".
    func testSpatialAutocorrectCapitalisesANameTypedInLowercase() throws {
        try useDecoder()
        for word in ["see", "mary", "on", "monday", "in", "boston"] { typeTaps(word) }
        XCTAssertEqual(target.text, "see Mary on Monday in Boston ")
    }

    /// Auto-capitalization off: a name typed in lowercase stays lowercase, and a slip in one
    /// of your own names still gets its capital.
    func testSpatialAutocorrectKeepsNamesLowercaseWithAutoCapitalizationOff() throws {
        try useDecoder()
        engine.autoCapitalizes = false
        engine.autocorrect.loadLexiconEntries([("Zorbix", "Zorbix")])
        for word in ["see", "mary", "on", "monday", "with", "zorbiz"] { typeTaps(word) }
        XCTAssertEqual(target.text, "see mary on monday with Zorbix ")
    }

    /// German's model is Android's, byte for byte: the same v3 file, with its own language
    /// and a vocabulary of lowercase German words ("haus", never "haua").
    func testTheBundledGermanModelLoads() throws {
        let model = try XCTUnwrap(TypingDecoderModel.bundled("de"), "the German model is bundled")
        XCTAssertEqual(model.language, "de")
        XCTAssertGreaterThan(model.wordCount, 50_000)
        XCTAssertNotNil(model.index(of: "haus"), "a common German word")
        XCTAssertNil(model.index(of: "haua"), "not a word")
    }

    /// The capitalisation sidecar rides beside the German model (same bundle, same embedded
    /// keyboard fallback): common nouns are written with a capital nearly always, while a word
    /// that is also a common verb form is in the list below that ("frage" 94, "reise" 94,
    /// "folge" 94), and a pure verb or function word is 50-94 too ("leben" 82) or absent.
    func testTheBundledGermanCapitalsLoad() throws {
        let capitals = try XCTUnwrap(GermanCapitals.bundled(), "de.capitals is bundled with the German model")
        for noun in ["haus", "auto", "zeit"] {
            XCTAssertGreaterThanOrEqual(capitals.percent(of: noun) ?? 0, 95, noun)
        }
        for verbForm in ["frage", "reise", "folge"] {
            XCTAssertLessThan(capitals.percent(of: verbForm) ?? 0, 95, verbForm)
        }
        for other in ["machen", "ist"] {
            XCTAssertNil(capitals.percent(of: other), other)
        }
    }

    /// Settings › Experiments › German spatial autocorrect, off by default, gates German alone.
    /// Without it the Deutsch layout keeps the dictionary engine exactly as before: no word
    /// list either, so no letter odds, no model completions in the strip, and no partner model.
    /// English and Spanish never wait on the experiment.
    func testTheGermanTypingModelNeedsItsExperiment() {
        let settings = FullKeyboardSettings(defaults: defaults)
        XCTAssertFalse(settings.germanSpatialAutocorrect, "off by default")
        XCTAssertNil(settings.typingModel(for: .german), "no German model unless the experiment is on")
        settings.spatialAutocorrect = true
        XCTAssertNil(settings.typingModel(for: .german), "and never without Spatial autocorrect")
        settings.germanSpatialAutocorrect = true
        XCTAssertEqual(settings.typingModel(for: .german)?.language, "de")
        settings.spatialAutocorrect = false
        XCTAssertNil(settings.typingModel(for: .german), "Spatial autocorrect off turns it off again")
        XCTAssertEqual(settings.typingModel(for: .english)?.language, "en", "English never waits on the experiment")
        XCTAssertEqual(settings.typingModel(for: .spanish)?.language, "es")
        settings.spatialAutocorrect = true
        XCTAssertEqual(settings.typingModel(for: .english)?.language, "en")
        XCTAssertEqual(settings.typingModel(for: .spanish)?.language, "es")
    }

    func testSpatialAutocorrectAddsSpanishAccents() throws {
        try useDecoder(.spanish)
        for word in ["esta", "aqui", "manana"] { typeTaps(word) }
        XCTAssertEqual(target.text, "esta aquí mañana ", "\"esta\" is a word too, so it stays")
    }

    /// German spells every noun with a capital, and its model is lowercase throughout: a noun
    /// the decoder corrects to takes its capital, and so does one typed right, as the list
    /// beside the model (`de.capitals`) decides. Apple's German dictionary takes "haus",
    /// "auto" and "zeit" lowercase, so it cannot be the one to tell.
    func testGermanSpatialAutocorrectCapitalisesTheNounItCorrects() throws {
        let layout = KeyboardLayout(width: 393, language: .german)
        let keys = try XCTUnwrap(TypingDecoder.Keys(layout: layout))
        let checker = StubChecker()
        checker.valid = ["ist", "das", "die", "zu", "haus", "auto", "zeit", "leben",
                         "ich", "frage", "wollt", "ihr", "essen", "heute"]
        checker.capitalised = ["Stadt", "Abend"]
        let autocorrect = AutocorrectEngine(checker: checker, learnedWords: LearnedWordsStore(defaults: defaults))
        autocorrect.setLanguage(.german, available: ["de"])
        autocorrect.germanCapitals = GermanCapitals(percentages: [
            "haus": 99, "auto": 99, "zeit": 98, "leben": 60, "frage": 94, "essen": 61, "abend": 94,
        ])
        autocorrect.decoder = TypingDecoder(model: try XCTUnwrap(TypingDecoderModel.bundled("de")))
        autocorrect.decoderKeys = keys
        func fix(_ before: String, taps: String) -> String? {
            autocorrect.decoderTaps = taps.map { keys.centres[$0]! }
            return autocorrect.replacementOnSpace(beforeInput: before, field: .text, autocorrectionType: .default)?.replacement
        }
        // A tap on a where s was meant, and on s where a was meant: both read "haus".
        XCTAssertEqual(fix("das haua", taps: "haua"), "Haus")
        XCTAssertEqual(fix("das hsus", taps: "hsus"), "Haus")
        // Typed right, the noun still takes its capital after the article.
        XCTAssertEqual(fix("das haus", taps: "haus"), "Haus")
        // Written with a capital only half the time: a noun after an article or determiner,
        // and the verb elsewhere ("zu leben").
        XCTAssertEqual(fix("das leben", taps: "leben"), "Leben")
        XCTAssertNil(fix("zu leben", taps: "leben"))
        // A 50-94% word with no determiner before it keeps the dictionary's rule: a verb form
        // stays lowercase ("ich frage", "wollt ihr essen"), and a word the dictionary knows
        // only capitalised still takes its capital ("heute abend" -> "heute Abend").
        XCTAssertNil(fix("ich frage", taps: "frage"))
        XCTAssertNil(fix("wollt ihr essen", taps: "essen"))
        XCTAssertEqual(fix("heute abend", taps: "abend"), "Abend")
        // A word the list doesn't know keeps the dictionary's rule too.
        XCTAssertEqual(fix("die stadt", taps: "stadt"), "Stadt")
        // Auto-capitalization off adds no capital you did not type.
        autocorrect.addsCapitals = false
        XCTAssertNil(fix("das haus", taps: "haus"))
        XCTAssertEqual(fix("das haua", taps: "haua"), "haus")
    }

    func testSpatialAutocorrectKnowsYourOwnWords() throws {
        try useDecoder()
        engine.autocorrect.loadLexiconEntries([("Zorbix", "Zorbix")])
        typeTaps("zorbiz")
        typeTaps("zorbix")
        XCTAssertEqual(target.text, "Zorbix zorbix ", "a slip in a name from Contacts, and the name typed right")
    }

    /// "th", then the e aimed high 80 ms later, landing on the strip's lower half under its
    /// left slot. Record typing logs showed that picked what was typed and a space ("th  "),
    /// and kept "th" from being corrected for the rest of the session. The keys take the
    /// tap now, on row 1's top edge, and Spatial autocorrect keeps the "the" it spells.
    private func tapCentre(_ letter: Character) {
        let key = engine.layout.key(.character(String(letter)))!.frame
        tap(at: CGPoint(x: key.midX, y: key.midY))
    }

    /// X (Erick, Justin): "Auto correct does not separate two words like apple does." A space
    /// tapped as the b, n, v or c above the space bar is read as one wherever on the key it
    /// lands (the system keyboard turns "ofbthe" into "of the" at the key's centre too), and
    /// a missed space splits the words, with English "i" a capital.
    func testASpaceTappedAsALetterAboveTheSpaceBarOrMissedSplitsTheWords() throws {
        try useDecoder()
        typeTaps("is")
        typeTaps("this")
        for letter in ["b", "n", "v", "c"] {
            for dy: CGFloat in [0, -8, 10] {
                for ch in "of" { tapCentre(ch) }
                let key = engine.layout.key(.character(letter))!.frame
                let point = CGPoint(x: key.midX, y: key.midY + dy)
                XCTAssertEqual(engine.layout.hitTest(point)?.kind, .character(letter))
                tap(at: point)
                typeTaps("the")
                XCTAssertTrue(target.text.hasSuffix(" of the "), "\(letter) \(dy): \(target.text)")
            }
        }
        typeTaps("iwant")
        XCTAssertTrue(target.text.hasSuffix(" I want "), target.text)
        typeTaps("lookat")
        XCTAssertTrue(target.text.hasSuffix(" look at "), target.text)
        // A closing mark from the 123 page reads the word's taps as space does.
        for ch in "ofthe" { tapCentre(ch) }
        tap(.layerSwitch(.numbers))
        tap(.character("."))
        XCTAssertTrue(target.text.hasSuffix(" of the."), target.text)
        tap(.space)
        // A word with a bottom-row letter stays one word.
        for word in ["number", "cabin", "seven", "become"] { typeTaps(word) }
        XCTAssertTrue(target.text.hasSuffix(" number cabin seven become "), target.text)
    }

    /// X (Justin): "Space bar accidental taps or lack of taps don't seem to be factored in." A
    /// space tapped at the space bar's top edge, under a letter the word needed, joins the
    /// words at the next word's space, as the system keyboard does ("deli ate" -> "delicate");
    /// one tapped well inside the bar never does, and one backspace puts back both words.
    func testASpaceTappedAtTheSpaceBarsEdgeForALetterJoinsTheWords() throws {
        try useDecoder()
        engine.letterOdds = engine.autocorrect.decoder?.model
        let space = engine.layout.key(.space)!
        func edgeSpace(under letter: Character) {
            let key = engine.layout.key(.character(String(letter)))!.frame
            let point = CGPoint(x: key.midX, y: space.hitFrame.minY + 1)
            XCTAssertEqual(engine.layout.hitTest(point)?.kind, .space)
            tap(at: point)
        }
        typeTaps("the")
        for ch in "deli" { tapCentre(ch) }
        edgeSpace(under: "c")
        typeTaps("ate")
        XCTAssertEqual(target.text, "the delicate ")
        tap(.delete)
        XCTAssertEqual(target.text, "the deli ate", "one backspace puts back both words")

        tap(.space)
        typeTaps("so")
        for ch in "ha" { tapCentre(ch) }
        edgeSpace(under: "n")
        typeTaps("ded")
        XCTAssertTrue(target.text.hasSuffix(" so handed "), target.text)

        for ch in "deli" { tapCentre(ch) }
        tap(.space)
        typeTaps("ate")
        XCTAssertTrue(target.text.hasSuffix(" deli ate "), "a space inside the bar: \(target.text)")

        // Two words space would fix on their own stay two ("i dkdnt" is "I didn't", never one word).
        tapCentre("i")
        edgeSpace(under: "b")
        typeTaps("didnt")
        XCTAssertTrue(target.text.hasSuffix(" I didn't "), target.text)
    }

    func testSpatialAutocorrectReadsAnETappedHighOnTheStripAsThe() throws {
        try useDecoder()
        typeTaps("on")
        for letter in ["t", "h"] {
            let key = engine.layout.key(.character(letter))!.frame
            engine.touchBegan(id: 1, at: CGPoint(x: key.midX, y: key.midY))
            engine.touchEnded(id: 1, at: CGPoint(x: key.midX, y: key.midY))
            now += 0.1
        }
        let e = engine.layout.key(.character("e"))!.frame
        let aimedHigh = CGPoint(x: e.midX, y: engine.layout.stripFrame.maxY - 6)
        XCTAssertTrue(engine.takesStripTouch(at: aimedHigh))
        tap(at: aimedHigh)
        tap(.space)
        XCTAssertEqual(target.text, "on the ")
    }

    // MARK: - Offensive words

    func testTheBundledListMarksSlursAndSwearWordsAndLeavesAnatomyAlone() {
        let en = OffensiveWords.bundled("en")
        XCTAssertEqual(en.kind(of: "nigger"), .slur)
        XCTAssertEqual(en.kind(of: "Faggots"), .slur, "inflections, any case")
        XCTAssertEqual(en.kind(of: "chink"), .slur, "slurs LDNOOBW lacks")
        XCTAssertEqual(en.kind(of: "fuck"), .swear)
        XCTAssertEqual(en.kind(of: "damn"), .swear)
        for word in ["penis", "vagina", "anus", "sex", "suck", "rape", "escort", "scat", "xx"] {
            XCTAssertNil(en.kind(of: word), "\(word) is an everyday, anatomy, medical or reporting word")
        }
        XCTAssertTrue(en.blocks("blow job", allowSwearing: false), "an expression")
        XCTAssertTrue(en.blocks("job", allowSwearing: false, after: "blow"), "an expression's second word after its first")
        XCTAssertFalse(en.blocks("blow", allowSwearing: false), "an expression's words alone are fine")
        XCTAssertFalse(en.blocks("job", allowSwearing: false, after: "a"))
        XCTAssertTrue(en.blocks("the shit", allowSwearing: false), "a listed word inside a split")
        XCTAssertFalse(en.blocks("the shit", allowSwearing: true))
        XCTAssertTrue(en.blocks("the nigger", allowSwearing: true))
        let es = OffensiveWords.bundled("es")
        XCTAssertEqual(es.kind(of: "maricón"), .slur)
        XCTAssertEqual(es.kind(of: "mierda"), .swear)
        XCTAssertNil(es.kind(of: "negro"), "Spanish for black")
        XCTAssertNil(es.kind(of: "cono"), "accents are kept: coño is listed, cono (cone) isn't")
        XCTAssertEqual(OffensiveWords.bundled("de").kind(of: "scheisse"), .swear)
        XCTAssertNil(OffensiveWords.bundled("de").kind(of: "dick"), "German for thick")
        XCTAssertEqual(OffensiveWords.bundled("nb").kind(of: "faen"), .swear)
        XCTAssertNil(OffensiveWords.bundled("nb").kind(of: "fag"), "Norwegian for a trade")
        let it = OffensiveWords.bundled("it")
        XCTAssertEqual(it.kind(of: "nigger"), .slur, "every layout shares the loanwords")
        XCTAssertEqual(it.kind(of: "fuck"), .swear)
        XCTAssertNil(it.kind(of: "chink"))
    }

    func testAnExpressionIsBlockedOnlyAfterItsFirstWord() {
        let autocorrect = stubbed {
            $0.valid = ["blow", "a", "jo", "job", "joke"]
            $0.completionTable = ["jo": ["job", "joke"]]
        }
        XCTAssertEqual(Array(strip(autocorrect, "blow jo").dropFirst()), ["joke"])
        XCTAssertEqual(Array(strip(autocorrect, "a jo").dropFirst()), ["job", "joke"])
        autocorrect.allowsSwearing = true
        XCTAssertEqual(Array(strip(autocorrect, "blow jo").dropFirst()), ["job", "joke"])
    }

    func testSwearWordsAreNeverGuessedOrCompletedUnlessTurnedOn() {
        let autocorrect = stubbed {
            $0.valid = ["ship", "duck", "fuchsia"]
            $0.guessTable = ["shiy": ["shit", "ship"]]
            $0.completionTable = ["fuc": ["fuck", "fucking", "fuchsia"]]
        }
        XCTAssertEqual(fix(autocorrect, "shiy"), "ship", "shit is a closer key, but never offered")
        XCTAssertFalse(strip(autocorrect, "shiy").contains("shit"))
        XCTAssertEqual(Array(strip(autocorrect, "fuc").dropFirst()), ["fuchsia"])

        autocorrect.allowsSwearing = true
        XCTAssertEqual(fix(autocorrect, "shiy"), "shit")
        XCTAssertEqual(Array(strip(autocorrect, "fuc").dropFirst()), ["fuck", "fucking"])
    }

    func testSlursAreNeverOfferedEvenWithSwearWordsOn() {
        let autocorrect = stubbed {
            $0.guessTable = ["niggr": ["nigger", "nigga"], "fagot": ["faggot", "fagots"]]
            $0.completionTable = ["nig": ["nigger", "night", "nigh"]]
            $0.valid = ["night", "nigh", "fagots"]
        }
        autocorrect.allowsSwearing = true
        XCTAssertNil(fix(autocorrect, "niggr"))
        XCTAssertEqual(strip(autocorrect, "niggr"), ["niggr"])
        XCTAssertEqual(fix(autocorrect, "fagot"), "fagots")
        XCTAssertEqual(Array(strip(autocorrect, "nig").dropFirst()), ["night", "nigh"])
    }

    func testAnOffensiveWordTypedInFullIsNeverCorrected() {
        let autocorrect = stubbed {
            $0.guessTable = ["fuck": ["duck", "luck"], "nigger": ["bigger", "niggle"]]
        }
        XCTAssertNil(fix(autocorrect, "fuck"), "UITextChecker may not know it, but you typed it")
        XCTAssertNil(fix(autocorrect, "Nigger"))
        XCTAssertFalse(autocorrect.suggestions(beforeInput: "fuck", field: .text, autocorrectionType: .default)
            .contains(where: \.willAutoReplace))
        autocorrect.allowsSwearing = true
        XCTAssertEqual(fix(autocorrect, "fuck"), "duck", "with swear words on, it's an ordinary word, as on Android")
        XCTAssertNil(fix(autocorrect, "Nigger"), "a slur typed in full still stays")
    }

    func testYourOwnContactsAndReplacementsAreStillOffered() {
        let autocorrect = stubbed { $0.completionTable = ["dic": ["dick", "dictionary"]] }
        autocorrect.loadLexiconEntries([("Dick", "Dick"), ("wtf", "what the fuck")])
        XCTAssertEqual(Array(strip(autocorrect, "Dic").dropFirst()), ["Dick", "Dictionary"])
        XCTAssertEqual(fix(autocorrect, "wtf"), "what the fuck")
    }

    /// What iOS's own dictionary does with near-misses of swear words and slurs (printed for
    /// the record), and that the strip and space never pass them on.
    func testTheSystemDictionarysOffensiveGuessesNeverReachTheStrip() {
        let checker = SystemTextChecker()
        let words = OffensiveWords.bundled("en")
        for typed in ["fuc", "fuvk", "fucm", "shi", "shiy", "shjt", "bitc", "bithc", "cun", "cunr", "dic", "dicj",
                      "nig", "nigg", "niggr", "niggee", "fagg", "faggit", "kik", "spi", "coo", "pus", "pussu", "ass", "asshol"] {
            let guesses = checker.guesses(for: typed)
            let completions = checker.completions(for: typed)
            let flagged = (guesses + completions).filter { words.kind(of: $0) != nil }
            print("UITextChecker \(typed): valid=\(checker.isWordValid(typed)) guesses=\(guesses.prefix(6)) completions=\(completions.prefix(6)) offensive=\(flagged)")
            for allow in [false, true] {
                engine.autocorrect.allowsSwearing = allow
                for item in strip(engine.autocorrect, typed).dropFirst() {
                    XCTAssertFalse(words.blocks(item, allowSwearing: allow), "\(typed) offered \(item), swear words \(allow)")
                }
                if let replacement = fix(engine.autocorrect, typed) {
                    XCTAssertFalse(words.blocks(replacement, allowSwearing: allow), "\(typed) corrected to \(replacement)")
                }
            }
        }
    }

    /// Types `word` at the keys' centres, no space, and returns the strip's words.
    private func stripAfterTaps(_ word: String, after before: [String] = []) -> [String] {
        for previous in before { typeTaps(previous) }
        for ch in word {
            let key = engine.layout.key(.character(String(ch)))!.frame
            tap(at: CGPoint(x: key.midX, y: key.midY))
        }
        return engine.suggestions.dropFirst().map(\.replacement)
    }

    func testSpatialAutocorrectNeverReadsTapsAsAnOffensiveWord() throws {
        try useDecoder()
        let words = OffensiveWords.bundled("en")
        engine.autocorrect.allowsSwearing = true
        XCTAssertTrue(stripAfterTaps("fucj", after: ["what", "the"]).contains("fuck"), "with swear words on, the model offers it")
        tap(.space)
        let probes = ["fucj", "fuxk", "shjt", "shiy", "bitcg", "cunr", "dicj", "pussu", "nigher", "niggee", "faggit", "kikw", "cocj"]
        for allow in [false, true] {
            engine.autocorrect.allowsSwearing = allow
            for probe in probes {
                let strip = stripAfterTaps(probe, after: ["what", "a"])
                for item in strip {
                    XCTAssertFalse(words.blocks(item, allowSwearing: allow), "\(probe) offered \(item), swear words \(allow)")
                }
                tap(.space)
                let typed = target.text.split(separator: " ").last.map(String.init) ?? ""
                XCTAssertFalse(words.blocks(typed, allowSwearing: allow), "\(probe) became \(typed), swear words \(allow)")
            }
        }
    }

    func testSpatialAutocorrectKeepsAnOffensiveWordTypedInFull() throws {
        try useDecoder()
        for word in ["what", "the", "fuck", "you", "bitch"] { typeTaps(word) }
        XCTAssertEqual(target.text, "what the fuck you bitch ")
    }

    /// Apple's keyboard with Auto-Correction off (black box, iOS 27.2) offers what you typed,
    /// your text replacements and completions ("th": The, This; "beau": Beautiful, Beauty;
    /// "im": Imagine), but no fix: nothing for "teh" or "dont", no "I'm" for "im".
    func testTurningAutocorrectSettingOffKeepsCompletionsWithoutFixes() {
        engine.autocorrect.isEnabled = false
        typeWord("dont")
        XCTAssertEqual(engine.suggestions.first?.replacement, "dont")
        XCTAssertFalse(engine.suggestions.contains { $0.replacement == "don't" }, "no contraction")
        tap(.space)
        XCTAssertEqual(target.text, "dont ")
        typeWord("teh")
        XCTAssertFalse(engine.suggestions.contains { $0.replacement == "the" }, "no spelling guess")
        tap(.space)
        typeWord("th")
        XCTAssertEqual(engine.suggestions.dropFirst().first?.replacement, "the", "completions stay")
        XCTAssertFalse(engine.suggestions.contains { $0.willAutoReplace })

        engine.autocorrect.isEnabled = true
        tap(.space)
        typeWord("teh")
        XCTAssertEqual(engine.suggestions.dropFirst().first?.replacement, "the", "on: the fix")
    }

    /// A TestFlight tester (build 56) kept Autocorrect on only for text replacements. Apple's
    /// keyboard with Auto-Correction off (iOS 27.2) still shows "omw" as "On my way!" in the
    /// strip and puts it in on space or "."; "teh" stays.
    func testTextReplacementsApplyWithAutocorrectOff() {
        engine.autocorrect.loadLexiconEntries([("omw", "On my way!"), ("addr", "123 Market St")])
        engine.autocorrect.isEnabled = false
        typeWord("omw")
        XCTAssertEqual(engine.suggestions.dropFirst().first?.replacement, "On my way!")
        XCTAssertTrue(engine.suggestions.dropFirst().first?.willAutoReplace == true)
        tap(.space)
        XCTAssertEqual(target.text, "On my way! ")
        tap(.delete)
        XCTAssertEqual(target.text, "omw", "one backspace puts back the shortcut")

        while target.hasText { target.deleteBackward() }
        engine.configureForField()
        // The backspace kept "omw" for the session, so another shortcut.
        typeWord("teh addr")
        tap(.layerSwitch(.numbers))
        tap(.character("."))
        XCTAssertEqual(target.text, "teh 123 Market St.", "a closing mark applies it too; typos stay")

        while target.hasText { target.deleteBackward() }
        target.autocorrectionType = .no
        engine.configureForField()
        typeWord("addr ")
        XCTAssertEqual(target.text, "addr ", "a field without autocorrection gets none, as with Apple's")
    }

    /// Chat shorthand Hex leaves alone ("brb", "idk") still takes your own text replacement,
    /// as Apple's keyboard applies every text replacement, with Autocorrect on or off.
    func testYourTextReplacementWinsOverShorthand() {
        engine.autocorrect.loadLexiconEntries([("brb", "be right back"), ("omg", "oh my god")])
        typeWord("brb")
        XCTAssertEqual(engine.suggestions.dropFirst().first?.replacement, "be right back")
        XCTAssertTrue(engine.suggestions.dropFirst().first?.willAutoReplace == true)
        typeWord(" idk ")
        XCTAssertEqual(target.text, "be right back idk ", "shorthand without a replacement stays")

        engine.autocorrect.isEnabled = false
        typeWord("omg")
        XCTAssertEqual(engine.suggestions.dropFirst().first?.replacement, "oh my god")
        tap(.space)
        XCTAssertEqual(target.text, "be right back idk oh my god ")
        tap(.delete)
        XCTAssertEqual(target.text, "be right back idk omg", "one backspace puts back the shortcut")
    }

    /// A text replacement's case, as Apple's keyboard writes it (black box, iOS 27.2): a
    /// leading capital, typed or auto-capitalised ("Brb"), capitalises its first letter; a
    /// shortcut in lowercase or all caps ("BRB") gets it as stored; stored capitals stay.
    func testTextReplacementsTakeTheShortcutsLeadingCapital() {
        engine.autocorrect.loadLexiconEntries([("brb", "be right back"), ("omw", "On my way!"), ("mip", "My iPhone")])
        typeWord("Brb")
        XCTAssertEqual(engine.suggestions.dropFirst().first?.replacement, "Be right back")
        for (typed, expected) in [("Brb", "Be right back"), ("brb", "be right back"), ("BRB", "be right back"),
                                  ("omw", "On my way!"), ("OMW", "On my way!"), ("mip", "My iPhone"),
                                  ("Mip", "My iPhone"), ("MIP", "My iPhone")] {
            while target.hasText { target.deleteBackward() }
            engine.configureForField()
            typeWord("ok " + typed + " ")
            XCTAssertEqual(target.text, "ok \(expected) ", typed)
        }

        // Auto-capitalised at a sentence start, with Autocorrect on and off.
        target.autocapitalizationType = .sentences
        for autocorrect in [true, false] {
            engine.autocorrect.isEnabled = autocorrect
            while target.hasText { target.deleteBackward() }
            engine.configureForField()
            typeWord("brb ")
            XCTAssertEqual(target.text, "Be right back ", "autocorrect \(autocorrect)")
        }
    }
}


// MARK: - Adaptive touch: personal centres rank readings, never correct on their own

extension AutocorrectEngineTests {
    func testPersonalCentresNeverRewriteAWordTheStockCentresKeep() throws {
        try useDecoder()
        let ac = engine.autocorrect
        let stock = try XCTUnwrap(TypingDecoder.Keys(layout: engine.layout))
        // A profile far past the experiment's bounds: b's centre sits on m, and c's on b.
        var personal = stock
        personal.centres["b"] = stock.centres["m"]
        personal.centres["c"] = stock.centres["b"]
        ac.decoderKeys = personal
        let bat = "bat".map { stock.centres[$0]! }
        func atSpace(_ taps: [CGPoint], stockKeys: TypingDecoder.Keys?) -> String? {
            ac.decoderBaselineKeys = stockKeys
            ac.decoderTaps = taps
            return ac.replacementOnSpace(beforeInput: "the bat", field: .text, autocorrectionType: .default)?.replacement
        }
        XCTAssertEqual(atSpace(bat, stockKeys: nil), "cat", "alone, the personal centres would rewrite \"bat\"")
        let nudged = bat.map { CGPoint(x: $0.x + 0.01, y: $0.y) }
        XCTAssertNil(atSpace(nudged, stockKeys: stock), "the stock centres keep it: no new rewrite")
        let strip = ac.suggestions(beforeInput: "the bat", field: .text, autocorrectionType: .default)
        XCTAssertTrue(strip.contains { $0.replacement == "cat" && !$0.willAutoReplace }, "offered, not applied: \(strip)")

        // A fix both make still goes in.
        ac.decoderKeys = stock
        let thw = "thw".map { stock.centres[$0]! }
        ac.decoderBaselineKeys = stock
        ac.decoderTaps = thw
        XCTAssertEqual(ac.replacementOnSpace(beforeInput: "see thw", field: .text, autocorrectionType: .default)?.replacement, "the")
    }

    func testTheDecodersReplacementCheckAgreesWithItsDecision() throws {
        try useDecoder()
        let decoder = try XCTUnwrap(engine.autocorrect.decoder)
        let keys = try XCTUnwrap(TypingDecoder.Keys(layout: engine.layout))
        for (typed, previous, valid) in [("thw", "see", false), ("teh", nil, false), ("bat", "the", true), ("helo", "say", false), ("ypu", "thank", false)] {
            let taps = typed.map { keys.centres[$0]! }
            let decision = try XCTUnwrap(decoder.decide(taps: taps, typed: typed, previous: previous, literalIsValid: valid, keys: keys))
            if let replacement = decision.replacement, !replacement.contains(" ") {
                XCTAssertEqual(decoder.replaces(with: replacement, taps: taps, typed: typed, previous: previous, literalIsValid: valid, keys: keys), true, typed)
            }
            let margin = valid || decoder.model.index(of: typed) != nil ? decoder.parameters.validMargin : decoder.parameters.autoMargin
            for reading in decision.readings where !reading.text.contains(" ") {
                XCTAssertEqual(
                    decoder.replaces(with: reading.text, taps: taps, typed: typed, previous: previous, literalIsValid: valid, keys: keys),
                    decision.literalCost - reading.cost >= margin, "\(typed) → \(reading.text)"
                )
            }
        }
    }
}


extension AutocorrectEngineTests {
    /// A strong personal profile (every key's centre at the bound, a third of a key to the
    /// right) leaves every word these suites protect as the stock centres do.
    func testPersonalCentresKeepNamesYourWordsAndWordsTypedRight() throws {
        let words = ["come", "and", "miss", "the", "quiz", "see", "mary", "on", "monday", "in", "boston", "with", "zorbix", "kit", "lol"]
        func typed(profile: Bool) throws -> String {
            target = FixtureTextTarget()
            target.autocapitalizationType = UITextAutocapitalizationType.none
            engine = FullKeyboardEngine(target: target, autocorrect: AutocorrectEngine(checker: SystemTextChecker(), learnedWords: LearnedWordsStore(defaults: defaults)))
            engine.width = 393
            engine.clock = { [unowned self] in self.now }
            engine.configureForField()
            try useDecoder()
            engine.autocorrect.loadLexiconEntries([("Zorbix", "Zorbix"), ("Kit", "Kit")])
            if profile { try engageStrongProfile() }
            for word in words { typeTaps(word) }
            return target.text
        }
        XCTAssertEqual(try typed(profile: true), try typed(profile: false))
    }

    /// Joining two words at a space tapped on the bar's edge ("deli ate") is the centres'
    /// read without Adaptive touch to decide.
    func testPersonalCentresNeverJoinWordsTheirOwnWay() throws {
        func typed(profile: Bool) throws -> String {
            target = FixtureTextTarget()
            target.autocapitalizationType = UITextAutocapitalizationType.none
            engine = FullKeyboardEngine(target: target, autocorrect: AutocorrectEngine(checker: SystemTextChecker(), learnedWords: LearnedWordsStore(defaults: defaults)))
            engine.width = 393
            engine.clock = { [unowned self] in self.now }
            engine.configureForField()
            try useDecoder()
            engine.letterOdds = engine.autocorrect.decoder?.model
            if profile { try engageStrongProfile() }
            let space = try XCTUnwrap(engine.layout.key(.space))
            typeTaps("the")
            for ch in "deli" { tapCentre(ch) }
            let c = try XCTUnwrap(engine.layout.key(.character("c"))).frame
            tap(at: CGPoint(x: c.midX, y: space.hitFrame.minY + 1))
            typeTaps("ate")
            return target.text
        }
        XCTAssertEqual(try typed(profile: false), "the delicate ")
        XCTAssertEqual(try typed(profile: true), "the delicate ")
    }

    /// Every key's personal centre at the bound, a third of a key to the right, in use.
    private func engageStrongProfile() throws {
        let touch = AdaptiveTouch(storage: .memory, settings: AdaptiveTouchSettings(defaults: defaults))
        engine.adaptiveTouch = touch
        let geometry = try XCTUnwrap(touch.geometry)
        for n in 0..<600 {
            if n % 150 == 0 { touch.beginSession() }
            let s = n % geometry.letters.count
            let f = geometry.frames[s]
            touch.admit(slot: s, point: CGPoint(x: f.midX + 0.45 * f.width, y: f.midY), quality: AdaptiveTouch.weakQuality)
        }
        XCTAssertTrue(touch.isActive)
        XCTAssertNotEqual(engine.autocorrect.decoderKeys, engine.autocorrect.decoderBaselineKeys)
    }
}
