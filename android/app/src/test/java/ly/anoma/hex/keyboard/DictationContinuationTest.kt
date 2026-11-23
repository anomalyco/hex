package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import ly.anoma.hex.speech.SpokenText

class DictationContinuationTest {
    /** What KeyboardEngine.insertDictation puts in at the cursor, without the editor: [before] is the text
     * before it, and [text] arrives already signed when commitSpeech appended the signature. */
    private fun dictate(continuation: DictationContinuation, text: String, before: String,
                        signature: Boolean = false, after: Char? = null): String {
        val standalone = continuation.standalone(before)
        val shaped = if (signature || standalone) text else SpokenText.continuing(text, before, "en")
        val inserted = dictationInsertion(shaped, before, after, standalone)
        if (inserted.isNotEmpty()) continuation.record(if (signature) inserted else null)
        return inserted
    }

    @Test fun aDictationAfterASignedOneStartsOnANewLine() {
        val continuation = DictationContinuation()
        val signed = dictate(continuation, "a\nSent from Hex", before = "", signature = true)
        assertEquals("a\nSent from Hex", signed)
        // No leading space, and the capital Whisper gave it stays: it is a line of its own, not the
        // signature's continuation.
        assertEquals("\nGood news.", dictate(continuation, "Good news.", before = signed))
    }

    @Test fun aDictationContinuesNormallyOnceTheSignedTextIsNotRightBeforeTheCursor() {
        val continuation = DictationContinuation()
        dictate(continuation, "a\nSent from Hex", before = "", signature = true)
        // The cursor moved past the signature, or another app edited the text: ordinary continuation.
        val before = "a\nSent from Hex and some more "
        assertFalse(continuation.standalone(before))
        assertEquals("thanks again.", dictate(continuation, "Thanks again.", before = before))
        assertFalse(continuation.standalone(before + "thanks again."))
    }

    @Test fun twoSignedDictationsInARowEachStartANewLine() {
        val continuation = DictationContinuation()
        var text = dictate(continuation, "a\nSent from Hex", before = "", signature = true)
        text += dictate(continuation, "b\nSent from Hex", before = text, signature = true)
        // Exactly this: insertion() adds no trailing space when nothing follows the cursor.
        assertEquals("a\nSent from Hex\nb\nSent from Hex", text)
    }

    @Test fun theTrailingSpaceInsertionAddsDoesNotEndTheStandaloneState() {
        val continuation = DictationContinuation()
        val signed = dictate(continuation, "a\nSent from Hex", before = "Hi ", signature = true, after = 'n')
        assertEquals("a\nSent from Hex ", signed)
        assertTrue(continuation.standalone("Hi $signed"))
        // Spacing after it is unchanged: the text after the cursor gets its own space.
        assertEquals("\nGood news. ", dictate(continuation, "Good news.", before = "Hi $signed", after = 'n'))
    }

    @Test fun anEditNewFieldOrLeavingTheEditorEndsIt() {
        val continuation = DictationContinuation()
        val signed = dictate(continuation, "a\nSent from Hex", before = "", signature = true)
        continuation.clear()
        assertFalse(continuation.standalone(signed))
    }

    @Test fun aDictationThatNeverWentInLeavesTheNextStandingAlone() {
        val continuation = DictationContinuation()
        val signed = dictate(continuation, "a\nSent from Hex", before = "", signature = true)
        // Reading is not consuming: a cancelled dictation records nothing, so the state stays.
        assertTrue(continuation.standalone(signed))
        assertTrue(continuation.standalone(signed))
    }

    @Test fun anUnsignedDictationEndsIt() {
        val continuation = DictationContinuation()
        val signed = dictate(continuation, "a\nSent from Hex", before = "", signature = true)
        dictate(continuation, "Good news.", before = signed)
        assertFalse(continuation.standalone(signed + "\nGood news."))
    }
}
