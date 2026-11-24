import XCTest
@testable import HEX

final class VocabularySpellingTests: XCTestCase {
    func testAppliesSpellingAcrossWords() {
        XCTAssertEqual(
            VocabularySpelling.apply(
                ["Claude", "OpenCode"],
                to: "i use claude in open code."
            ),
            "i use Claude in OpenCode."
        )
    }

    func testJoinsWordsAcrossASingleHyphen() {
        XCTAssertEqual(
            VocabularySpelling.apply(["OpenCode"], to: "Open-Code is great"),
            "OpenCode is great"
        )
    }

    func testDoesNotJoinAcrossOtherPunctuation() {
        XCTAssertEqual(
            VocabularySpelling.apply(["OpenCode"], to: "open, code"),
            "open, code"
        )
    }

    func testMatchesWholeWordsOnly() {
        XCTAssertEqual(
            VocabularySpelling.apply(["Hex"], to: "hexagon"),
            "hexagon"
        )
        XCTAssertEqual(
            VocabularySpelling.apply(["Hex"], to: "hex"),
            "Hex"
        )
    }

    func testKeepsAnEnglishPossessive() {
        XCTAssertEqual(
            VocabularySpelling.apply(["Claude"], to: "claude's idea"),
            "Claude's idea"
        )
        XCTAssertEqual(
            VocabularySpelling.apply(["Claude"], to: "claude’s idea"),
            "Claude’s idea"
        )
    }

    func testDoesNotMatchASimilarWord() {
        XCTAssertEqual(
            VocabularySpelling.apply(["Claude"], to: "a cloud"),
            "a cloud"
        )
    }

    func testEmptyAndTooShortTermsDoNothing() {
        let text = "i use claude in open code."
        XCTAssertEqual(VocabularySpelling.apply([], to: text), text)
        XCTAssertEqual(VocabularySpelling.apply(["a"], to: text), text)
    }

    func testMatchesAMultiWordTerm() {
        XCTAssertEqual(
            VocabularySpelling.apply(["iPhone 17"], to: "my iphone 17 broke"),
            "my iPhone 17 broke"
        )
    }

    func testMatchesSpacedTermsThatIncludeAOneLetterWord() {
        XCTAssertEqual(
            VocabularySpelling.apply(["iPhone 7"], to: "my iphone 7 broke"),
            "my iPhone 7 broke"
        )
        XCTAssertEqual(
            VocabularySpelling.apply(["Plan B"], to: "go with plan b"),
            "go with Plan B"
        )
        XCTAssertEqual(
            VocabularySpelling.apply(["Formula 1"], to: "the formula 1 race"),
            "the Formula 1 race"
        )
    }

    func testMatchesAHyphenatedTermAcrossAHyphenOrASpace() {
        XCTAssertEqual(
            VocabularySpelling.apply(["X-ray"], to: "an x-ray today"),
            "an X-ray today"
        )
        XCTAssertEqual(
            VocabularySpelling.apply(["X-ray"], to: "an x ray"),
            "an X-ray"
        )
    }

    func testMatchesACompoundTermAcrossThePlainPhrase() {
        XCTAssertEqual(
            VocabularySpelling.apply(["WeChat"], to: "message me on we chat"),
            "message me on WeChat"
        )
    }

    func testPrefersTheLongestWindow() {
        XCTAssertEqual(
            VocabularySpelling.apply(["Open", "OpenCode"], to: "open code"),
            "OpenCode"
        )
    }

    func testDoesNotGlueOrdinaryWordsIntoAName() {
        XCTAssertEqual(
            VocabularySpelling.apply(["Ina"], to: "live in a house"),
            "live in a house"
        )
        XCTAssertEqual(
            VocabularySpelling.apply(["Isa"], to: "this is a test"),
            "this is a test"
        )
        XCTAssertEqual(
            VocabularySpelling.apply(["Arun"], to: "go for a run"),
            "go for a run"
        )
    }

    func testDoesNotMatchAcrossAnApostrophe() {
        XCTAssertEqual(
            VocabularySpelling.apply(["Shell"], to: "she'll come"),
            "she'll come"
        )
        XCTAssertEqual(
            VocabularySpelling.apply(["Well"], to: "we'll go"),
            "we'll go"
        )
    }

    func testDoesNotJoinWordsAcrossNewlines() {
        XCTAssertEqual(
            VocabularySpelling.apply(["OpenCode"], to: "open\n\ncode"),
            "open\n\ncode"
        )
    }

    func testKeepsQuotesOutsideTheMatchedRange() {
        XCTAssertEqual(
            VocabularySpelling.apply(["Claude"], to: "'claude'"),
            "'Claude'"
        )
        XCTAssertEqual(
            VocabularySpelling.apply(["Claude"], to: "‘claude’"),
            "‘Claude’"
        )
    }
}

/// Engines write a name's commonest spelling (TestFlight build 130: "I added the name 'Stacey'
/// but it keeps coming back and says Stacy"). The heard words are what Apple Speech and
/// Parakeet V2 wrote on the Mac Studio for these names.
final class SoundAlikeSpellingTests: XCTestCase {
    private func spell(_ terms: [String], _ text: String, language: String = "en-US") -> String {
        SoundAlikeSpelling.apply(terms, to: text, language: language)
    }

    func testWritesTheListedSpellingOfANameHeardAnotherWay() {
        let names = ["Stacey", "Jon", "Katherine", "Sara", "Kris", "Aimee", "Bryan", "Rachael", "Meghan", "Phillip"]
        XCTAssertEqual(spell(names, "Can you send the invoice to Stacy tomorrow?"), "Can you send the invoice to Stacey tomorrow?")
        XCTAssertEqual(spell(names, "Stacy and John are coming over tonight."), "Stacey and Jon are coming over tonight.")
        XCTAssertEqual(spell(names, "We're going to Catherine's party."), "We're going to Katherine's party.")
        XCTAssertEqual(spell(names, "I borrowed Sarah’s car."), "I borrowed Sara’s car.")
        XCTAssertEqual(spell(names, "Chris sent over the designs."), "Kris sent over the designs.")
        XCTAssertEqual(spell(names, "Thanks, Amy, that helped."), "Thanks, Aimee, that helped.")
        XCTAssertEqual(spell(names, "Brian booked it with Rachel and Megan."), "Bryan booked it with Rachael and Meghan.")
        XCTAssertEqual(spell(names, "Philip is late."), "Phillip is late.")
    }

    /// Sounding alike without the same folded spelling counts only inside a sentence, where a
    /// capital means a name, and not right after another capital (a title).
    func testSoundsAlikeOnlyInsideASentence() {
        XCTAssertEqual(spell(["Geoff"], "Let's invite Jeff to the dinner."), "Let's invite Geoff to the dinner.")
        XCTAssertEqual(spell(["Vercel"], "We deploy the site on Versal."), "We deploy the site on Vercel.")
        XCTAssertEqual(spell(["Geoff"], "Jeff fixed the printer."), "Jeff fixed the printer.")
        XCTAssertEqual(spell(["Geoff"], "Done. Jeff fixed it."), "Done. Jeff fixed it.")
        XCTAssertEqual(spell(["Studio"], "We read A Study in Scarlet."), "We read A Study in Scarlet.")
    }

    func testLeavesOtherWordsAlone() {
        XCTAssertEqual(spell(["Marc"], "I mark the spot."), "I mark the spot.", "lowercase words are never names")
        XCTAssertEqual(spell(["Jon"], "Join us. Jen and Jan came."), "Join us. Jen and Jan came.")
        XCTAssertEqual(spell(["Aimee"], "Aim higher. Am I late?"), "Aim higher. Am I late?")
        XCTAssertEqual(spell(["Rune"], "Run the tests."), "Run the tests.")
        XCTAssertEqual(spell(["Sara", "Sarah"], "Sarah and Sara met."), "Sarah and Sara met.", "both are listed")
        XCTAssertEqual(spell(["kubectl", "Al"], "Ask Cubectl and Al."), "Ask Cubectl and Al.", "lowercase and short terms")
        XCTAssertEqual(spell(["Stacey"], "STACY said hi."), "STACY said hi.", "all capitals")
        XCTAssertEqual(spell(["Stacey"], "Stacy hat angerufen.", language: "de-DE"), "Stacy hat angerufen.", "English only")
    }

    func testTwoTermsThatFitEquallyLeaveTheWordAsHeard() {
        XCTAssertEqual(spell(["Stacey", "Stacie"], "Ask Stacy."), "Ask Stacy.")
    }

    @MainActor
    func testDictationPipelineSpellsASoundAlikeName() async {
        let pipeline = DictationPipeline(style: .verbatim, language: "en-US", vocabulary: ["Stacey"])
        let stages = await pipeline.run("I had lunch with stacy and Stacy.")
        XCTAssertEqual(stages.spelled, "I had lunch with stacy and Stacey.")
        XCTAssertEqual(stages.formatted, "I had lunch with stacy and Stacey.")
    }
}

final class CustomVocabularySettingsTests: XCTestCase {
    func testIsOffByDefaultAndPersistsTerms() {
        let suite = "CustomVocabularySettingsTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let settings = CustomVocabularySettings(defaults: defaults)
        XCTAssertFalse(settings.isEnabled)

        settings.terms = ["OpenCode"]
        XCTAssertEqual(settings.activeTerms, [])
        XCTAssertEqual(CustomVocabularySettings(defaults: defaults).terms, ["OpenCode"])

        settings.isEnabled = true
        XCTAssertTrue(CustomVocabularySettings(defaults: defaults).isEnabled)
        XCTAssertEqual(settings.activeTerms, ["OpenCode"])
    }

    func testSanitizesTerms() {
        let suite = "CustomVocabularySettingsTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let settings = CustomVocabularySettings(defaults: defaults)
        let tooLong = String(repeating: "x", count: CustomVocabularySettings.maxTermLength + 1)
        let longestAllowed = String(repeating: "y", count: CustomVocabularySettings.maxTermLength)
        var rawTerms = [
            "  Claude  ", "", "   ", "claude", "OpenCode", "open code",
            "C++", "C#", "a", tooLong, longestAllowed,
        ]
        rawTerms += (1...105).map { "Term \($0)" }

        settings.terms = rawTerms
        let saved = settings.terms

        XCTAssertEqual(saved.count, CustomVocabularySettings.maxTermCount)
        XCTAssertEqual(saved[0], "Claude")
        XCTAssertEqual(saved[1], "OpenCode")
        XCTAssertEqual(saved[2], longestAllowed)
        XCTAssertFalse(saved.contains(tooLong))
        XCTAssertFalse(saved.contains(""))
        XCTAssertFalse(saved.contains("C++"))
        XCTAssertFalse(saved.contains("C#"))
        XCTAssertFalse(saved.contains("a"))
        XCTAssertFalse(saved.contains("open code"))
        XCTAssertEqual(saved.last, "Term 97")
    }
}


final class WordReplacementsTests: XCTestCase {
    private func withDefaults(_ body: (UserDefaults) throws -> Void) rethrows {
        let suite = "WordReplacementsTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        try body(defaults)
    }

    @MainActor
    func testReplacementsApplyBeforeLearnedFixesAndWinOverOneWithTheSameWords() async {
        let pipeline = DictationPipeline(
            style: .verbatim,
            language: "en-US",
            corrections: [
                DictationCorrection(heard: "jet hub", meant: "JetHub"),
                DictationCorrection(heard: "cloud code", meant: "Claude Code"),
            ],
            replacements: [
                DictationCorrection(heard: "jet hub", meant: "GitHub"),
                DictationCorrection(heard: "one two three four five six seven eight", meant: "eight words"),
            ]
        )
        let stages = await pipeline.run("Push to jet hub with cloud code, one two three four five six seven eight.")
        XCTAssertEqual(stages.fixed, "Push to GitHub with Claude Code, eight words.")
    }

    /// Never a translated dictation: the translator gets the words as heard, and a dictation
    /// that goes in as spoken still gets its replacements.
    @MainActor
    func testReplacementsNeverTouchATranslatedDictation() async {
        let translation = DictationTranslationSettings()
        let wasEnabled = translation.isEnabled, wasTarget = translation.targetLanguage, wasFailure = translation.lastFailure
        defer {
            translation.isEnabled = wasEnabled
            translation.targetLanguage = wasTarget
            translation.lastFailure = wasFailure
        }
        translation.isEnabled = true
        translation.targetLanguage = "en"
        let stub = StubTranslator()
        stub.replies = [.success("Push it to jet hub")]
        var pipeline = DictationPipeline(
            style: .verbatim,
            language: "es-ES",
            replacements: [DictationCorrection(heard: "jet hub", meant: "GitHub")]
        )
        pipeline.translationEngine = stub.engine
        var stages = await pipeline.run("Súbelo a jet hub")
        XCTAssertEqual(stub.inputs, ["Súbelo a jet hub"])
        XCTAssertEqual(stages.formatted, "Push it to jet hub")

        stub.status = .supported
        stages = await pipeline.run("Súbelo a jet hub")
        XCTAssertEqual(stages.formatted, "Súbelo a GitHub", "went in as spoken")
    }

    func testReplacementsApplyOnlyWhileCustomVocabularyIsOn() {
        withDefaults { defaults in
            let settings = WordReplacementsSettings(defaults: defaults)
            settings.entries = [
                WordReplacement(heard: "  jet   hub ", write: "GitHub"),
                WordReplacement(heard: "Jet Hub", write: "Twice"),
                WordReplacement(heard: "um", write: " "),
            ]
            XCTAssertEqual(settings.entries, [WordReplacement(heard: "jet hub", write: "GitHub")])
            XCTAssertEqual(settings.activeCorrections, [])
            CustomVocabularySettings(defaults: defaults).isEnabled = true
            XCTAssertEqual(settings.activeCorrections, [DictationCorrection(heard: "jet hub", meant: "GitHub")])
            XCTAssertEqual(WordReplacementsSettings.problem(heard: "JET HUB", write: "x", existing: settings.entries), .duplicate)
            XCTAssertEqual(WordReplacementsSettings.problem(heard: "um", write: "  ", existing: []), .emptyWrite)
            XCTAssertEqual(WordReplacementsSettings.problem(heard: "a b c d e f g h i", write: "x", existing: []), .tooManyWords)
        }
    }

    func testAnImportThatTurnsCustomVocabularyOnIsUndoneWithItsSwitch() {
        withDefaults { defaults in
            let store = HexWordsImport(defaults: defaults)
            let file = HexWordsFile(
                vocabulary: [HexWordsTerm(term: "OpenCode", soundsLike: ["open code"])],
                replacements: [HexWordsReplacement(heard: "jet hub", write: "GitHub")]
            )
            store.add(file, turnOnCustomVocabulary: true)
            XCTAssertTrue(CustomVocabularySettings(defaults: defaults).isEnabled)
            XCTAssertEqual(store.lists.vocabulary, ["OpenCode"])
            XCTAssertNotNil(store.availableUndo)
            XCTAssertTrue(store.undo())
            XCTAssertFalse(CustomVocabularySettings(defaults: defaults).isEnabled)
            XCTAssertEqual(store.lists, HexWordsLists())
            XCTAssertNil(store.availableUndo)
        }
    }

    func testEditingEitherListEndsUndo() {
        withDefaults { defaults in
            let store = HexWordsImport(defaults: defaults)
            store.add(HexWordsFile(vocabulary: [HexWordsTerm(term: "Rune")]))
            XCTAssertNotNil(store.availableUndo)
            WordReplacementsSettings(defaults: defaults).entries = [WordReplacement(heard: "jet hub", write: "GitHub")]
            XCTAssertNil(store.availableUndo)
            XCTAssertFalse(store.undo())
            XCTAssertEqual(store.lists.vocabulary, ["Rune"])
        }
    }

    /// The review sheet's and confirmation's English, with its plurals.
    @MainActor
    func testReviewSheetWords() {
        XCTAssertEqual(
            HexWordsReviewSheet.notUsable(.tooShortForThisDevice, count: 1, examples: "C#"),
            "1 term has fewer than 2 letters or digits, too short for this iPhone: C#"
        )
        XCTAssertEqual(
            HexWordsReviewSheet.notUsable(.deletionNotSupported, count: 2, examples: "um, uh"),
            "2 replacements remove words instead of writing something, which this iPhone can’t do yet: um, uh"
        )
        var summary = HexWordsSummary()
        summary.addedTerms = 2
        summary.addedReplacements = 1
        summary.alreadyHaveTerms = 1
        summary.keptYours = 1
        XCTAssertEqual(HexWordsReviewSheet.confirmation(summary), "Added 2 terms and 1 replacement · 1 already there · kept yours for 1")
        XCTAssertEqual(HexWordsReviewSheet.counts(new: 2, there: 1, clashes: 1), "2 new · 1 already there · 1 different from yours")
        XCTAssertEqual(HexWordsReviewSheet.confirmation(HexWordsSummary()), "No new words in that file")
    }

    /// Keys that differ only in normalization are two keys to JSON; bridging them into one Swift
    /// dictionary would trap, so the reader never does.
    func testKeysThatDifferOnlyInNormalizationAreTwoUnknownFields() {
        let text = "{\"format\": \"hex.words\", \"version\": 1, \"\u{E9}\": 1, \"e\u{301}\": 2}"
        guard case .read(let file) = HexWordsReader.read(Data(text.utf8)) else { return XCTFail("refused") }
        XCTAssertEqual(file.skipped.count, 2)
        XCTAssertTrue(file.skipped.allSatisfy { $0.reason == .unknownField })
    }
}
