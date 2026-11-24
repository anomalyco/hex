package ly.anoma.hex.keyboard

import android.content.Context
import android.view.Gravity
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import ly.anoma.hex.R
import ly.anoma.hex.label
import ly.anoma.hex.speech.DictationLanguage
import ly.anoma.hex.speech.Style
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** Stays inside the IME window; opening a choice never changes the active editor. */
internal class SpeechMenu(context: Context, private val ime: HexKeyboard,
                          private val title: (String) -> Unit, private val close: () -> Unit) : ScrollView(context) {
    private enum class Page(val title: Int) {
        Home(R.string.keyboard_menu), Style(R.string.keyboard_menu_style), Language(R.string.keyboard_menu_language),
        NextDictation(R.string.keyboard_menu_next_dictation), Feedback(R.string.keyboard_menu_feedback),
        Accents(R.string.keyboard_menu_accents),
    }
    private val body = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    private var page = Page.Home
    private val prefs get() = ime.preferences
    init { addView(body); isFillViewport = true }
    private fun dp(n: Int) = (resources.displayMetrics.density * n).roundToInt()
    private fun s(id: Int) = context.getString(id)
    private fun more(value: String) = context.getString(R.string.keyboard_menu_more_value, value)
    private fun onOff(on: Boolean) = s(if (on) R.string.keyboard_on else R.string.keyboard_off)
    private val chosen get() = s(R.string.keyboard_menu_chosen)
    fun home() = show(Page.Home)
    fun language() = show(Page.Language)
    fun accents(values: List<String>, pick: (String) -> Unit) {
        page = Page.Accents; title(s(page.title)); body.removeAllViews(); scrollTo(0, 0)
        values.forEach { v -> row(v, "") { pick(v) } }
    }
    /** Automatic with the language it dictates in now, in that language's own name ("Automatic · English"): it
     * follows the keyboard layout or the phone, never the speech, so the menu says which one it is. With Labs › Automatic
     * listens for the language choosing among several, it names none ("Automatic"), and its entry in the list names
     * them all ([choices]: "Automatic · English, Español"). */
    private fun automatic(choices: Boolean = false): String {
        val among = prefs.automaticCandidates
        if (among.size <= 1) return s(R.string.keyboard_menu_automatic) + " · " + among.first().label
        return s(R.string.keyboard_menu_automatic) + if (choices) " · " + among.joinToString(s(R.string.language_list_separator)) { it.label } else ""
    }
    /** More characters came from a key, so going back leaves the menu. */
    fun back() { if (page == Page.Home || page == Page.Accents) close() else home() }

    private fun show(next: Page) {
        page = next; title(s(next.title)); body.removeAllViews(); scrollTo(0, 0)
        when (next) {
            Page.Home -> {
                row(s(R.string.keyboard_menu_style), more(s(prefs.style.label))) { show(Page.Style) }
                row(s(R.string.keyboard_menu_language), more(prefs.language?.label ?: automatic())) { show(Page.Language) }
                if (prefs.translation && prefs.translatable)
                    row(s(R.string.keyboard_menu_next_dictation),
                        more(s(if (ime.nextEnglish != false) R.string.keyboard_paste_english else R.string.keyboard_paste_spoken))) {
                        show(Page.NextDictation)
                    }
                row(s(R.string.keyboard_menu_feedback), s(R.string.keyboard_menu_arrow)) { show(Page.Feedback) }
                row(s(R.string.keyboard_menu_clean), onOff(prefs.clean)) { prefs.clean = !prefs.clean; home() }
                row(s(R.string.keyboard_menu_translate), onOff(prefs.translation)) {
                    prefs.translation = !prefs.translation; ime.nextEnglish = null; home()
                }
                // The system's own switcher sits in the navigation bar; this reaches the next keyboard without it.
                row(s(R.string.keyboard_next), "") { close(); ime.nextIme() }
            }
            Page.Feedback -> {
                val f = ime.feedback
                row(s(R.string.keyboard_menu_popups), onOff(f.popups)) { f.popups = !f.popups; show(next) }
                row(s(R.string.keyboard_menu_haptics), onOff(f.haptics)) { f.haptics = !f.haptics; show(next) }
            }
            Page.Style -> Style.entries.forEach { value -> row(s(value.label), if (prefs.style == value) chosen else "") {
                prefs.style = value; home()
            } }
            Page.Language -> (listOf(null) + DictationLanguage.entries).forEach { value ->
                row(value?.label ?: automatic(choices = true), if (prefs.language == value) chosen else "") {
                    prefs.language = value; ime.nextEnglish = null; close()
                }
            }
            Page.NextDictation -> listOf(true, false).forEach { english ->
                row(s(if (english) R.string.keyboard_paste_english else R.string.keyboard_paste_spoken),
                    if ((ime.nextEnglish ?: true) == english) chosen else "") {
                    ime.nextEnglish = english; close()
                }
            }
            Page.Accents -> Unit
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
