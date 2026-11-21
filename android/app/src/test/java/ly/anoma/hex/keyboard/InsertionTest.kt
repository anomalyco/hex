package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Test
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.speech.SpokenCleanup
import ly.anoma.hex.speech.SpokenText
import ly.anoma.hex.speech.SpeechOptions
import ly.anoma.hex.speech.Style

class InsertionTest {
    @Test fun replacesASelectedWordWithoutJoiningItsNeighbours() {
        assertEquals(" hello ", insertion(" hello ", 'a', 'b'))
    }
    @Test fun respectsWhitespaceAndClosingPunctuationInTheEditor() {
        assertEquals("Hello", insertion("  Hello ", '\n', '!'))
        assertEquals("hello", insertion("hello", '(', ')'))
        assertEquals(", yes", insertion(", yes", 'e', null))
    }
    @Test fun emptyResultMakesNoEdit() {
        assertEquals("", insertion(" \n ", 'a', 'b'))
    }

    @Test fun stylesChangeWhatReachesTheEditor() {
        fun render(text: String, style: Style) = insertion(
            SpokenText.format(text, SpeechOptions(style, Spoken.English, true, false)), null, null)
        assertEquals("Can you send it Thursday.", render("Can you, um, send it Tuesday, no wait, Thursday.", Style.Polished))
        assertEquals("see you soon", render("Um, see you soon.", Style.Casual))
        assertEquals("- Buy milk\n- Call Sam", render("bullet buy milk next bullet call Sam", Style.Notes))
        assertEquals("Um, Tuesday, sorry, Thursday.", render("Um, Tuesday, sorry, Thursday.", Style.Verbatim))
        assertEquals("Das ist eh egal.", SpokenText.format("Äh, das ist eh egal.",
            SpeechOptions(Style.Polished, Spoken.German, false, false)))
    }
    @Test fun removesOnlyAnchoredRepairsAndPreservesNegation() {
        assertEquals("Meet at 4pm?", SpokenCleanup.clean("Meet at 3pm, actually 4pm?", "en"))
        assertEquals("I didn't say Friday.", SpokenCleanup.clean("I didn't say Friday.", "en"))
        assertEquals("I want the red one, sorry, eventually.", SpokenCleanup.clean("I want the red one, sorry, eventually.", "en"))
        assertEquals("I sent the invoice, sorry I am late.", SpokenCleanup.clean("I sent the invoice, sorry I am late.", "en"))
        assertEquals("It isn’t blue, I mean, it is green.", SpokenCleanup.clean("It isn’t blue, I mean, it is green.", "en"))
        assertEquals("Uno\ndos", SpokenCleanup.clean("Uno\ndos", "es"))
        assertEquals("eBay works.", SpokenCleanup.clean("Um, eBay works.", "en"))
        assertEquals("Te digo que no puedo ir.", SpokenCleanup.clean("Te digo que no puedo ir.", "es"))
        assertEquals("¿El miércoles?", SpokenCleanup.clean("¿El martes, perdón, el miércoles?", "es"))
        assertEquals("Am Mittwoch.", SpokenCleanup.clean("Am Dienstag, ich meine, Mittwoch.", "de"))
        assertEquals("Det er torsdag.", SpokenCleanup.clean("Det er tirsdag, nei, torsdag.", "no"))
    }

    // iOS TranscriptCleanupTests: the styles, End with a period and Lowercase.
    private fun styled(text: String, style: Style = Style.Polished, language: String? = null, period: Boolean = true) =
        SpokenText.style(text, style, language, period)
    private fun pipeline(text: String, style: Style, spoken: Spoken = Spoken.English, lowercase: Boolean = false) =
        SpokenText.format(text, SpeechOptions(style, spoken, false, false, lowercase))

    @Test fun stylesKeepAnAddressWhole() {
        assertEquals("john@gmail.com", styled("john@gmail.com"))
        assertEquals("Send it to john@gmail.com.", styled("send it to john@gmail.com"))
        assertEquals("john@gmail.com is mine. Call me.", styled("john@gmail.com is mine. call me"))
        assertEquals("mail john@gmail.com", styled("Mail john@gmail.com", Style.Casual))
        assertEquals("- john@gmail.com\n- Call Ravi", styled("bullet john@gmail.com bullet call Ravi", Style.Notes))
        assertEquals("Hello. World! Ok? Yes.", styled("Hello. world! ok? yes"))
    }
    @Test fun stylesTrimSpanishAndGermanHesitations() {
        assertEquals("Creo que sí, pero no estoy seguro.", styled("Mmm, creo que sí, eh, pero no estoy seguro.", language = "es-ES"))
        assertEquals("¿Qué tal? Padre e hijo.", styled("Eh, ¿qué tal? Padre e hijo.", language = "es-ES"))
        assertEquals("Das ist eh egal.", styled("Äh, das ist eh egal.", language = "de-DE"))
        assertEquals("Eh, nos vemos.", styled("Eh, nos vemos.", Style.Verbatim, "es-ES"))
    }
    @Test fun stylesDifferOnOrdinaryShortDictation() {
        fun all(text: String) = Style.entries.map { styled(text, it) }
        assertEquals(listOf("Sounds good, see you at 5.", "sounds good, see you at 5", "Sounds good, see you at 5",
            "Sounds good, see you at 5."), all("Sounds good, see you at 5."))
        assertEquals(listOf("Buy milk. Call the dentist. Book the flights.", "buy milk. call the dentist. book the flights",
            "- Buy milk\n- Call the dentist\n- Book the flights", "Um, buy milk. Call the dentist. Book the flights."),
            all("Um, buy milk. Call the dentist. Book the flights."))
        assertEquals(listOf("I'll be there in 10.", "i'll be there in 10", "I'll be there in 10", "I'll be there in 10."),
            all("I'll be there in 10."))
        assertEquals(listOf("I'm running late start without me.", "i'm running late start without me",
            "I'm running late start without me", "i'm running late start without me"), all("i'm running late start without me"))
    }
    @Test fun polishedWritesFullSentences() {
        assertEquals("I think I'll go, it's fine.", styled("erm i think i'll go, it's fine"))
        assertEquals("First line.\nSecond line.", styled("first line\nsecond line"))
        assertEquals("Shopping list:", styled("Shopping list:"))
        assertEquals("好的，明天见。", styled("好的，明天见。", language = "zh-CN"))
        assertEquals("好的，明天见。", styled("好的，明天见", language = "zh-CN"))
        assertEquals("I ragazzi.", styled("i ragazzi", language = "it-IT"))
        assertEquals("Dimmi i nomi.", styled("dimmi i nomi", language = "it-IT"))
    }
    @Test fun casualTexts() {
        assertEquals("yeah i'll text John about the iPhone order tomorrow",
            styled("Yeah, I'll text John about the iPhone order tomorrow.", Style.Casual))
        assertEquals("i think i can, i.e. maybe", styled("I think I can, i.e. maybe.", Style.Casual))
        assertEquals("ok can you send me the deck?", styled("OK, can you send me the deck?", Style.Casual))
        assertEquals("NASA launched it. wow!", styled("NASA launched it. Wow!", Style.Casual))
        assertEquals("iPhone is charged", styled("iPhone is charged.", Style.Casual))
        assertEquals("one. two. three. four.", styled("One. Two. Three. Four.", Style.Casual))
        assertEquals("hi.\nsee you.", styled("Hi.\nSee you.", Style.Casual))
        assertEquals("sí, claro", styled("Sí, claro.", Style.Casual, "es-ES"))
        assertEquals("好的，明天见", styled("好的，明天见。", Style.Casual, "zh-CN"))
    }
    @Test fun notesListSeveralItemsConcisely() {
        assertEquals("Call the dentist tomorrow at 9", styled("Call the dentist tomorrow at 9.", Style.Notes))
        assertEquals("Can you send the deck?", styled("Can you send the deck?", Style.Notes))
        assertEquals("I'll call Ravi", styled("Yeah, I'll call Ravi.", Style.Notes))
        assertEquals("- Buy milk\n- Call the dentist", styled("So, first, buy milk. And then call the dentist.", Style.Notes))
        assertEquals("Shopping list:\n- Eggs\n- Milk\n- Bread", styled("Shopping list: eggs, milk, and bread.", Style.Notes))
        assertEquals("- Dr. Patel called\n- Call her back", styled("Dr. Patel called. Call her back.", Style.Notes))
        assertEquals("Note: the build is green", styled("Note: the build is green.", Style.Notes))
        assertEquals("Todo\n- Ship it. Then tag it\n- Tell Ravi", styled("todo bullet ship it. Then tag it bullet tell Ravi", Style.Notes))
        assertEquals("- 买牛奶\n- 给妈妈打电话", styled("买牛奶。给妈妈打电话。", Style.Notes, "zh-CN"))
    }
    @Test fun endWithAPeriodOff() {
        fun open(text: String, style: Style = Style.Polished, language: String = "en-US") = styled(text, style, language, false)
        assertEquals("I think I'll go, it's fine", open("erm i think i'll go, it's fine"))
        assertEquals("Sounds good. See you at 5", open("Sounds good. See you at 5."))
        assertEquals("Can you send the deck?", open("Can you send the deck?"))
        assertEquals("That's great!", open("That's great!"))
        assertEquals("He said \"see you later\"", open("He said \"see you later.\""))
        assertEquals("Wait...", open("Wait..."))
        assertEquals("Bring eggs, milk, etc.", open("Bring eggs, milk, etc."))
        assertEquals("We moved to the U.S.", open("We moved to the U.S."))
        assertEquals("Email john@gmail.com", open("Email john@gmail.com."))
        assertEquals("First line\nSecond line", open("first line\nsecond line."))
        assertEquals("好的，明天见", open("好的，明天见。", Style.Polished, "zh-CN"))
        assertEquals("好的，明天见", open("好的，明天见", Style.Polished, "zh-CN"))
        assertEquals("你今天来吗？", open("你今天来吗？", Style.Polished, "zh-CN"))
        assertEquals("太好了！", open("太好了！", Style.Polished, "zh-CN"))
        assertEquals("我到了。你在哪里", open("我到了。你在哪里。", Style.Polished, "zh-CN"))
        assertEquals("「わかりました」", open("「わかりました。」", Style.Polished, "ja-JP"))
        assertEquals("one. two. three. four", open("One. Two. Three. Four.", Style.Casual))
        assertEquals("hi\nsee you", open("Hi.\nSee you.", Style.Casual))
        assertEquals("Call the dentist tomorrow at 9", open("Call the dentist tomorrow at 9.", Style.Notes))
        assertEquals("Um, so it's done.", open("Um, so it's done.", Style.Verbatim))
        assertEquals("Sounds good.", styled("Sounds good", period = true))
    }
    @Test fun otherScriptsKeepTheirLettersAndLowercase() {
        assertEquals("Ґанок і сад. І все.", styled("ґанок і сад. і все", language = "uk-UA"))
        assertEquals("ґанок і сад. і все", styled("Ґанок і сад. І все.", Style.Casual, "uk-UA"))
        assertEquals("Żółw i jeż. I gęś.", styled("żółw i jeż. i gęś", language = "pl-PL"))
        assertEquals("łabędź i jeż. ósma", styled("Łabędź i jeż. Ósma.", Style.Casual, "pl-PL"))
        assertEquals("čau. úterý", styled("Čau. Úterý.", Style.Casual, "cs-CZ"))
        assertEquals("όλα καλά. έλα αύριο", styled("Όλα καλά. Έλα αύριο.", Style.Casual, "el-GR"))
        assertEquals("Dzień dobry spóźnię się 10 min będę w domu o 19:30 zacznijcie beze mnie.",
            styled("dzień dobry spóźnię się 10 min będę w domu o 19:30 zacznijcie beze mnie", language = "pl-PL"))
        assertEquals("text john about the iphone.", pipeline("Text JOHN about the IPHONE", Style.Polished, lowercase = true))
    }
    @Test fun lowercaseRunsAfterTheStyle() {
        val heard = "Yeah, text John about the iphone order."
        assertEquals("Yeah, text John about the iphone order.", pipeline(heard, Style.Polished))
        assertEquals("yeah, text john about the iphone order.", pipeline(heard, Style.Polished, lowercase = true))
        assertEquals("yeah text john about the iphone order", pipeline(heard, Style.Casual, lowercase = true))
        assertEquals("yeah, text john about the iphone order.", pipeline(heard, Style.Verbatim, lowercase = true))
        assertEquals("der fluss", pipeline("Der Fluss", Style.Casual, Spoken.German, lowercase = true))
    }
    @Test fun polishedWritesOutInformalContractions() {
        assertEquals("I'm going to call you later. I want to finish this first.",
            styled("I'm gonna call you later. I wanna finish this first.", language = "en-US"))
        assertEquals("I got to go, let me know. It's kind of late.", styled("I gotta go, lemme know. It's kinda late", language = "en-US"))
        assertEquals("Going to be a long day.", styled("Gonna be a long day.", language = "en-US"))
        assertEquals("Ok. Want to grab lunch?", styled("ok. wanna grab lunch?", language = "en-US"))
        assertEquals("He said \"I'm gonna win\" and left.", styled("He said \"I'm gonna win\" and left.", language = "en-US"))
        assertEquals("She sang “Wanna Be Startin' Somethin'” all night.",
            styled("She sang “Wanna Be Startin' Somethin'” all night.", language = "en-US"))
        assertEquals("We played Gonna Fly Now on repeat.", styled("We played Gonna Fly Now on repeat.", language = "en-US"))
        assertEquals("The gonnabe kinds and the wannabes.", styled("The gonnabe kinds and the wannabes.", language = "en-US"))
        assertEquals("Voy a gonna mañana.", styled("Voy a gonna mañana.", language = "es-ES"))
        val heard = "I'm gonna be late, wanna start without me?"
        assertEquals("i'm gonna be late, wanna start without me?", styled(heard, Style.Casual, "en-US"))
        assertEquals(heard, styled(heard, Style.Verbatim, "en-US"))
    }

    // The space bar (SpaceGesture, CursorSlide). These belong in a KeyboardGestureTest once new test files are agreed.
    @Test fun quickSpaceTapsThatDriftStillTypeTheirSpace() {
        for (ms in listOf(60L, 100L, 150L)) for (drift in listOf(8f, 21f, 42f, 63f)) {
            val space = SpaceGesture(start = 45f, step = 33f, stillSlop = 21f)
            space.down(500f, 50f, 0)
            for (t in 10L..ms step 10L) assertEquals(null, space.move(500f - drift * t / ms, 50f, t))
            assertEquals(true, space.up())
        }
    }
    @Test fun aSpaceSlideMovesTheCursorAtGboardsSteps() {
        val space = SpaceGesture(start = 45f, step = 33f, stillSlop = 21f)
        space.down(500f, 50f, 0)
        assertEquals(null, space.move(458f, 50f, 300))
        assertEquals(-1, space.move(455f, 50f, 310))
        assertEquals(-3, space.move(374f, 50f, 320))
        assertEquals(-15, space.move(-25f, 50f, 330))
        assertEquals(2, space.move(580f, 50f, 340))
        assertEquals(false, space.up())
    }
    @Test fun aCursorSlideStepsOverWholeCharacters() {
        val moves = ArrayList<Int>()
        val text = "ab\uD83D\uDC4D\uD83C\uDFFDcd"
        val target = object : KeyboardTextTarget {
            override val connected = true
            override fun before(length: Int) = text
            override fun after(length: Int) = ""
            override fun selected() = ""
            override fun capsMode(mask: Int) = 0
            override fun commit(text: String) = true
            override fun deleteBefore(length: Int) = true
            override fun deleteCodePointsBefore(count: Int) = true
            override fun beginBatch() {}
            override fun endBatch() {}
            override fun setSelection(start: Int, end: Int) = moves.add(start)
            override fun performEditorAction(id: Int) = true
            override fun defaultEditorAction() = true
            override fun sendKey(code: Int) {}
        }
        val slide = CursorSlide(target, text.length, text.length, readable = true)
        for (n in listOf(-1, -2, -3, -4, -5, -9, 1)) slide.moveTo(n)
        assertEquals(listOf(7, 6, 2, 1, 0, 8), moves)
    }
    @Test fun aDeleteSlideSelectsWholeWordsWithTheirSpaces() {
        assertEquals(listOf(4, 9, 14), DeleteSlide.wordLengths("far away from home").take(3))
        assertEquals(listOf(7, 14), DeleteSlide.wordLengths("Hello, world. ").toList())
        assertEquals(listOf(6), DeleteSlide.wordLengths("don't ").toList())
        val delete = DeleteGesture(start = 140f, step = 95f, cancel = 84f, stillSlop = 21f)
        delete.down(1000f)
        assertEquals(0, delete.move(940f)); assertEquals(true, delete.tap)
        assertEquals(0, delete.move(870f)); assertEquals(false, delete.tap)
        assertEquals(1, delete.move(840f)); assertEquals(2, delete.move(750f)); assertEquals(0, delete.move(900f))
    }
    @Test fun shiftChordsLockAndAutoCapitals() {
        val shift = ShiftState()
        shift.down(0); shift.typed(); shift.typed(); shift.up()
        assertEquals(false, shift.uppercase)                 // a chord: capitals while held, off after
        shift.down(1000); shift.up(); shift.down(1200); shift.up()
        assertEquals(true, shift.locked)                     // two taps within 300 ms
        shift.typed(); assertEquals(true, shift.uppercase)
        shift.down(5000); shift.up(); assertEquals(false, shift.uppercase)
        shift.down(6000); shift.up(); shift.down(6400); shift.up()
        assertEquals(false, shift.locked)                    // 400 ms apart: two single shifts
        shift.auto(true); assertEquals(true, shift.uppercase)
        shift.typed(); assertEquals(false, shift.uppercase)
    }
    @Test fun spacesAndMarksFollowGboard() {
        assertEquals(true, SpaceRules.periodFits("Hi "))
        assertEquals(true, SpaceRules.periodFits("at 5 "))
        assertEquals(false, SpaceRules.periodFits("Hi. "))
        assertEquals(false, SpaceRules.periodFits(" "))
        assertEquals(true, SpaceRules.separates(" fox"))
        assertEquals(true, SpaceRules.separates(", fox"))
        assertEquals(false, SpaceRules.separates("\nfox"))
        assertEquals(false, SpaceRules.separates("fox"))
        assertEquals(false, SpaceRules.separates(null))
    }
}
