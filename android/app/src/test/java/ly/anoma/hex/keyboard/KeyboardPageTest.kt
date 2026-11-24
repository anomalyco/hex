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
        // A row-1 letter shows its Gboard symbol in place of the hidden digit, and holds it first.
        assertEquals("|", both.hint(0, 2)?.toString() ?: LetterFeedback.letterSymbol('e', topRow = true)?.toString())
        assertEquals(listOf("|") + LetterFeedback.options('e', "fr"),
            LetterFeedback.holds('e', "fr", both.hint(0, 2), symbolsOnLetters = true, topRow = true))
        // A QWERTY home-row key shows no digit either way, so its symbol hint and hold are unchanged.
        val qwerty = page(numberRow = true)
        assertNull(qwerty.hint(1, 0))
        assertEquals(listOf("@") + LetterFeedback.options('a', "en"),
            LetterFeedback.holds('a', "en", qwerty.hint(1, 0), symbolsOnLetters = true))
    }

    @Test fun withBothLabsTheTopRowShowsGboardsSymbols() {
        // With the number row on, the hidden digit lets q to p show and hold Gboard's symbols.
        val both = page(numberRow = true)
        assertEquals(listOf("%", "\\", "|", "=", "[", "]", "<", ">", "{", "}"),
            "qwertyuiop".map { LetterFeedback.letterSymbol(it, topRow = true)?.toString() })
        assertEquals("%", both.hint(0, 0)?.toString() ?: LetterFeedback.letterSymbol('q', topRow = true)?.toString())
        assertEquals("}", both.hint(0, 9)?.toString() ?: LetterFeedback.letterSymbol('p', topRow = true)?.toString())
        assertEquals(listOf("%"), LetterFeedback.holds('q', "en", both.hint(0, 0), symbolsOnLetters = true, topRow = true))
        assertEquals(listOf("}"), LetterFeedback.holds('p', "en", both.hint(0, 9), symbolsOnLetters = true, topRow = true))
        // With the number row off, the digit is still the hint and the first hold, before any symbol.
        val digits = page(numberRow = false)
        assertEquals("1", digits.hint(0, 0)?.toString())
        assertEquals(listOf("1"),
            LetterFeedback.holds('q', "en", digits.hint(0, 0), symbolsOnLetters = true, topRow = true))
        // Off the lab, the top row is what it always was: its digit and accents.
        assertEquals(listOf("1"), LetterFeedback.holds('q', "en", digits.hint(0, 0)))
    }

    @Test fun theTopRowSymbolsStayOnTheTopRow() {
        // Symbols follow the letter on the other rows; the top row's are its own, so a layout that draws
        // one of those letters lower keeps the holds it had. AZERTY's q is on row 2 and w on row 3, and
        // QWERTZ's y on row 3.
        assertNull(LetterFeedback.letterSymbol('q', topRow = false))
        val azerty = page(numberRow = true, language = Spoken.French)
        assertEquals(emptyList<String>(),
            LetterFeedback.holds('q', "fr", azerty.hint(1, 0), symbolsOnLetters = true, topRow = false))
        assertEquals(emptyList<String>(),
            LetterFeedback.holds('w', "fr", azerty.hint(2, 0), symbolsOnLetters = true, topRow = false))
        assertEquals(LetterFeedback.options('y', "de"),
            LetterFeedback.holds('y', "de", null, symbolsOnLetters = true, topRow = false))
        // AZERTY's own top row reads Gboard's symbols where its letters have none of their own, and
        // keeps the home-row symbol a key already had where it draws one of those instead.
        assertEquals("|", LetterFeedback.letterSymbol('e', topRow = true)?.toString())
        assertEquals("@", LetterFeedback.letterSymbol('a', topRow = true)?.toString())
        assertEquals("_", LetterFeedback.letterSymbol('z', topRow = true)?.toString())
    }

    // Android counts the stored Keyboard vibration value only where the phone declares that setting (Pixel);
    // elsewhere (the emulator, phones without the switch) a stored 0 must not silence Hex's taps.
    @Test fun keyboardVibrationOffSilencesOnlyWhereThePhoneHasTheSwitch() {
        assertEquals(false, keyboardVibrationAllows(supported = true, stored = 0))
        assertEquals(true, keyboardVibrationAllows(supported = true, stored = 1))
        assertEquals(true, keyboardVibrationAllows(supported = false, stored = 0))
        assertEquals(true, keyboardVibrationAllows(supported = false, stored = 1))
    }
}
