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
}
