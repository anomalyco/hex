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

    @Test fun offThePagesAndHoldsAreUnchanged() {
        assertEquals(listOf("1234567890", "-/:;()$&@\"", ".,?!'"), page().lines)
        assertEquals(listOf("[]{}#%^*+=", "_\\|~<>€£¥•", ".,?!'"), page(more = true).lines)
        assertEquals(listOf("¢", "€", "£", "₹", "¥", "₱"), LetterFeedback.symbolHolds('$'))
    }

    @Test fun britainPutsPoundsOnThe123Page() {
        assertEquals('£', LocalCurrency.symbol(locales("en-GB")))
        assertEquals("-/:;()£&@\"", page('£').lines[1])
        // Held, the key offers the $ it took the place of first, then $'s own options without a second £.
        val holds = LetterFeedback.symbolHolds('£', '£')
        assertEquals("$", holds.first())
        assertEquals(listOf("$", "¢", "€", "₹", "¥", "₱"), holds)
        assertEquals(listOf("№"), LetterFeedback.symbolHolds('#', '£'))
    }

    @Test fun theSecondSymbolsPagePutsDollarsWhereItCan() {
        val symbols = page('£', more = true).lines[1]
        assertEquals("_\\|~<>€$¥•", symbols)
        assertEquals(10, symbols.length)
        // The $ it shows there holds the rest too, without another £.
        assertEquals(listOf("¢", "€", "₹", "¥", "₱"), LetterFeedback.symbolHolds('$', '£'))
        // A symbol the page never draws leaves it as it was.
        assertEquals("_\\|~<>€£¥•", page('₹', more = true).lines[1])
    }

    @Test fun germanyAndFrancePutEurosOnThe123Page() {
        assertEquals('€', LocalCurrency.symbol(locales("de-DE")))
        assertEquals('€', LocalCurrency.symbol(locales("fr-FR")))
        assertEquals("-/:;()€&@\"", page('€').lines[1])
        assertEquals("_\\|~<>$£¥•", page('€', more = true).lines[1])
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
        assertEquals(listOf("1234567890", "-/:;()$&@\"", ".,?!'"), page().lines)
    }

    @Test fun everyCurrencyTheLabAcceptsIsOneHexAlreadyDrawsOrOffers() {
        val offered = setOf('£', '€', '¥', '₹', '₱', '₩', '₽', '₴', '¢')
        val codes = mapOf("GBP" to '£', "EUR" to '€', "JPY" to '¥', "CNY" to '¥', "INR" to '₹',
            "PHP" to '₱', "KRW" to '₩', "RUB" to '₽', "UAH" to '₴')
        for ((code, symbol) in codes) {
            assertEquals(symbol, LocalCurrency.symbol(code))
            assertEquals(offered, offered + symbol)
        }
    }
}
