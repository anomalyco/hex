package ly.anoma.hex.speech

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
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
