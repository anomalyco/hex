package ly.anoma.hex.keyboard

import ly.anoma.hex.speech.Spoken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Gboard's two symbol pages (?123 and =<), which every layout shares: the keys each page draws, that
 * every printable ASCII character types from the letters or one of them, and that no page draws a key
 * twice, the bottom row included. */
class SymbolPagesTest {
    private fun page(language: Spoken = Spoken.English, more: Boolean = false, currency: Char? = null,
                     kind: FieldKind = FieldKind.Text) =
        KeyboardPage(language, symbols = true, more, shifted = false, locked = false, enter = EnterKey(null, 0),
            kind = kind, currency = currency)

    private fun lettersPage(language: Spoken) =
        KeyboardPage(language, symbols = false, more = false, shifted = false, locked = false, enter = EnterKey(null, 0))

    /** Every currency Labs › Keyboard › Local currency can put in $'s place, read as it reads them. */
    private val currencies = listOf("GBP", "EUR", "JPY", "CNY", "INR", "PHP", "KRW", "RUB", "UAH", "USD", "CHF")
        .map { LocalCurrency.symbol(it) }

    /** What the letters page types beside its letters: the digits and accents their holds offer, the
     * field's own mark, a period, and the punctuation a held period offers. */
    private fun letters(language: Spoken): Set<Char> {
        val page = lettersPage(language)
        val out = LinkedHashSet<Char>()
        for ((index, line) in page.lines.withIndex()) for ((position, c) in line.withIndex()) {
            out += c
            out += c.uppercaseChar() // shift types the same key's capital
            out += LetterFeedback.holds(c, language.layoutCode, page.hint(index, position)).flatMap { it.toList() }
        }
        out += page.commaText[0]
        out += page.periodText[0]
        out += LetterFeedback.periodHolds.flatten().flatMap { it.toList() }
        return out
    }

    /** What a symbol page types: each key's own character and what holding it offers, the bottom row's
     * comma and period keys, and what holding those offers. */
    private fun symbols(page: KeyboardPage): Set<Char> {
        val out = LinkedHashSet<Char>()
        for (line in page.lines) for (c in line) {
            out += c
            out += LetterFeedback.symbolHolds(c, page.currency).flatMap { it.toList() }
        }
        for (text in listOf(page.commaText, page.periodText)) {
            out += text[0]
            out += LetterFeedback.symbolHolds(text[0]).flatMap { it.toList() }
        }
        return out
    }

    @Test fun thePagesAreGboardsOwn() {
        // ?123: its ten digits, its ten marks, then the seven keys between the page switch and delete.
        assertEquals(listOf("1234567890", "@#\$_&-+()/", "*\"':;!?"), page(more = false).lines)
        assertEquals(",", page(more = false).commaText)
        assertEquals(".", page(more = false).periodText)
        // =<: the marks the first page has no room for, then the money and math keys, then seven more.
        assertEquals(listOf("~`|•√π÷×§Δ", "£¢€¥^°={}\\", "%©®™✓[]"), page(more = true).lines)
        // Gboard puts < and > beside space on this page, where its rows leave them out.
        assertEquals("<", page(more = true).commaText)
        assertEquals(">", page(more = true).periodText)
    }

    @Test fun everyPrintableAsciiCharacterTypesOnEveryLayout() {
        val ascii = ('!'..'~').toList()
        for (language in Spoken.entries) for (currency in currencies) {
            val typed = LinkedHashSet<Char>()
            typed += letters(language)
            typed += symbols(page(language, more = false, currency = currency))
            typed += symbols(page(language, more = true, currency = currency))
            val missing = ascii.filter { it !in typed }
            assertTrue("${language.code} with ${currency ?: '$'} cannot type $missing", missing.isEmpty())
        }
    }

    @Test fun aLocalCurrencyKeepsEveryMoneyKeyOnTheTwoPages() {
        // The =< page's money row always has four keys, and £ ¢ € ¥ $ stay typeable on a page or a hold.
        for (code in listOf("GBP", "EUR", "JPY", "CNY", "INR", "PHP", "KRW", "RUB", "UAH", "USD", "CHF")) {
            val currency = LocalCurrency.symbol(code)
            val row = page(currency = currency, more = true).lines[1]
            assertEquals("$code's =< row 2 has $row", 10, row.length)
            val money = row.take(4)
            assertEquals("$code's money row draws $money", 4, money.toSet().size)
            assertTrue("$code's money row is $money", money.all { it in "£¢€¥$" })
            val typed = symbols(page(currency = currency, more = false)) + symbols(page(currency = currency, more = true))
            val missing = "£¢€¥$".filter { it !in typed }
            assertTrue("$code cannot type $missing", missing.isEmpty())
        }
    }

    @Test fun noPageDrawsTheSameKeyTwice() {
        for (language in Spoken.entries) for (currency in currencies) for (kind in FieldKind.entries) {
            val pages = listOf(lettersPage(language).copy(kind = kind),
                page(language, more = false, currency = currency, kind = kind),
                page(language, more = true, currency = currency, kind = kind))
            for (shown in pages) {
                val drawn = shown.lines.joinToString("") + shown.commaText + shown.periodText
                val twice = drawn.groupBy { it }.filterValues { it.size > 1 }.keys
                assertTrue("${language.code} $kind ${shown.lines.joinToString(" ")} draws $twice twice", twice.isEmpty())
            }
        }
    }

    @Test fun theThirdRowFitsSevenKeysBetweenTheSwitchAndDelete() {
        for (more in listOf(false, true)) {
            val page = page(more = more)
            assertEquals(10, page.lines[0].length)
            assertEquals(10, page.lines[1].length)
            assertEquals(7, page.lines[2].length)
            val columns = KeyboardMetrics.columns(page.lines).toFloat()
            val side = KeyboardMetrics.sideWidth(page, page.lines)
            val width = (columns - 2 * side) / page.lines[2].length
            // The switch and delete take a cell and a half each and the seven keys share the rest, at
            // Gboard's width for the marks above them.
            assertEquals(1.5f, side, 0f)
            assertEquals(columns, side + 7 * width + side, 0.0001f)
            assertEquals(columns / page.lines[1].length, width, 0.0001f)
        }
    }
}
