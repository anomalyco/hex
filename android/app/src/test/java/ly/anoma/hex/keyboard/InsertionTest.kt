package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Test
import ly.anoma.hex.speech.DictationLanguage
import ly.anoma.hex.speech.FrenchSpacing
import ly.anoma.hex.speech.SpokenCleanup
import ly.anoma.hex.speech.SpokenPunctuation
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
}
