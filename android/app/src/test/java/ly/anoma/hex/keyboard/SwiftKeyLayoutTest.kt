package ly.anoma.hex.keyboard

import ly.anoma.hex.speech.Spoken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Labs › Keyboard › SwiftKey layout (#3021): SwiftKey's held letter symbols, its one-row period, and its 123 and
 * {&= pages, which still type every printable ASCII character and never draw a key twice. */
class SwiftKeyLayoutTest {
    private fun page(symbols: Boolean, more: Boolean = false, currency: Char? = null, swiftKey: Boolean = true,
                     language: Spoken = Spoken.English) =
        KeyboardPage(language, symbols, more, shifted = false, locked = false, enter = EnterKey(null, 0),
            currency = currency, swiftKey = swiftKey)

    @Test fun offThePagesAreGboardsAsBefore() {
        assertEquals(page(symbols = true, swiftKey = false).lines, KeyboardLetters.numbers())
        assertEquals(page(symbols = true, more = true, swiftKey = false).lines, KeyboardLetters.symbols())
        assertEquals("<", page(symbols = true, more = true, swiftKey = false).commaText)
        assertEquals("", page(symbols = true, more = true, swiftKey = false).bottomMarks)
        assertEquals('@', LetterFeedback.letterSymbol('a'))
        assertEquals(listOf("3", "e", "è", "é", "ê", "ë", "ē", "ė", "ę"), LetterFeedback.holds('e', "en", '3'))
    }

    @Test fun theLettersHoldSwiftKeysSymbols() {
        val home = "asdfghjkl".map { LetterFeedback.swiftKeySymbol(it) }.joinToString("")
        val bottom = "zxcvbnm".map { LetterFeedback.swiftKeySymbol(it) }.joinToString("")
        assertEquals("@#&*-+=()", home)
        assertEquals("_\$\"':;/", bottom)
        // The top row's only beside the number row; otherwise those letters keep their digit.
        assertNull(LetterFeedback.swiftKeySymbol('q'))
        assertEquals("%^~|[]<>{}", "qwertyuiop".map { LetterFeedback.swiftKeySymbol(it, topRow = true) }.joinToString(""))
        // A held letter offers its symbol first, then its accents; a digit still comes first where row 1 shows one.
        assertEquals(listOf("/"), LetterFeedback.holds('m', "en", null, swiftKey = true))
        assertEquals(listOf("@", "a", "à", "á", "â", "ä", "æ", "ã", "å", "ā"), LetterFeedback.holds('a', "en", null, swiftKey = true))
        assertEquals("3", LetterFeedback.holds('e', "en", '3', swiftKey = true, topRow = true).first())
        // A local currency takes $'s place on x, as on the 123 page.
        assertEquals('€', LetterFeedback.swiftKeySymbol('x', currency = '€'))
    }

    @Test fun thePeriodHoldsOneRowWithThePeriodUnderTheFinger() {
        val row = LetterFeedback.swiftKeyPeriodHolds
        assertEquals(listOf(listOf("!", ",", ".", "?")), row)
        assertEquals(".", row[0][LetterFeedback.SWIFTKEY_PERIOD_DEFAULT])
    }

    @Test fun thePagesAreSwiftKeys() {
        val numbers = page(symbols = true)
        assertEquals(listOf("1234567890", "@#\$&_-()=%", "\"*':/!?+"), numbers.lines)
        assertEquals(",", numbers.commaText)
        assertEquals(".", numbers.periodText)
        assertEquals("", numbers.bottomMarks)
        val more = page(symbols = true, more = true)
        assertEquals(listOf("£€¥¢©®™~¿", "\t[]{}<>^¡", "`;÷\\|¦¬"), more.lines)
        assertEquals("×§¶°", more.bottomMarks)
        // The letters keep the field's own mark and a period.
        assertEquals(",", page(symbols = false).commaText)
        assertEquals(".", page(symbols = false).periodText)
    }

    /** What the two pages and the letters type with SwiftKey layout on. */
    private fun typed(currency: Char?): Set<Char> {
        val out = LinkedHashSet<Char>()
        val letters = page(symbols = false, currency = currency)
        for ((index, line) in letters.lines.withIndex()) for ((position, c) in line.withIndex()) {
            out += c; out += c.uppercaseChar()
            out += LetterFeedback.holds(c, "en", letters.hint(index, position), swiftKey = true, topRow = index == 0,
                currency = currency).flatMap { it.toList() }
        }
        out += letters.commaText[0]; out += letters.periodText[0]
        out += LetterFeedback.swiftKeyPeriodHolds.flatten().flatMap { it.toList() }
        for (more in listOf(false, true)) {
            val shown = page(symbols = true, more = more, currency = currency)
            for (c in shown.lines.joinToString("") + shown.bottomMarks) {
                out += c; out += LetterFeedback.symbolHolds(c, currency).flatMap { it.toList() }
            }
            if (shown.bottomMarks.isEmpty()) { out += shown.commaText[0]; out += shown.periodText[0] }
        }
        return out
    }

    private val currencies = listOf("GBP", "EUR", "JPY", "INR", "USD", "CHF").map { LocalCurrency.symbol(it) }

    @Test fun everyPrintableAsciiCharacterStillTypes() {
        for (currency in currencies) {
            val missing = ('!'..'~').filter { it !in typed(currency) }
            assertTrue("with ${currency ?: '$'} cannot type $missing", missing.isEmpty())
            assertTrue("with ${currency ?: '$'} cannot type $", '$' in typed(currency))
        }
    }

    @Test fun noPageDrawsTheSameKeyTwice() {
        for (currency in currencies) for (more in listOf(false, true)) {
            val shown = page(symbols = true, more = more, currency = currency)
            val drawn = shown.lines.joinToString("") + shown.bottomMarks +
                if (shown.bottomMarks.isEmpty()) shown.commaText + shown.periodText else ""
            val twice = drawn.groupBy { it }.filterValues { it.size > 1 }.keys
            assertTrue("${shown.lines.joinToString(" ")} draws $twice twice", twice.isEmpty())
            if (more) assertTrue("{&= draws no \$ with $currency", '$' in drawn || currency == null)
        }
    }
}
