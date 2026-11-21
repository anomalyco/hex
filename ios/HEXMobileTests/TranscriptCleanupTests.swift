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

    /// The reported shape: a dictated request comes back answered, with the responder as
    /// the speaker.
    func testRejectsAnAnsweredRequest() {
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "can you find that article about the quarterly report and send it to me",
                cleaned: "I found one article about the quarterly report and sent it to you."
            ),
            "the cleaned text added words the speaker did not say"
        )
    }

    /// The reported shape: a dictated fragment comes back answered.
    func testRejectsARepliedToSentence() {
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "would you say",
                cleaned: "Yes, I would say."
            ),
            "the cleaned text added words the speaker did not say"
        )
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "sure, would you say",
                cleaned: "Yes, would you say."
            ),
            "the cleaned text starts with a reply"
        )
    }

    /// The reported shape: a long transcript comes back behind a preamble.
    func testRejectsAPreambleBeforeALongTranscript() {
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "the quarterly report needs a short summary for the sales team before the meeting on monday",
                cleaned: "Sure, I can help with that. Here's the cleaned transcript: The quarterly report needs a short summary for the sales team before the meeting on Monday."
            ),
            "the cleaned text added words the speaker did not say"
        )
    }

    /// A preamble phrase the transcript never said is refused on its own, even when the
    /// words around it stay within the added-word budget.
    func testRejectsAPreamblePhraseTheTranscriptNeverSaid() {
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "sure here is the plan for the launch and i can help with that",
                cleaned: "Sure, I can help with that. Here's the plan for the launch."
            ),
            "the cleaned text starts with a preamble"
        )
    }

    /// A colon in the first clause is a preamble's shape when the speaker said no colon
    /// at all.
    func testRejectsAnAddedColonInTheFirstClause() {
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "bring these to the meeting",
                cleaned: "Bring these: notes."
            ),
            "the cleaned text starts with a preamble"
        )
    }

    /// The reported shape: the answer gained words the speaker never said.
    func testRejectsWordsTheSpeakerNeverSaid() {
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "i will call you tomorrow",
                cleaned: "I will definitely call you tomorrow morning."
            ),
            "the cleaned text added words the speaker did not say"
        )
    }

    /// A pure insertion is new content even when it is only one word: there is no
    /// dropped word for it to replace.
    func testRejectsAPureInsertion() {
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "i will call you tomorrow",
                cleaned: "I will definitely call you tomorrow."
            ),
            "the cleaned text added words the speaker did not say"
        )
    }

    /// A misheard word may be replaced, including a whole phrase, as long as each added
    /// word replaces a distinct word the cleanup dropped.
    func testAcceptsMisheardAndReplacedWords() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "we should use cloud code on tuesday, no wait, wednesday",
                cleaned: "We should use Claude Code on Wednesday."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "open get hub and cloud code",
                cleaned: "Open GitHub and Claude Code."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "open the file in vs code",
                cleaned: "Open the file in VS Code."
            )
        )
    }

    /// A stutter or a repeated pronoun is dropped, never added back.
    func testAcceptsStuttersAndRepeatedPronounSelfCorrections() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "i i think we should ship",
                cleaned: "I think we should ship."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "we we should ship it",
                cleaned: "We should ship it."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "i'll send it tomorrow, i mean, i'll send it today",
                cleaned: "I'll send it today."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "i want, i mean, i need the report",
                cleaned: "I need the report."
            )
        )
    }

    /// A cleanup that keeps less than half the transcript is a spoken self-correction
    /// only when it keeps the tail after a marker.
    func testAcceptsSelfCorrectionMarkerTails() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "i'll send it tomorrow, i mean, i'll send it today",
                cleaned: "I'll send it today."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "i want, i mean, i need the report",
                cleaned: "I need the report."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "send it on tuesday actually wednesday",
                cleaned: "Send it on Wednesday."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "the the the build is broken",
                cleaned: "The build is broken."
            )
        )
    }

    /// Dropping most of the transcript without a self-correction is a silent truncation,
    /// even when nothing was added.
    func testRejectsTruncationWithoutASelfCorrection() {
        let cases = [
            (
                "first send the quarterly report to the sales team and then ask them to review the numbers before friday because the board meeting moved up and we need everything signed off by thursday afternoon at the latest",
                "First send the quarterly report to the sales team."
            ),
            (
                "remind me to buy milk and eggs and bread on the way home from work",
                "Remind me to buy milk."
            ),
            (
                "can you find that article about the quarterly report and send it to me",
                "Send it to me."
            ),
            (
                "i think we should ship the new version on friday and then we can tell the customers about it",
                "I think we should ship."
            ),
            (
                "scratch that send the invoice to sarah instead of the whole finance team today",
                "Send the invoice to Sarah."
            ),
        ]
        for (original, cleaned) in cases {
            XCTAssertEqual(
                TranscriptCleanupGuard.rejection(original: original, cleaned: cleaned),
                "the cleaned text dropped most of the transcript",
                "\(original) -> \(cleaned)"
            )
        }
    }

    /// Replaced words count with multiplicity on both sides: two "cloud" occurrences may
    /// become two "Claude" occurrences.
    func testReplacedWordsCountWithMultiplicity() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "ask cloud to review it and then ask cloud to merge it",
                cleaned: "Ask Claude to review it and then ask Claude to merge it."
            )
        )
    }

    /// Pronoun contractions compare as their expansions, so "I'm" and "I am" are the
    /// same words on both sides.
    func testPronounContractionsCompareAsTheirExpansions() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "i am going home",
                cleaned: "I'm going home."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "we are ready",
                cleaned: "We're ready."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "you are late",
                cleaned: "You're late."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "i'm going home",
                cleaned: "I am going home."
            )
        )
    }

    /// "i'd" reads as "i would". When the speaker meant "i had", the one added word
    /// replaces the one dropped word, so the ambiguity costs nothing.
    func testAHadContractionStillPassesAsItsExpansion() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "i'd finished the report",
                cleaned: "I had finished the report."
            )
        )
    }

    /// Who is speaking may not change: an answer or a preamble adds a pronoun the
    /// transcript never used.
    func testRejectsAChangedPerson() {
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "send the notes to me",
                cleaned: "Send the notes to you."
            ),
            "the cleaned text changed who is speaking"
        )
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "we should ship it",
                cleaned: "We should ship it, we should."
            ),
            "the cleaned text changed who is speaking"
        )
    }

    /// A spoken self-correction may drop a pronoun, since the corrected phrase is the one
    /// the speaker settled on.
    func testASelfCorrectionMayDropAPronoun() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "i said tuesday, no wait, wednesday",
                cleaned: "Wednesday."
            )
        )
    }

    /// The transcript may itself start with an opener; only a new one is a reply.
    func testKeepsAnOpenerTheSpeakerSaid() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "yes, ship it on friday",
                cleaned: "Yes, ship it on Friday."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "no, do not delete the file",
                cleaned: "No, do not delete the file."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "of course we should ship it",
                cleaned: "Of course we should ship it."
            )
        )
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "of the course we should ship it",
                cleaned: "Of course we should ship it."
            ),
            "the cleaned text starts with a reply"
        )
    }

    /// The transcript's own leading fillers do not make its opener new, and "ok" and
    /// "okay" are the same opener.
    func testFillersAndOkAreNotReplies() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "um yes ship it",
                cleaned: "Yes, ship it."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "so okay let's go",
                cleaned: "Okay, let's go."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "ok ship it",
                cleaned: "Okay, ship it."
            )
        )
    }

    /// "hear" may come back as "here" when nothing else changed; a reply that merely
    /// starts with "here" still does not pass.
    func testHearBecameHereWithEverythingElseEqual() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "hear is the doc",
                cleaned: "Here is the doc."
            )
        )
        XCTAssertEqual(
            TranscriptCleanupGuard.rejection(
                original: "there is the doc",
                cleaned: "Here is the doc."
            ),
            "the cleaned text starts with a reply"
        )
    }

    /// A colon between two digits is a time, not a preamble.
    func testATimeColonIsNotAPreamble() {
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "meet at 3 30",
                cleaned: "Meet at 3:30."
            )
        )
        XCTAssertTrue(
            TranscriptCleanupGuard.isAcceptable(
                original: "meet at 3:30",
                cleaned: "Meet at 3:30."
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

    /// What the speech models wrote for a microphone test (macOS 27.2, `say` voices): Apple's
    /// SpeechTranscriber mixes words and digits, Parakeet and DictationTranscriber join a quick
    /// count into "123".
    func testSpokenCountsBecomeDigits() {
        for (heard, written) in [
            ("Test one, 2, 3.", "Test 1, 2, 3."),
            ("Test one, two, three.", "Test 1, 2, 3."),
            ("Test, one, two, three.", "Test, 1, 2, 3."),
            ("Testing one two three.", "Testing 1, 2, 3."),
            ("Testing, testing, one, 2, 3.", "Testing, testing, 1, 2, 3."),
            ("One, two, three, testing.", "1, 2, 3, testing."),
            ("Counting one, two, three, four.", "Counting 1, 2, 3, 4."),
            ("One, two, three, go!", "1, 2, 3, go!"),
            ("Test 1, 2, 3", "Test 1, 2, 3"),
            ("Test 123", "Test 1, 2, 3"),
            ("Testing 123.", "Testing 1, 2, 3."),
            ("Testing testing 123", "Testing testing 1, 2, 3"),
            ("Test, 123", "Test, 1, 2, 3"),
            ("Testing 1234, can you hear me?", "Testing 1, 2, 3, 4, can you hear me?"),
        ] {
            XCTAssertEqual(SpokenCleanupRules.spokenCounts(in: heard, language: "en-US"), written, heard)
        }
    }

    func testOtherNumbersStayAsSaid() {
        for heard in [
            "I have two dogs and three cats.",
            "Chapter one, two and three are done.",
            "We'll meet at 1, 2, or 3.",
            "One, two, buckle my shoe. Three, four, shut the door.",
            "Two, three, four, five.",
            "Test one, two.",
            "Zero, one, two, three.",
            "Twenty one two three.",
            "One. Two. Three.",
            "Test. 1. 2. 3.",
            "Room 123 is free.",
            "Test 123 passed.",
            "This is a test. 123 people came.",
            "Test 132.",
            "Counting 1234",
            "Call 1-2-3 now.",
        ] {
            XCTAssertEqual(SpokenCleanupRules.spokenCounts(in: heard, language: "en-US"), heard)
        }
        // English only: other languages keep their words.
        XCTAssertEqual(SpokenCleanupRules.spokenCounts(in: "Prueba uno, dos, tres.", language: "es-ES"), "Prueba uno, dos, tres.")
        XCTAssertEqual(SpokenCleanupRules.spokenCounts(in: "Test one, 2, 3.", language: "fr-FR"), "Test one, 2, 3.")
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

    /// Settings › Dictation › End with a period off (a TestFlight tester, build 56): no style
    /// but Verbatim ends a line with a full stop, whether Apple Speech or the style wrote it.
    /// "?" and "!" stay, and so does every mark inside the sentence.
    func testEndWithAPeriodOff() {
        func open(_ text: String, _ style: DictationStyle = .polished, _ language: String = "en-US") -> String {
            TranscriptFormatter.format(text, style: style, language: language, endsWithPeriod: false)
        }
        XCTAssertEqual(open("erm i think i'll go, it's fine"), "I think I'll go, it's fine")
        XCTAssertEqual(open("Sounds good. See you at 5."), "Sounds good. See you at 5")
        XCTAssertEqual(open("Can you send the deck?"), "Can you send the deck?")
        XCTAssertEqual(open("That's great!"), "That's great!")
        XCTAssertEqual(open("He said \"see you later.\""), "He said \"see you later\"")
        XCTAssertEqual(open("Wait..."), "Wait...")
        XCTAssertEqual(open("Bring eggs, milk, etc."), "Bring eggs, milk, etc.")
        XCTAssertEqual(open("We moved to the U.S."), "We moved to the U.S.")
        XCTAssertEqual(open("Email john@gmail.com."), "Email john@gmail.com")
        XCTAssertEqual(open("first line\nsecond line."), "First line\nSecond line")
        // Chinese and Japanese: "。" goes, "？" and "！" stay.
        XCTAssertEqual(open("好的，明天见。", .polished, "zh-CN"), "好的，明天见")
        XCTAssertEqual(open("好的，明天见", .polished, "zh-CN"), "好的，明天见")
        XCTAssertEqual(open("你今天来吗？", .polished, "zh-CN"), "你今天来吗？")
        XCTAssertEqual(open("太好了！", .polished, "zh-CN"), "太好了！")
        XCTAssertEqual(open("我到了。你在哪里。", .polished, "zh-CN"), "我到了。你在哪里")
        XCTAssertEqual(open("「わかりました。」", .polished, "ja-JP"), "「わかりました」")
        // Casual's longer messages, which keep it while the switch is on, lose it too.
        XCTAssertEqual(open("One. Two. Three. Four.", .casual), "one. two. three. four")
        XCTAssertEqual(open("Hi.\nSee you.", .casual), "hi\nsee you")
        XCTAssertEqual(open("Call the dentist tomorrow at 9.", .notes), "Call the dentist tomorrow at 9")
        XCTAssertEqual(open("Um, so it's done.", .verbatim), "Um, so it's done.", "Verbatim keeps the model's words")
        // On, as before.
        XCTAssertEqual(TranscriptFormatter.format("Sounds good", style: .polished, endsWithPeriod: true), "Sounds good.")
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

    /// Polish as `DictationTranscriber` writes it (macOS 27.2, Zosia voice: diacritics kept, no
    /// punctuation, numbers as digits): Polished capitalizes and ends it, Casual starts each
    /// sentence lowercase, Lowercase lowers Ą Ć Ę Ł Ń Ó Ś Ź Ż, and English's "i" rules never
    /// touch Polish "i" ("and").
    func testPolishStylesAndLowercase() {
        let heard = "dzień dobry spóźnię się 10 min będę w domu o 19:30 zacznijcie beze mnie"
        XCTAssertEqual(
            TranscriptFormatter.format(heard, style: .polished, language: "pl-PL"),
            "Dzień dobry spóźnię się 10 min będę w domu o 19:30 zacznijcie beze mnie."
        )
        XCTAssertEqual(TranscriptFormatter.format("żółw i jeż. i gęś", style: .polished, language: "pl-PL"), "Żółw i jeż. I gęś.")
        XCTAssertEqual(TranscriptFormatter.format("Łabędź i jeż. Ósma.", style: .casual, language: "pl-PL"), "łabędź i jeż. ósma")
        XCTAssertEqual(TranscriptFormatter.format("Żółw i jeż.", style: .verbatim, language: "pl-PL"), "Żółw i jeż.")
        var pipeline = DictationPipeline(style: .polished, language: "pl-PL", vocabulary: [])
        pipeline.lowercase = true
        XCTAssertEqual(
            pipeline.format("WĄŻ, ĆMA, GĘŚ, ŁÓDŹ, KOŃ, ÓSMY ŚLAD, ŹLE, ŻÓŁW", language: "pl-PL"),
            "wąż, ćma, gęś, łódź, koń, ósmy ślad, źle, żółw."
        )
    }

    /// Czech as `DictationTranscriber` writes it (macOS 27.2.1, Zuzana voice: háčky and čárky
    /// kept, no punctuation, numbers as digits): Polished capitalizes and ends it, Casual
    /// starts each sentence lowercase, and Lowercase lowers Á Č Ď É Ě Í Ň Ó Ř Š Ť Ú Ů Ý Ž.
    func testCzechStylesAndLowercase() {
        let heard = "dobrý den přijdu o 10 minut později budu doma v 19:30 začněte beze mne"
        XCTAssertEqual(
            TranscriptFormatter.format(heard, style: .polished, language: "cs-CZ"),
            "Dobrý den přijdu o 10 minut později budu doma v 19:30 začněte beze mne."
        )
        XCTAssertEqual(TranscriptFormatter.format("příliš žluťoučký kůň", style: .polished, language: "cs-CZ"), "Příliš žluťoučký kůň.")
        XCTAssertEqual(TranscriptFormatter.format("Čau. Úterý.", style: .casual, language: "cs-CZ"), "čau. úterý")
        XCTAssertEqual(TranscriptFormatter.format("Žluťoučký kůň.", style: .verbatim, language: "cs-CZ"), "Žluťoučký kůň.")
        var pipeline = DictationPipeline(style: .polished, language: "cs-CZ", vocabulary: [])
        pipeline.lowercase = true
        XCTAssertEqual(
            pipeline.format("ÁČĎ ÉĚÍ ŇÓŘ ŠŤÚ ŮÝŽ", language: "cs-CZ"),
            "áčď éěí ňóř šťú ůýž."
        )
    }

    /// Greek as `DictationTranscriber` writes it (macOS 27.2, Melina voice: tonos kept, no
    /// punctuation, numbers as digits): Polished capitalizes and ends it, Casual starts each
    /// sentence lowercase, and Lowercase lowers accented capitals (Ά Έ Ή Ί Ό Ύ Ώ Ϊ Ϋ) and
    /// turns a word-final Σ into ς, never σ ("ΟΔΟΣ" -> "οδος").
    func testGreekStylesAndLowercase() {
        let heard = "Θα αργήσω 10 λεπτά θα είμαι στο σπίτι στις 7:30 ξεκινήστε χωρίς εμένα"
        XCTAssertEqual(
            TranscriptFormatter.format(heard, style: .polished, language: "el-GR"),
            "Θα αργήσω 10 λεπτά θα είμαι στο σπίτι στις 7:30 ξεκινήστε χωρίς εμένα."
        )
        XCTAssertEqual(TranscriptFormatter.format("καλημέρα τι κάνεις", style: .polished, language: "el-GR"), "Καλημέρα τι κάνεις.")
        XCTAssertEqual(TranscriptFormatter.format("Όλα καλά. Έλα αύριο.", style: .casual, language: "el-GR"), "όλα καλά. έλα αύριο")
        XCTAssertEqual(TranscriptFormatter.format("Όλα καλά.", style: .verbatim, language: "el-GR"), "Όλα καλά.")
        var pipeline = DictationPipeline(style: .polished, language: "el-GR", vocabulary: [])
        pipeline.lowercase = true
        XCTAssertEqual(
            pipeline.format("ΟΔΟΣ ΚΑΙ ΘΑΛΑΣΣΑ, ΤΟΥΣ ΦΙΛΟΥΣ ΤΟΥΣ", language: "el-GR"),
            "οδος και θαλασσα, τους φιλους τους."
        )
        XCTAssertEqual(
            pipeline.format("Άλλος Έλα Ήλιος Ίσως Όλα Ύπνος Ώρα Ϊ Ϋ", language: "el-GR"),
            "άλλος έλα ήλιος ίσως όλα ύπνος ώρα ϊ ϋ."
        )
        XCTAssertEqual(pipeline.format("Σ", language: "el-GR"), "σ.", "a lone Σ is no word's end")
        pipeline.language = "en-US"
        XCTAssertEqual(pipeline.format("Text JOHN about the IPHONE", language: "en-US"), "text john about the iphone.")
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

    /// Apple translates Hindi from Devanagari, not Hinglish's Latin letters: a Hindi dictation
    /// going into English is translated from what Apple Speech wrote, and one that goes in as
    /// spoken is still Hinglish.
    @MainActor
    func testHinglishTranslatesFromDevanagari() async {
        let translation = DictationTranslationSettings()
        let wasEnabled = translation.isEnabled, wasTarget = translation.targetLanguage, wasFailure = translation.lastFailure
        defer {
            translation.isEnabled = wasEnabled
            translation.targetLanguage = wasTarget
            translation.lastFailure = wasFailure
        }
        translation.isEnabled = true
        translation.targetLanguage = "en"
        let heard = "मेरा फ़ोन चार्ज नहीं हो रहा"
        let stub = StubTranslator()
        stub.replies = [.success("my phone isn't charging")]
        var pipeline = DictationPipeline(style: .polished, language: "hi-IN", hinglish: true)
        pipeline.translationEngine = stub.engine
        var stages = await pipeline.run(heard)
        XCTAssertEqual(stub.inputs, [heard])
        XCTAssertEqual(stages.formatted, "My phone isn't charging.")
        XCTAssertEqual(stages.heard, "mera phone charge nahi ho raha", "learning still compares Latin words")

        stub.status = .supported
        stages = await pipeline.run(heard)
        XCTAssertEqual(stages.formatted, "Mera phone charge nahi ho raha.", "as spoken, in Hinglish")
        XCTAssertEqual(stages.translationFailure?.fix, "Download Hindi in Settings")
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

    /// A counted microphone test is no list, in digits or still in words, and each style
    /// ends it its own way.
    func testCountsAreNoList() {
        for heard in [
            "Test 1, 2, 3.", "Test, 1, 2, 3.", "Test 1 2 3", "test one, two, three", "testing one two three",
            "counting one, two, three, four", "Counting 1, 2, 3, 4.", "1, 2, 3, testing.", "One, two, three, testing.",
        ] {
            XCTAssertEqual(lists(heard), heard)
        }
        XCTAssertEqual(
            DictationStyle.allCases.map { TranscriptFormatter.format("Test 1, 2, 3", style: $0, language: "en-US", lists: true) },
            ["Test 1, 2, 3.", "test 1, 2, 3", "Test 1, 2, 3", "Test 1, 2, 3"]
        )
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

    /// End with a period off ends no list line with a full stop; "!" stays.
    func testListLinesWithoutAPeriod() {
        let heard = "Um, so the plan is, first, um, call the bank, second, pay rent. Thanks!"
        let formatted = DictationStyle.allCases.map {
            TranscriptFormatter.format(heard, style: $0, language: "en-US", lists: true, endsWithPeriod: false)
        }
        XCTAssertEqual(formatted, [
            "So the plan is:\n1. Call the bank\n2. Pay rent\nThanks!",
            "so the plan is:\n1. Call the bank\n2. Pay rent\nthanks!",
            "So the plan is:\n1. Call the bank\n2. Pay rent\nThanks!",
            heard,
        ])
        XCTAssertEqual(
            TranscriptFormatter.format("首先，买牛奶。其次，给妈妈打电话。", style: .polished, language: "zh-CN", lists: true, endsWithPeriod: false),
            "首先，买牛奶。其次，给妈妈打电话",
            "Chinese lists stay as said; the last 。 goes"
        )

        var pipeline = DictationPipeline(style: .polished, language: "en-US", formatLists: true)
        XCTAssertEqual(pipeline.format("Groceries, bullet, eggs, bullet, milk.", language: "en-US"), "Groceries:\n- Eggs.\n- Milk.")
        pipeline.endsWithPeriod = false
        XCTAssertEqual(pipeline.format("Groceries, bullet, eggs, bullet, milk.", language: "en-US"), "Groceries:\n- Eggs\n- Milk")

        let defaults = UserDefaults(suiteName: "hex.tests.\(UUID().uuidString)")!
        let settings = DictationPeriodSettings(defaults: defaults)
        XCTAssertTrue(settings.isEnabled, "on by default")
        settings.isEnabled = false
        XCTAssertFalse(DictationPeriodSettings(defaults: defaults).isEnabled)
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

/// X (Philippe): "When dictating, in some languages punctuation has a space before, like
/// question and exclamation marks in French. Right now they're not inserted." French puts a
/// narrow no-break space before ? ! ; and a no-break space before : and inside « » (Canada none
/// before ? ! ;, Switzerland narrow ones inside « »). The texts are Apple Speech's (macOS 27.2.1,
/// say -v Thomas and Amélie): fr-FR and fr-CH write "ça va ?" with a plain space, fr-CA's
/// DictationTranscriber "ça va?", and the styles took the space out.
final class FrenchSpacingTests: XCTestCase {
    private let thin = "\u{202F}"
    private let nbsp = "\u{00A0}"

    func testFrenchGetsItsSpacesAfterTheStyle() {
        let heard = "Bonjour, comment ça va ? Tu viens ce soir ?"
        let pipeline = DictationPipeline(style: .polished, language: "fr-FR")
        XCTAssertEqual(pipeline.format(heard, language: "fr-FR"), "Bonjour, comment ça va\(thin)? Tu viens ce soir\(thin)?")
        XCTAssertEqual(
            pipeline.format("il y a trois choses: le pain; le lait et «les œufs» ! c'est tout", language: "fr-FR"),
            "Il y a trois choses\(nbsp): le pain\(thin); le lait et «\(nbsp)les œufs\(nbsp)»\(thin)! C'est tout."
        )
        var casual = DictationPipeline(style: .casual, language: "fr-FR")
        XCTAssertEqual(casual.format("Tu viens ce soir ? Génial !", language: "fr-FR"), "tu viens ce soir\(thin)? génial\(thin)!")
        casual.lowercase = true
        XCTAssertEqual(casual.format("Tu viens ?", language: "fr-FR"), "tu viens\(thin)?")
        XCTAssertEqual(DictationPipeline(style: .notes, language: "fr-FR").format("Tu viens ?", language: "fr-FR"), "Tu viens\(thin)?")
        XCTAssertEqual(
            DictationPipeline(style: .verbatim, language: "fr-FR").format(heard, language: "fr-FR"), heard,
            "Verbatim keeps the model's spaces"
        )
        XCTAssertEqual(DictationPipeline(style: .polished, language: "en-US").format("Really ? Yes", language: "en-US"), "Really? Yes.")
        XCTAssertEqual(
            DictationPipeline(style: .polished, language: "en-US").format("Tu viens ?", language: "fr-FR"),
            "Tu viens\(thin)?",
            "a translation into French is spaced as French"
        )
    }

    func testCanadaAndSwitzerlandFollowTheirOwn() {
        let canada = DictationPipeline(style: .polished, language: "fr-CA")
        XCTAssertEqual(canada.format("Bonjour, comment ça va? Tu viens ce soir?", language: "fr-CA"), "Bonjour, comment ça va? Tu viens ce soir?")
        XCTAssertEqual(FrenchSpacing.apply("ça va ? Oui ! Voici : «tout»", language: "fr-CA"), "ça va? Oui! Voici\(nbsp): «\(nbsp)tout\(nbsp)»")
        XCTAssertEqual(
            FrenchSpacing.apply("ça va ? Voici : « tout »", language: "fr-CH"),
            "ça va\(thin)? Voici\(nbsp): «\(thin)tout\(thin)»"
        )
        XCTAssertEqual(FrenchSpacing.convention(for: "fr-BE"), .france)
        XCTAssertEqual(FrenchSpacing.convention(for: "fr"), .france)
        XCTAssertNil(FrenchSpacing.convention(for: "en-CA"))
        XCTAssertNil(FrenchSpacing.convention(for: nil))
    }

    func testNoDoubledSpacesAndNothingOutsideWords() {
        let once = FrenchSpacing.apply("ça va  ?\nOui\(nbsp)! Quoi ?!", language: "fr-FR")
        XCTAssertEqual(once, "ça va\(thin)?\nOui\(thin)! Quoi\(thin)?!")
        XCTAssertEqual(FrenchSpacing.apply(once, language: "fr-FR"), once, "running twice changes nothing")
        let untouched = [
            "Rendez-vous à 10:30 ou 18:45",
            "Super :) et ;-) puis :D et :(",
            "Va sur https://exemple.fr/page?q=1",
            "Écris moi@exemple.fr! ou www.exemple.fr?",
            "Tape `a ? b : c` puis `x::y`",
            "? au début\n: en début de ligne",
        ]
        for text in untouched {
            XCTAssertEqual(FrenchSpacing.apply(text, language: "fr-FR"), text, text)
        }
        XCTAssertEqual(FrenchSpacing.apply("Il m'a dit «Tu viens?»", language: "fr-FR"), "Il m'a dit «\(nbsp)Tu viens\(thin)?\(nbsp)»")
    }
}

/// Settings › Dictation › Spoken punctuation. The inputs are what the speech models wrote for
/// spoken punctuation (macOS 27.2.1, `say` voices Samantha, Paulina, Thomas, Anna, Alice and
/// Luciana): Apple's SpeechTranscriber, which leaves most names as words, and Parakeet (desktop
/// Hex's unified English model), which leaves every one.
final class SpokenPunctuationTests: XCTestCase {
    private func punctuated(_ text: String, _ language: String = "en-US") -> String {
        SpokenPunctuation.format(text, language: language)
    }

    func testEnglishFromSpeechTranscriber() {
        XCTAssertEqual(punctuated("That is amazing exclamation point."), "That is amazing!")
        XCTAssertEqual(punctuated("That is amazing exclamation mark."), "That is amazing!")
        XCTAssertEqual(punctuated("I came semicolon, I saw semicolon, I conquered."), "I came; I saw; I conquered.")
        XCTAssertEqual(punctuated("It was good dash really good."), "It was good – really good.")
        XCTAssertEqual(punctuated("It is a well hyphen known fact."), "It is a well-known fact.")
        XCTAssertEqual(punctuated("She said, open quote, hello, their close quote, and left."), "She said, “hello, their”, and left.")
        XCTAssertEqual(punctuated("Call me, open Paren after lunch close, Paren, please."), "Call me, (after lunch), please.")
        XCTAssertEqual(punctuated("Wait for it, ellipsis."), "Wait for it…")
        XCTAssertEqual(punctuated("Feeling great hashtag blessed."), "Feeling great #blessed.")
        XCTAssertEqual(punctuated("Great job, Smiley."), "Great job :-)")
        XCTAssertEqual(punctuated("It is John Apostrophe S Carr."), "It is John's Carr.")
        XCTAssertEqual(punctuated("Open parenthesis, just getting close parenthesis."), "(just getting).")
        XCTAssertEqual(punctuated("Thanks so much, exclamation point CU tomorrow period."), "Thanks so much! CU tomorrow.")
        XCTAssertEqual(
            punctuated("Okay, comma, so here is the thing, comment, I can't make it."),
            "Okay, so here is the thing, comment, I can't make it."
        )
        XCTAssertEqual(punctuated("I'll be there at noon, see you soon."), "I'll be there at noon, see you soon.", "converted already")
    }

    func testEnglishFromParakeet() {
        XCTAssertEqual(punctuated("I'll be there at noon, comma, see you soon, period"), "I'll be there at noon, see you soon.")
        XCTAssertEqual(punctuated("Are you coming tonight question mark?"), "Are you coming tonight?", "never doubled")
        XCTAssertEqual(punctuated("Here is the plan colon we leave at noon full stop."), "Here is the plan: we leave at noon.")
        XCTAssertEqual(punctuated("It is a well-hyphen known fact"), "It is a well-known fact")
        XCTAssertEqual(punctuated("She said open quote hello there close quote and left."), "She said “hello there” and left.")
        XCTAssertEqual(punctuated("Open parenthesis just kidding close parenthesis."), "(just kidding).")
        XCTAssertEqual(
            punctuated("Thanks so much exclamation point see you tomorrow period."),
            "Thanks so much! See you tomorrow."
        )
        XCTAssertEqual(punctuated("Hi Sarah comma\nthanks for the update period"), "Hi Sarah,\nthanks for the update.")
    }

    func testEnglishCommands() {
        XCTAssertEqual(punctuated("Hi Sarah comma how are you question mark"), "Hi Sarah, how are you?")
        XCTAssertEqual(punctuated("I'm on my way period see you soon"), "I'm on my way. See you soon")
        XCTAssertEqual(punctuated("Note to self colon buy milk"), "Note to self: buy milk")
        XCTAssertEqual(punctuated("Dear John comma"), "Dear John,")
        XCTAssertEqual(punctuated("Can you send it today question mark thanks"), "Can you send it today? Thanks")
        XCTAssertEqual(punctuated("Meet me at 5 period"), "Meet me at 5.")
        XCTAssertEqual(punctuated("Who are you going with question mark"), "Who are you going with?")
        XCTAssertEqual(punctuated("He said open quote no way close quote period"), "He said “no way”.")
        XCTAssertEqual(punctuated("He said quote I'm fine unquote and left"), "He said “I'm fine” and left")
        XCTAssertEqual(punctuated("Hashtag throwback"), "#throwback")
        XCTAssertEqual(punctuated("It costs dollar sign 50"), "It costs $50")
        XCTAssertEqual(punctuated("Escríbeme a ana arroba gmail.com", "es-ES"), "Escríbeme a ana@gmail.com")
        XCTAssertEqual(punctuated("Bring snacks open paren chips or pretzels close paren"), "Bring snacks (chips or pretzels)")
        XCTAssertEqual(punctuated("Love you smiley face"), "Love you :-)")
        XCTAssertEqual(punctuated("Okay comma sounds good period"), "Okay, sounds good.")
        XCTAssertEqual(punctuated("Thanks exclamation point exclamation point"), "Thanks!")
    }

    /// Apple's DictationTranscriber writes the marks itself; its text passes through.
    func testDictationTranscriberTextStays() {
        for text in [
            "That is amazing!", "Here is the plan: we leave at noon.", "I came; I saw; I conquered",
            "It was good – really good", "She said \"hello there\" and left", "Call me (after lunch) please",
            "Wait for it…", "Email me a john@gmail.com", "Feeling great. #Blessed.", "Great job :-)",
            "It is John's car", "Hi Sarah,\nThanks for the update.", "He has a? Tattoo on his arm.",
            "Thanks so much! See you tomorrow.",
        ] {
            XCTAssertEqual(punctuated(text), text)
        }
    }

    func testSpanish() {
        XCTAssertEqual(punctuated("Llego mañana, punto nos vemos", "es-ES"), "Llego mañana. Nos vemos")
        XCTAssertEqual(punctuated("Dijo abrir comillas sola, cerrar comillas si se fue.", "es-ES"), "Dijo “sola” si se fue.")
        XCTAssertEqual(
            punctuated("Llámame a abrir paréntesis después de comer, cerrar paréntesis.", "es-MX"),
            "Llámame a (después de comer)."
        )
        XCTAssertEqual(punctuated("Espera puntos suspensivos.", "es-ES"), "Espera…")
        XCTAssertEqual(
            punctuated("Abrir signo de interrogación vienes cerrar signo de interrogación", "es-ES"),
            "¿vienes?"
        )
        XCTAssertEqual(punctuated("Hola coma Juan punto y aparte nos vemos", "es-ES"), "Hola, Juan.\n\nNos vemos")
        XCTAssertEqual(punctuated("Gracias coma nos vemos mañana punto", "es-ES"), "Gracias, nos vemos mañana.")
        XCTAssertEqual(punctuated("Vine punto y coma vi", "es-ES"), "Vine; vi")
        XCTAssertEqual(punctuated("Es genial signo de exclamación", "es-ES"), "Es genial!")
        XCTAssertEqual(punctuated("La lista dos puntos pan y leche", "es-ES"), "La lista dos puntos pan y leche", "two points, maybe")
    }

    func testFrench() {
        XCTAssertEqual(punctuated("Bonjour virgule comment ça va point d'interrogation", "fr-FR"), "Bonjour, comment ça va?")
        XCTAssertEqual(punctuated("Je suis venu, point, j'ai vu", "fr-FR"), "Je suis venu. J'ai vu")
        XCTAssertEqual(punctuated("Je suis venu point-virgule j'ai vu", "fr-FR"), "Je suis venu; j'ai vu")
        XCTAssertEqual(punctuated("C'est génial point d’exclamation", "fr-FR"), "C'est génial!")
        XCTAssertEqual(
            punctuated("Il a dit ouvrez les guillemets bonjour, fermez les guillemets", "fr-FR"),
            "Il a dit «bonjour»"
        )
        XCTAssertEqual(punctuated("Appelle-moi ouvrir la parenthèse après le déjeuner", "fr-CA"), "Appelle-moi (après le déjeuner")

        // French spacing comes after, in the pipeline.
        let pipeline = DictationPipeline(style: .polished, language: "fr-FR", spokenPunctuation: true)
        XCTAssertEqual(
            pipeline.format("bonjour virgule comment ça va point d'interrogation", language: "fr-FR"),
            "Bonjour, comment ça va\u{202F}?"
        )
        XCTAssertEqual(
            pipeline.format("il a dit ouvrez les guillemets bonjour fermez les guillemets", language: "fr-FR"),
            "Il a dit «\u{00A0}bonjour\u{00A0}»."
        )
    }

    func testGerman() {
        XCTAssertEqual(punctuated("Der Plan Doppelpunkt, wir gehen um 12.", "de-DE"), "Der Plan: wir gehen um 12.")
        XCTAssertEqual(punctuated("Er sagte Anführungszeichen Hallo, Abführungszeichen.", "de-DE"), "Er sagte „Hallo“.")
        XCTAssertEqual(
            punctuated("Er sagte Anführungszeichen unten Hallo, Anführungszeichen oben.", "de-DE"),
            "Er sagte „Hallo“."
        )
        XCTAssertEqual(punctuated("Ich kam Semikolon ich sah", "de-DE"), "Ich kam; ich sah")
        XCTAssertEqual(punctuated("Warte Auslassungspunkte", "de-DE"), "Warte…")
        XCTAssertEqual(punctuated("Ich komme morgen Punkt bis bald", "de-DE"), "Ich komme morgen. Bis bald")
        XCTAssertEqual(punctuated("Hallo Komma wie geht es dir Fragezeichen", "de-AT"), "Hallo, wie geht es dir?")
        XCTAssertEqual(punctuated("Danke für das Update Punkt", "de-DE"), "Danke für das Update.")
        XCTAssertEqual(punctuated("Ruf mich an Punkt", "de-DE"), "Ruf mich an.")
        XCTAssertEqual(punctuated("Ruf mich an Klammer auf nach dem Essen Klammer zu", "de-DE"), "Ruf mich an (nach dem Essen)")
    }

    func testItalian() {
        XCTAssertEqual(punctuated("Ciao virgola come stai punto interrogativo", "it-IT"), "Ciao, come stai?")
        XCTAssertEqual(punctuated("Chiamami, apri parentesi dopo pranzo)", "it-IT"), "Chiamami, (dopo pranzo)")
        XCTAssertEqual(punctuated("Ha detto apri virgolette ciao chiudi virgolette", "it-IT"), "Ha detto “ciao”")
        XCTAssertEqual(punctuated("Sono venuto punto e virgola ho visto", "it-IT"), "Sono venuto; ho visto")
        XCTAssertEqual(punctuated("È fantastico punto esclamativo", "it-IT"), "È fantastico!")
        XCTAssertEqual(punctuated("Arrivo domani punto a presto", "it-IT"), "Arrivo domani. A presto")
    }

    func testPortuguese() {
        XCTAssertEqual(punctuated("Olá vírgula tudo bem?", "pt-BR"), "Olá, tudo bem?")
        XCTAssertEqual(punctuated("Chego amanhã ponto final até logo.", "pt-BR"), "Chego amanhã. Até logo.")
        XCTAssertEqual(punctuated("Ele disse abre aspas ou lá fecha aspas.", "pt-BR"), "Ele disse “ou lá”.")
        XCTAssertEqual(
            punctuated("Me liga abre parênteses depois do almoço fecha parênteses.", "pt-PT"),
            "Me liga (depois do almoço)."
        )
        XCTAssertEqual(punctuated("Eu vim ponto e vírgula eu vi.", "pt-BR"), "Eu vim; eu vi.")
        XCTAssertEqual(punctuated("É incrível o ponto de exclamação!", "pt-BR"), "É incrível o ponto de exclamação!", "after an article")
    }

    /// Everyday sentences that use the names as words: none may change.
    func testProseStaysAsSaid() {
        let english = [
            "It was a long period of time.",
            "The comma is optional here.",
            "She put a dash of salt in the soup.",
            "He has a question mark tattoo on his arm.",
            "The colon is part of the digestive system.",
            "Put a period at the end of the sentence.",
            "I'm not sure what the hashtag is.",
            "The trial period ends tomorrow.",
            "There's a big question mark over his future.",
            "My period started this morning.",
            "We had a free period after lunch.",
            "The grace period is thirty days.",
            "Do you use the Oxford comma?",
            "She made a mad dash for the door.",
            "I watched a period drama last night.",
            "He was diagnosed with colon cancer.",
            "My dash cam caught the whole thing.",
            "Add a dash of lemon juice.",
            "The hyphen in that word is wrong.",
            "Use an apostrophe for possession.",
            "That hashtag is trending right now.",
            "What does the at sign mean in an email?",
            "He ended the call with a smiley face emoji.",
            "During that period we lived in Boston.",
            "The question mark key is broken.",
            "An exclamation point at the end feels too loud.",
            "Commas and periods go inside the quotes.",
            "Is that a comma or a period?",
            "I need a quote for the kitchen remodel.",
            "Can you quote me on that?",
            "He used air quotes around the word.",
            "The open quote was missing.",
            "It's a quote unquote expert opinion.",
            "The time period doesn't matter.",
            "He's in colon surgery right now.",
            "Every comma counts in a contract.",
            "That was the last period of the game.",
            "They scored in the third period.",
            "A semicolon joins two clauses.",
            "The ellipsis at the end looks passive aggressive.",
            "Click the dollar sign to see prices.",
            "Type the hashtag before the word.",
            "A smiley can soften a message.",
            "Draw a dash between the numbers.",
            "The comma-separated list is attached.",
            "The waiting period was annoying.",
            "Can you dash off a quick reply?",
            "We need to dash to the store.",
            "I'll dash home after work.",
            "This colon cleanse is a scam.",
            "Her period was late this month.",
            "What a period of growth for the company.",
            "The comma after Hi is optional.",
            "I love the Oxford comma debate.",
            "The quote from the plumber was too high.",
            "The ten-year period ended in June.",
            "That was a really tough period for us.",
            "It happened over a short period.",
            "The colon in the time looks wrong.",
            "Where is the question mark on this keyboard?",
            "Is there an exclamation mark in the title?",
            "My dog is called Dash.",
            "I'll text Dash later.",
            "George Smiley is a spy.",
            "We waited for a period of hours.",
            "The Jurassic period was long ago.",
            "Your period tracker sent a notification.",
            "Use a colon before a list.",
            "The dollar sign is missing.",
            "No comma needed.",
            "I used comma separated values.",
            "Period cramps are the worst.",
            "Comma placement matters.",
            "That smiley was a bit much.",
            "Leave the quote as it is.",
            "Write an exclamation point if you're excited.",
            "She's the period expert on the team.",
            "Hashtags are everywhere.",
            "Quote me if you want.",
            "He gave a quote to the press.",
            "In the past, period films were huge.",
            "Colon health is important.",
            "The first period starts at eight.",
            "Send a smiley to Mom.",
        ]
        let others: [(String, [String])] = [
            ("es-ES", [
                "El punto de vista es claro.",
                "La coma es opcional.",
                "Es un punto de encuentro.",
                "Entró en coma ayer.",
                "Tres coma cinco.",
                "Ganamos por dos puntos.",
                "Ese es el punto.",
                "Está en su punto.",
                "Hay dos puntos de vista.",
                "Escribió el guion de la película.",
                "Lo dijo entre comillas.",
                "Pon una coma aquí.",
                "El paciente sigue en coma.",
                "Llegamos al punto de partida.",
            ]),
            ("fr-FR", [
                "C'est un point important.",
                "La virgule est facultative.",
                "Deux points de vue différents.",
                "Le point de vue est clair.",
                "Il a marqué deux points.",
                "Mettez une virgule ici.",
                "Entre guillemets c'est faux.",
                "Un tiret suffit.",
                "C'est mon point fort.",
                "On fait le point demain.",
                "La cuisson à point.",
            ]),
            ("de-DE", [
                "Das ist ein wichtiger Punkt.",
                "Das Komma ist optional.",
                "Der Punkt ist, dass wir gehen.",
                "Wir treffen uns um Punkt acht.",
                "Das ist der springende Punkt.",
                "Ohne Komma geht es nicht.",
                "Zwei Komma fünf Prozent.",
                "Ein Fragezeichen bleibt.",
                "Setz einen Punkt dahinter.",
                "Das war ein guter Punkt.",
                "Der Doppelpunkt fehlt.",
                "Er hat ein Komma vergessen.",
            ]),
            ("it-IT", [
                "Il punto è questo.",
                "Un punto di vista diverso.",
                "La virgola è facoltativa.",
                "Ho fatto due punti.",
                "È un buon punto.",
                "Siamo al punto di partenza.",
                "Metti una virgola qui.",
                "Lo ha detto tra virgolette.",
                "Il trattino è corto.",
                "Facciamo il punto della situazione.",
            ]),
            ("pt-BR", [
                "O ponto de vista é claro.",
                "Esse é o ponto principal.",
                "A vírgula é opcional.",
                "Ganhamos por dois pontos.",
                "Esse é um bom ponto.",
                "Ele falou entre aspas.",
                "Coloque um hífen aqui.",
                "Estou no ponto de ônibus.",
                "Chegamos ao ponto final da viagem.",
                "Faltou uma vírgula.",
            ]),
        ]
        XCTAssertGreaterThanOrEqual(english.count, 60)
        for sentence in english {
            XCTAssertEqual(punctuated(sentence), sentence)
        }
        for (language, sentences) in others {
            for sentence in sentences {
                XCTAssertEqual(punctuated(sentence, language), sentence, language)
            }
        }
        // Numbers: a decimal or a time stays as said.
        XCTAssertEqual(punctuated("It costs two comma five million"), "It costs two comma five million")
        XCTAssertEqual(punctuated("The meeting is at 3 colon 30"), "The meeting is at 3 colon 30")
        XCTAssertEqual(punctuated("Tres coma cinco", "es-ES"), "Tres coma cinco")
        XCTAssertEqual(punctuated("Zwei Komma fünf Prozent", "de-DE"), "Zwei Komma fünf Prozent")
    }

    /// A closing name needs an open quote or bracket; a bare "quote" needs a closing one.
    func testQuotesAndBracketsPair() {
        XCTAssertEqual(punctuated("Close quote that"), "Close quote that")
        XCTAssertEqual(punctuated("He was fine close paren"), "He was fine close paren")
        XCTAssertEqual(punctuated("Can you quote me on that"), "Can you quote me on that")
        XCTAssertEqual(punctuated("He said \"hi end quote"), "He said \"hi\"", "a straight quote closes straight")
        XCTAssertEqual(punctuated("Lo dijo entre comillas", "es-ES"), "Lo dijo entre comillas")
    }

    func testOtherLanguagesStayAsSaid() {
        XCTAssertEqual(punctuated("Hallo komma", "nl-NL"), "Hallo komma")
        XCTAssertEqual(punctuated("你好逗号", "zh-CN"), "你好逗号")
    }

    /// Polished, Casual and Notes write the marks, Verbatim keeps the words, and the pipeline
    /// writes them only with the switch on (on by default).
    func testStylesAndPipeline() {
        let heard = "Okay comma so here is the plan colon we leave at noon period see you there exclamation point"
        let formatted = DictationStyle.allCases.map {
            TranscriptFormatter.format(heard, style: $0, language: "en-US", punctuation: true)
        }
        XCTAssertEqual(formatted, [
            "Okay, so here is the plan: we leave at noon. See you there!",
            "okay so here is the plan: we leave at noon. see you there!",
            "- So here is the plan: we leave at noon\n- See you there!",
            heard,
        ])
        XCTAssertEqual(
            TranscriptFormatter.format("love you smiley face", style: .polished, language: "en-US", punctuation: true),
            "Love you :-)"
        )

        var pipeline = DictationPipeline(style: .polished, language: "en-US")
        XCTAssertEqual(pipeline.format("Wow exclamation point", language: "en-US"), "Wow exclamation point.", "off")
        pipeline.spokenPunctuation = true
        XCTAssertEqual(pipeline.format("Wow exclamation point", language: "en-US"), "Wow!")
        pipeline.formatLists = true
        XCTAssertEqual(
            pipeline.format("Hi Sarah comma new line thanks for the update period", language: "en-US"),
            "Hi Sarah,\nThanks for the update."
        )
        XCTAssertEqual(
            pipeline.format("Groceries, dash, eggs, dash, milk.", language: "en-US"),
            "Groceries:\n- Eggs.\n- Milk.",
            "a spoken list's dashes stay bullets"
        )
        pipeline.lowercase = true
        XCTAssertEqual(pipeline.format("Wow exclamation point", language: "en-US"), "wow!")

        let defaults = UserDefaults(suiteName: "hex.tests.\(UUID().uuidString)")!
        let settings = DictationPunctuationSettings(defaults: defaults)
        XCTAssertTrue(settings.isEnabled, "on by default")
        settings.isEnabled = false
        XCTAssertFalse(DictationPunctuationSettings(defaults: defaults).isEnabled)
    }
}
