package ly.anoma.hex.keyboard

import java.util.Locale
import ly.anoma.hex.speech.Spoken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LocalCurrencyTest {
    private fun page(currency: Char? = null, more: Boolean = false) =
        KeyboardPage(Spoken.English, symbols = true, more, shifted = false, locked = false, enter = EnterKey(null, 0),
            currency = currency)
    private fun locales(vararg tags: String) = tags.map { Locale.forLanguageTag(it) }

    @Test fun offThePagesDrawGboardsRowsAndTheSameHolds() {
        assertEquals(listOf("1234567890", "@#\$_&-+()/", "*\"':;!?"), page().lines)
        assertEquals(listOf("~`|•√π÷×§Δ", "£¢€¥^°={}\\", "%©®™✓[]"), page(more = true).lines)
        assertEquals(listOf("¢", "€", "£", "₹", "¥", "₱"), LetterFeedback.symbolHolds('$'))
    }

    @Test fun britainPutsPoundsOnThe123Page() {
        assertEquals('£', LocalCurrency.symbol(locales("en-GB")))
        assertEquals("@#£_&-+()/", page('£').lines[1])
        // Held, the key offers the $ it took the place of first, then $'s own options without a second £.
        val holds = LetterFeedback.symbolHolds('£', '£')
        assertEquals("$", holds.first())
        assertEquals(listOf("$", "¢", "€", "₹", "¥", "₱"), holds)
        assertEquals(listOf("№"), LetterFeedback.symbolHolds('#', '£'))
    }

    @Test fun theSecondSymbolsPagePutsDollarsWhereItCan() {
        val symbols = page('£', more = true).lines[1]
        assertEquals("\$¢€¥^°={}\\", symbols)
        assertEquals(10, symbols.length)
        // The $ it shows there holds the rest too, without another £.
        assertEquals(listOf("¢", "€", "₹", "¥", "₱"), LetterFeedback.symbolHolds('$', '£'))
        // A currency it never drew gives only ¥'s slot to $, so £, ¢ and € stay and the row keeps ten keys.
        assertEquals("£¢€\$^°={}\\", page('₹', more = true).lines[1])
        assertEquals("£¢€\$^°={}\\", page('₩', more = true).lines[1])
        // That $ keeps ¥ reachable, offers every other symbol, and never a second copy of the local one.
        assertEquals(listOf("¢", "€", "£", "₹", "¥", "₱"), LetterFeedback.symbolHolds('$', '₩'))
    }

    @Test fun everyCurrencyKeepsTenKeysOnTheFirstTwoRowsOfBothPages() {
        for (code in listOf("GBP", "EUR", "JPY", "CNY", "INR", "PHP", "KRW", "RUB", "UAH", "USD", "CHF")) {
            val currency = LocalCurrency.symbol(code)
            for (more in listOf(false, true)) {
                val lines = page(currency, more).lines
                assertEquals("$code, more = $more, row 1", 10, lines[0].length)
                assertEquals("$code, more = $more, row 2", 10, lines[1].length)
            }
        }
    }

    @Test fun germanyAndFrancePutEurosOnThe123Page() {
        assertEquals('€', LocalCurrency.symbol(locales("de-DE")))
        assertEquals('€', LocalCurrency.symbol(locales("fr-FR")))
        assertEquals("@#€_&-+()/", page('€').lines[1])
        assertEquals("£¢\$¥^°={}\\", page('€', more = true).lines[1])
        assertEquals(listOf("$", "¢", "£", "₹", "¥", "₱"), LetterFeedback.symbolHolds('€', '€'))
    }

    @Test fun americaAndUnknownCurrenciesKeepDollars() {
        assertNull(LocalCurrency.symbol(locales("en-US")))
        assertNull(LocalCurrency.symbol(locales("en")))
        assertNull(LocalCurrency.symbol(emptyList()))
        assertNull(LocalCurrency.symbol(locales("en-ZZ")))
        assertEquals('£', LocalCurrency.symbol(locales("en", "en-GB")))
        assertEquals('€', LocalCurrency.symbol(locales("de-DE", "en-US")))
        assertNull(LocalCurrency.symbol("USD"))
        assertNull(LocalCurrency.symbol("CHF"))
        assertNull(LocalCurrency.symbol("CAD"))
        assertEquals(listOf("1234567890", "@#\$_&-+()/", "*\"':;!?"), page().lines)
    }

    @Test fun aRegionWithoutACurrencyKeepsDollars() {
        // Antarctica has a country and no currency: the lookup either returns null for it or rejects the
        // region, and either way Hex keeps $. A region that is not a country (Latin America's 419) keeps it
        // the same way.
        assertNull(LocalCurrency.symbol(listOf(Locale.forLanguageTag("en-AQ"))))
        assertNull(LocalCurrency.symbol(listOf(Locale.forLanguageTag("es-419"))))
        assertEquals(listOf("1234567890", "@#\$_&-+()/", "*\"':;!?"), page().lines)
    }
}
