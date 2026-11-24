package ly.anoma.hex.keyboard

import java.net.ServerSocket
import java.util.Locale
import kotlin.concurrent.thread
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ly.anoma.hex.feedback.FeedbackContext
import ly.anoma.hex.feedback.FeedbackDraft
import ly.anoma.hex.feedback.FeedbackFailure
import ly.anoma.hex.feedback.FeedbackKind
import ly.anoma.hex.feedback.FeedbackOutcome
import ly.anoma.hex.feedback.FeedbackRequest
import ly.anoma.hex.feedback.FeedbackRetry
import ly.anoma.hex.feedback.FeedbackSender
import ly.anoma.hex.speech.DictationLanguage
import ly.anoma.hex.speech.PhoneLanguage
import ly.anoma.hex.speech.FrenchSpacing
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.speech.SpokenCleanup
import ly.anoma.hex.speech.SpokenPunctuation
import ly.anoma.hex.speech.SpokenText
import ly.anoma.hex.speech.SpeechOptions
import ly.anoma.hex.speech.Style
import ly.anoma.hex.speech.typingLayout
import ly.anoma.hex.speech.phoneLanguage
import ly.anoma.hex.speech.SpeechEdges
import ly.anoma.hex.speech.speechSpan

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
            SpokenText.format(text, SpeechOptions(style, DictationLanguage.English, true, false)), null, null)
        assertEquals("Can you send it Thursday.", render("Can you, um, send it Tuesday, no wait, Thursday.", Style.Polished))
        assertEquals("see you soon", render("Um, see you soon.", Style.Casual))
        assertEquals("- Buy milk\n- Call Sam", render("bullet buy milk next bullet call Sam", Style.Notes))
        assertEquals("Um, Tuesday, sorry, Thursday.", render("Um, Tuesday, sorry, Thursday.", Style.Verbatim))
        assertEquals("Das ist eh egal.", SpokenText.format("Äh, das ist eh egal.",
            SpeechOptions(Style.Polished, DictationLanguage.German, false, false)))
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
    private fun pipeline(text: String, style: Style, spoken: DictationLanguage = DictationLanguage.English, lowercase: Boolean = false) =
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
        assertEquals("der fluss", pipeline("Der Fluss", Style.Casual, DictationLanguage.German, lowercase = true))
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
    // iOS TranscriptCleanupTests: spoken "@" and counts (English only, every style but Verbatim).
    @Test fun spokenAtSignsBecomeAddresses() {
        for ((spoken, written) in listOf(
            "john at the rate gmail dot com" to "john@gmail.com",
            "Mail it to John at the rate Gmail dot com." to "Mail it to john@gmail.com.",
            "My email is priya dot sharma at the rate company dot co dot in, call me too" to
                "My email is priya.sharma@company.co.in, call me too",
            "Write to support at the rate hex.app today" to "Write to support@hex.app today",
            "rahul123 at sign yahoo dot com" to "rahul123@yahoo.com",
            "At the rate gmail dot com addresses work" to "@gmail.com addresses work",
            "Type the at sign, then your name" to "Type the @, then your name",
            "Press at sign." to "Press @.",
            "ravi at the rate, gmail dot com" to "ravi@gmail.com",
            "My email is John at the rategmail.com." to "My email is john@gmail.com.",
            "Send it to Priya at signyahoo.com." to "Send it to priya@yahoo.com.",
            "My email is John at the rate gmail.com" to "My email is john@gmail.com",
            "My email is john at therategmail.com." to "My email is john@gmail.com.",
        )) assertEquals(spoken, written, SpokenCleanup.symbols(spoken, "en-IN"))
    }
    @Test fun atInItsOwnMeaningStays() {
        for (spoken in listOf("Prices are growing at the rate of 5% a year.", "At the rate of knots, we'll be there soon.",
            "We are spending at the rate we planned.", "At the rate you're going, you'll finish early.",
            "Meet me at the office at 5.", "Look at that dot on the map.", "I'm good at sign language.",
            "john at the rate gmail", "Send it at the rate", "Wait at the gate dot com", "Send it to priya@yahoo.com",
            "My email is john at gmail.com.", "Prices Are growing at the rate of5% a year",
            "Rates went up at the ratepayers' meeting.")) assertEquals(spoken, SpokenCleanup.symbols(spoken, "en-IN"))
        assertEquals("john at the rate gmail dot com", SpokenCleanup.symbols("john at the rate gmail dot com", "es"))
    }
    @Test fun spokenCountsBecomeDigits() {
        for ((heard, written) in listOf("Test one, 2, 3." to "Test 1, 2, 3.", "Test one, two, three." to "Test 1, 2, 3.",
            "Test, one, two, three." to "Test, 1, 2, 3.", "Testing one two three." to "Testing 1, 2, 3.",
            "Testing, testing, one, 2, 3." to "Testing, testing, 1, 2, 3.", "One, two, three, testing." to "1, 2, 3, testing.",
            "Counting one, two, three, four." to "Counting 1, 2, 3, 4.", "One, two, three, go!" to "1, 2, 3, go!",
            "Test 1, 2, 3" to "Test 1, 2, 3", "Test 123" to "Test 1, 2, 3", "Testing 123." to "Testing 1, 2, 3.",
            "Testing testing 123" to "Testing testing 1, 2, 3", "Test, 123" to "Test, 1, 2, 3",
            "Testing 1234, can you hear me?" to "Testing 1, 2, 3, 4, can you hear me?",
        )) assertEquals(heard, written, SpokenCleanup.counts(heard, "en-US"))
    }
    @Test fun otherNumbersStayAsSaid() {
        for (heard in listOf("I have two dogs and three cats.", "Chapter one, two and three are done.", "We'll meet at 1, 2, or 3.",
            "One, two, buckle my shoe. Three, four, shut the door.", "Two, three, four, five.", "Test one, two.",
            "Zero, one, two, three.", "Twenty one two three.", "One. Two. Three.", "Test. 1. 2. 3.", "Room 123 is free.",
            "Test 123 passed.", "This is a test. 123 people came.", "Test 132.", "Counting 1234", "Call 1-2-3 now.",
        )) assertEquals(heard, SpokenCleanup.counts(heard, "en-US"))
        assertEquals("Prueba uno, dos, tres.", SpokenCleanup.counts("Prueba uno, dos, tres.", "es"))
    }
    @Test fun symbolsAndCountsRunInTheStylesButNotVerbatim() {
        assertEquals("Mail it to john@gmail.com.", pipeline("mail it to John at the rate Gmail dot com", Style.Polished))
        assertEquals("testing 1, 2, 3", pipeline("Testing one two three.", Style.Casual))
        assertEquals("john at the rate gmail dot com", pipeline("john at the rate gmail dot com", Style.Verbatim))
        assertEquals("Test 1, 2, 3.", SpokenText.format("Test one, two, three.", SpeechOptions(Style.Polished, DictationLanguage.English, true, false)))
    }

    // iOS TranscriptCleanupTests: Format lists and Spoken punctuation inside the styles.
    private fun spoken(text: String, style: Style, lists: Boolean = true, punctuation: Boolean = true, period: Boolean = true,
                       lowercase: Boolean = false, spoken: DictationLanguage = DictationLanguage.English) =
        SpokenText.format(text, SpeechOptions(style, spoken, false, false, lowercase, period, lists, punctuation))

    @Test fun listsInsideTheStyles() {
        val plan = "Um, so the plan is, first, um, call the bank, second, pay rent. Thanks!"
        assertEquals("So the plan is:\n1. Call the bank.\n2. Pay rent.\nThanks!", spoken(plan, Style.Polished))
        assertEquals("so the plan is:\n1. Call the bank\n2. Pay rent\nthanks!", spoken(plan, Style.Casual))
        assertEquals("So the plan is:\n1. Call the bank\n2. Pay rent\nThanks!", spoken(plan, Style.Notes))
        assertEquals(plan, spoken(plan, Style.Verbatim))
        assertEquals("So the plan is, first, call the bank, second, pay rent. Thanks!", spoken(plan, Style.Polished, lists = false))
        assertEquals("So the plan is:\n1. Call the bank\n2. Pay rent\nThanks!", spoken(plan, Style.Polished, period = false))
        assertEquals("Groceries:\n- Eggs.\n- Milk.", spoken("Groceries, bullet, eggs, bullet, milk.", Style.Polished))
        assertEquals("Groceries:\n- Eggs\n- Milk", spoken("Groceries, bullet, eggs, bullet, milk.", Style.Polished, period = false))
        assertEquals("首先，买牛奶。其次，给妈妈打电话", SpokenText.style("首先，买牛奶。其次，给妈妈打电话。", Style.Polished, "zh-CN", false, true))
        for ((style, written) in listOf(Style.Polished to "Test 1, 2, 3.", Style.Casual to "test 1, 2, 3",
            Style.Notes to "Test 1, 2, 3", Style.Verbatim to "Test 1, 2, 3")) assertEquals(written, spoken("Test 1, 2, 3", style))
        assertEquals("- Eggs\n- Milk", spoken("First, eggs. Second, milk.", Style.Notes, lists = false))
        assertEquals("1. Eggs\n2. Milk", spoken("First, eggs. Second, milk.", Style.Notes))
        assertEquals("1. eggs\n2. milk", spoken("First, eggs. Second, milk.", Style.Notes, lowercase = true))
        assertEquals("1. Going to call Sam.\n2. Want to book the hotel.",
            spoken("First, gonna call Sam. Second, wanna book the hotel.", Style.Polished))
        assertEquals("Todo:\n- Got to pack.\n- Let me check the car.", spoken("Todo bullet gotta pack bullet lemme check the car", Style.Polished))
        assertEquals("Mi lista de compras.\n1. Huevos.\n2. Leche.\n3. Pan.",
            spoken("Mi lista de compras. Primero, huevos, segundo, leche, tercero, pan.", Style.Polished, spoken = DictationLanguage.Spanish))
        assertEquals("The first time I saw it, I loved it.", spoken("The first time I saw it, I loved it.", Style.Polished))
    }
    @Test fun spokenPunctuationInsideTheStyles() {
        val plan = "Okay comma so here is the plan colon we leave at noon period see you there exclamation point"
        assertEquals("Okay, so here is the plan: we leave at noon. See you there!", spoken(plan, Style.Polished))
        assertEquals("okay so here is the plan: we leave at noon. see you there!", spoken(plan, Style.Casual))
        assertEquals("- So here is the plan: we leave at noon\n- See you there!", spoken(plan, Style.Notes))
        assertEquals(plan, spoken(plan, Style.Verbatim))
        assertEquals("Love you :-)", spoken("love you smiley face", Style.Polished))
        assertEquals("Wow exclamation point.", spoken("Wow exclamation point", Style.Polished, punctuation = false))
        assertEquals("Wow!", spoken("Wow exclamation point", Style.Polished))
        assertEquals("Hi Sarah,\nThanks for the update.", spoken("Hi Sarah comma new line thanks for the update period", Style.Polished))
        assertEquals("Groceries:\n- Eggs.\n- Milk.", spoken("Groceries, dash, eggs, dash, milk.", Style.Polished))
        assertEquals("wow!", spoken("Wow exclamation point", Style.Polished, lowercase = true))
        assertEquals("Llego mañana. Nos vemos.", spoken("Llego mañana, punto nos vemos", Style.Polished, spoken = DictationLanguage.Spanish))
        assertEquals("Hi Sarah, how are you?", SpokenPunctuation.format("Hi Sarah comma how are you question mark", "en"))
        assertEquals("Wir sehen uns morgen, okay?", spoken("Wir sehen uns morgen Komma okay Fragezeichen", Style.Polished, spoken = DictationLanguage.German))
        for (prose in listOf("A period of time passed.", "The comma is wrong.", "Use the Oxford comma.", "He has a question mark tattoo.",
            "Two comma five.", "Colon cancer is treatable.")) assertEquals(prose, SpokenPunctuation.format(prose, "en"))
    }

    // iOS KeyboardBridgeTests: dictation continuing a sentence starts lowercase.
    @Test fun dictationContinuingASentenceStartsLowercase() {
        val words = setOf("that's", "the", "self", "a", "don't")
        fun insert(text: String, before: String?) =
            insertion(SpokenText.continuing(text, before, "en-US") { it in words }, before?.lastOrNull(), null)
        assertEquals(" that's great.", insert("That's great.", "I think"))
        assertEquals("the plan works.", insert("The plan works.", "and so "))
        assertEquals(" self-driving cars.", insert("Self-driving cars.", "I like"))
        assertEquals(" a dog.", insert("A dog.", "We saw"))
        for (before in listOf(null, "", "   ", "Hello.", "Hello. ", "Really?", "Yes!", "Wait…", "Line one\n", "He said \"stop.\" ")) {
            assertEquals(true, SpokenText.startsSentence(before))
            assertEquals("The end.", insert("The end.", before).trim())
        }
        assertEquals(false, SpokenText.startsSentence("Note: "))
        assertEquals(false, SpokenText.startsSentence("see (this)"))
        assertEquals(" I think so.", insert("I think so.", "and"))
        assertEquals(" I'm here.", insert("I'm here.", "and"))
        assertEquals(" I’ll go.", insert("I’ll go.", "and"))
        assertEquals(" NASA launched it.", insert("NASA launched it.", "and"))
        assertEquals(" McDonald's is open.", insert("McDonald's is open.", "and"))
        assertEquals(" Sarah said so.", insert("Sarah said so.", "and"))
        assertEquals(" iPhone works.", insert("iPhone works.", "my"))
        // German keeps the case; the built-in word lists know no names.
        assertEquals("Das Haus", SpokenText.continuing("Das Haus", "und", "de-DE") { true })
        assertEquals("the plan", SpokenText.continuing("The plan", "and", "en"))
        assertEquals("Sarah said", SpokenText.continuing("Sarah said", "and", "en"))
        assertEquals("que sí", SpokenText.continuing("Que sí", "Creo", "es"))
        assertEquals("Madrid es", SpokenText.continuing("Madrid es", "en", "es"))
    }

    // iOS TranscriptCleanupTests and CustomVocabularyTests: vocabulary before cleanup and after the style.
    @Test fun vocabularySpellsBeforeAndAfterTheStyle() {
        fun spelled(text: String, style: Style, lowercase: Boolean = false, terms: List<String> = listOf("iPhone")) =
            SpokenText.format(text, SpeechOptions(style, DictationLanguage.English, false, false, lowercase, vocabulary = terms))
        val heard = "Yeah, text John about the iphone order."
        assertEquals("Yeah, text John about the iPhone order.", spelled(heard, Style.Polished))
        assertEquals("yeah, text john about the iphone order.", spelled(heard, Style.Polished, lowercase = true))
        assertEquals("yeah text john about the iphone order", spelled(heard, Style.Casual, lowercase = true))
        assertEquals("yeah, text john about the iphone order.", spelled(heard, Style.Verbatim, lowercase = true))
        assertEquals("iPhone is charged.", spelled("iphone is charged", Style.Polished))
        assertEquals("Ask about OpenCode and WeChat.", spelled("ask about open code and we chat", Style.Polished, terms = listOf("OpenCode", "WeChat")))
        assertEquals("Ask about open code.", spelled("ask about open code", Style.Polished, terms = emptyList()))
    }

    // iOS FrenchSpacingTests (French isn't a dictation language on Android yet; this is ready for it).
    @Test fun frenchSpacing() {
        val thin = "\u202F"; val nbsp = "\u00A0"
        assertEquals("Bonjour, comment ça va${thin}? Tu viens ce soir${thin}?",
            FrenchSpacing.apply(SpokenText.style("Bonjour, comment ça va ? Tu viens ce soir ?", Style.Polished, "fr-FR"), "fr-FR"))
        assertEquals("Il y a trois choses${nbsp}: le pain${thin}; le lait et «${nbsp}les œufs${nbsp}»${thin}! C'est tout.",
            FrenchSpacing.apply(SpokenText.style("il y a trois choses: le pain; le lait et «les œufs» ! c'est tout", Style.Polished, "fr-FR"), "fr-FR"))
        assertEquals("tu viens ce soir${thin}? génial${thin}!",
            FrenchSpacing.apply(SpokenText.style("Tu viens ce soir ? Génial !", Style.Casual, "fr-FR"), "fr-FR"))
        assertEquals("ça va? Oui! Voici${nbsp}: «${nbsp}tout${nbsp}»", FrenchSpacing.apply("ça va ? Oui ! Voici : «tout»", "fr-CA"))
        assertEquals("ça va${thin}? Voici${nbsp}: «${thin}tout${thin}»", FrenchSpacing.apply("ça va ? Voici : « tout »", "fr-CH"))
        assertEquals(FrenchSpacing.Convention.France, FrenchSpacing.convention("fr-BE"))
        assertEquals(FrenchSpacing.Convention.France, FrenchSpacing.convention("fr"))
        assertEquals(null, FrenchSpacing.convention("en-CA"))
        assertEquals(null, FrenchSpacing.convention(null))
        val once = FrenchSpacing.apply("ça va  ?\nOui${nbsp}! Quoi ?!", "fr-FR")
        assertEquals("ça va${thin}?\nOui${thin}! Quoi${thin}?!", once)
        assertEquals(once, FrenchSpacing.apply(once, "fr-FR"))
        for (text in listOf("Rendez-vous à 10:30 ou 18:45", "Super :) et ;-) puis :D et :(", "Va sur https://exemple.fr/page?q=1",
            "Écris moi@exemple.fr! ou www.exemple.fr?", "Tape \u0060a ? b : c\u0060 puis \u0060x::y\u0060", "? au début\n: en début de ligne"))
            assertEquals(text, FrenchSpacing.apply(text, "fr-FR"))
        assertEquals("Il m'a dit «${nbsp}Tu viens${thin}?${nbsp}»", FrenchSpacing.apply("Il m'a dit «Tu viens?»", "fr-FR"))
        assertEquals("Really? Yes.", SpokenText.format("Really ? Yes", SpeechOptions(Style.Polished, DictationLanguage.English, false, false)))
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
        // A picked word's space (Gboard adds one in Chrome's address bar too): a clause mark takes it anywhere, and in
        // a URL field so do . / and the other joiners ("github." and "github/", never "github /").
        for (mark in ",.!?;:)") {
            assertEquals("$mark", true, SpaceRules.takesPickedSpace(mark, url = false))
            assertEquals("$mark", true, SpaceRules.takesPickedSpace(mark, url = true))
        }
        assertEquals(false, SpaceRules.takesPickedSpace('/', url = false))
        assertEquals(true, SpaceRules.takesPickedSpace('/', url = true))
        assertEquals(true, SpaceRules.takesPickedSpace('-', url = true))
        assertEquals(false, SpaceRules.takesPickedSpace('(', url = true))
        assertEquals(false, SpaceRules.takesPickedSpace('w', url = true))
    }
    // iOS CleanupTaggerTests' English examples and its hand-written set, met with rules (no tagger).
    @Test fun englishCleanupRulesMeetTheTaggersExamples() {
        for ((spoken, cleaned) in listOf(
            "Send the follow-up to Ji-woo, no wait, to Smith-Jones." to "Send the follow-up to Smith-Jones.",
            "Let's ship it Tuesday, no wait, Thursday." to "Let's ship it Thursday.",
            "Can you send it to John, I mean Mike?" to "Can you send it to Mike?",
            "Meet me at 5, no, at 6." to "Meet me at 6.",
            "We need 20, sorry, 25 chairs." to "We need 25 chairs.",
            "I I think we're good to go." to "I think we're good to go.",
            "Can you, um, send me the address again?" to "Can you send me the address again?",
            "It was, like, really long." to "It was really long.",
            "Um, tell Hye-jin I said thanks." to "Tell Hye-jin I said thanks.",
            "It costs 15 euros, er, 50 euros." to "It costs 50 euros.",
            "It's on the third floor, uh, fourth floor." to "It's on the fourth floor.",
            "We'll need about 20 chairs, make that 25 chairs." to "We'll need about 25 chairs.",
            "We'll need about 20 chairs. Make that 25 chairs." to "We'll need about 25 chairs.",
            "Tell Priya the deadline is the 14th, I meant the 15th." to "Tell Priya the deadline is the 15th.",
            "I'm landing in Denver, no, Dallas around noon." to "I'm landing in Dallas around noon.",
            "I'm landing in Denver, no Dallas around noon." to "I'm landing in Dallas around noon.",
            "Can you ask Jordan, actually ask Sam to review the PR?" to "Can you ask Sam to review the PR?",
            "Our standup moved to 9:15, correction, 9:45." to "Our standup moved to 9:45.",
            "Send the invoice to Acme, or actually, send it to Globex." to "Send it to Globex.",
            "Add olive oil to the list. Scratch that, add avocado oil." to "Add avocado oil.",
            "My flight number is UA 512, oh sorry, UA 521." to "My flight number is UA 521.",
            "Let's meet at Blue Bottle, wait, no, at Philz." to "Let's meet at Philz.",
            "The kickoff is in Austin, rather, in San Antonio." to "The kickoff is in San Antonio.",
            "I mean, the demo went fine overall." to "The demo went fine overall.",
            "Honestly, I mean, it could've gone worse." to "Honestly, it could've gone worse.",
            "We, we need to talk about the budget." to "We need to talk about the budget.",
            "It's at the, at the corner of 3rd and Pine." to "It's at the corner of 3rd and Pine.",
            "I want to, I want to go to the beach this weekend." to "I want to go to the beach this weekend.",
            "We w- we left." to "We left.",
            "Um, so I'll be at the, the office until 5, actually 6, and then, uh, home." to "So I'll be at the office until 6, and then home.",
            "Hey Ana, uh, quick question, is the, is the review on Thursday, no wait, Friday?" to "Hey Ana, quick question, is the review on Friday?",
        )) assertEquals(spoken, cleaned, SpokenCleanup.clean(spoken, "en"))
        for (prose in listOf("I mean what I say.", "I mean it, you did a great job.", "I'd rather stay in tonight.",
            "Uh oh, I left my wallet at home.", "Bye bye, talk soon!", "I think that that's the right call.",
            "It was a really, really good dinner.", "No, wait for me at the entrance.", "Wait, are you serious?",
            "I meant to call you yesterday.", "Can you make that reservation for four?", "Get two coffees, make that three.",
            "Book a table for four at 7, actually make it five people.", "Rather than driving, let's take the train.",
            "Actually, can you grab some ice on the way?", "It isn’t blue, I mean, it is green.", "Thanks for the gift, sorry for the delay.",
            "Hi Ana, no problem.", "Hey Tim, sorry, I'm late.", "Okay, no, Sam is right."))
            assertEquals(prose, SpokenCleanup.clean(prose, "en"))
    }
    @Test fun automaticDictationFollowsTheKeyboardLayout() {
        assertEquals(DictationLanguage.Spanish, DictationLanguage.automatic(Spoken.Spanish, listOf("en")))
        assertEquals(DictationLanguage.Norwegian, DictationLanguage.automatic(Spoken.Norwegian, listOf("en")))
        assertEquals(DictationLanguage.German, DictationLanguage.automatic(null, listOf("ja", "de", "en")))
        assertEquals(DictationLanguage.Norwegian, DictationLanguage.automatic(null, listOf("nb")))
        assertEquals(DictationLanguage.English, DictationLanguage.automatic(null, listOf("ja")))
    }
    @Test fun whisperHearsTheSpeechAndAMarginNotTheQuietAround() {
        val second = 16_000
        // A tap, 3 s of speech from 1 s, then quiet to the 60 s limit: 0.3 s before it to 0.5 s after it.
        assertEquals(second * 7 / 10 until second * 9 / 2, speechSpan(second, 4 * second, 60 * second))
        // Speech from the first block to the last keeps the whole recording.
        assertEquals(0 until 3 * second, speechSpan(0, 3 * second, 3 * second))
        // The margins stop at the recording's ends.
        assertEquals(0 until 2 * second, speechSpan(second / 10, 2 * second - 640, 2 * second))
        // Nothing voiced: the recording as it was (MicCapture says "No speech heard" before it gets here).
        assertEquals(0 until 5 * second, speechSpan(-1, 0, 5 * second))
    }
    @Test fun aClickAwayFromTheSpeechIsNotSpeech() {
        val block = 640
        fun edges(vararg voiced: IntRange) = SpeechEdges().apply {
            for (i in 0 until 1500) block(i * block, block, voiced.any { i in it })
        }
        // A tap on the key (one block), then words from 1 s to 4 s, then the tap on ✓ (two blocks) at 12 s.
        val e = edges(2..2, 25..99, 300..301)
        assertEquals(25 * block, e.first)
        assertEquals(100 * block, e.last)
        // Syllables with short gaps between them are one stretch; a short word on its own still counts.
        assertEquals(40 * block, edges(10..12, 15..17, 20..22, 37..39).last)
        // Only clicks: no speech found, so Whisper gets the whole recording.
        assertEquals(-1, edges(5..6, 200..201).first)
    }
    @Test fun frenchItalianAndDutchAreDictationLanguages() {
        assertEquals(DictationLanguage.French, DictationLanguage.automatic(Spoken.French, listOf("en")))
        assertEquals(DictationLanguage.Italian, DictationLanguage.automatic(Spoken.Italian, listOf("en")))
        assertEquals(DictationLanguage.Dutch, DictationLanguage.automatic(null, listOf("nl", "en")))
        assertEquals(DictationLanguage.French, DictationLanguage.automatic(null, listOf("fr")))
        assertEquals(Spoken.French, DictationLanguage.French.layout)
        assertEquals(null, DictationLanguage.Dutch.layout)
        fun polished(text: String, language: DictationLanguage) =
            SpokenText.format(text, SpeechOptions(Style.Polished, language, false, false, spokenPunctuation = true))
        assertEquals("Bonjour, comment ça va\u202F?", polished("bonjour virgule comment ça va point d'interrogation", DictationLanguage.French))
        assertEquals("Ciao, come stai?", polished("ciao virgola come stai punto interrogativo", DictationLanguage.Italian))
        assertEquals("Heel erg bedankt voor je hulp.", polished("heel erg bedankt voor je hulp", DictationLanguage.Dutch))
        assertEquals("la réunion", SpokenText.continuing("La réunion", "et", "fr"))
        assertEquals("Paris est", SpokenText.continuing("Paris est", "et", "fr"))
        assertEquals("il treno", SpokenText.continuing("Il treno", "e", "it"))
        assertEquals("de trein", SpokenText.continuing("De trein", "en", "nl"))
    }
    @Test fun portugueseIsADictationLanguageWithoutEnglishRules() {
        // A Brazilian or Portuguese phone (Locale language "pt") used to fall back to English.
        assertEquals(DictationLanguage.Portuguese, DictationLanguage.automatic(null, listOf("pt")))
        assertEquals(DictationLanguage.Portuguese, DictationLanguage.automatic(null, listOf("pt", "en")))
        assertEquals(DictationLanguage.Portuguese, DictationLanguage.matching("pt"))
        assertEquals(null, DictationLanguage.Portuguese.layout)
        fun format(text: String, style: Style) = SpokenText.format(text, SpeechOptions(style, DictationLanguage.Portuguese,
            clean = true, translate = false, formatLists = true, spokenPunctuation = true))
        // English cleanup would drop "um" (a filler there, "a" here) and read "no" as a correction.
        assertEquals("Fiquei no hotel um dia, não dois.", format("fiquei no hotel um dia, não dois", Style.Polished))
        assertEquals("Primeiro, um café.", format("primeiro, um café", Style.Polished))
        assertEquals("Obrigado pela ajuda, até amanhã?", format("obrigado pela ajuda vírgula até amanhã ponto de interrogação", Style.Polished))
        assertEquals("obrigado pela ajuda", format("Obrigado pela ajuda.", Style.Casual))
        assertEquals("Fiquei no hotel um dia", format("Fiquei no hotel um dia", Style.Verbatim))
        assertEquals("o ônibus", SpokenText.continuing("O ônibus", "e", "pt"))
        assertEquals("Maria chegou", SpokenText.continuing("Maria chegou", "e", "pt"))
    }
    @Test fun setupSaysWhichLanguageAutomaticDictatesAndWhy() {
        fun phone(vararg tags: String) = phoneLanguage(null, null, tags.map { Locale.forLanguageTag(it) })
        assertEquals(PhoneLanguage(DictationLanguage.English), phone("en-US"))
        // Two languages Hex dictates: the first, the other a hold of the space bar away.
        assertEquals(PhoneLanguage(DictationLanguage.Portuguese, others = listOf(DictationLanguage.English)), phone("pt-BR", "en-US", "pt-PT"))
        // A first language Hex can't dictate is named in itself; Automatic takes the next one it can.
        assertEquals(PhoneLanguage(DictationLanguage.German, "日本語"), phone("ja-JP", "de-DE"))
        assertEquals(PhoneLanguage(DictationLanguage.English, "中文"), phone("zh-CN"))
        // A language chosen in Settings, or a pinned layout, is its own reason.
        assertEquals(PhoneLanguage(DictationLanguage.Spanish, automatic = false), phoneLanguage(DictationLanguage.Spanish, null, listOf(Locale.forLanguageTag("ja-JP"))))
        assertEquals(PhoneLanguage(DictationLanguage.German, automatic = false), phoneLanguage(null, Spoken.German, listOf(Locale.forLanguageTag("pt-BR"), Locale.ENGLISH)))
    }
    @Test fun polishAndRussianAreDictationLanguagesWithoutEnglishRules() {
        // Polish and Russian phones used to fall back to English, and Whisper told "en" translates.
        assertEquals(DictationLanguage.Polish, DictationLanguage.automatic(null, listOf("pl", "en")))
        assertEquals(DictationLanguage.Russian, DictationLanguage.automatic(null, listOf("ru")))
        assertEquals(DictationLanguage.Russian, DictationLanguage.matching("ru"))
        assertEquals(null, DictationLanguage.Polish.layout)
        assertEquals(null, DictationLanguage.Russian.layout)
        fun format(text: String, style: Style, language: DictationLanguage) = SpokenText.format(text, SpeechOptions(style, language,
            clean = true, translate = false, formatLists = true, spokenPunctuation = true))
        // Cleanup has no Polish rules, so the particle "no" ("well") is not English's correction marker.
        assertEquals("Kupię dwa, no, trzy bilety.", format("kupię dwa, no, trzy bilety", Style.Polished, DictationLanguage.Polish))
        assertEquals("dziękuję za pomoc", format("Dziękuję za pomoc.", Style.Casual, DictationLanguage.Polish))
        assertEquals("Спасибо за помощь, до завтра.", format("спасибо за помощь, до завтра", Style.Polished, DictationLanguage.Russian))
        assertEquals("спасибо за помощь", format("Спасибо за помощь.", Style.Casual, DictationLanguage.Russian))
        assertEquals("Я немного опоздаю", format("Я немного опоздаю", Style.Verbatim, DictationLanguage.Russian))
        assertEquals("to jest", SpokenText.continuing("To jest", "i", "pl"))
        assertEquals("Warszawa jest", SpokenText.continuing("Warszawa jest", "i", "pl"))
        assertEquals("это мой", SpokenText.continuing("Это мой", "и", "ru"))
        assertEquals("Москва большая", SpokenText.continuing("Москва большая", "и", "ru"))
    }
    @Test fun aDictationLanguageWithoutItsOwnLayoutHasNoTypingModel() {
        // Português keeps English's keys and accents, but its typing model would be English's: until a
        // layout is pinned, English's word list only stands in for the keys' geometry and your own words.
        assertEquals(null, typingLayout(null, DictationLanguage.Portuguese))
        assertEquals(null, typingLayout(null, DictationLanguage.Dutch))
        assertEquals(null, typingLayout(null, DictationLanguage.Polish))
        assertEquals(null, typingLayout(null, DictationLanguage.Russian))
        // A pinned Keyboard › Keyboard layout has its own model whatever is dictated.
        assertEquals(Spoken.English, typingLayout(Spoken.English, DictationLanguage.Portuguese))
        assertEquals(Spoken.English, typingLayout(Spoken.English, DictationLanguage.Dutch))
        assertEquals(Spoken.Spanish, typingLayout(null, DictationLanguage.Spanish))
        assertEquals(Spoken.French, typingLayout(Spoken.French, DictationLanguage.Spanish))
    }
    // Batch 3 (Rollover, holds, field kinds). These belong in a KeyboardTouchTest once new test files are agreed.
    @Test fun overlappingKeysTypeInTheOrderTheyWentDown() {
        val typed = StringBuilder()
        val roll = Rollover()
        fun down(key: String) = roll.down(key) { typed.append(key) }
        fun up(key: String) { if (roll.up(key)) typed.append(key) }
        down("a"); down("s"); up("s"); up("a")              // a down, s down, s up, a up
        down("d"); up("d")
        down(" "); down("f"); up(" "); up("f")             // space held, f down: the space first
        roll.down("shift"); down("g"); up("g"); up("shift") // shift acts as it goes down; it types nothing
        down("h"); roll.down("shift"); up("h")              // h held, shift down: h types first
        assertEquals("asd fgh", typed.toString())
    }
    @Test fun heldKeysOfferGboardsOptions() {
        assertEquals(listOf("3", "e", "è", "é"), LetterFeedback.holds('e', "en", KeyboardLetters.digit(0, 2)).take(4))
        assertEquals(listOf("1"), LetterFeedback.holds('q', "en", KeyboardLetters.digit(0, 0)))
        assertEquals(listOf("0"), LetterFeedback.holds('p', "en", KeyboardLetters.digit(0, 9)))
        assertEquals(null, KeyboardLetters.digit(0, 10))                 // å, ü: no digit
        assertEquals(null, KeyboardLetters.digit(1, 0))
        assertEquals("¢", LetterFeedback.symbolHolds('$').first())
        assertEquals(",", LetterFeedback.periodHolds[0][LetterFeedback.PERIOD_DEFAULT])
        assertEquals(emptyList<String>(), LetterFeedback.symbolHolds('@'))
    }
    @Test fun heldLettersOfferSymbolsWhenTheLabIsOn() {
        // Off, every key offers exactly what it did: accents alone, and none on a letter without them.
        assertEquals(LetterFeedback.options('a', "en"), LetterFeedback.holds('a', "en", null))
        assertEquals(emptyList<String>(), LetterFeedback.holds('m', "en", null))
        // On, a key without a digit offers its symbol first, then the same accents after it.
        val a = LetterFeedback.holds('a', "en", null, symbolsOnLetters = true)
        assertEquals("@", a.first())
        assertEquals(LetterFeedback.options('a', "en"), a.drop(1))
        assertEquals(listOf("?"), LetterFeedback.holds('m', "en", null, symbolsOnLetters = true))
        assertEquals(LetterFeedback.options('e', "en"), LetterFeedback.holds('e', "en", null, symbolsOnLetters = true))
        // On the top row, with the number row hiding its digit, a letter offers Gboard's symbol first.
        assertEquals(listOf("%"), LetterFeedback.holds('q', "en", null, symbolsOnLetters = true, topRow = true))
        assertEquals(listOf("|") + LetterFeedback.options('e', "en"),
            LetterFeedback.holds('e', "en", null, symbolsOnLetters = true, topRow = true))
        // A capital offers the same symbol, and row 1 keeps its digit first.
        assertEquals("@", LetterFeedback.holds('A', "en", null, symbolsOnLetters = true).first())
        assertEquals("1", LetterFeedback.holds('q', "en", '1', symbolsOnLetters = true).first())
    }
    @Test fun emailAndWebAddressFieldsSwapTheComma() {
        val text = android.text.InputType.TYPE_CLASS_TEXT
        assertEquals("@", FieldKind.of(text or android.text.InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS).mark)
        assertEquals("@", FieldKind.of(text or android.text.InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS).mark)
        assertEquals("/", FieldKind.of(text or android.text.InputType.TYPE_TEXT_VARIATION_URI).mark)
        assertEquals(",", FieldKind.of(text or android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES).mark)
        assertEquals(",", FieldKind.of(android.text.InputType.TYPE_CLASS_NUMBER).mark)
    }
    @Test fun deleteTakesAWholeEmoji() {
        assertEquals(4, DeleteSlide.lastCharacter("a\uD83D\uDC4D\uD83C\uDFFD"))                  // 👍🏽
        assertEquals(4, DeleteSlide.lastCharacter("\uD83C\uDDEB\uD83C\uDDF7"))                   // 🇫🇷
        assertEquals(8, DeleteSlide.lastCharacter("\uD83D\uDC68\u200D\uD83D\uDC69\u200D\uD83D\uDC67")) // 👨‍👩‍👧
        assertEquals(7, DeleteSlide.lastCharacter("\uD83D\uDE4B\uD83C\uDFFC\u200D\u2640\uFE0F")) // 🙋🏼‍♀️
        assertEquals(1, DeleteSlide.lastCharacter("ab"))
        assertEquals(0, DeleteSlide.lastCharacter(""))
    }
    @Test fun fieldsGetGboardsPads() {
        assertEquals(Pad.Number, Pad.of(android.text.InputType.TYPE_CLASS_NUMBER))
        assertEquals(Pad.Number, Pad.of(android.text.InputType.TYPE_CLASS_NUMBER or android.text.InputType.TYPE_NUMBER_FLAG_DECIMAL))
        assertEquals(Pad.Pin, Pad.of(android.text.InputType.TYPE_CLASS_NUMBER or android.text.InputType.TYPE_NUMBER_VARIATION_PASSWORD))
        assertEquals(Pad.Phone, Pad.of(android.text.InputType.TYPE_CLASS_PHONE))
        assertEquals(Pad.Date, Pad.of(android.text.InputType.TYPE_CLASS_DATETIME or android.text.InputType.TYPE_DATETIME_VARIATION_DATE))
        assertEquals(Pad.Time, Pad.of(android.text.InputType.TYPE_CLASS_DATETIME or android.text.InputType.TYPE_DATETIME_VARIATION_TIME))
        assertEquals(Pad.DateTime, Pad.of(android.text.InputType.TYPE_CLASS_DATETIME))
        assertEquals(null, Pad.of(android.text.InputType.TYPE_CLASS_TEXT))
        // Every pad has four rows that fill their cells without overlapping (the symbol pad's first three
        // leave its operator column's 1.5 cells), and the phone pad's 0 holds a plus.
        for (pad in Pad.entries) {
            val rows = KeyPads.rows(pad)
            assertEquals(4, rows.size)
            for ((i, row) in rows.withIndex()) {
                val first = if (pad == Pad.Symbols && i < 3) 1.5f else if (pad == Pad.Pin) 0.5f else 0f
                var at = first
                for (key in row) { assertEquals(at, key.start, 0.001f); at += key.width }
                assertEquals(if (pad == Pad.Pin) 3.5f else KeyPads.cells(pad), at, 0.001f)
            }
        }
        assertEquals("+", KeyPads.rows(Pad.Phone)[3][1].hold)
        assertEquals(listOf(",", "0", "."), KeyPads.rows(Pad.Number)[3].take(3).map { it.label })
    }

    // Send feedback (ly.anoma.hex.feedback): what the app sends Bellhop, checked before it goes out, and its retries.
    private val feedbackContext = FeedbackContext("0.2.2", "5", "Android 16", "Google Pixel 9", "en-US",
        linkedMapOf("screen" to "Home", "channel" to "play"))

    @Test fun feedbackWithAnEmailSendsItAndTheInstallId() {
        val draft = FeedbackDraft(" It \"froze\"\n", subject = "[test] Hi", kind = FeedbackKind.Problem, email = " a@b.co ")
        assertEquals("""{"subject":"[test] Hi","message":"It \"froze\"","category":"bug","email":"a@b.co","install_id":"id-1",""" +
            """"app":{"name":"Hex","version":"0.2.2","build":"5"},"device":{"platform":"android","os":"Android 16",""" +
            """"model":"Google Pixel 9","locale":"en-US"},"metadata":{"screen":"Home","channel":"play"}}""",
            FeedbackRequest(draft, feedbackContext, installId = { "id-1" }).payload)
    }
    @Test fun feedbackWithoutAnEmailIsAnonymousAndMakesNoInstallId() {
        var made = false
        val payload = FeedbackRequest(FeedbackDraft("Hello", kind = FeedbackKind.Other), feedbackContext, installId = { made = true; "id" }).payload
        assertFalse(made)
        assertTrue(payload.contains(""""anonymous":true"""))
        for (field in listOf("email", "install_id", "user", "logs", "category")) assertFalse(payload.contains(""""$field""""))
    }
    @Test fun feedbackIsCheckedBeforeItGoesOut() {
        assertEquals(FeedbackDraft.Problem.InvalidEmail, FeedbackDraft("Hi", email = "ryan@anoma").problem)
        assertEquals(FeedbackDraft.Problem.InvalidEmail, FeedbackDraft("Hi", email = "ryan anoma.ly").problem)
        assertNull(FeedbackDraft("Hi", email = "ryan@anoma.ly").problem)
        assertEquals(FeedbackDraft.Problem.EmptyMessage, FeedbackDraft("  \n").problem)
        assertEquals(FeedbackDraft.Problem.MessageTooLong, FeedbackDraft("x".repeat(20_001)).problem)
        assertNull(FeedbackDraft("x".repeat(20_000)).problem)
        assertEquals(FeedbackDraft.Problem.SubjectTooLong, FeedbackDraft("Hi", subject = "s".repeat(201)).problem)
        assertEquals(FeedbackDraft.Problem.TooManyPictures, FeedbackDraft("Hi", pictures = List(6) { ByteArray(1) }).problem)
        assertEquals(FeedbackDraft.Problem.PicturesTooLarge, FeedbackDraft("Hi", pictures = listOf(ByteArray(10 * 1024 * 1024 + 1))).problem)
        assertEquals(FeedbackDraft.Problem.PicturesTooLarge, FeedbackDraft("Hi", pictures = List(3) { ByteArray(7 * 1024 * 1024) }).problem)
    }
    @Test fun feedbackIsMultipartWithThePayloadThenEachPicture() {
        val request = FeedbackRequest(FeedbackDraft("Hi", pictures = listOf(byteArrayOf(1, 2), byteArrayOf(3))), feedbackContext,
            installId = { "" }, idempotencyKey = "k", boundary = "b")
        val body = String(request.body, Charsets.ISO_8859_1)
        assertEquals("--b\r\nContent-Disposition: form-data; name=\"payload\"\r\nContent-Type: application/json\r\n\r\n${request.payload}\r\n" +
            "--b\r\nContent-Disposition: form-data; name=\"screenshot\"; filename=\"hex-1.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n\u0001\u0002\r\n" +
            "--b\r\nContent-Disposition: form-data; name=\"screenshot\"; filename=\"hex-2.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n\u0003\r\n--b--\r\n", body)
        assertEquals(mapOf("Content-Type" to "multipart/form-data; boundary=b", "Idempotency-Key" to "k", "Accept" to "application/json"), request.headers)
    }
    @Test fun feedbackRetriesOnlyWhatBellhopSaysToRetry() {
        assertEquals(1000L, FeedbackRetry.delay(null, null, null, 1))
        assertEquals(4000L, FeedbackRetry.delay(null, null, null, 3))
        assertNull(FeedbackRetry.delay(null, null, null, 4))
        assertEquals(2000L, FeedbackRetry.delay(503, "misconfigured", null, 2))
        assertEquals(3000L, FeedbackRetry.delay(409, "request_pending", "3", 1))
        assertEquals(5000L, FeedbackRetry.delay(429, "rate_limited", "5", 1))
        assertNull(FeedbackRetry.delay(429, "feedback_daily_limit", "3600", 1))
        for (status in listOf(400, 401, 403, 404, 413, 415, 422)) assertNull(FeedbackRetry.delay(status, null, "1", 1))
        assertEquals(FeedbackFailure.Unavailable, FeedbackRetry.failure(403, "feedback_disabled"))
        assertEquals(FeedbackFailure.Unavailable, FeedbackRetry.failure(401, "invalid_report_key"))
        assertEquals(FeedbackFailure.Other, FeedbackRetry.failure(403, "blocked"))
        assertEquals(FeedbackFailure.Offline, FeedbackRetry.failure(null, null))
    }
    /** A one-connection-at-a-time HTTP server on localhost: each request gets the next answer. */
    private class Bellhop(answers: List<Pair<Int, String>>) : AutoCloseable {
        val seen = mutableListOf<Triple<String, Map<String, String>, ByteArray>>()
        private val socket = ServerSocket(0)
        val url = "http://127.0.0.1:${socket.localPort}/v1/feedback"
        init {
            thread(isDaemon = true) {
                for ((status, body) in answers) runCatching {
                    socket.accept().use { client ->
                        val input = client.getInputStream().buffered()
                        fun line() = generateSequence { input.read().takeIf { it >= 0 } }.takeWhile { it != '\n'.code }
                            .map { it.toChar() }.joinToString("").trimEnd('\r')
                        val start = line()
                        val headers = generateSequence(::line).takeWhile { it.isNotEmpty() }
                            .associate { it.substringBefore(':').lowercase() to it.substringAfter(':').trim() }
                        val sent = ByteArray(headers["content-length"]?.toInt() ?: 0).also { var n = 0; while (n < it.size) n += input.read(it, n, it.size - n) }
                        synchronized(seen) { seen += Triple(start, headers, sent) }
                        client.getOutputStream().write(("HTTP/1.1 $status X\r\nContent-Type: application/json\r\nRetry-After: 2\r\n" +
                            "Content-Length: ${body.length}\r\nConnection: close\r\n\r\n$body").toByteArray())
                    }
                }
            }
        }
        override fun close() = socket.close()
    }

    @Test fun feedbackRetriesWithTheSameBodyAndIdempotencyKey() {
        // A lost answer, a 409 and a 429 with Retry-After, then the issue: four identical requests.
        Bellhop(listOf(201 to """{"number":7}""", 409 to """{"error":{"code":"request_pending"}}""",
            429 to """{"error":{"code":"rate_limited"}}""", 201 to """{"number":7,"received_at":"x","email":"pending"}""")).use { server ->
            val waits = mutableListOf<Long>()
            val progress = mutableListOf<Float>()
            val request = FeedbackRequest(FeedbackDraft("Hi", pictures = listOf(ByteArray(40_000))), feedbackContext, installId = { "" })
            val outcome = FeedbackSender("bh_pub_test", server.url, sleep = { waits += it }, dropFirstResponse = true)
                .send(request) { _, fraction -> progress += fraction }
            assertEquals(FeedbackOutcome.Sent(7, emailPending = true), outcome)
            assertEquals(listOf(1000L, 2000L, 2000L), waits)
            assertEquals(4, server.seen.size)
            for ((start, headers, body) in server.seen) {
                assertEquals("POST /v1/feedback HTTP/1.1", start)
                assertEquals("Bearer bh_pub_test", headers["authorization"])
                assertEquals(request.idempotencyKey, headers["idempotency-key"])
                assertEquals(request.contentType, headers["content-type"])
                assertTrue(body.contentEquals(request.body))
            }
            assertTrue(progress.any { it > 0f && it < 1f } && progress.last() == 1f)
        }
    }
    @Test fun feedbackDoesNotRetryAClientError() {
        for ((status, failure) in listOf(400 to FeedbackFailure.Other, 401 to FeedbackFailure.Unavailable,
            413 to FeedbackFailure.TooLarge, 415 to FeedbackFailure.UnsupportedImage, 422 to FeedbackFailure.Other)) {
            Bellhop(listOf(status to """{"error":{"code":"x"}}""", 201 to """{"number":1}""")).use { server ->
                val outcome = FeedbackSender("bh_pub_test", server.url, sleep = { error("no retry") })
                    .send(FeedbackRequest(FeedbackDraft("Hi"), feedbackContext, installId = { "" })) { _, _ -> }
                assertEquals(FeedbackOutcome.Failed(failure), outcome)
                assertEquals(1, server.seen.size)
            }
        }
        // 403 feedback_disabled is the app's setup: unavailable, and not retried.
        Bellhop(listOf(403 to """{"error":{"code":"feedback_disabled","message":"no"}}""")).use { server ->
            assertEquals(FeedbackOutcome.Failed(FeedbackFailure.Unavailable), FeedbackSender("bh_pub_test", server.url, sleep = { error("no retry") })
                .send(FeedbackRequest(FeedbackDraft("Hi"), feedbackContext, installId = { "" })) { _, _ -> })
        }
    }

    // Keyboard › Swipe typing: where a swiped word's space goes (Gboard's, and iPhone Hex's where Gboard says nothing).
    @Test fun aSwipedWordGetsASpaceBeforeItUnlessTheTextOpensOne() {
        assertFalse(SwipeText.needsSpace(null))
        assertFalse(SwipeText.needsSpace(""))
        assertTrue(SwipeText.needsSpace("Hello"))
        assertTrue(SwipeText.needsSpace("Hello,"))
        assertTrue(SwipeText.needsSpace("Hi."))
        assertFalse(SwipeText.needsSpace("Hello "))
        assertFalse(SwipeText.needsSpace("Hello\n"))
        for (opening in listOf("(", "[", "“", "¿", "¡", "@", "#", "/")) assertFalse(opening, SwipeText.needsSpace("see $opening"))
        assertFalse(SwipeText.needsSpace("he said \""))
        assertTrue(SwipeText.needsSpace("he said \"yes\""))
    }

    @Test fun aSwipeWhereDeleteTookAWordBackTypesItsNextReading() {
        val retries = SwipeRetries()
        val readings = listOf("there", "they're", "three")
        assertEquals(readings, retries.typing(readings, "Hello world "))
        retries.tookBack("there", "Hello world ")
        assertEquals(listOf("they're", "three", "there"), retries.typing(readings, "Hello world "))
        retries.tookBack("they're", "Hello world")
        assertEquals(listOf("three", "there", "they're"), retries.typing(readings, "Hello world "))
        // Somewhere else, or after any other edit, the words are forgotten.
        assertEquals(readings, retries.typing(readings, "Goodbye "))
        retries.tookBack("there", "Hello world ")
        retries.forget()
        assertEquals(readings, retries.typing(readings, "Hello world "))
    }

    // Labs › Keyboard › Delete swiped words letter by letter: on, delete after a swipe is an ordinary one.
    @Test fun deleteTakesASwipedWordBackOnlyWhereTheSwipeLeftItAndOnlyWithTheLabOff() {
        assertTrue(SwipeText.takesBack("Hello world", "Hello world", "world", letterByLetter = false))
        assertFalse(SwipeText.takesBack("Hello world", "Hello world", "world", letterByLetter = true))
        // The text changed or the cursor moved since the swipe: the word is ordinary text by then.
        assertFalse(SwipeText.takesBack("Hello worl", "Hello world", "world", letterByLetter = false))
        assertFalse(SwipeText.takesBack("Hello world", null, "world", letterByLetter = false))
        assertFalse(SwipeText.takesBack("Hello world!", "Hello world!", "world", letterByLetter = false))
    }
}
