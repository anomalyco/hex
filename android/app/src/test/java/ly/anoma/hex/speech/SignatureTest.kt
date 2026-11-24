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
                        translate: Boolean = false, vocabulary: List<String> = emptyList()) =
        SpeechOptions(Style.Polished, spoken, clean = false, translate = translate, vocabulary = vocabulary, fastEnglish = fastEnglish)

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
