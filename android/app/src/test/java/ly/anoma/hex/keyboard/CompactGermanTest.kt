package ly.anoma.hex.keyboard

import ly.anoma.hex.speech.Spoken
import org.junit.Assert.assertEquals
import org.junit.Test

/** Labs › Keyboard › Compact German layout (#3039): German QWERTZ without the ü, ö and ä keys, which u, o and a hold
 * first instead; off, and on every other layout, the keys are as before. */
class CompactGermanTest {
    private fun page(language: Spoken, compact: Boolean) =
        KeyboardPage(language, symbols = false, more = false, shifted = false, locked = false, enter = EnterKey(null, 0),
            compactGerman = compact)

    @Test fun offGermanKeepsItsUmlautKeys() {
        val off = page(Spoken.German, compact = false)
        assertEquals("de", off.letterLayout)
        assertEquals(listOf("qwertzuiopü", "asdfghjklöä", "yxcvbnm"), off.lines)
        assertEquals(listOf("a", "à", "á", "â", "æ", "ã", "å", "ā"), LetterFeedback.holds('a', off.letterLayout, null))
    }

    @Test fun onGermanDropsTheUmlautKeysAndHoldsThemFirst() {
        val on = page(Spoken.German, compact = true)
        assertEquals(listOf("qwertzuiop", "asdfghjkl", "yxcvbnm"), on.lines)
        assertEquals(listOf("a", "ä"), LetterFeedback.holds('a', on.letterLayout, null).take(2))
        // Row 1's digit stays first under the finger, the umlaut right after the letter.
        assertEquals(listOf("9", "o", "ö"), LetterFeedback.holds('o', on.letterLayout, '9').take(3))
        assertEquals(listOf("7", "u", "ü"), LetterFeedback.holds('u', on.letterLayout, '7').take(3))
        // Shift offers the capitals, and s still holds ß (ẞ with shift).
        assertEquals(listOf("A", "Ä"), LetterFeedback.holds('A', on.letterLayout, null).take(2))
        assertEquals(listOf("U", "Ü"), LetterFeedback.options('U', on.letterLayout).take(2))
        assertEquals(listOf("s", "ß"), LetterFeedback.options('s', on.letterLayout).take(2))
        assertEquals(listOf("S", "ẞ"), LetterFeedback.options('S', on.letterLayout).take(2))
    }

    @Test fun otherLayoutsIgnoreIt() {
        for (language in Spoken.entries.filter { it.layoutCode != "de" }) {
            assertEquals(page(language, compact = false).lines, page(language, compact = true).lines)
            assertEquals(language.layoutCode, page(language, compact = true).letterLayout)
        }
    }
}
