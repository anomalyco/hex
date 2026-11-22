import XCTest
@testable import HEX

/// Experiments › Greeklish: Greek dictation in Latin letters, by rule rather than a table.
final class GreeklishRomanizerTests: XCTestCase {
    func testDigraphs() {
        let cases = [
            "ου": "ou", "αι": "ai", "ει": "ei", "οι": "oi", "αυ": "av", "ευ": "ev",
            "ουρανός": "ouranos", "παιδί": "paidi", "είμαι": "eimai",
            "οικογένεια": "oikogeneia", "αυτοκίνητο": "avtokinito", "ευχαριστώ": "evcharisto",
            "τζατζίκι": "tzatziki", "τσιγάρο": "tsigaro", "αγγελος": "angelos",
        ]
        for (greek, latin) in cases {
            XCTAssertEqual(GreeklishRomanizer.romanize(greek), latin, greek)
        }
    }

    func testUppercaseDigraphs() {
        XCTAssertEqual(GreeklishRomanizer.romanize("ΟΥΡΑΝΟΣ"), "OURANOS")
        XCTAssertEqual(GreeklishRomanizer.romanize("Μπάμπης"), "Bampis")
    }

    /// μπ, ντ and γκ are b, d and g at the start of a word and mp, nt and gk inside one.
    func testWordInitialNasalClusters() {
        let cases = [
            "μπαμπάς": "bampas", "μπύρα": "byra", "Μπαμπάς": "Bampas",
            "ντουλάπα": "doulapa", "πέντε": "pente", "Ντομάτα": "Domata",
            "γκαράζ": "garaz", "αγκάλη": "agkali", "Γκρεμός": "Gremos",
        ]
        for (greek, latin) in cases {
            XCTAssertEqual(GreeklishRomanizer.romanize(greek), latin, greek)
        }
    }

    /// Punctuation starts a word, so a cluster after it is word-initial.
    func testWordInitialClusterAfterPunctuation() {
        XCTAssertEqual(GreeklishRomanizer.romanize("(μπύρα)"), "(byra)")
        XCTAssertEqual(GreeklishRomanizer.romanize("— ντομάτα"), "— domata")
    }

    func testAccentsAreRemoved() {
        let cases = [
            "καλημέρα": "kalimera", "Θα αργήσω": "Tha argiso", "Άλλος": "Allos",
            "Έλα": "Ela", "Ήλιος": "Ilios", "Ίσως": "Isos", "Όλα": "Ola",
            "Ύπνος": "Ypnos", "Ώρα": "Ora", "Αθήνα": "Athina",
        ]
        for (greek, latin) in cases {
            XCTAssertEqual(GreeklishRomanizer.romanize(greek), latin, greek)
        }
    }

    /// A mark typed on its own (NFD input) belongs to the letter before it.
    func testSeparatelyTypedCombiningAccent() {
        XCTAssertEqual(GreeklishRomanizer.romanize("ε\u{0301}"), "e")
        XCTAssertEqual(GreeklishRomanizer.romanize("α\u{0301}λλος"), "allos")
    }

    /// Variant Greek letters copied from fonts and keyboards read as their base letter.
    func testVariantLettersUseTheirBaseLetters() {
        let cases = [
            "ϐ": "v", "ϑ": "th", "ϕ": "f", "ϖ": "p",
            "ϰ": "k", "ϱ": "r", "ϲ": "s", "ϵ": "e",
            "ϑεός": "theos", "ϕίλος": "filos",
        ]
        for (greek, latin) in cases {
            XCTAssertEqual(GreeklishRomanizer.romanize(greek), latin, greek)
        }
    }

    /// A diaeresis marks a new vowel, so it never joins the digraph beside it: "οϋ" is "oy",
    /// and "ουϊ" keeps its ϊ as i.
    func testDiaeresisBlocksDigraphs() {
        let cases = [
            "αϊ": "ai", "οϊ": "oi", "οϋ": "oy", "ουϊ": "oui",
            "προϋπόθεση": "proypothesi", "αϋπνία": "aypnia", "Μάϊος": "Maios",
        ]
        for (greek, latin) in cases {
            XCTAssertEqual(GreeklishRomanizer.romanize(greek), latin, greek)
        }
    }

    func testCasingFollowsTheGreekWord() {
        let cases = [
            "Θα": "Tha", "ΘΑ": "THA", "ΚΑΛΗΜΕΡΑ": "KALIMERA", "Καλημέρα": "Kalimera",
            "καλημέρα": "kalimera", "ΟΔΟΣ": "ODOS", "Σ": "S", "Θα ΠΑΩ": "Tha PAO",
            "Θ' αργήσω": "Th' argiso", "Χ": "Ch",
        ]
        for (greek, latin) in cases {
            XCTAssertEqual(GreeklishRomanizer.romanize(greek), latin, greek)
        }
    }

    func testGreekPunctuation() {
        XCTAssertEqual(GreeklishRomanizer.romanize("Τι κάνεις\u{037E}"), "Ti kaneis?")
        XCTAssertEqual(GreeklishRomanizer.romanize("Τι κάνεις;"), "Ti kaneis?", "normalized Greek question mark")
        XCTAssertEqual(GreeklishRomanizer.romanize("Ναι\u{0387} όχι"), "Nai; ochi")
        XCTAssertEqual(GreeklishRomanizer.romanize("Ναι\u{00B7} όχι"), "Nai; ochi", "normalized ano teleia")
        XCTAssertEqual(GreeklishRomanizer.romanize("Καλημέρα, κόσμε!"), "Kalimera, kosme!")
    }

    /// Only punctuation directly after Greek is the Greek it was normalized from.
    func testPunctuationAfterLatinStaysAsWritten() {
        XCTAssertEqual(GreeklishRomanizer.romanize("hello; world"), "hello; world")
        XCTAssertEqual(GreeklishRomanizer.romanize("a\u{00B7}b"), "a\u{00B7}b")
    }

    func testLeavesNonGreekUntouched() {
        let cases = [
            "Το meeting είναι στις 5": "To meeting einai stis 5",
            "café Μάτι 🎉": "café Mati 🎉",
            "Hello, world!": "Hello, world!",
            "": "",
        ]
        for (text, latin) in cases {
            XCTAssertEqual(GreeklishRomanizer.romanize(text), latin, text)
        }
    }

    /// Greeklish runs first, only for Greek and only while the experiment is on; what was heard
    /// is kept in Latin to learn from.
    @MainActor
    func testStylesApplyToGreeklish() async throws {
        var pipeline = DictationPipeline(style: .verbatim, language: "el-GR", greeklish: true)
        var stages = await pipeline.run("Θα αργήσω 10 λεπτά")
        XCTAssertEqual(stages.heard, "Tha argiso 10 lepta")
        XCTAssertEqual(stages.formatted, "Tha argiso 10 lepta")

        pipeline.greeklish = false
        stages = await pipeline.run("Θα αργήσω")
        XCTAssertEqual(stages.heard, "Θα αργήσω", "off: Greek as Apple wrote it")
        XCTAssertEqual(stages.formatted, "Θα αργήσω")

        let english = await DictationPipeline(style: .verbatim, language: "en-US", greeklish: true).run("Καλημέρα")
        XCTAssertEqual(english.formatted, "Καλημέρα", "only Greek")

        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = GreeklishSettings(defaults: defaults)
        XCTAssertFalse(settings.isEnabled, "off by default")
        XCTAssertNil(settings.name(for: "el-GR"))
        settings.applyLaunchArguments([GreeklishSettings.launchArgument])
        XCTAssertTrue(GreeklishSettings(defaults: defaults).isEnabled)
        XCTAssertEqual(settings.name(for: "el-GR"), "Greeklish")
        XCTAssertEqual(settings.name(for: "el"), "Greeklish")
        XCTAssertNil(settings.name(for: "en-US"))
    }

    /// Apple translates Greek from Greek letters, not Greeklish: a Greek dictation going into
    /// English is translated from what Apple Speech wrote, and one that goes in as spoken is
    /// still Greeklish.
    @MainActor
    func testGreeklishTranslatesFromGreekScript() async {
        let translation = DictationTranslationSettings()
        let wasEnabled = translation.isEnabled, wasTarget = translation.targetLanguage, wasFailure = translation.lastFailure
        defer {
            translation.isEnabled = wasEnabled
            translation.targetLanguage = wasTarget
            translation.lastFailure = wasFailure
        }
        translation.isEnabled = true
        translation.targetLanguage = "en"
        let heard = "Θα αργήσω"
        let stub = StubTranslator()
        stub.replies = [.success("I'll be late")]
        var pipeline = DictationPipeline(style: .verbatim, language: "el-GR", greeklish: true)
        pipeline.translationEngine = stub.engine
        var stages = await pipeline.run(heard)
        XCTAssertEqual(stub.inputs, [heard])
        XCTAssertEqual(stages.formatted, "I'll be late")
        XCTAssertEqual(stages.heard, "Tha argiso", "learning still compares Latin words")

        stub.status = .supported
        stages = await pipeline.run(heard)
        XCTAssertEqual(stages.formatted, "Tha argiso", "as spoken, in Greeklish")
        XCTAssertEqual(stages.translationFailure?.fix, "Download Greek in Settings")
    }
}
