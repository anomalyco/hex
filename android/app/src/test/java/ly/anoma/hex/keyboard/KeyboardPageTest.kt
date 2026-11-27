package ly.anoma.hex.keyboard

import ly.anoma.hex.speech.Spoken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
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

    @Test fun russianIsYcukenWithShiftAndDeleteOneLetterWide() {
        val russian = page(numberRow = false, language = Spoken.Russian)
        assertEquals(listOf("йцукенгшщзх", "фывапролджэ", "ячсмитьбю"), russian.lines)
        assertEquals(1f, KeyboardMetrics.sideWidth(russian, russian.lines))
        // ё and ъ have no keys: they are held on е and ь, capitals too.
        assertEquals(listOf("е", "ё"), LetterFeedback.options('е', "ru"))
        assertEquals(listOf("Е", "Ё"), LetterFeedback.options('Е', "ru"))
        assertEquals(listOf("ь", "ъ"), LetterFeedback.options('ь', "ru"))
        assertEquals(emptyList<String>(), LetterFeedback.options('к', "ru"))
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
        assertEquals("*", LetterFeedback.letterSymbol('z', topRow = true)?.toString())
    }

    @Test fun theHomeAndBottomRowsShowGboardsSymbols() {
        // Gboard's English QWERTY: the marks its ?123 page draws on the same rows.
        assertEquals(listOf("@", "#", "$", "_", "&", "-", "+", "(", ")"),
            "asdfghjkl".map { LetterFeedback.letterSymbol(it)?.toString() })
        assertEquals(listOf("*", "\"", "'", ":", ";", "!", "?"),
            "zxcvbnm".map { LetterFeedback.letterSymbol(it)?.toString() })
        // ! and ? are holdable from n and m, as Gboard has them; $ stays d's own symbol.
        assertEquals(listOf("!") + LetterFeedback.options('n', "en"),
            LetterFeedback.holds('n', "en", null, symbolsOnLetters = true))
        assertEquals(listOf("?"), LetterFeedback.holds('m', "en", null, symbolsOnLetters = true))
        assertEquals(listOf("$"), LetterFeedback.holds('d', "en", null, symbolsOnLetters = true))
    }

    /** Every symbol the letters page shows in a key's corner with both labs on, exactly as KeyPlaneView
     * resolves it: its digit, else its letter symbol. */
    private fun letterHints(shown: KeyboardPage): List<Char> = shown.lines.flatMapIndexed { row, line ->
        line.withIndex().mapNotNull { (position, c) ->
            shown.hint(row, position) ?: LetterFeedback.letterSymbol(c, topRow = row == 0, currency = shown.currency)
        }
    }

    @Test fun noSymbolIsDrawnOnTwoLetterKeys() {
        // With both labs on, every letter key's corner is a symbol and no mark lands on two keys, on every
        // layout and for every currency d can hold: r's = is no longer j's too.
        val currencies = listOf<Char?>(null) + listOf("GBP", "EUR", "JPY", "CNY", "INR", "PHP", "KRW", "RUB", "UAH")
            .map { LocalCurrency.symbol(it) }
        for (language in Spoken.entries) for (currency in currencies) {
            val shown = letterHints(page(numberRow = true, language = language).copy(currency = currency))
            assertEquals("${language.code} with ${currency ?: '$'}", emptyList<Char>(),
                shown.groupBy { it }.filterValues { it.size > 1 }.keys.toList())
        }
    }

    @Test fun theLocalCurrencyTakesDsSymbol() {
        // Off, or in a region with no symbol Hex knows, d holds Gboard's $.
        assertEquals("$", LetterFeedback.letterSymbol('d')?.toString())
        // On, d holds the phone's own, as the ?123 page shows it.
        assertEquals("£", LetterFeedback.letterSymbol('d', currency = '£')?.toString())
        assertEquals("€", LetterFeedback.letterSymbol('D', currency = '€')?.toString())
        assertEquals(listOf("¥"), LetterFeedback.holds('d', "en", null, symbolsOnLetters = true, currency = '¥'))
        // As the letters page resolves it: with both labs on, d's corner shows the page's currency, and $
        // when the lab is off or the region has none.
        val letters = page(numberRow = true).copy(currency = '£')
        val d = letters.lines[1].indexOf('d')
        assertEquals("£", letters.hint(1, d)?.toString()
            ?: LetterFeedback.letterSymbol('d', topRow = false, currency = letters.currency)?.toString())
        assertEquals("$", LetterFeedback.letterSymbol('d', currency = page(numberRow = true).currency)?.toString())
        // Only d: the rest of the rows keep Gboard's marks.
        assertEquals("@", LetterFeedback.letterSymbol('a', currency = '¥')?.toString())
        assertEquals("?", LetterFeedback.letterSymbol('m', currency = '¥')?.toString())
    }

    // Inter's shapes per px of size (v4.1: ascender 0.969, descender 0.242, capitals 0.727 em; W the widest letter).
    private val letterW = TextShape(advance = 0.94f, ascent = 0.969f, descent = 0.242f, cap = 0.727f)
    private val wideHint = TextShape(advance = 0.98f, ascent = 0.969f, descent = 0.242f, cap = 0.727f) // @ at weight 500

    /** Whether the centred letter's capitals and the corner hint's box share a pixel, in a keycap [w] by [h]. */
    private fun touches(w: Float, h: Float, s: KeyLabelSizes, label: TextShape, hint: TextShape): Boolean {
        val letterRight = (w + label.advance * s.label) / 2
        val letterTop = h / 2 + ((label.ascent - label.descent) / 2 - label.cap) * s.label
        val hintLeft = w - s.hintEnd - hint.advance * s.hint
        val hintBottom = s.hintTop + hint.ascent * s.hint
        return letterRight > hintLeft && letterTop < hintBottom
    }

    @Test fun keyRowsFollowGboardsThreeScreenWidths() {
        assertEquals(RowSize(KeyboardMetrics.ROW, KeyboardMetrics.KEY_HEIGHT), KeyRows.of(411.4f, landscape = false))
        assertEquals(KeyRows.of(400f, false), KeyRows.of(480f, false))
        assertEquals(RowSize(50.3f, 40.3f), KeyRows.of(360f, false))
        assertEquals(RowSize(50.3f, 40.3f), KeyRows.of(392.7f, false))
        assertEquals(RowSize(48f, 37.7f), KeyRows.of(344.4f, false))
        assertEquals(RowSize(KeyboardMetrics.ROW_LANDSCAPE, KeyboardMetrics.KEY_HEIGHT_LANDSCAPE), KeyRows.of(411.4f, landscape = true))
    }

    @Test fun keyLettersKeepGboardsShareOfTheKeyOnEveryScreen() {
        val d = 2.625f
        val pixel8 = KeyLabels.sizes(KeyLabels.REF_WIDTH * d, KeyLabels.REF_HEIGHT * d, d, KeyLabels.REF_HEIGHT, 26f, letterW, wideHint)
        assertEquals(26f * d, pixel8.label, 0.01f)
        assertEquals(KeyLabels.HINT * d, pixel8.hint, 0.01f)
        // A 375 dp phone's 31.9 x 40.3 dp keycap: Gboard's capitals are 0.41 of it, about 16.5 dp (Inter's: 0.727 em).
        val narrow = KeyLabels.sizes(31.9f * d, 40.3f * d, d, 40.3f, 26f, letterW, wideHint)
        assertEquals(0.41f * 40.3f, narrow.label / d * letterW.cap, 0.6f)
        assertTrue(narrow.hint < pixel8.hint)
        // A taller row (Settings › Keyboard › Height) keeps the letters as they are; a much shorter one shrinks them.
        assertEquals(pixel8.label, KeyLabels.sizes(KeyLabels.REF_WIDTH * d, 60f * d, d, KeyLabels.REF_HEIGHT, 26f, letterW, null).label, 0.01f)
        assertTrue(KeyLabels.sizes(KeyLabels.REF_WIDTH * d, 30f * d, d, KeyLabels.REF_HEIGHT, 26f, letterW, null).label < pixel8.label)
    }

    @Test fun aLetterNeverTouchesItsHintOrLeavesItsKeycap() {
        val d = 2.625f
        // Narrow and wide phones, short and tall rows, turned keys.
        for (w in listOf(24f, 27.1f, 30.3f, 35.43f, 44f, 60f)) for (h in listOf(28f, 37.7f, 39f, 40.3f, 47f, 56f)) {
            val s = KeyLabels.sizes(w * d, h * d, d, minOf(h, KeyLabels.REF_HEIGHT), 26f, letterW, wideHint)
            assertFalse("${w}x$h", touches(w * d, h * d, s, letterW, wideHint))
            assertTrue("${w}x$h", letterW.advance * s.label <= w * d)
        }
    }

    @Test fun aLongLabelFitsItsKeyInsteadOfAnEllipsis() {
        val d = 2.625f
        // ?123 on a 1.5-cell modifier key of a 360 dp phone.
        val label = TextShape(advance = 2.4f, ascent = 0.969f, descent = 0.242f, cap = 0.727f)
        val s = KeyLabels.sizes(45f * d, 40.3f * d, d, 40.3f, 17f, label, null)
        assertTrue(label.advance * s.label <= 45f * d - 2 * KeyLabels.SIDE * d)
        assertTrue(s.label > 0f)
    }

    @Test fun theNumberRowTakesGboardsHeightNotAWholeLetterRow() {
        // A Pixel 8 upright: letters every 56.4 dp, digits every 52.6 dp; the keyboard grows by 45 dp, not 59.
        val upright = KeyRows.of(411.4f, false)
        val on = KeyRows.withNumberRow(upright, landscape = false)
        assertEquals(56.4f, on.letters.pitch, 0.1f)
        assertEquals(52.6f, on.digits.pitch, 0.1f)
        assertEquals(upright.key, on.letters.key, 0.01f)
        assertEquals(on.letters.gap, on.digits.gap, 0.01f)
        assertEquals(45f, on.digits.pitch + 3 * on.letters.pitch - 3 * upright.pitch, 0.5f)
        // Turned: 35 dp digits over 41.1 dp letters, about 30 dp more where a whole row was 43.
        val turned = KeyRows.of(411.4f, true)
        val turnedOn = KeyRows.withNumberRow(turned, landscape = true)
        assertEquals(35f, turnedOn.digits.pitch, 0.1f)
        assertEquals(30f, turnedOn.digits.pitch + 3 * turnedOn.letters.pitch - 3 * turned.pitch, 1f)
        // Every screen step keeps a gap under its keys.
        for (w in listOf(320f, 375f, 411f)) assertTrue(KeyRows.withNumberRow(KeyRows.of(w, false), false).letters.gap > 0f)
    }
}
