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
