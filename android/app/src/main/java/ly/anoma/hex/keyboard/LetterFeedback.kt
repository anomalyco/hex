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
internal class LetterFeedback(private val host: ViewGroup) {
    private val context = host.context
    private fun dp(n: Int) = (context.resources.displayMetrics.density * n).roundToInt()
    private fun face(id: Int, radius: Int, border: Boolean = false) = GradientDrawable().apply {
        setColor(context.getColor(id)); cornerRadius = dp(radius).toFloat()
        if (border) setStroke(dp(1), context.getColor(R.color.modifier))
    }
    private var panel: LinearLayout? = null
    private var choices: List<String> = emptyList()
    private var index = 0
    private var left = 0
    private var cell = 0
    val choosing get() = choices.size > 1
    fun selected() = choices.getOrNull(index)
    fun clear() { panel?.let { host.overlay.remove(it) }; panel = null; choices = emptyList() }
    fun present(key: View, options: List<String>) {
        clear()
        if (host.width <= 0 || options.isEmpty()) return
        val r = Rect(0, 0, key.width, key.height); host.offsetDescendantRectToMyCoords(key, r)
        val reverse = options.size > 1 && r.centerX() > host.width * .52
        choices = if (reverse) options.reversed() else options
        index = if (reverse) choices.lastIndex else 0
        val pad = dp(3); val edge = dp(4)
        cell = if (choosing) minOf(key.width.coerceIn(dp(28), dp(38)), (host.width - 2 * (edge + pad)) / choices.size) else key.width + dp(6)
        val w = cell * choices.size + 2 * pad; val h = dp(42)
        left = (r.centerX() - pad - cell * index - cell / 2).coerceIn(edge, host.width - w - edge)
        val y = (r.top - h - dp(2)).coerceAtLeast(0)
        val bar = LinearLayout(context).apply {
            background = face(R.color.sheet, 10, true); setPadding(pad, pad, pad, pad)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        }
        for (s in choices) bar.addView(TextView(context).apply {
            text = s; gravity = Gravity.CENTER; setTextColor(context.getColor(R.color.ink))
            useInter(500); textSize = if (choosing) 21f else 27f; includeFontPadding = false
        }, LinearLayout.LayoutParams(cell, -1))
        panel = bar; highlight()
        bar.measure(View.MeasureSpec.makeMeasureSpec(w, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(h, View.MeasureSpec.EXACTLY))
        bar.layout(left, y, left + w, y + h); host.overlay.add(bar)
    }
    fun move(x: Float) {
        if (!choosing) return
        val next = ((x - left - dp(3)) / cell).toInt().coerceIn(0, choices.lastIndex)
        if (index != next) { index = next; highlight() }
    }
    private fun highlight() {
        val bar = panel ?: return
        for (i in choices.indices) {
            val label = bar.getChildAt(i) as TextView
            label.background = if (choosing && i == index) face(R.color.ink, 7) else null
            label.setTextColor(context.getColor(if (choosing && i == index) R.color.page else R.color.ink))
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
                else -> accents[lower]
            }
            return raw.orEmpty().map { (if (c.isUpperCase()) (if (it == 'ß') 'ẞ' else it.uppercaseChar()) else it).toString() }
        }
    }
}
