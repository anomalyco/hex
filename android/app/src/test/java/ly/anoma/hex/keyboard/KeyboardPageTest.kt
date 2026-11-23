package ly.anoma.hex.keyboard

import ly.anoma.hex.speech.Spoken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class KeyboardPageTest {
    private fun page(numberRow: Boolean, symbols: Boolean = false, more: Boolean = false, pad: Pad? = null,
                     language: Spoken = Spoken.English) =
        KeyboardPage(language, symbols, more, shifted = false, locked = false, enter = EnterKey(null, 0),
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

    @Test fun aPadPageKeepsItsOwnKeysWithoutTheNumberRow() {
        // A pad page can be drawn without the symbol pages' flag; the number row is still not its row.
        assertNull(page(numberRow = true, pad = Pad.Number).digits)
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

    @Test fun withBothLabsTheCornerShowsOneHintAndHoldsTheSymbolFirst() {
        // AZERTY's a is on a digit row whose corner hint the number row takes away, so its
        // Symbols on letter keys hint is the one hint left, never a digit and a symbol together.
        val both = page(numberRow = true, language = Spoken.French)
        val digitHint = both.hint(0, 0)
        assertNull(digitHint)
        assertEquals("@", digitHint?.toString() ?: LetterFeedback.letterSymbol('a')?.toString())
        // Its hold offers the symbol first, then the same accents, with no digit before either.
        assertEquals(listOf("@") + LetterFeedback.options('a', "fr"),
            LetterFeedback.holds('a', "fr", digitHint, symbolsOnLetters = true))
        // A row-1 letter without a symbol keeps its accents alone, and its digit hint stays hidden.
        assertNull(both.hint(0, 2))
        assertEquals(LetterFeedback.options('e', "fr"),
            LetterFeedback.holds('e', "fr", both.hint(0, 2), symbolsOnLetters = true))
        // A QWERTY home-row key shows no digit either way, so its symbol hint and hold are unchanged.
        val qwerty = page(numberRow = true)
        assertNull(qwerty.hint(1, 0))
        assertEquals(listOf("@") + LetterFeedback.options('a', "en"),
            LetterFeedback.holds('a', "en", qwerty.hint(1, 0), symbolsOnLetters = true))
    }
}
