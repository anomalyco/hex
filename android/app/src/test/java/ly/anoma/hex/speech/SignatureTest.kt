package ly.anoma.hex.speech

import java.io.File
import java.io.RandomAccessFile
import kotlin.io.path.createTempDirectory
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class SignatureTest {
    @Test fun theSignatureIsAppendedOnceOnItsOwnLine() {
        assertEquals("we ship friday\nSent from Hex",
            Signature.append("we ship friday", Signature.DEFAULT_TEXT, true))
        assertEquals("we ship friday\n\nSent from Hex",
            Signature.append("we ship friday\n", Signature.DEFAULT_TEXT, true))
    }
    @Test fun aSavedSignatureIsTrimmedBeforeItIsAppended() {
        assertEquals("hi\nSent from Hex", Signature.append("hi", "  Sent from Hex  ", true))
    }
    @Test fun offAppendsNothing() {
        assertNull(Signature.append("we ship friday", Signature.DEFAULT_TEXT, false))
    }
    @Test fun emptyTextAppendsNothing() {
        assertNull(Signature.append("", Signature.DEFAULT_TEXT, true))
        assertNull(Signature.append("   \n\t", Signature.DEFAULT_TEXT, true))
    }
    @Test fun anEmptyOrWhitespaceSignatureAppendsNothing() {
        assertNull(Signature.append("we ship friday", "", true))
        assertNull(Signature.append("we ship friday", "  \n\t ", true))
    }
    @Test fun textThatAlreadyEndsWithTheSignatureIsLeftAlone() {
        assertNull(Signature.append("we ship friday\nSent from Hex", Signature.DEFAULT_TEXT, true))
        assertNull(Signature.append("Sent from Hex", Signature.DEFAULT_TEXT, true))
        // A trailing space or newline after it still reads as signed.
        assertNull(Signature.append("we ship friday\nSent from Hex\n", Signature.DEFAULT_TEXT, true))
    }
    @Test fun aSignatureThatEndsALineOfProseIsStillAppended() {
        val signature = "Hex"
        assertEquals("I love Hex\nHex", Signature.append("I love Hex", signature, true))
        assertEquals("we ship friday, Hex\nHex", Signature.append("we ship friday, Hex", signature, true))
        assertEquals("Hex is the best\nHex", Signature.append("Hex is the best", signature, true))
    }
    @Test fun aSignatureAfterALineBreakIsLeftAlone() {
        for (text in listOf("we ship friday\nSent from Hex", "we ship friday\n  Sent from Hex",
            "we ship friday\r\nSent from Hex", "we ship friday\rSent from Hex", "\nSent from Hex")) {
            assertNull(text, Signature.append(text, Signature.DEFAULT_TEXT, true))
        }
    }
    @Test fun aSignatureMustBeOneLineOfAtMostTheLimit() {
        assertNull(Signature.textProblem(""))
        assertNull(Signature.textProblem("Sent from Hex"))
        assertEquals(Signature.Problem.Line, Signature.textProblem("two\nlines"))
        assertEquals(Signature.Problem.Line, Signature.textProblem("two\r\nlines"))
        assertNull(Signature.textProblem("x".repeat(Signature.MAX_CHARS)))
        assertEquals(Signature.Problem.Length, Signature.textProblem("x".repeat(Signature.MAX_CHARS + 1)))
        // Characters, not UTF-16 code units: an emoji counts once, as on macOS.
        assertNull(Signature.textProblem("\uD83D\uDE00".repeat(Signature.MAX_CHARS)))
        assertEquals(Signature.Problem.Length, Signature.textProblem("\uD83D\uDE00".repeat(Signature.MAX_CHARS + 1)))
    }
    @Test fun aSignatureRefusesLineAndControlCharacters() {
        for (text in listOf("a\u2028b", "a\u2029b", "a\u0085b", "a\tb", "a\u0007b", "a\u001bb")) {
            assertEquals(text, Signature.Problem.Line, Signature.textProblem(text))
        }
    }
    @Test fun aTextThatIsOnlyTheSignatureIsNotSignedAgain() {
        assertNull(Signature.append(Signature.DEFAULT_TEXT, Signature.DEFAULT_TEXT, true))
    }
}

// Labs › Faster English dictation: which model a dictation runs on, and what happens when the faster one can't run.
// Kept here, in one class, until it has a file of its own.
class FastEnglishRoutingTest {
    private fun options(spoken: DictationLanguage = DictationLanguage.English, fastEnglish: Boolean = true,
                        translate: Boolean = false, vocabulary: List<String> = emptyList(),
                        candidates: List<DictationLanguage> = emptyList()) =
        SpeechOptions(Style.Polished, spoken, clean = false, translate = translate, vocabulary = vocabulary, fastEnglish = fastEnglish,
            candidates = candidates)

    /** Records what each model was given, and answers as told. */
    private class Models(val english: EnglishResult, val standard: String? = "standard text") {
        val englishGot = mutableListOf<Pair<ShortArray, Int>>()
        val standardGot = mutableListOf<Pair<ShortArray, Int>>()
        fun run(engine: Engine, pcm: ShortArray, count: Int = pcm.size, abandoned: Boolean = false) =
            transcribe(engine, pcm, count, { abandoned },
                { audio, n -> englishGot += audio to n; english },
                { audio, n -> standardGot += audio to n; standard })
    }

    @Test fun englishWithTheSwitchOnUsesTheFasterModel() {
        assertEquals(Engine.English, engineFor(options()))
    }
    @Test fun offEveryDictationUsesTodaysModel() {
        assertEquals(Engine.Standard, engineFor(options(fastEnglish = false)))
        val models = Models(EnglishResult.Text("never"))
        assertEquals(Decoded("standard text"), models.run(Engine.Standard, ShortArray(16_000)))
        assertTrue(models.englishGot.isEmpty())
    }
    @Test fun everyOtherLanguageStaysWithTodaysModel() {
        for (language in DictationLanguage.entries - DictationLanguage.English)
            assertEquals(language.name, Engine.Standard, engineFor(options(spoken = language)))
        // Automatic on a German layout resolves to German before the dictation's options are made.
        val automatic = DictationLanguage.automatic(Spoken.German, listOf("en"))
        assertEquals(Engine.Standard, engineFor(options(spoken = automatic)))
    }
    @Test fun pasteInEnglishStaysWithTodaysModel() {
        // Translation is Spanish spoken, English written: today's model translates, the faster one can't.
        assertEquals(Engine.Standard, engineFor(options(spoken = DictationLanguage.Spanish, translate = true)))
        assertEquals(Engine.Standard, engineFor(options(translate = true)))
    }
    @Test fun automaticChoosingAmongSeveralLanguagesStaysWithTodaysModel() {
        // Labs › Automatic listens for the language: which one is spoken is known only once today's model heard it.
        assertEquals(Engine.Standard, engineFor(options(candidates = listOf(DictationLanguage.English, DictationLanguage.Spanish))))
        // With one candidate nothing is chosen: the dictation is today's, the faster model included.
        val one = options(candidates = listOf(DictationLanguage.English))
        assertEquals(Engine.English, engineFor(one))
        assertEquals(listOf(DictationLanguage.English), one.languages)
        assertEquals(options().languages, one.languages)
    }
    @Test fun aDictationIsWrittenInTheLanguageAutomaticChose() {
        val both = options(candidates = listOf(DictationLanguage.English, DictationLanguage.Spanish), translate = true)
        // Spanish heard with Paste in English: translated, so written (and formatted) as English.
        val spanish = both.writtenIn(DictationLanguage.Spanish)
        assertEquals(DictationLanguage.Spanish, spanish.spoken)
        assertTrue(spanish.translate)
        // English heard: never translated, only transcribed.
        val english = both.writtenIn(DictationLanguage.English)
        assertEquals(DictationLanguage.English, english.spoken)
        assertFalse(english.translate)
        // Another language heard without translation keeps everything else.
        val german = options(candidates = listOf(DictationLanguage.English, DictationLanguage.German)).writtenIn(DictationLanguage.German)
        assertEquals(options(candidates = listOf(DictationLanguage.English, DictationLanguage.German)).copy(spoken = DictationLanguage.German), german)
    }
    @Test fun theLanguageWhenTheDictationStartedDecides() {
        // MicCapture keeps the options a dictation started with; a language picked while it transcribes is the next one's.
        val started = options()
        val changed = started.copy(spoken = DictationLanguage.Spanish)
        assertEquals(Engine.English, engineFor(started))
        assertEquals(Engine.Standard, engineFor(changed))
    }
    @Test fun aMissingFileGoesToTodaysModelSilently() {
        val models = Models(EnglishResult.Unavailable(Unready.NoFile))
        val decoded = models.run(Engine.English, ShortArray(16_000))
        assertEquals(Decoded("standard text", RanOn.NoFile), decoded)
        assertFalse(decoded.lowMemory)
        assertEquals(null, ModelStore.english.verifiedIn(createTempDirectory().toFile()))
    }
    @Test fun aFileFailingItsHashIsDeletedSoTheDownloadIsOfferedAgain() {
        val dir = createTempDirectory().toFile()
        // The right size, the wrong bytes: installed by size, refused by its SHA-256.
        val file = File(dir, ModelStore.english.fileName)
        RandomAccessFile(file, "rw").use { it.setLength(135_373_280L) }
        assertNotNull(ModelStore.english.installedIn(dir))
        assertEquals(null, ModelStore.english.verifiedIn(dir))
        assertFalse(file.exists())
        assertEquals(ModelStore.State.Available, ModelStore.english.state.value)
    }
    @Test fun theSpeechModelWaitsForWifiOnlyOnANetworkThatChargesForData() {
        assertTrue(waitsForWifi(mobileDataChosen = false, unmetered = false))
        assertFalse(waitsForWifi(mobileDataChosen = false, unmetered = true))
        // The person chose mobile data: it goes ahead, and a resumed download doesn't ask again.
        assertFalse(waitsForWifi(mobileDataChosen = true, unmetered = false))
        // No network: it goes ahead and says there's no connection, as before.
        assertFalse(waitsForWifi(mobileDataChosen = false, unmetered = null))
    }
    @Test fun shortOfMemoryTheSameAudioGoesToTodaysModel() {
        val pcm = ShortArray(48_000) { (it % 200).toShort() }
        val before = pcm.copyOf()
        val models = Models(EnglishResult.Unavailable(Unready.LowMemory))
        val decoded = models.run(Engine.English, pcm)
        assertEquals(Decoded("standard text", RanOn.LowMemory), decoded)
        assertTrue(decoded.lowMemory)
        assertSame(pcm, models.standardGot.single().first)
        assertEquals(48_000, models.standardGot.single().second)
        assertArrayEquals(before, pcm)
    }
    @Test fun aFailedEnglishDecodeTriesTodaysModel() {
        val models = Models(EnglishResult.Failed)
        assertEquals(Decoded("standard text", RanOn.FasterFailed), models.run(Engine.English, ShortArray(16_000)))
        assertEquals(1, models.standardGot.size)
    }
    @Test fun aFailedLoadGoesToTodaysModelAndSaysSo() {
        val models = Models(EnglishResult.Unavailable(Unready.LoadFailed))
        assertEquals(Decoded("standard text", RanOn.LoadFailed), models.run(Engine.English, ShortArray(16_000)))
        assertEquals(1, models.standardGot.size)
    }
    @Test fun aDiscardedDictationIsNotTriedAgain() {
        // ✕ stops the faster model's decode; today's model must not start on the same audio.
        val models = Models(EnglishResult.Failed)
        assertEquals(Decoded(null, RanOn.Faster), models.run(Engine.English, ShortArray(16_000), abandoned = true))
        assertTrue(models.standardGot.isEmpty())
    }
    @Test fun englishTextIsUsedAsItIs() {
        val models = Models(EnglishResult.Text("Meet me on Tuesday, sorry, Wednesday."))
        assertEquals(Decoded("Meet me on Tuesday, sorry, Wednesday.", RanOn.Faster), models.run(Engine.English, ShortArray(16_000)))
        assertTrue(models.standardGot.isEmpty())
    }
    @Test fun customVocabularyKeepsTheRouteAndTodaysPrompt() {
        assertEquals(Engine.English, engineFor(options(vocabulary = listOf("Anomaly", "Hex"))))
        // Today's model gets the same initial prompt as before; the faster one gets the terms themselves.
        assertEquals("Anomaly, Hex", whisperPrompt(listOf("Anomaly", "Hex")))
        assertNull(whisperPrompt(emptyList()))
    }
    @Test fun aMinuteLongDictationIsDecodedWholeAndKeepsItsNote() {
        val pcm = ShortArray(16_000 * 60)
        val models = Models(EnglishResult.Text("a minute of words"))
        assertEquals(Decoded("a minute of words", RanOn.Faster), models.run(Engine.English, pcm))
        assertEquals(16_000 * 60, models.englishGot.single().second)
        assertEquals("Stopped at 1 minute", dictationNote("Stopped at 1 minute", LOW_MEMORY_NOTE))
        assertEquals(LOW_MEMORY_NOTE, dictationNote(null, LOW_MEMORY_NOTE))
        assertNull(dictationNote(null, null))
    }
}

// Faster English dictation: SentenceEnds puts back the sentence ends its model runs together. Kept here with the routing
// cases until they have a file of their own.
class SentenceEndsTest {
    private val english = ly.anoma.hex.typing.TypingModel.file(File("src/main/assets/typing/en.hextm"), "en")
    private fun lowercase(word: String) = english.index(word) >= 0 && (english.capitals?.percent(word) ?: -1) < 0

    /** 16 kHz audio: speech (a loud tone) where [spoken] says, silence elsewhere, [ms] long. */
    private fun audio(ms: Int, vararg spoken: IntRange) = ShortArray(ms * 16) { i ->
        if (spoken.any { i / 16 in it }) (8000 * kotlin.math.sin(i * 0.3)).toInt().toShort() else 0
    }
    private fun decoded(vararg words: Triple<String, Int, Int>) =
        words.joinToString(" ") { it.first } + words.joinToString("") { "\n${it.second}\t${it.third}\t${it.first}" }

    @Test fun aCommonWordCapitalisedAfterAPauseStartsASentence() {
        // "…back of the bus We should…": 600 ms of quiet between the sentences, as between two of the benchmark's clips.
        val pcm = audio(3000, 0..1199, 1800..2999)
        val out = SentenceEnds.restore(decoded(Triple("the", 600, 900), Triple("bus", 900, 1200),
            Triple("We", 1800, 2000), Triple("should", 2000, 2400)), pcm, pcm.size, ::lowercase)
        assertEquals("the bus. We should", out)
    }
    @Test fun withoutAPauseTheCapitalAloneIsNotEnough() {
        // "…in New York…": the model capitalises a name that is also a common word, with no pause before it.
        val pcm = audio(2000, 0..1999)
        assertEquals("lives in New York", SentenceEnds.restore(decoded(Triple("lives", 0, 400), Triple("in", 400, 600),
            Triple("New", 600, 900), Triple("York", 900, 1300)), pcm, pcm.size, ::lowercase))
    }
    @Test fun aNameAfterAPauseIsLeftAlone() {
        val pcm = audio(3000, 0..999, 1700..2999)
        assertEquals("I talked to Sarah", SentenceEnds.restore(decoded(Triple("I", 0, 200), Triple("talked", 200, 600),
            Triple("to", 600, 1000), Triple("Sarah", 1700, 2200)), pcm, pcm.size, ::lowercase))
    }
    @Test fun iIsNeverASentenceStartOnItsOwn() {
        val pcm = audio(3000, 0..999, 1700..2999)
        assertEquals("on Saturday I didn't", SentenceEnds.restore(decoded(Triple("on", 0, 300), Triple("Saturday", 300, 1000),
            Triple("I", 1700, 1800), Triple("didn't", 1800, 2200)), pcm, pcm.size, ::lowercase))
    }
    @Test fun punctuationTheModelWroteIsKept() {
        val pcm = audio(3000, 0..999, 1700..2999)
        assertEquals("the blue folder, I mean", SentenceEnds.restore(decoded(Triple("the", 0, 300), Triple("blue", 300, 600),
            Triple("folder,", 600, 1000), Triple("I", 1700, 1800), Triple("mean", 1800, 2100)), pcm, pcm.size, ::lowercase))
        assertEquals("warm. Move the", SentenceEnds.restore(decoded(Triple("warm.", 0, 1000), Triple("Move", 1700, 2000),
            Triple("the", 2000, 2200)), pcm, pcm.size, ::lowercase))
    }
    @Test fun aShortPauseIsNotASentenceEnd() {
        // 60 ms of quiet measured before a capitalised common word, the benchmark's longest inside a sentence: no period.
        // (The frame that ends where the next word starts isn't counted, so 80 ms of silence measures 60.)
        val pcm = audio(2000, 0..999, 1080..1999)
        assertEquals("is Dune", SentenceEnds.restore(decoded(Triple("is", 600, 1000), Triple("Dune", 1080, 1500)),
            pcm, pcm.size, ::lowercase))
        assertEquals(60, SentenceEnds.quietMs(pcm, pcm.size, 600, 1080))
    }
    @Test fun aNegativeWordStartIsClampedToTheAudio() {
        // The library's word times aren't trusted: a start before the audio measures from its first sample.
        val pcm = audio(3000, 600..1199, 1800..2999)
        assertEquals(600, SentenceEnds.quietMs(pcm, pcm.size, -400, 1800))
        assertEquals("the bus. We should", SentenceEnds.restore(decoded(Triple("the", -400, 900), Triple("bus", 900, 1200),
            Triple("We", 1800, 2000), Triple("should", 2000, 2400)), pcm, pcm.size, ::lowercase))
    }
    @Test fun aStartAfterTheNextWordsAddsNothing() {
        val pcm = audio(3000, 0..1199, 1800..2999)
        assertEquals(0, SentenceEnds.quietMs(pcm, pcm.size, 2000, 1800))
        assertEquals(0, SentenceEnds.quietMs(pcm, pcm.size, 1800, 1800))
        assertEquals("the bus We should", SentenceEnds.restore(decoded(Triple("the", 600, 900), Triple("bus", 2500, 2600),
            Triple("We", 1800, 2000), Triple("should", 2000, 2400)), pcm, pcm.size, ::lowercase))
        // Times past the audio's end measure only what the audio has.
        assertEquals(0, SentenceEnds.quietMs(pcm, pcm.size, 9000, 12000))
    }
        @Test fun wordsThatDontSpellTheTextLeaveItAsItCame() {
        val pcm = audio(3000, 0..1199, 1800..2999)
        val out = SentenceEnds.restore("the bus We should\n900\t1200\tbus\n1800\t2000\tWe", pcm, pcm.size, ::lowercase)
        assertEquals("the bus We should", out)
    }
}

// Faster English dictation: SpokenNumbers writes the numbers its model says in words in digits, as the iPhone does.
// The cases are the iPhone's (ios/HEXMobileTests/TranscriptCleanupTests.swift) and sentences the faster model wrote on
// Hex's clips, each with what the iPhone's rules write. Kept here with the other faster-English cases until they have
// a file of their own.
class SpokenNumbersTest {
    private fun assertWritten(cases: List<Pair<String, String>>, terms: List<String> = emptyList()) {
        for ((heard, written) in cases) assertEquals(heard, written, SpokenNumbers.digits(heard, terms))
    }
    private fun assertAsSaid(cases: List<String>, terms: List<String> = emptyList()) {
        for (heard in cases) assertEquals(heard, heard, SpokenNumbers.digits(heard, terms))
    }

    @Test fun spokenNumbersBecomeDigits() {
        assertWritten(listOf(
            "Between five and ten minutes." to "Between 5 and 10 minutes.",
            "Two or three days." to "2 or 3 days.",
            "One to two hours." to "1 to 2 hours.",
            "Meet at seven fifteen pm." to "Meet at 7:15 pm.",
            "Seven oh five am." to "7:05 am.",
            "At five o'clock." to "At 5 o'clock.",
            "It's one mile away." to "It's 1 mile away.",
            "It costs one dollar." to "It costs \$1.",
            "Five million dollars." to "\$5 million.",
            "Two thousand and five." to "2005.",
            "May second." to "May 2nd.",
            "On the first of may." to "On the 1st of may.",
            "On September thirtieth." to "On September 30th.",
            "Five point five dollars." to "\$5.50.",
            "Call five five five, one two three, four five six seven." to "Call 555-123-4567.",
            "Built in nineteen oh five." to "Built in 1905.",
            "It's minus one." to "It's -1.",
            "Twenty-five people." to "25 people.",
            "Two hundred people." to "200 people.",
            "Three point five million." to "3.5 million.",
            "Twenty one guns." to "21 guns.",
            "Six feet." to "6 feet.",
            "A hundred and fifty people came." to "150 people came.",
            "About a hundred and twenty." to "About 120.",
            "It was a hundred and five degrees." to "It was 105 degrees.",
            "I gave a hundred and ten percent." to "I gave 110%.",
            "A thousand and one nights." to "1001 nights.",
            "One point twenty one gigawatts." to "1.21 gigawatts.",
            "Ten to fifteen percent." to "10 to 15%.",
            "Five to ten percent." to "5 to 10%.",
            "Fifty one percent." to "51%.",
            "One in ten people." to "1 in 10 people.",
            "Eight out of ten cats." to "8 out of 10 cats.",
            "Nine times out of ten." to "9 times out of 10.",
            "From zero to one hundred." to "From 0 to 100.",
            "She is five foot six." to "She is 5 foot 6.",
            "The code is oh four seven two." to "The code is 0472.",
            "My zip is nine oh two one oh." to "My zip is 90210.",
            "Area fifty one." to "Area 51.",
            "Apollo thirteen." to "Apollo 13.",
            "I shop at Forever Twenty One." to "I shop at Forever 21.",
            "iPhone seventeen pro." to "iPhone 17 pro.",
            "Windows eleven." to "Windows 11.",
            "Twenty people came." to "20 people came.",
        ))
    }
    /** The iPhone's 80 sentences said with numbers, as the faster model writes them in words (its other cases there
     * already have digits, which this leaves alone). */
    @Test fun eitherEnginesSentencesReadTheSame() {
        assertWritten(listOf(
            "The ratio is five point five." to "The ratio is 5.5.",
            "Set the opacity to zero point two." to "Set the opacity to 0.2.",
            "It weighs three point seven five kilos." to "It weighs 3.75 kilos.",
            "The rating went up to four point eight stars." to "The rating went up to 4.8 stars.",
            "I have two dogs and three cats." to "I have 2 dogs and 3 cats.",
            "Give me ten minutes." to "Give me 10 minutes.",
            "There were twenty five people at the party." to "There were 25 people at the party.",
            "Room three hundred and twelve is on the left." to "Room 312 is on the left.",
            "The town has fifteen thousand people." to "The town has 15,000 people.",
            "About one point two million people live there." to "About 1.2 million people live there.",
            "I walked eight thousand steps today." to "I walked 8000 steps today.",
            "She was born in nineteen ninety nine." to "She was born in 1999.",
            "The plan for twenty twenty six is ready." to "The plan for 2026 is ready.",
            "The battery is at twenty percent." to "The battery is at 20%.",
            "Sales grew by three point five percent." to "Sales grew by 3.5%.",
            "I'm one hundred percent sure." to "I'm 100% sure.",
            "It costs five dollars." to "It costs \$5.",
            "Can you lend me twenty bucks?" to "Can you lend me 20 bucks?",
            "The bill came to fifteen dollars and forty cents." to "The bill came to \$15.40.",
            "Wake me up at six AM to morrow." to "Wake me up at 6 AM to morrow.",
            "My birthday is november twenty second." to "My birthday is november 22nd.",
            "The deadline is March third." to "The deadline is March 3rd.",
            "We moved on the fourth of July." to "We moved on the 4th of July.",
            "My number is five five five one two three four five six seven." to "My number is 555-123-4567.",
            "The code is four seven two nine." to "The code is 4729.",
            "He's 6 foot two." to "He's 6 foot 2.",
            "He's six foot two." to "He's 6 foot 2.",
            "I ran ten miles yesterday." to "I ran 10 miles yesterday.",
            "It's seventy two degrees outside." to "It's 72 degrees outside.",
            "Add two cups of flour." to "Add 2 cups of flour.",
            "The screen is six point one inches." to "The screen is 6.1 inches.",
            "Update to IOS twenty seven point two." to "Update to IOS 27.2.",
            "It takes five to ten minutes." to "It takes 5 to 10 minutes.",
            "It's minus five degrees outside." to "It's -5 degrees outside.",
            "The balance is negative forty dollars." to "The balance is -\$40.",
            "Claude Opus five point five is out." to "Claude Opus 5.5 is out.",
            "It's about five point five." to "It's about 5.5.",
            "Five point five" to "5.5",
            "Sonnet four point five is cheaper." to "Sonnet 4.5 is cheaper.",
            "I need two more minutes." to "I need 2 more minutes.",
            "There are three things to fix." to "There are 3 things to fix.",
            "We have four options." to "We have 4 options.",
            "Wait five seconds." to "Wait 5 seconds.",
        ))
        // Left as said: kinds left for later, numbers already in digits, and a count ("Test one, two, three."),
        // which the iPhone writes in a step after this one.
        assertAsSaid(listOf(
            "About 1.2 million people live there.", "I'll be there at quarter past two.",
            "The train leaves at seven fifteen in the morning.", "The screen is 6.one inches.",
            "I'll be there in one. 5 hours.", "Two-thirds of the team agreed.", "The score was 3 to one.",
            "Test one, 2, 3.", "Test one, two, three.",
            "Can you check whether the 5.5 model is faster than the 4.5 one, and send me the numbers by 3.30?",
        ))
    }
    /** "one" alone, ordinals outside dates, fractions, and times, prices, ranges and scores without a marker or unit
     * stay as said for now, as do digits, names the engine capitalised and arithmetic. */
    @Test fun theRestStaysAsSaid() {
        assertAsSaid(listOf(
            "One of them is broken.", "No one came to the meeting.", "That's the one I wanted.",
            "For once, it worked.", "To be honest, I won the game.", "I want to go too.", "Give me a second.",
            "We'll go through them one by one.", "We need one more chair.", "Use half a cup of sugar.",
            "Two thirds of the team agreed.", "I'll be there in one and a half hours.",
            "She finished first in the race.", "This is the third time I've asked.", "Let's meet at three thirty.",
            "The app is four ninety nine a month.", "Lunch was twelve fifty.", "The score was three to one.",
            "Kids aged eight to twelve can join.", "I ate breakfast at eight.",
            "We shipped build one thirty this morning.", "I work nine to five.", "The five point plan.",
            "Back in the nineteen nineties.", "Just my two cents.", "Ten to five.", "One or two people came.",
            "We sold 2,400 units.", "The app is \$4.99 a month.", "Meet at 7:15.",
            "He lives on the twenty first floor.", "Have you tried GPT five yet?", "The deadline is march third.",
            "Two plus two equals four.", "Five, six, seven.", "Five five.", "It's a two-way street.",
            "Take a ten-minute break.", "Quarter past two.", "Twenty past six.", "Two of us went.",
            "Those two are fine.", "The train leaves at seven fifteen.", "Two dozen eggs.", "A hundred people.",
            "I love Twenty One Pilots.", "Use Eleven Labs for the voice.", "Ten Thousand Villages has nice gifts.",
            "I watched Fifty Shades of Grey.", "We saw Twelve Angry Men.", "I watch Sixty Minutes.",
            "Let's get Five Guys for lunch.", "We went to Six Flags.", "Ten minus five is five.",
            "Ten times two is twenty.", "Ten minus five.", "At one point two people left.", "Oh one more thing.",
            "A hundred percent.", "It cost a hundred dollars.", "It's a couple hundred bucks.",
            "A few thousand people came.", "A million reasons.", "Half a million people.",
            "Two to three hundred people.", "One or two hundred people.",
        ))
    }
    /** What the faster model wrote on Hex's clips. */
    @Test fun sentencesFromHexsClips() {
        assertWritten(listOf(
            "The ticket costs twenty five dollars, and parking is another eight dollars and fifty cents." to
                "The ticket costs \$25, and parking is another \$8.50.",
            "We need three chairs and twelve plates for the party." to "We need 3 chairs and 12 plates for the party.",
            "Call me at four o'clock about the twenty twenty four budget." to
                "Call me at 4 o'clock about the 2024 budget.",
            "About fifteen percent of the order shipped late." to "About 15% of the order shipped late.",
            "Her flight lands at six forty five PM on march third." to "Her flight lands at 6:45 PM on march third.",
        ))
        // A small number before anything but a plural noun or a unit stays a word, "march" in lower case isn't a month
        // (it could be the verb), and an ordinal outside a date stays too.
        assertAsSaid(listOf(
            "Let's meet at nine tomorrow morning.", "I'll meet you at the Hilton near Heathrow at nine.",
            "Her flight lands at 6:45 PM on march third.",
        ))
    }
    @Test fun customVocabularyStaysAsSaid() {
        assertAsSaid(listOf("let's get five guys for lunch"), listOf("Five Guys"))
        assertAsSaid(listOf("We love twenty one pilots."), listOf("Twenty One Pilots"))
        assertAsSaid(listOf("I got a gift at ten thousand villages."), listOf("Ten Thousand Villages"))
        assertAsSaid(listOf("I listen to nine inch nails."), listOf("Nine Inch Nails"))
        assertAsSaid(listOf("Let's grab dinner at five guys and twenty one pilots plays at nine."),
            listOf("Five Guys", "Twenty One Pilots"))
        // Without it the same words are a number, and only the term's own words stay.
        assertWritten(listOf("We love twenty one pilots." to "We love 21 pilots."))
        assertWritten(listOf("Five guys ordered twenty five burgers." to "Five guys ordered 25 burgers."),
            listOf("Five Guys"))
    }
    /** Nothing, only marks, a number cut off at the end, and long runs of number words: no exception, and the
     * iPhone's result. */
    @Test fun oddInputIsSafe() {
        assertWritten(listOf(
            "" to "",
            " " to " ",
            "\n" to "\n",
            "..." to "...",
            "?! , ." to "?! , .",
            "\"\"" to "\"\"",
            "\$" to "\$",
            "-" to "-",
            "—" to "—",
            "'" to "'",
            "a" to "a",
            "oh" to "oh",
            "point" to "point",
            "and" to "and",
            "the" to "the",
            "minus" to "minus",
            "negative" to "negative",
            "a hundred" to "a hundred",
            "a thousand and" to "a thousand and",
            "may" to "may",
            "the fourth" to "the fourth",
            "the fourth of" to "the fourth of",
            "five point" to "five point",
            "five point." to "five point.",
            "one two" to "one two",
            "nine one one" to "911",
            "five dollars and" to "\$5 and",
            "five dollars and forty" to "\$5 and 40",
            "between five and" to "between five and",
            "five to" to "five to",
            "five to ten" to "five to ten",
            "five foot" to "5 foot",
            "five per" to "five per",
            "seven fifteen" to "seven fifteen",
            "seven oh" to "seven oh",
            "seven oh five" to "705",
            "twenty-" to "20-",
            "-twenty-five-" to "-25-",
            "twenty--five" to "25",
            "1️⃣ two three" to "1️⃣ two three",
            "½ two three" to "½ two three",
            "Shopping list:\ntwo dogs" to "Shopping list:\n2 dogs",
            "nine hundred ninety nine trillion nine hundred ninety nine billion dollars" to "\$999,999,000,000,000",
        ))
        assertWritten(listOf(
            List(30) { "nine" }.joinToString(" ") to "9".repeat(30),
            (1..14).joinToString(" in ") { numbers[it - 1] } + " people" to
                (1..14).joinToString(" in ") + " people",
        ))
        // Runs of number words that could be read many ways stay as said: 30 of them in a row, and long chains.
        assertAsSaid(listOf(
            numbers.joinToString(" "),
            List(15) { "twenty one" }.joinToString(" "),
            List(15) { "one in" }.joinToString(" ") + " people",
        ))
    }
    private val numbers = ("one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen " +
        "sixteen seventeen eighteen nineteen twenty thirty forty fifty sixty seventy eighty ninety hundred thousand " +
        "million").split(" ")
}
