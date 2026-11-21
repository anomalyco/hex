import XCTest
@testable import HEX

final class TranscriptCleanupGuardTests: XCTestCase {
    func testAcceptsMisheardWordsAndSpokenSelfCorrections() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "we should use cloud code on tuesday, no wait, wednesday",
                cleaned: "We should use Claude Code on Wednesday."
            )
        )
    }

    func testAcceptsPunctuationAndCaseChanges() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "meet at ten on thursday and bring the notes",
                cleaned: "Meet at ten on Thursday, and bring the notes."
            )
        )
    }

    func testRejectsDigitChanges() {
        XCTAssertFalse(
            TranscriptCleanupGuard.isAcceptable(
                original: "call me at 555 0100",
                cleaned: "call me at 555 0101"
            )
        )
        XCTAssertFalse(
            TranscriptCleanupGuard.isAcceptable(
                original: "see you at ten",
                cleaned: "see you at 10"
            )
        )
    }

    func testRejectsLostNegation() {
        XCTAssertFalse(
            TranscriptCleanupGuard.isAcceptable(
                original: "do not delete the file",
                cleaned: "delete the file"
            )
        )
        XCTAssertFalse(
            TranscriptCleanupGuard.isAcceptable(
                original: "I don't think that is right",
                cleaned: "I think that is right"
            )
        )
    }

    func testRejectsAddedNegation() {
        XCTAssertFalse(
            TranscriptCleanupGuard.isAcceptable(
                original: "I will go tomorrow",
                cleaned: "I will not go tomorrow"
            )
        )
    }

    func testRejectsRunawayLength() {
        XCTAssertFalse(
            TranscriptCleanupGuard.isAcceptable(
                original: "ship it",
                cleaned: String(repeating: "ship it ", count: 30)
            )
        )
    }

    func testRejectsEmptyOutputAndEmptyInput() {
        XCTAssertFalse(TranscriptCleanupGuard.isAcceptable(original: "", cleaned: ""))
        XCTAssertFalse(TranscriptCleanupGuard.isAcceptable(original: "ship it", cleaned: "   "))
    }

    func testRejectsAnAnswerWithLittleOverlap() {
        XCTAssertFalse(
            TranscriptCleanupGuard.isAcceptable(
                original: "what time is the meeting",
                cleaned: "The weather tomorrow will be sunny and warm."
            )
        )
    }

    func testCleanupSettingIsOffByDefaultAndPersists() {
        let suite = "AppleIntelligenceCleanupSettingsTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        XCTAssertFalse(AppleIntelligenceCleanupSettings(defaults: defaults).isEnabled)

        AppleIntelligenceCleanupSettings(defaults: defaults).isEnabled = true
        XCTAssertTrue(AppleIntelligenceCleanupSettings(defaults: defaults).isEnabled)
    }
}

/// Clean up what I say in English: `CleanupGuard` after the tagger keeps names, hyphenated
/// words, numbers and content adverbs, and still lets fillers, repeats and self-corrections go.
final class CleanupGuardTests: XCTestCase {
    private func cleaned(_ text: String, _ probabilities: [Double], terms: [String] = []) -> String {
        let words = CleanupTagger.words(in: text)
        let deletes = CleanupGuard.deletes(
            words, probabilities: probabilities, protectedTerms: CleanupGuard.protectedTerms(terms)
        )
        return CleanupReconstruction.reconstruct(words, deletes: deletes)
    }

    func testUnsetSwitchIsOnAndAStoredChoiceStays() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        XCTAssertTrue(CleanupTaggerSettings(defaults: defaults).isEnabled, "an unset switch is on")
        defaults.set(false, forKey: "experiments.cleanupTagger")
        XCTAssertFalse(CleanupTaggerSettings(defaults: defaults).isEnabled, "turning it off is kept")
        CleanupTaggerSettings(defaults: defaults).isEnabled = true
        XCTAssertTrue(CleanupTaggerSettings(defaults: defaults).isEnabled)
    }

    func testTheModelSeesHyphenatedWordsWhole() {
        XCTAssertEqual(CleanupGuard.modelWord("Hye-jin"), "Hyejin")
        XCTAssertEqual(CleanupGuard.modelWord("follow-up,"), "followup,")
        XCTAssertEqual(CleanupGuard.modelWord("w-"), "w-", "a stutter fragment stays one")
        XCTAssertEqual(CleanupGuard.modelWord("415-555-0198"), "415-555-0198")
        XCTAssertEqual(CleanupGuard.modelWord("-"), "-")
    }

    /// The tagger was sure (P 0.996 to 1.0) about each of these words; none is a disfluency.
    func testProtectedWordsStayWhenTheTaggerIsSure() {
        XCTAssertEqual(cleaned("Tell Hye-jin I said thanks.", [0, 1, 0.999, 0.999, 0]), "Tell Hye-jin I said thanks.")
        XCTAssertEqual(cleaned("Ask Jean-Luc and Anne-Marie.", [0, 1, 0, 1]), "Ask Jean-Luc and Anne-Marie.")
        XCTAssertEqual(cleaned("Send the follow-up today.", [0, 0, 1, 0]), "Send the follow-up today.")
        XCTAssertEqual(cleaned("Call me later, maybe after dinner.", [0, 0, 0, 1, 0, 0]), "Call me later, maybe after dinner.")
        XCTAssertEqual(cleaned("It's really good.", [0, 1, 0]), "It's really good.")
        XCTAssertEqual(cleaned("Meet at gate 12 now.", [0, 0, 0, 1, 0]), "Meet at gate 12 now.")
        XCTAssertEqual(cleaned("I told Priya already.", [0, 0, 1, 0]), "I told Priya already.")
        XCTAssertEqual(cleaned("Ship the opencode build.", [0, 0, 1, 0], terms: ["OpenCode"]), "Ship the opencode build.")
        XCTAssertEqual(cleaned("It was really, really long.", [0, 0, 1, 0, 0]), "It was really, really long.")
        XCTAssertEqual(cleaned("The cat keeps trying to scratch it.", [0, 0, 0, 0.99, 0.99, 0, 0]), "The cat keeps trying to scratch it.")
        XCTAssertEqual(cleaned("I mean what I say.", [0, 1, 0, 0, 0]), "I mean what I say.")
    }

    func testFillersRepeatsAndSelfCorrectionsStillGo() {
        XCTAssertEqual(cleaned("Um, tell Hye-jin I said thanks.", [1, 0, 0, 0, 0, 0]), "Tell Hye-jin I said thanks.")
        XCTAssertEqual(cleaned("It was, like, really long.", [0, 0, 1, 0, 0]), "It was really long.")
        XCTAssertEqual(cleaned("It's, you know, been rough.", [0, 1, 1, 0, 0]), "It's been rough.")
        XCTAssertEqual(cleaned("I I think so.", [1, 0, 0, 0]), "I think so.")
        XCTAssertEqual(cleaned("Ask Ji-woo Ji-woo now.", [0, 1, 0, 0]), "Ask Ji-woo now.", "a repeated name loses only its copy")
        XCTAssertEqual(cleaned("We w- we left.", [0, 1, 0, 0]), "We we left.")
        XCTAssertEqual(cleaned("Meet me at 5, no, at 6.", [0, 0, 1, 1, 1, 0, 0]), "Meet me at 6.")
        XCTAssertEqual(cleaned("Send it to John, I mean Mike.", [0, 0, 0, 1, 1, 1, 0]), "Send it to Mike.")
        XCTAssertEqual(
            cleaned("Send it to Ji-woo, no wait, to Smith-Jones.", [0, 0, 1, 1, 1, 1, 0, 0]),
            "Send it to Smith-Jones.", "a hyphenated name goes in a self-correction"
        )
        XCTAssertEqual(cleaned("It costs 15 euros, er, 50 euros.", [0, 0, 1, 1, 1, 0, 0]), "It costs 50 euros.")
        XCTAssertEqual(cleaned("Did you, have you seen it?", [0.99, 0.99, 0, 0, 0, 0]), "Have you seen it?")
    }

    /// A self-correction or false start the tagger is unsure of stays as said.
    func testUnsureRepairsStay() {
        XCTAssertEqual(cleaned("Meet me at 5, no, 6.", [0, 0, 0, 0.99, 0.99, 0]), "Meet me at 5, no, 6.")
        XCTAssertEqual(cleaned("Miss you, see you soon.", [0.99, 0.99, 0, 0, 0]), "Miss you, see you soon.")
    }

    /// The shipped model with the guard: hyphenated names it used to delete at P 0.996 to 1.0
    /// stay, and disfluencies around them still go.
    func testTaggerKeepsHyphenatedNamesAndStillCleans() throws {
        let tagger = CleanupTagger()
        for (spoken, expected) in [
            ("Tell Hye-jin I said thanks.", "Tell Hye-jin I said thanks."),
            ("Can you ask Jean-Luc to send the deck?", "Can you ask Jean-Luc to send the deck?"),
            ("Anne-Marie and Ji-woo are coming on Friday.", "Anne-Marie and Ji-woo are coming on Friday."),
            ("I met Mr. Smith-Jones and Catherine Zeta-Jones.", "I met Mr. Smith-Jones and Catherine Zeta-Jones."),
            ("I'll ask Bo-ra about the launch.", "I'll ask Bo-ra about the launch."),
            ("Call me later, maybe after dinner.", "Call me later, maybe after dinner."),
            ("The cat keeps trying to scratch that chair.", "The cat keeps trying to scratch that chair."),
            ("Um, tell Hye-jin I said thanks.", "Tell Hye-jin I said thanks."),
            ("Send the follow-up to Ji-woo, no wait, to Smith-Jones.", "Send the follow-up to Smith-Jones."),
            ("Let's ship it Tuesday, no wait, Thursday.", "Let's ship it Thursday."),
            ("Can you send it to John, I mean Mike?", "Can you send it to Mike?"),
            ("Meet me at 5, no, at 6.", "Meet me at 6."),
            ("We need 20, sorry, 25 chairs.", "We need 25 chairs."),
            ("I I think we're good to go.", "I think we're good to go."),
            ("Can you, um, send me the address again?", "Can you send me the address again?"),
            ("It was, like, really long.", "It was really long."),
        ] {
            XCTAssertEqual(try tagger.clean(spoken), expected, spoken)
        }
    }
}

/// Clean up what I say's word rules for Spanish, German and Norwegian: what goes, and what
/// must stay because the words mean something there.
final class SpokenCleanupRulesTests: XCTestCase {
    func testSpanishFillersAndCorrections() {
        for (spoken, cleaned) in [
            ("Nos vemos mañana a las 3, digo, a las 4.", "Nos vemos mañana a las 4."),
            ("Quedamos el martes, perdón, el miércoles en la oficina.", "Quedamos el miércoles en la oficina."),
            ("Compra dos kilos, mejor dicho, tres kilos de tomates.", "Compra tres kilos de tomates."),
            ("Te llamo hoy, no, mañana por la tarde.", "Te llamo mañana por la tarde."),
            ("Llegué a las 8, perdón, a las 9 de la mañana.", "Llegué a las 9 de la mañana."),
            ("Llego a las 3. Digo, a las 4.", "Llego a las 4."),
            ("Ven el lunes, no, mejor el martes.", "Ven el martes."),
            ("A las 3, no, espera, a las 5.", "A las 5."),
            ("¿El martes, perdón, el miércoles?", "¿El miércoles?"),
            ("Este, quería preguntarte si vienes a la cena.", "Quería preguntarte si vienes a la cena."),
            ("Es muy caro, o sea, no lo voy a comprar.", "Es muy caro, no lo voy a comprar."),
            ("Pues, este, no sé qué decirte.", "Pues, no sé qué decirte."),
            ("Y, este, fuimos al cine.", "Y fuimos al cine."),
            ("Mmm, creo que sí, eh, pero no estoy seguro.", "Creo que sí, pero no estoy seguro."),
            ("Eh, nos vemos a las 3, digo, a las 4.", "Nos vemos a las 4."),
        ] {
            XCTAssertEqual(SpokenCleanupRules.clean(spoken, language: "es-ES"), cleaned)
        }
    }

    func testSpanishWordsInTheirOwnMeaningStay() {
        for spoken in [
            "Este libro me gustó mucho.",
            "Quiero este, el azul.",
            "Te digo que no puedo ir.",
            "Vamos a la playa, digo yo, si hace buen tiempo.",
            "No, gracias, ya comí.",
            "Perdón por llegar tarde, había mucho tráfico.",
            "Lo siento, perdón, Ana.",
            "O sea que al final no vienes.",
            "Le dije a Juan que no, que mejor otro día.",
            "Un coche no mejor que el otro.",
            "¿Vienes el sábado o el domingo?",
            "No te olvides, ¿eh?",
            "Padre e hijo.",
            "Uno\ndos",
        ] {
            XCTAssertEqual(SpokenCleanupRules.clean(spoken, language: "es-419"), spoken)
        }
    }

    func testGermanAndNorwegianCorrections() {
        XCTAssertEqual(SpokenCleanupRules.clean("Wir treffen uns um 3, nein, um 4.", language: "de-DE"), "Wir treffen uns um 4.")
        XCTAssertEqual(
            SpokenCleanupRules.clean("Ich komme am Montag, ich meine, am Dienstag.", language: "de-AT"),
            "Ich komme am Dienstag."
        )
        XCTAssertEqual(SpokenCleanupRules.clean("Nein, ich meine das ernst.", language: "de-DE"), "Nein, ich meine das ernst.")
        XCTAssertEqual(SpokenCleanupRules.clean("Vi sender den tirsdag, nei vent, torsdag.", language: "nb-NO"), "Vi sender den torsdag.")
        XCTAssertEqual(SpokenCleanupRules.clean("Møtet er klokka 3, nei, klokka 4.", language: "nn"), "Møtet er klokka 4.")
        XCTAssertEqual(SpokenCleanupRules.clean("Nei, jeg vil ikke.", language: "nb-NO"), "Nei, jeg vil ikke.")
        XCTAssertEqual(SpokenCleanupRules.clean("Eh, jeg kommer snart.", language: "nb-NO"), "Jeg kommer snart.")
        XCTAssertEqual(SpokenCleanupRules.clean("Äh, das ist eh egal.", language: "de-DE"), "Das ist eh egal.")
    }

    func testOnlyItsLanguagesHaveRules() {
        XCTAssertTrue(SpokenCleanupRules.supports("es-US"))
        XCTAssertTrue(SpokenCleanupRules.supports("no"))
        XCTAssertFalse(SpokenCleanupRules.supports("en-US"))
        XCTAssertFalse(SpokenCleanupRules.supports("pt-BR"))
        XCTAssertEqual(SpokenCleanupRules.clean("A las 3, digo, a las 4.", language: "pt-BR"), "A las 3, digo, a las 4.")
    }

    /// "at the rate" is how India says "@": an address joins into one, "dot" becoming "."
    /// only inside it. "at sign" is "@" anywhere.
    func testSpokenAtSignsBecomeAddresses() {
        for (spoken, written) in [
            ("john at the rate gmail dot com", "john@gmail.com"),
            ("Mail it to John at the rate Gmail dot com.", "Mail it to john@gmail.com."),
            ("My email is priya dot sharma at the rate company dot co dot in, call me too",
             "My email is priya.sharma@company.co.in, call me too"),
            ("Write to support at the rate hex.app today", "Write to support@hex.app today"),
            ("rahul123 at sign yahoo dot com", "rahul123@yahoo.com"),
            ("At the rate gmail dot com addresses work", "@gmail.com addresses work"),
            ("Type the at sign, then your name", "Type the @, then your name"),
            ("Press at sign.", "Press @."),
            ("ravi at the rate, gmail dot com", "ravi@gmail.com"),
            // What Apple's on-device models wrote for an Indian English voice: SpeechTranscriber
            // glues the domain to "rate" or "sign"; DictationTranscriber keeps them apart.
            ("My email is John at the rategmail.com.", "My email is john@gmail.com."),
            ("Send it to Priya at signyahoo.com.", "Send it to priya@yahoo.com."),
            ("My email is John at the rate gmail.com", "My email is john@gmail.com"),
        ] {
            XCTAssertEqual(SpokenCleanupRules.spokenSymbols(in: spoken, language: "en-IN"), written, spoken)
        }
    }

    func testAtInItsOwnMeaningStays() {
        for spoken in [
            "Prices are growing at the rate of 5% a year.",
            "At the rate of knots, we'll be there soon.",
            "We are spending at the rate we planned.",
            "At the rate you're going, you'll finish early.",
            "Meet me at the office at 5.",
            "Look at that dot on the map.",
            "I'm good at sign language.",
            "john at the rate gmail",
            "Send it at the rate",
            "Wait at the gate dot com",
            // Apple's own output that must stay: "@" already there, "at" alone, "at the rate of".
            "Send it to priya@yahoo.com",
            "My email is john at gmail.com.",
            "Prices Are growing at the rate of5% a year",
            "Rates went up at the ratepayers' meeting.",
        ] {
            XCTAssertEqual(SpokenCleanupRules.spokenSymbols(in: spoken, language: "en-IN"), spoken)
        }
        // English only: other languages say it their own way, and keep their words.
        XCTAssertEqual(
            SpokenCleanupRules.spokenSymbols(in: "john at the rate gmail dot com", language: "es-ES"),
            "john at the rate gmail dot com"
        )
    }

    /// Every style but Verbatim keeps the address whole: no capital after its dots, none on
    /// it at a sentence start, and no full stop after an address said alone.
    func testStylesKeepAnAddressWhole() {
        XCTAssertEqual(TranscriptFormatter.format("john@gmail.com", style: .polished), "john@gmail.com")
        XCTAssertEqual(
            TranscriptFormatter.format("send it to john@gmail.com", style: .polished),
            "Send it to john@gmail.com."
        )
        XCTAssertEqual(
            TranscriptFormatter.format("john@gmail.com is mine. call me", style: .polished),
            "john@gmail.com is mine. Call me."
        )
        XCTAssertEqual(TranscriptFormatter.format("Mail john@gmail.com", style: .casual), "mail john@gmail.com")
        XCTAssertEqual(
            TranscriptFormatter.format("bullet john@gmail.com bullet call Ravi", style: .notes),
            "- john@gmail.com\n- Call Ravi"
        )
        XCTAssertEqual(TranscriptFormatter.format("Hello. world! ok? yes", style: .polished), "Hello. World! Ok? Yes.")
    }

    /// The styles trim each language's own hesitation sounds, never a word ("e" is "and" in
    /// Spanish, "eh" is "anyway" in German).
    func testStylesTrimSpanishAndGermanHesitations() {
        XCTAssertEqual(
            TranscriptFormatter.format("Mmm, creo que sí, eh, pero no estoy seguro.", style: .polished, language: "es-ES"),
            "Creo que sí, pero no estoy seguro."
        )
        XCTAssertEqual(
            TranscriptFormatter.format("Eh, ¿qué tal? Padre e hijo.", style: .polished, language: "es-ES"),
            "¿Qué tal? Padre e hijo."
        )
        XCTAssertEqual(
            TranscriptFormatter.format("Äh, das ist eh egal.", style: .polished, language: "de-DE"),
            "Das ist eh egal."
        )
        XCTAssertEqual(
            TranscriptFormatter.format("Eh, nos vemos.", style: .verbatim, language: "es-ES"),
            "Eh, nos vemos."
        )
    }

    /// Ordinary short dictations as Apple Speech writes them read differently in each style.
    func testStylesDifferOnOrdinaryShortDictation() {
        func styled(_ text: String) -> [String] {
            DictationStyle.allCases.map { TranscriptFormatter.format(text, style: $0) }
        }
        // Polished, Casual, Notes, Verbatim.
        XCTAssertEqual(styled("Sounds good, see you at 5."), [
            "Sounds good, see you at 5.", "sounds good, see you at 5", "Sounds good, see you at 5",
            "Sounds good, see you at 5.",
        ])
        XCTAssertEqual(styled("Um, buy milk. Call the dentist. Book the flights."), [
            "Buy milk. Call the dentist. Book the flights.", "buy milk. call the dentist. book the flights",
            "- Buy milk\n- Call the dentist\n- Book the flights",
            "Um, buy milk. Call the dentist. Book the flights.",
        ])
        // An already punctuated sentence reads the same in Polished and Verbatim.
        XCTAssertEqual(styled("I'll be there in 10."), [
            "I'll be there in 10.", "i'll be there in 10", "I'll be there in 10", "I'll be there in 10.",
        ])
        XCTAssertEqual(styled("i'm running late start without me"), [
            "I'm running late start without me.", "i'm running late start without me",
            "I'm running late start without me", "i'm running late start without me",
        ])
    }

    /// Polished writes full sentences: "I", a capital and an ending for each paragraph, in
    /// Chinese with "。" (never a second mark after one).
    func testPolishedWritesFullSentences() {
        XCTAssertEqual(
            TranscriptFormatter.format("erm i think i'll go, it's fine", style: .polished),
            "I think I'll go, it's fine."
        )
        XCTAssertEqual(TranscriptFormatter.format("first line\nsecond line", style: .polished), "First line.\nSecond line.")
        XCTAssertEqual(TranscriptFormatter.format("Shopping list:", style: .polished), "Shopping list:")
        XCTAssertEqual(TranscriptFormatter.format("好的，明天见。", style: .polished, language: "zh-CN"), "好的，明天见。")
        XCTAssertEqual(TranscriptFormatter.format("好的，明天见", style: .polished, language: "zh-CN"), "好的，明天见。")
        // "i" is English only.
        XCTAssertEqual(TranscriptFormatter.format("i ragazzi", style: .polished, language: "it-IT"), "I ragazzi.")
        XCTAssertEqual(TranscriptFormatter.format("dimmi i nomi", style: .polished, language: "it-IT"), "Dimmi i nomi.")
    }

    /// Casual texts: sentences may start lowercase, "I" is "i", an opening interjection loses
    /// its comma, and a short message its final full stop. Acronyms, names inside a sentence
    /// and words with inner capitals keep their case.
    func testCasualTexts() {
        XCTAssertEqual(
            TranscriptFormatter.format("Yeah, I'll text John about the iPhone order tomorrow.", style: .casual),
            "yeah i'll text John about the iPhone order tomorrow"
        )
        XCTAssertEqual(TranscriptFormatter.format("I think I can, i.e. maybe.", style: .casual), "i think i can, i.e. maybe")
        XCTAssertEqual(
            TranscriptFormatter.format("OK, can you send me the deck?", style: .casual),
            "ok can you send me the deck?"
        )
        XCTAssertEqual(TranscriptFormatter.format("NASA launched it. Wow!", style: .casual), "NASA launched it. wow!")
        XCTAssertEqual(TranscriptFormatter.format("iPhone is charged.", style: .casual), "iPhone is charged")
        // Longer than a short message: four sentences, or more than one line, keep the full stop.
        XCTAssertEqual(TranscriptFormatter.format("One. Two. Three. Four.", style: .casual), "one. two. three. four.")
        XCTAssertEqual(TranscriptFormatter.format("Hi.\nSee you.", style: .casual), "hi.\nsee you.")
        // Interjections are English; Chinese drops its final "。".
        XCTAssertEqual(TranscriptFormatter.format("Sí, claro.", style: .casual, language: "es-ES"), "sí, claro")
        XCTAssertEqual(TranscriptFormatter.format("好的，明天见。", style: .casual, language: "zh-CN"), "好的，明天见")
    }

    /// Notes writes concise lines, and a list when there are several items.
    func testNotesListSeveralItemsConcisely() {
        XCTAssertEqual(TranscriptFormatter.format("Call the dentist tomorrow at 9.", style: .notes), "Call the dentist tomorrow at 9")
        XCTAssertEqual(TranscriptFormatter.format("Can you send the deck?", style: .notes), "Can you send the deck?")
        XCTAssertEqual(TranscriptFormatter.format("Yeah, I'll call Ravi.", style: .notes), "I'll call Ravi")
        XCTAssertEqual(
            TranscriptFormatter.format("So, first, buy milk. And then call the dentist.", style: .notes),
            "- Buy milk\n- Call the dentist"
        )
        XCTAssertEqual(
            TranscriptFormatter.format("Shopping list: eggs, milk, and bread.", style: .notes),
            "Shopping list:\n- Eggs\n- Milk\n- Bread"
        )
        // No list from an abbreviation's full stop or a colon before a sentence.
        XCTAssertEqual(
            TranscriptFormatter.format("Dr. Patel called. Call her back.", style: .notes),
            "- Dr. Patel called\n- Call her back"
        )
        XCTAssertEqual(
            TranscriptFormatter.format("Note: the build is green.", style: .notes),
            "Note: the build is green"
        )
        // Spoken markers decide the lines when there are any; Chinese splits at "。".
        XCTAssertEqual(
            TranscriptFormatter.format("todo bullet ship it. Then tag it bullet tell Ravi", style: .notes),
            "Todo\n- Ship it. Then tag it\n- Tell Ravi"
        )
        XCTAssertEqual(
            TranscriptFormatter.format("买牛奶。给妈妈打电话。", style: .notes, language: "zh-CN"),
            "- 买牛奶\n- 给妈妈打电话"
        )
    }

    /// Ukrainian as `DictationTranscriber` writes it (macOS 27.2, Lesya voice: Cyrillic, no
    /// punctuation): Polished capitalizes and ends it, Casual starts each sentence lowercase,
    /// Lowercase lowers Ґ, Є and Ї, and English's "i" rules never touch Cyrillic "і" ("and").
    func testUkrainianStylesAndLowercase() {
        let heard = "привіт як справи їжак Єнот завтра о дев'ятій ми підемо на каву"
        XCTAssertEqual(
            TranscriptFormatter.format(heard, style: .polished, language: "uk-UA"),
            "Привіт як справи їжак Єнот завтра о дев'ятій ми підемо на каву."
        )
        XCTAssertEqual(TranscriptFormatter.format("ґанок і сад. і все", style: .polished, language: "uk-UA"), "Ґанок і сад. І все.")
        XCTAssertEqual(TranscriptFormatter.format("Ґанок і сад. І все.", style: .casual, language: "uk-UA"), "ґанок і сад. і все")
        XCTAssertEqual(TranscriptFormatter.format("Ґанок і сад.", style: .verbatim, language: "uk-UA"), "Ґанок і сад.")
        var pipeline = DictationPipeline(style: .polished, language: "uk-UA", vocabulary: [])
        pipeline.lowercase = true
        XCTAssertEqual(pipeline.format("Єнот, Ґанок і Їжак", language: "uk-UA"), "єнот, ґанок і їжак.")
    }

    /// Lowercase comes after the style and the vocabulary's spellings, as desktop's Lowercase
    /// step lowercases the final text, custom vocabulary included.
    func testLowercaseRunsAfterTheStyleAndVocabulary() {
        let heard = "Yeah, text John about the iphone order."
        var pipeline = DictationPipeline(style: .polished, language: "en-US", vocabulary: ["iPhone"])
        XCTAssertEqual(pipeline.format(heard, language: "en-US"), "Yeah, text John about the iPhone order.")
        pipeline.lowercase = true
        XCTAssertEqual(pipeline.format(heard, language: "en-US"), "yeah, text john about the iphone order.")
        pipeline.style = .casual
        XCTAssertEqual(pipeline.format(heard, language: "en-US"), "yeah text john about the iphone order")
        pipeline.style = .verbatim
        XCTAssertEqual(pipeline.format(heard, language: "en-US"), "yeah, text john about the iphone order.")

        let defaults = UserDefaults(suiteName: "hex.tests.\(UUID().uuidString)")!
        let settings = DictationLowercaseSettings(defaults: defaults)
        XCTAssertFalse(settings.isEnabled, "off by default")
        settings.isEnabled = true
        XCTAssertTrue(DictationLowercaseSettings(defaults: defaults).isEnabled)
    }
}

/// Experiments › Hinglish: Hindi dictation in Latin letters, as people text it. The Devanagari
/// inputs are what `DictationTranscriber` wrote for spoken Hinglish (macOS 27.2, Lekha voice);
/// the Latin ones are `SpeechTranscriber`'s.
final class HinglishRomanizerTests: XCTestCase {
    func testDevanagariBecomesCasualLatin() {
        let cases = [
            "क्या कर रहे हो": "kya kar rahe ho",
            "मेरा फ़ोन चार्ज नहीं हो रहा": "mera phone charge nahi ho raha",
            "बच्चे स्कूल गए हैं": "bacche school gaye hain",
            "धन्यवाद बहुत मदद हुई": "dhanyawaad bahut madad hui",
            "कमल और प्रिया कल दिल्ली जा रहे हैं": "kamal aur priya kal Delhi ja rahe hain",
            "ठीक है। में आ रहा हूँ": "theek hai. main aa raha hoon",
            "घर में कौन है।": "ghar mein kaun hai.",
        ]
        for (devanagari, latin) in cases {
            XCTAssertEqual(HinglishRomanizer.romanize(devanagari), latin, devanagari)
        }
    }

    /// The inherent "a" goes where Hindi drops it, and long vowels are doubled only where
    /// people stress them.
    func testSpellsWordsAsTheyAreSaid() {
        let cases = [
            "कमल": "kamal", "करना": "karna", "समझना": "samajhna", "ज़िंदगी": "zindagi", "मित्र": "mitra",
            "दोस्त": "dost", "नमस्ते": "namaste", "लड़की": "ladki", "अच्छा": "accha", "भूख": "bhookh",
            "आज": "aaj", "बात": "baat", "कहाँ": "kahan", "मैंने": "maine", "चाहिए": "chahiye",
            "जाऊँगा": "jaunga", "आएगा": "aayega", "जवाब": "jawaab", "कुछ": "kuch",
            // Common words from the table: है hai, नहीं nahi, मैं main, हूँ hoon.
            "है": "hai", "नहीं": "nahi", "मैं": "main", "हूँ": "hoon", "हूं": "hoon", "यह": "yeh",
        ]
        for (devanagari, latin) in cases {
            XCTAssertEqual(HinglishRomanizer.romanize(devanagari), latin, devanagari)
        }
    }

    func testEnglishWordsComeBackInEnglish() {
        XCTAssertEqual(HinglishRomanizer.romanize("मीटिंग ऑफिस कौल"), "meeting office call")
        XCTAssertEqual(HinglishRomanizer.romanize("मुझे 10 मिनिट्स दो"), "mujhe 10 minutes do")
    }

    func testNumbersAreDigits() {
        XCTAssertEqual(HinglishRomanizer.romanize("मीटिंग कल ५ बजे है"), "meeting kal 5 baje hai")
        XCTAssertEqual(HinglishRomanizer.romanize("meeting kal panch baje hai"), "meeting kal 5 baje hai")
        XCTAssertEqual(HinglishRomanizer.romanize("hum saadhe aath baje niklenge"), "hum 8:30 baje niklenge")
        XCTAssertEqual(HinglishRomanizer.romanize("dedh baje"), "1:30 baje")
        XCTAssertEqual(HinglishRomanizer.romanize("pizza order kar do"), "pizza order kar do", "do is a verb here")
    }

    /// SpeechTranscriber's Latin Hindi keeps its words; only its unusual spellings and the
    /// capitals it puts on common words mid-sentence change. Names and English stay.
    func testAppleLatinKeepsItsWords() {
        XCTAssertEqual(HinglishRomanizer.romanize("mei Ghar ja raha hoo"), "main ghar ja raha hoon")
        XCTAssertEqual(HinglishRomanizer.romanize("Mei thik hoon."), "Main thik hoon.")
        XCTAssertEqual(HinglishRomanizer.romanize("uska naam Rahul hai"), "uska naam Rahul hai")
        XCTAssertEqual(HinglishRomanizer.romanize("Let's meet at 5, Home is fine."), "Let's meet at 5, Home is fine.")
    }

    /// Hinglish runs first, so the style and Lowercase apply to the Latin words, only for Hindi
    /// and only while the experiment is on; what was heard is kept in Latin to learn from.
    @MainActor
    func testStylesAndLowercaseApplyToHinglish() async {
        let heard = "मेरा फ़ोन चार्ज नहीं हो रहा"
        var pipeline = DictationPipeline(style: .polished, language: "hi-IN", hinglish: true)
        var stages = await pipeline.run(heard)
        XCTAssertEqual(stages.heard, "mera phone charge nahi ho raha")
        XCTAssertEqual(stages.formatted, "Mera phone charge nahi ho raha.")
        pipeline.style = .casual
        stages = await pipeline.run(heard)
        XCTAssertEqual(stages.formatted, "mera phone charge nahi ho raha")
        pipeline.style = .polished
        pipeline.lowercase = true
        stages = await pipeline.run(heard)
        XCTAssertEqual(stages.formatted, "mera phone charge nahi ho raha.")

        pipeline.hinglish = false
        stages = await pipeline.run(heard)
        XCTAssertEqual(stages.heard, heard, "off: Devanagari as Apple wrote it")
        let english = await DictationPipeline(style: .verbatim, language: "en-US", hinglish: true).run("Mei Ghar")
        XCTAssertEqual(english.formatted, "Mei Ghar", "only Hindi")

        let defaults = UserDefaults(suiteName: "hex.tests.\(UUID().uuidString)")!
        let settings = HinglishSettings(defaults: defaults)
        XCTAssertFalse(settings.isEnabled, "off by default")
        XCTAssertNil(settings.name(for: "hi-IN"))
        settings.applyLaunchArguments([HinglishSettings.launchArgument])
        XCTAssertTrue(HinglishSettings(defaults: defaults).isEnabled)
        XCTAssertEqual(settings.name(for: "hi-IN"), "Hinglish")
        XCTAssertNil(settings.name(for: "en-IN"))
    }
}


/// Settings › Dictation › Format lists. The English and Spanish inputs are what Apple Speech
/// (SpeechTranscriber, macOS 27.2, Samantha and Paulina voices) wrote for spoken lists.
final class SpokenListsTests: XCTestCase {
    private func lists(_ text: String, _ language: String = "en-US") -> String {
        SpokenLists.format(text, language: language)
    }

    func testNumberedListsInEnglish() {
        XCTAssertEqual(
            lists("Here is my shopping list. First, eggs. Second, milk, third, bread."),
            "Here is my shopping list.\n1. Eggs\n2. Milk\n3. Bread"
        )
        XCTAssertEqual(
            lists("My shopping list, 1st egg, 2nd milk, and 3rd bread."),
            "My shopping list:\n1. Egg\n2. Milk\n3. Bread"
        )
        XCTAssertEqual(
            lists("Number one, call the bank. Number two, pay rent. Number three, email Sam."),
            "1. Call the bank\n2. Pay rent\n3. Email Sam"
        )
        XCTAssertEqual(
            lists("One, buy tickets. Two, book the hotel. Three, pack."),
            "1. Buy tickets\n2. Book the hotel\n3. Pack"
        )
        XCTAssertEqual(
            lists("Step one preheat the oven, step two, mix the flour, step three, bake for 20 minutes."),
            "1. Preheat the oven\n2. Mix the flour\n3. Bake for 20 minutes"
        )
        XCTAssertEqual(lists("1. Eggs. 2. Milk."), "1. Eggs\n2. Milk")
        XCTAssertEqual(lists("First of all, thanks. Second of all, sorry."), "1. Thanks\n2. Sorry")
    }

    /// "finally" ends a list of two or more; what follows the last item's sentence is its own
    /// line, and a later list is found too.
    func testListEndsAndWhatFollows() {
        XCTAssertEqual(
            lists("First, preheat the oven. Second, mix. And finally, bake. Thanks!"),
            "1. Preheat the oven\n2. Mix\n3. Bake\nThanks!"
        )
        XCTAssertEqual(
            lists("Third, bake for 20 minutes. Let it cool. First, preheat. Second, mix."),
            "Third, bake for 20 minutes. Let it cool.\n1. Preheat\n2. Mix"
        )
        XCTAssertEqual(
            lists("Who's coming? First, Ana. Second, who else?"),
            "Who's coming?\n1. Ana\n2. Who else?"
        )
        XCTAssertEqual(
            lists("Packing: first, the iPhone charger, second, passport."),
            "Packing:\n1. The iPhone charger\n2. Passport"
        )
    }

    func testBulletsAndLineBreaks() {
        XCTAssertEqual(lists("Groceries, bullet, eggs, bullet, milk, bullet bread."), "Groceries:\n- Eggs\n- Milk\n- Bread")
        XCTAssertEqual(lists("Todo bullet ship it bullet tell Ravi"), "Todo:\n- Ship it\n- Tell Ravi")
        XCTAssertEqual(lists("Agenda: next item, budget. Next item, hiring."), "Agenda:\n- Budget\n- Hiring")
        XCTAssertEqual(lists("Dash, eggs. Dash, milk."), "- Eggs\n- Milk")
        XCTAssertEqual(
            lists("Dear Sam, new line, thanks for the help, new paragraph see you soon."),
            "Dear Sam\nthanks for the help\n\nsee you soon."
        )
        XCTAssertEqual(lists("Shopping new line bullet eggs new line bullet milk"), "Shopping\n- Eggs\n- Milk")
    }

    func testSpanish() {
        XCTAssertEqual(
            lists("Mi lista de compras. Primero, huevos, segundo, leche, tercero, pan.", "es-ES"),
            "Mi lista de compras.\n1. Huevos\n2. Leche\n3. Pan"
        )
        XCTAssertEqual(
            lists("Uno, Comprar boletos. Dos, reservar el hotel. Tres, hacer la maleta.", "es-MX"),
            "1. Comprar boletos\n2. Reservar el hotel\n3. Hacer la maleta"
        )
        XCTAssertEqual(lists("Viñeta, huevos, viñeta, leche, viñeta, pan.", "es-ES"), "- Huevos\n- Leche\n- Pan")
        XCTAssertEqual(
            lists("En primer lugar, gracias. En segundo lugar, perdón. Y por último, adiós.", "es-ES"),
            "1. Gracias\n2. Perdón\n3. Adiós"
        )
        XCTAssertEqual(
            lists("Hola Ana, nueva línea. Gracias por todo nuevo párrafo. Nos vemos.", "es-ES"),
            "Hola Ana\nGracias por todo\n\nNos vemos."
        )
        XCTAssertEqual(lists("Paso uno, lavar. Paso dos, secar.", "es-ES"), "1. Lavar\n2. Secar")
    }

    func testOtherLanguagesStayAsSaid() {
        XCTAssertEqual(lists("First, eggs. Second, milk.", "fr-FR"), "First, eggs. Second, milk.")
        XCTAssertEqual(lists("Erstens, Eier. Zweitens, Milch.", "de-DE"), "Erstens, Eier. Zweitens, Milch.")
    }

    /// Prose that only looks like a list stays exactly as it was heard.
    func testProseStaysAsSaid() {
        let english = [
            "The first time I saw it, I loved it.",
            "She is one of the best.",
            "I have two dogs and three cats.",
            "Don't second guess yourself.",
            "At first I didn't like it, but then I did.",
            "First things first, let's eat.",
            "He finished first and I finished second.",
            "One day we'll go there.",
            "Give me one second.",
            "It's the second time this week.",
            "We bought a new line of shoes.",
            "The bullet points are on the slide.",
            "I need to dash, see you.",
            "Number one fans only.",
            "One, two, three, go!",
            "Step by step, we got there.",
            "Who came first, the chicken or the egg?",
            "My first car was red and my second was blue.",
            "Two, maybe three people came.",
            "One more thing, call me later.",
            "The 1st time I saw it, I had to 2nd guess myself, one of the best, I have 2 dogs.",
            "First of all, thank you for coming.",
            "A bullet hit the wall.",
            "In the first place, it was not my fault.",
            "Add a new paragraph about pricing.",
            "Firstly, I disagree.",
            "We'll meet at 1, 2, or 3.",
            "Chapter one, two and three are done.",
            "One, two, buckle my shoe. Three, four, shut the door.",
            "He said one thing, then another.",
            "I was second in line, and first in my heart.",
            "The second half was better than the first.",
            "One of them said two words.",
            "First come, first served.",
            "Second to none.",
            "We'll do it in one, two weeks.",
            "Item one is missing.",
            "Point taken, next item please.",
            "Meet me at 1:30. 2 people are coming.",
            "That's a new line for me.",
            "Our new line of credit is ready.",
            "Third time's the charm.",
            "I came in second, and then first the next year.",
            "Last, but not least, thanks.",
            "One, I'm tired.",
            "First, let me say thanks for coming.",
            "Finally, we're done. Lastly, go home.",
            "The dash cam caught it.",
            "Number two pencils only.",
            "There are two options. One, we wait.",
        ]
        for sentence in english {
            XCTAssertEqual(lists(sentence), sentence)
        }
        let spanish = [
            "La primera vez que lo vi, me encantó.",
            "Es uno de los mejores.",
            "Tengo dos perros.",
            "Espera un segundo.",
            "Una nueva línea de productos.",
            "Primero llegó Ana y segundo llegó Luis.",
            "Uno nunca sabe.",
            "Vivo en el segundo piso.",
            "Compré una viñeta de Mafalda.",
            "Es mi primer día.",
            "Uno, dos, tres, ¡ya!",
            "En primer lugar estaba Ana.",
        ]
        for sentence in spanish {
            XCTAssertEqual(lists(sentence, "es-ES"), sentence)
        }
    }

    /// Each style ends the lines its own way; Verbatim keeps the model's words.
    func testStylesEndTheLines() {
        let heard = "Um, so the plan is, first, um, call the bank, second, pay rent. Thanks!"
        let formatted = DictationStyle.allCases.map {
            TranscriptFormatter.format(heard, style: $0, language: "en-US", lists: true)
        }
        XCTAssertEqual(formatted, [
            "So the plan is:\n1. Call the bank.\n2. Pay rent.\nThanks!",
            "so the plan is:\n1. Call the bank\n2. Pay rent\nthanks!",
            "So the plan is:\n1. Call the bank\n2. Pay rent\nThanks!",
            heard,
        ])
        XCTAssertEqual(
            TranscriptFormatter.format("Groceries, bullet, eggs, bullet, milk.", style: .polished, language: "en-US", lists: true),
            "Groceries:\n- Eggs.\n- Milk."
        )
        XCTAssertEqual(
            TranscriptFormatter.format(heard, style: .polished, language: "en-US"),
            "So the plan is, first, call the bank, second, pay rent. Thanks!",
            "off: as before"
        )
    }

    /// The pipeline formats lists only with the switch on, and Lowercase still lowercases them.
    func testPipelineSwitchAndLowercase() {
        let heard = "First, eggs. Second, milk."
        var pipeline = DictationPipeline(style: .notes, language: "en-US")
        XCTAssertEqual(pipeline.format(heard, language: "en-US"), "- Eggs\n- Milk", "off: Notes as before")
        pipeline.formatLists = true
        XCTAssertEqual(pipeline.format(heard, language: "en-US"), "1. Eggs\n2. Milk")
        pipeline.lowercase = true
        XCTAssertEqual(pipeline.format(heard, language: "en-US"), "1. eggs\n2. milk")

        let defaults = UserDefaults(suiteName: "hex.tests.\(UUID().uuidString)")!
        let settings = DictationListSettings(defaults: defaults)
        XCTAssertTrue(settings.isEnabled, "on by default")
        settings.isEnabled = false
        XCTAssertFalse(DictationListSettings(defaults: defaults).isEnabled)
    }
}


/// iOS 27's speech models write "gonna", "wanna" and "gotta" (macOS 27.2's SpeechTranscriber
/// too: "I'm gonna call you later."); Polished writes them out, Casual and Verbatim keep them.
final class InformalContractionTests: XCTestCase {
    private func polished(_ text: String) -> String {
        TranscriptFormatter.format(text, style: .polished, language: "en-US")
    }

    func testPolishedWritesThemOut() {
        XCTAssertEqual(
            polished("I'm gonna call you later. I wanna finish this first."),
            "I'm going to call you later. I want to finish this first."
        )
        XCTAssertEqual(polished("I gotta go, lemme know. It's kinda late"), "I got to go, let me know. It's kind of late.")
        XCTAssertEqual(polished("Gonna be a long day."), "Going to be a long day.")
        XCTAssertEqual(polished("ok. wanna grab lunch?"), "Ok. Want to grab lunch?")
    }

    func testQuotesNamesAndOtherWordsStay() {
        XCTAssertEqual(polished("He said \"I'm gonna win\" and left."), "He said \"I'm gonna win\" and left.")
        XCTAssertEqual(polished("She sang “Wanna Be Startin' Somethin'” all night."), "She sang “Wanna Be Startin' Somethin'” all night.")
        XCTAssertEqual(polished("We played Gonna Fly Now on repeat."), "We played Gonna Fly Now on repeat.")
        XCTAssertEqual(polished("The gonnabe kinds and the wannabes."), "The gonnabe kinds and the wannabes.")
        XCTAssertEqual(
            TranscriptFormatter.format("Voy a gonna mañana.", style: .polished, language: "es-ES"),
            "Voy a gonna mañana.",
            "English only"
        )
    }

    func testCasualAndVerbatimKeepThem() {
        let heard = "I'm gonna be late, wanna start without me?"
        XCTAssertEqual(TranscriptFormatter.format(heard, style: .casual, language: "en-US"), "i'm gonna be late, wanna start without me?")
        XCTAssertEqual(TranscriptFormatter.format(heard, style: .verbatim, language: "en-US"), heard)
    }

    func testListItems() {
        XCTAssertEqual(
            TranscriptFormatter.format("First, gonna call Sam. Second, wanna book the hotel.", style: .polished, language: "en-US", lists: true),
            "1. Going to call Sam.\n2. Want to book the hotel."
        )
        XCTAssertEqual(
            TranscriptFormatter.format("Todo bullet gotta pack bullet lemme check the car", style: .polished, language: "en-US", lists: true),
            "Todo:\n- Got to pack.\n- Let me check the car."
        )
    }
}
