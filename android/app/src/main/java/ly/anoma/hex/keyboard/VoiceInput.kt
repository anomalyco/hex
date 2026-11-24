package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.Resources
import android.view.inputmethod.InputMethodManager
import android.view.inputmethod.InputMethodSubtype
import java.util.Locale
import ly.anoma.hex.R
import ly.anoma.hex.speech.SpeechPhase

/** Labs › Keyboard › Voice input mode: Hex's second subtype, added only while the lab is on, so another
 * keyboard stays the main one and Hex opens from the keyboard switcher (or a keyboard that asks Android for
 * its voice input) just to dictate. Android calls that one-shot kind auxiliary: the switcher lists it,
 * Settings never makes it the default, and switchToLastInputMethod never returns to it. */
internal object VoiceInput {
    const val MODE = "voice"
    /** Android remembers enabled subtypes by this id, so adding the same one again after the lab was off
     * keeps it enabled. */
    private const val SUBTYPE_ID = 1

    /** Whether Hex shows the voice bar instead of its keys: the lab is on and Android started Hex in the
     * "voice" subtype. Off, or in the full keyboard subtype, it is the keyboard it always was. */
    fun active(lab: Boolean, subtypeMode: String?) = lab && subtypeMode == MODE

    /** Android's id for this IME, the same one InputMethodInfo.getId() returns. */
    fun imeId(context: Context) = context.packageName + "/" + HexKeyboard::class.java.name

    /** The voice subtype. Its language is the phone's, because Android enables implicitly only the subtypes
     * whose language matches the system's (while Hex's languages follow the system); its label has no format
     * specifier, so the picker shows it as written. */
    fun subtype(): InputMethodSubtype {
        // The phone's language, not a context's: the app's resources can follow another locale.
        val (locale, tag) = subtypeLocale(Resources.getSystem().configuration.locales[0])
        return InputMethodSubtype.InputMethodSubtypeBuilder()
            .setSubtypeId(SUBTYPE_ID)
            .setSubtypeNameResId(R.string.voice_input_label)
            .setSubtypeLocale(locale)
            .setLanguageTag(tag)
            .setSubtypeMode(MODE)
            .setIsAuxiliary(true)
            .setIsAsciiCapable(false)
            .build()
    }

    /** Adds or removes the voice subtype, but only when Android's own list disagrees: app start, a package
     * update and a locale change all call this whether or not anything changed. */
    @Suppress("DEPRECATION") // The app API for a runtime subtype; deprecated in 29 but still served in 36.
    fun apply(context: Context, on: Boolean) {
        val manager = context.getSystemService(InputMethodManager::class.java) ?: return
        val wanted = subtype()
        if (!subtypeSyncNeeded(on, VoiceSubtype(wanted.locale, wanted.languageTag), registered(manager, context))) return
        val subtypes: Array<InputMethodSubtype> = if (on) arrayOf(wanted) else emptyArray()
        manager.setAdditionalInputMethodSubtypes(imeId(context), subtypes)
    }

    /** Android's record of the voice subtype, or null while it holds none. */
    private fun registered(manager: InputMethodManager, context: Context): VoiceSubtype? {
        val id = imeId(context)
        val info = manager.inputMethodList.firstOrNull { it.id == id } ?: return null
        val subtype = (0 until info.subtypeCount).map { info.getSubtypeAt(it) }
            .firstOrNull { it.mode == MODE } ?: return null
        return VoiceSubtype(subtype.locale, subtype.languageTag)
    }
}

/** What Android holds for Hex's voice subtype: the locale and language tag it recorded. */
internal data class VoiceSubtype(val locale: String, val languageTag: String)

/** Whether Android's subtype list needs this write: nothing while the lab is on, the wrong locale after the
 * phone's language changed, or a leftover while it is off. */
internal fun subtypeSyncNeeded(on: Boolean, wanted: VoiceSubtype, registered: VoiceSubtype?) =
    if (on) registered != wanted else registered != null

/** Whether a start in [mode] arms the one automatic listen: only a move into "voice" from another mode. */
internal fun armsVoice(mode: String?, handled: String?) = mode == VoiceInput.MODE && handled != mode

/** Whether a voice commit may leave for the usual keyboard: only once nothing records or transcribes. */
internal fun voiceCanLeave(leaving: Boolean, phase: SpeechPhase) = leaving && phase == SpeechPhase.Idle

/** The subtype locale and language tag for a phone locale: language or language_COUNTRY, never a script or
 * extension, so zh-Hans-CN becomes zh_CN with zh-CN. */
internal fun subtypeLocale(locale: Locale): Pair<String, String> =
    if (locale.country.isEmpty()) locale.language to locale.language
    else (locale.language + "_" + locale.country) to (locale.language + "-" + locale.country)
