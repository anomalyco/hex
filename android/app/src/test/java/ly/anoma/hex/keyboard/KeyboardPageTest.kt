package ly.anoma.hex.keyboard

import ly.anoma.hex.speech.Spoken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class KeyboardPageTest {
    private fun page(numberRow: Boolean, symbols: Boolean = false, more: Boolean = false, pad: Pad? = null) =
        KeyboardPage(Spoken.English, symbols, more, shifted = false, locked = false, enter = EnterKey(null, 0),
            pad = pad, numberRow = numberRow)

    @Test fun theNumberRowShowsOneToZeroAboveTheLetters() {
        assertEquals("1234567890", page(numberRow = true).digits)
    }

    @Test fun offThereIsNoNumberRow() {
        assertNull(page(numberRow = false).digits)
    }

    @Test fun theNumberRowNeverChangesTheLettersOrTheSymbolPages() {
        assertEquals(KeyboardLetters.rows("en"), page(numberRow = true).lines)
        assertNull(page(numberRow = true, symbols = true).digits)
        assertNull(page(numberRow = true, symbols = true, more = true).digits)
        // A phone or number field's pad draws its own keys; the number row is for the letters.
        assertNull(page(numberRow = true, symbols = true, pad = Pad.Number).digits)
    }

    @Test fun theNumberRowHidesTheLetterRowsCornerDigits() {
        val plain = page(numberRow = false)
        assertEquals("1", plain.hint(0, 0)?.toString())
        assertEquals("0", plain.hint(0, 9)?.toString())
        assertNull(plain.hint(1, 0))
        assertNull(page(numberRow = true).hint(0, 0))
        assertNull(page(numberRow = true).hint(0, 9))
    }

    @Test fun heldLettersKeepTheirAccentsWithoutTheirDigit() {
        assertEquals(listOf("e", "è", "é", "ê", "ë", "ē", "ė", "ę"),
            LetterFeedback.holds('e', "en", page(numberRow = true).hint(0, 2)))
        assertEquals(listOf("3", "e", "è", "é", "ê", "ë", "ē", "ė", "ę"),
            LetterFeedback.holds('e', "en", page(numberRow = false).hint(0, 2)))
    }
}
