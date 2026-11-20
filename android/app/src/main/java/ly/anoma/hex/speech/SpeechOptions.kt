package ly.anoma.hex.speech

import android.content.Context

internal enum class Style { Polished, Casual, Notes, Verbatim }
internal enum class Spoken(val code: String, val label: String) {
    English("en", "English"), Spanish("es", "Español"), German("de", "Deutsch"), Norwegian("no", "Norsk")
}
internal data class SpeechOptions(val style: Style, val spoken: Spoken, val clean: Boolean, val translate: Boolean)
internal class SpeechPreferences(context: Context) {
    private val prefs = context.getSharedPreferences("speech", Context.MODE_PRIVATE)
    var style: Style
        get() = Style.entries.firstOrNull { it.name == prefs.getString("style", null) } ?: Style.Polished
        set(value) { prefs.edit().putString("style", value.name).apply() }
    var spoken: Spoken
        get() = Spoken.entries.firstOrNull { it.code == prefs.getString("spoken", null) } ?: Spoken.English
        set(value) { prefs.edit().putString("spoken", value.code).apply() }
    var clean: Boolean
        get() = prefs.getBoolean("clean", false)
        set(value) { prefs.edit().putBoolean("clean", value).apply() }
    var translation: Boolean
        get() = prefs.getBoolean("translation", false)
        set(value) { prefs.edit().putBoolean("translation", value).apply() }
    fun options(nextEnglish: Boolean?) = SpeechOptions(style, spoken, clean,
        translation && spoken == Spoken.Spanish && (nextEnglish ?: true))
}
