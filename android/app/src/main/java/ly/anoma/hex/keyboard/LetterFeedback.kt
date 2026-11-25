package ly.anoma.hex.keyboard

import android.graphics.Rect
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import ly.anoma.hex.R
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** Both callouts live inside the input window. No popup takes focus away from the editor. */
internal class LetterFeedback(private val host: ViewGroup, private val paints: KeyPaints) {
    private val context = host.context
    private fun dp(n: Int) = (context.resources.displayMetrics.density * n).roundToInt()
    private fun face(color: Int, radius: Int, border: Boolean = false) = GradientDrawable().apply {
        setColor(color); cornerRadius = dp(radius).toFloat()
        if (border) setStroke(dp(1), paints.popupRim)
    }
    private var panel: LinearLayout? = null
    // While a held key's list is open the keys behind it fade, as Gboard's do, so the list's edges never
    // sit against half a key.
    private val scrim = View(context).apply {
        setBackgroundColor(context.getColor(R.color.keyboard)); alpha = 0.67f
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
    }
    private var dimmed = false
    // The options shown, nearest the key first; a held key's options are chosen by sliding.
    private var rows: List<List<String>> = emptyList()
    private var row = 0
    private var index = 0
    private var left = 0
    private var bottom = 0
    private var cell = 0
    private var holding = false
    private val pad get() = dp(3)
    private val rowHeight get() = dp(36)
    val choosing get() = holding
    fun selected() = rows.getOrNull(row)?.getOrNull(index)
    fun clear() {
        panel?.let { host.overlay.remove(it) }; panel = null; rows = emptyList(); holding = false
        if (dimmed) { host.overlay.remove(scrim); dimmed = false }
    }
    /** A key the keys were drawn again without, while its finger was down, has nothing to show a pop-up over. */
    private fun inHost(key: View): Boolean {
        var parent = key.parent
        while (parent != null) { if (parent === host) return true; parent = parent.parent }
        return false
    }
    /** The key's pop-up: one option shows the letter above the finger; more are a held key's list in one
     * row, the first under the finger and the rest reading away from the nearer edge. */
    fun present(key: View, options: List<String>) = show(key, listOf(options), 0, options.size > 1)
    /** A held key's options in rows, the first row nearest the key, with [default] of it under the finger;
     * held with a single option, that option still types on lifting. */
    fun hold(key: View, options: List<List<String>>, default: Int = 0) = show(key, options, default, true)
    private fun show(key: View, options: List<List<String>>, default: Int, hold: Boolean) {
        clear()
        if (host.width <= 0 || options.firstOrNull().isNullOrEmpty() || !inHost(key)) return
        val r = Rect(0, 0, key.width, key.height); host.offsetDescendantRectToMyCoords(key, r)
        val reverse = hold && options.size == 1 && options[0].size > 1 && r.centerX() > host.width * .52
        rows = if (reverse) listOf(options[0].reversed()) else options
        holding = hold; row = 0
        index = (if (reverse) rows[0].lastIndex - default else default).coerceIn(0, rows[0].lastIndex)
        val columns = rows.maxOf { it.size }
        val edge = dp(4)
        cell = if (hold) minOf(key.width.coerceIn(dp(28), dp(38)), (host.width - 2 * (edge + pad)) / columns) else key.width + dp(6)
        val w = cell * columns + 2 * pad; val h = rowHeight * rows.size + 2 * pad
        left = (r.centerX() - pad - cell * index - cell / 2).coerceIn(edge, host.width - w - edge)
        val y = (r.top - h - dp(2)).coerceAtLeast(0)
        bottom = y + h
        val bar = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            background = face(paints.popup, 10, true); setPadding(pad, pad, pad, pad)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        }
        for (line in rows.asReversed()) bar.addView(LinearLayout(context).apply {
            for (s in line) addView(TextView(context).apply {
                text = s; gravity = Gravity.CENTER; setTextColor(paints.ink)
                useInter(500); textSize = if (hold) 21f else 27f; includeFontPadding = false
            }, LinearLayout.LayoutParams(cell, -1))
        }, LinearLayout.LayoutParams(-1, rowHeight))
        panel = bar; highlight()
        bar.measure(View.MeasureSpec.makeMeasureSpec(w, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(h, View.MeasureSpec.EXACTLY))
        bar.layout(left, y, left + w, y + h)
        if (hold) { scrim.layout(0, 0, host.width, host.height); host.overlay.add(scrim); dimmed = true }
        host.overlay.add(bar)
    }
    /** The finger moved to ([x], [y]) in the host's coordinates: the option under it, the nearest row's below the list. */
    fun move(x: Float, y: Float) {
        if (!holding) return
        val nextRow = ((bottom - pad - y) / rowHeight).toInt().coerceIn(0, rows.lastIndex)
        val next = ((x - left - pad) / cell).toInt().coerceIn(0, rows[nextRow].lastIndex)
        if (row != nextRow || index != next) { row = nextRow; index = next; highlight() }
    }
    private fun highlight() {
        val bar = panel ?: return
        for ((r, line) in rows.withIndex()) {
            val cells = bar.getChildAt(rows.lastIndex - r) as LinearLayout
            for (i in line.indices) {
                val label = cells.getChildAt(i) as TextView
                val on = holding && r == row && i == index
                label.background = if (on) face(paints.choice, 7) else null
                label.setTextColor(if (on) paints.choiceInk else paints.ink)
            }
        }
    }
    companion object {
        private val accents = mapOf('e' to "eèéêëēėę", 'a' to "aàáâäæãåā", 'i' to "iìíîïīį",
            'o' to "oôöòóœøōõ", 'u' to "uûüùúū", 'y' to "yÿ", 'n' to "nñń", 'c' to "cçćč",
            's' to "sßśš", 'z' to "zžźż", 'l' to "lł", 'ñ' to "ñn")
        fun options(c: Char, language: String): List<String> {
            val lower = c.lowercaseChar()
            val raw = when (language) {
                "es" -> when (lower) {
                    'a' -> "aáàäâãåąæāª"; 'e' -> "eéèëêęėē"; 'i' -> "iíïìîįī"; 'o' -> "oóòöôõøœōº"; 'u' -> "uúüùûū"; else -> accents[lower]
                }
                "nb" -> when (lower) {
                    'a' -> "aàáâäãā"; 'o' -> "oôöòóœōõ"; 'æ' -> "æä"; 'ø' -> "øö"; else -> accents[lower]
                }
                // Gboard's French and Italian long-press lists (Pixel 8 emulator, black box), most used first;
                // the apostrophe key holds the typographic quotes Gboard offers there.
                "fr" -> when (lower) {
                    'a' -> "aàâæáäãåāª"; 'e' -> "eéèêëēęė"; 'i' -> "iîïìíīįĩ"; 'o' -> "oôœöòóõøōº"; 'u' -> "uùûüúūũ"
                    'y' -> "yÿ"; 'c' -> "cçćč"; 'n' -> "nñ"; 's' -> "sß"; '\'' -> "'’‘‚‹›"; else -> null
                }
                "it" -> when (lower) {
                    'a' -> "aàáâäæãåāª"; 'e' -> "eèéêëęėē"; 'i' -> "iìíîïĩīį"; 'o' -> "oòóôöõøœōº"; 'u' -> "uùúûüũū"
                    'c' -> "cçčć"; 'n' -> "nñ"; 's' -> "sß"; else -> null
                }
                "de" -> when (lower) {
                    'a' -> "aàáâæãåā"; 'o' -> "oôòóœøōõ"; 'u' -> "uûùúū"; else -> accents[lower]
                }
                // iPhone Hex's Portuguese long presses (KeyboardLanguage.swift), Portugal's and Brazil's
                // letters first; other letters hold nothing.
                "pt" -> when (lower) {
                    'a' -> "aáãàâäåæª"; 'e' -> "eéêèęėēë"; 'i' -> "iíîìïįī"; 'o' -> "oôóõòöœøōº"; 'u' -> "uúüùûū"
                    'c' -> "cç"; 'n' -> "nñ"; else -> null
                }
                else -> accents[lower]
            }
            return raw.orEmpty().map { (if (c.isUpperCase()) (if (it == 'ß') 'ẞ' else it.uppercaseChar()) else it).toString() }
        }

        /** What a held letter offers: on row 1 its digit first, under the finger (Gboard types the digit
         * when a held q to p lifts), then its accents with the letter itself. With [symbolsOnLetters], a
         * letter with no digit to show offers its symbol first instead: Gboard's on the second and third
         * rows. On the top row, while the number row holds its digit, Gboard's set only fills in for a
         * letter with no home- or bottom-row symbol of its own. [currency] is Labs › Keyboard › Local
         * currency's symbol, which takes $'s place on d as it does on the ?123 page. */
        fun holds(c: Char, language: String, digit: Char?, symbolsOnLetters: Boolean = false,
                  topRow: Boolean = false, currency: Char? = null): List<String> {
            val accents = options(c, language)
            val first = digit?.toString() ?: (if (symbolsOnLetters) letterSymbol(c, topRow, currency)?.toString() else null)
            return if (first == null) accents else listOf(first) + accents
        }

        // Gboard's symbols on the second and third letter rows, the same the ?123 page draws there, keyed
        // by letter so every layout keeps them wherever it draws that letter. A row-1 letter with none of
        // its own keeps the digit Hex already offers.
        private val letterSymbols = mapOf('a' to '@', 's' to '#', 'd' to '$', 'f' to '_', 'g' to '&',
            'h' to '-', 'j' to '+', 'k' to '(', 'l' to ')', 'z' to '*', 'x' to '"', 'c' to '\'',
            'v' to ':', 'b' to ';', 'n' to '!', 'm' to '?')
        // Gboard's top row, which the number row leaves without digits. Keyed by letter too, but offered
        // only while the letter is drawn on row 1, so AZERTY's q/w and QWERTZ's y keep none where their
        // layouts draw them lower.
        private val topRowSymbols = mapOf('q' to '%', 'w' to '\\', 'e' to '|', 'r' to '=', 't' to '[',
            'y' to ']', 'u' to '<', 'i' to '>', 'o' to '{', 'p' to '}')
        fun letterSymbol(c: Char, topRow: Boolean = false, currency: Char? = null): Char? {
            val lower = c.lowercaseChar()
            // A local currency replaces Gboard's $ on d, as it does on the ?123 page's own key.
            if (lower == 'd' && currency != null) return currency
            return letterSymbols[lower] ?: (if (topRow) topRowSymbols[lower] else null)
        }

        // Gboard's held symbols (Pixel 8 emulator, API 36, black box): the option Gboard selects first,
        // then the rest left to right. Digits offer themselves first. Each key here is drawn on one of the
        // two symbol pages, < and > as the =< page's keys beside space; ? and ! hold Spanish's ¿ and ¡,
        // which neither page has a key of its own for.
        private val symbols = mapOf('1' to "1½⅓¼⅕⅙⅐⅛⅑⅒", '2' to "2⅔⅖", '3' to "3⅗¾⅜", '4' to "4⅘", '5' to "5⅝⅚",
            '7' to "7⅞", '0' to "0∅ⁿ", '#' to "№", '$' to "¢€£₹¥₱", '-' to "_—–·", '+' to "±", '(' to "<[{",
            ')' to ">]}", '*' to "†★‡", '"' to "”„“«»", '\'' to "’‚‘‹›", '!' to "¡", '?' to "¿‽", '.' to "…",
            '%' to "‰℅", '^' to "↑←↓→", '=' to "≠∞≈", '<' to "≤«‹⟨", '>' to "≥⟩»›", '•' to "♪♣♠♥♦")
        /** A symbol key's held options. With a Labs local currency in [currency]'s place, that key offers the
         * $ it replaced first and the list has no duplicate of itself. */
        fun symbolHolds(c: Char, currency: Char? = null): List<String> {
            val held = if (currency != null && c == currency) "\$" + symbols.getValue('$') else symbols[c].orEmpty()
            return held.filter { it != currency }.map { it.toString() }
        }

        /** The period key's held punctuation on the letters page, as Gboard's three rows (nearest the key
         * first), with the comma under the finger. */
        val periodHolds = listOf(listOf("(", ")", "#", "!", ",", "?"), listOf("-", ":", "@", "'", "/", ";"),
            listOf("·", "_", "&", "%", "\"", "+"))
        const val PERIOD_DEFAULT = 4
    }
}
