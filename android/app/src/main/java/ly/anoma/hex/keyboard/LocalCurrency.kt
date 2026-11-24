package ly.anoma.hex.keyboard

import android.os.LocaleList
import java.util.Currency
import java.util.Locale

/** Labs › Keyboard › Local currency: the ?123 page's $ becomes the phone's own from its region, when its
 * currency has one of a fixed set of single-character symbols. The currency code decides, so a symbol
 * never changes with the phone's language; no country, no currency, an unknown one and USD keep $. */
internal object LocalCurrency {
    private val symbols = mapOf("GBP" to '£', "EUR" to '€', "JPY" to '¥', "CNY" to '¥', "INR" to '₹',
        "PHP" to '₱', "KRW" to '₩', "RUB" to '₽', "UAH" to '₴')

    /** [currencyCode]'s symbol, or null when Hex has none for it. */
    fun symbol(currencyCode: String): Char? = symbols[currencyCode]

    /** The symbol of the first locale in [locales] with a country, read by its currency code, or null. */
    fun symbol(locales: List<Locale>): Char? {
        val locale = locales.firstOrNull { it.country.isNotEmpty() } ?: return null
        return try { Currency.getInstance(locale)?.currencyCode?.let(::symbol) } catch (_: IllegalArgumentException) { null }
    }

    /** The phone's own symbol, read once for the page and not per key. */
    fun local(): Char? {
        val locales = LocaleList.getDefault()
        return symbol((0 until locales.size()).map { locales[it] })
    }
}
