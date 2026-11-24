package ly.anoma.hex.keyboard

import android.content.Context
import android.view.inputmethod.InputMethodManager
import android.view.inputmethod.InputMethodSubtype
import ly.anoma.hex.R

/** Labs › Keyboard › Voice input mode: Hex's second subtype, added only while the lab is on, so another
 * keyboard stays the main one and Hex opens from the keyboard switcher or another keyboard's voice input
 * shortcut just to dictate. Android calls that kind of one-shot subtype auxiliary: the IME switcher lists it
 * (getShortcutInputMethodsAndSubtypes() finds it by its "voice" mode), Settings never makes it the default,
 * and switchToLastInputMethod never returns to it. */
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
     * whose language matches the system's; its label has no format specifier, so the picker shows it as written. */
    fun subtype(context: Context): InputMethodSubtype {
        val locale = context.resources.configuration.locales[0]
        return InputMethodSubtype.InputMethodSubtypeBuilder()
            .setSubtypeId(SUBTYPE_ID)
            .setSubtypeNameResId(R.string.voice_input_label)
            .setSubtypeLocale(locale.toString())
            .setLanguageTag(locale.toLanguageTag())
            .setSubtypeMode(MODE)
            .setIsAuxiliary(true)
            .setIsAsciiCapable(false)
            .build()
    }

    /** Adds the voice subtype while [on], else removes it. Android keeps additional subtypes across reboots,
     * so every launch and every IME start calls this again with the saved toggle. */
    @Suppress("DEPRECATION") // The app API for a runtime subtype; deprecated in 29 but still served in 36.
    fun apply(context: Context, on: Boolean) {
        val manager = context.getSystemService(InputMethodManager::class.java) ?: return
        val subtypes: Array<InputMethodSubtype> = if (on) arrayOf(subtype(context)) else emptyArray()
        manager.setAdditionalInputMethodSubtypes(imeId(context), subtypes)
    }
}
