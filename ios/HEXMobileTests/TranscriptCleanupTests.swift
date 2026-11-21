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
}
