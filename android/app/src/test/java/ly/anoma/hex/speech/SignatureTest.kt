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
        val models = Models(EnglishResult.Unavailable(memory = false))
        assertEquals(Decoded("standard text", lowMemory = false), models.run(Engine.English, ShortArray(16_000)))
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
    @Test fun shortOfMemoryTheSameAudioGoesToTodaysModel() {
        val pcm = ShortArray(48_000) { (it % 200).toShort() }
        val before = pcm.copyOf()
        val models = Models(EnglishResult.Unavailable(memory = true))
        assertEquals(Decoded("standard text", lowMemory = true), models.run(Engine.English, pcm))
        assertSame(pcm, models.standardGot.single().first)
        assertEquals(48_000, models.standardGot.single().second)
        assertArrayEquals(before, pcm)
    }
    @Test fun aFailedEnglishDecodeTriesTodaysModel() {
        val models = Models(EnglishResult.Failed)
        assertEquals(Decoded("standard text"), models.run(Engine.English, ShortArray(16_000)))
        assertEquals(1, models.standardGot.size)
    }
    @Test fun aDiscardedDictationIsNotTriedAgain() {
        // ✕ stops the faster model's decode; today's model must not start on the same audio.
        val models = Models(EnglishResult.Failed)
        assertEquals(Decoded(null), models.run(Engine.English, ShortArray(16_000), abandoned = true))
        assertTrue(models.standardGot.isEmpty())
    }
    @Test fun englishTextIsUsedAsItIs() {
        val models = Models(EnglishResult.Text("Meet me on Tuesday, sorry, Wednesday."))
        assertEquals(Decoded("Meet me on Tuesday, sorry, Wednesday."), models.run(Engine.English, ShortArray(16_000)))
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
        assertEquals(Decoded("a minute of words"), models.run(Engine.English, pcm))
        assertEquals(16_000 * 60, models.englishGot.single().second)
        assertEquals("Stopped at 1 minute", dictationNote("Stopped at 1 minute", LOW_MEMORY_NOTE))
        assertEquals(LOW_MEMORY_NOTE, dictationNote(null, LOW_MEMORY_NOTE))
        assertNull(dictationNote(null, null))
    }
}
