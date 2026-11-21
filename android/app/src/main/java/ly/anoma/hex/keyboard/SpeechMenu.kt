package ly.anoma.hex.keyboard

import android.content.Context
import android.view.Gravity
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import ly.anoma.hex.R
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.speech.Style
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** Stays inside the IME window; opening a choice never changes the active editor. */
internal class SpeechMenu(context: Context, private val ime: HexKeyboard,
                          private val title: (String) -> Unit, private val close: () -> Unit) : ScrollView(context) {
    private val body = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    private var page = "Hex menu"
    private val prefs get() = ime.preferences
    init { addView(body); isFillViewport = true }
    private fun dp(n: Int) = (resources.displayMetrics.density * n).roundToInt()
    fun home() = show("Hex menu")
    fun language() = show("Language")
    fun accents(values: List<String>, pick: (String) -> Unit) {
        page = "Accents"; title(page); body.removeAllViews(); scrollTo(0, 0)
        values.forEach { s -> row(s, "") { pick(s) } }
    }
    fun back() { if (page == "Hex menu") close() else home() }

    private fun show(next: String) {
        page = next; title(next); body.removeAllViews(); scrollTo(0, 0)
        when (next) {
            "Hex menu" -> {
                row("Style", prefs.style.name + "  ›") { show("Style") }
                row("Language", (prefs.language?.label ?: "Automatic") + "  ›") { show("Language") }
                if (prefs.translation && prefs.spoken == Spoken.Spanish)
                    row("Next dictation", (if (ime.nextEnglish != false) "Paste in English" else "Paste as spoken") + "  ›") {
                        show("Next dictation")
                    }
                row("Typing feedback", "›") { show("Typing feedback") }
                row("Clean up what I say", if (prefs.clean) "On" else "Off") { prefs.clean = !prefs.clean; home() }
                row("Translate what I say", if (prefs.translation) "On" else "Off") {
                    prefs.translation = !prefs.translation; ime.nextEnglish = null; home()
                }
            }
            "Typing feedback" -> {
                val f = ime.feedback
                row("Key pop-ups", if (f.popups) "On" else "Off") { f.popups = !f.popups; show(next) }
                row("Key haptics", if (f.haptics) "On" else "Off") { f.haptics = !f.haptics; show(next) }
            }
            "Style" -> Style.entries.forEach { value -> row(value.name, if (prefs.style == value) "✓" else "") {
                prefs.style = value; home()
            } }
            "Language" -> (listOf(null) + Spoken.entries).forEach { value ->
                row(value?.label ?: "Automatic", if (prefs.language == value) "✓" else "") {
                    prefs.language = value; ime.nextEnglish = null; close()
                }
            }
            else -> listOf(true, false).forEach { english ->
                row(if (english) "Paste in English" else "Paste as spoken", if ((ime.nextEnglish ?: true) == english) "✓" else "") {
                    ime.nextEnglish = english; close()
                }
            }
        }
    }

    private fun row(label: String, value: String, click: () -> Unit) {
        val line = LinearLayout(context).apply {
            gravity = Gravity.CENTER_VERTICAL; setPadding(dp(16), dp(4), dp(16), dp(4))
            minimumHeight = dp(44); isFocusable = true
            contentDescription = if (value.isEmpty()) label else label + ", " + value.trimEnd(' ', '›')
            setOnClickListener { click() }
        }
        fun text(s: String, weight: Int, color: Int) = TextView(context).apply {
            text = s; textSize = 15f; useInter(weight); includeFontPadding = false
            setTextColor(context.getColor(color)); importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        }
        line.addView(text(label, 500, R.color.ink), LinearLayout.LayoutParams(0, -2, 1f))
        line.addView(text(value, 450, R.color.quiet))
        body.addView(line)
    }
}
