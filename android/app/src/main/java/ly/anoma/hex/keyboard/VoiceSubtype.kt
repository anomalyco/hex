package ly.anoma.hex.keyboard

import android.content.ComponentName
import android.content.Context
import android.view.inputmethod.InputMethodManager
import android.view.inputmethod.InputMethodSubtype
import ly.anoma.hex.R

/** Labs › Keyboard › Voice-only mode: a "voice" subtype other keyboards switch to for dictation alone.
 * It is registered at runtime, never in input_method.xml, so Hex with the lab off is the keyboard it was. */
internal object VoiceOnly {
    /** The subtype's mode, which [shows] reads back to tell the voice bar from the full keyboard. */
    const val MODE = "voice"

    /** Stable, so registering it again keeps its enabled state instead of adding a second copy. */
    private const val SUBTYPE_ID = 0x564F4943 // "VOIC"

    /** Whether the bar replaces the keyboard: the lab is on and the current subtype is the voice one. */
    fun shows(lab: Boolean, subtypeMode: String?): Boolean = lab && subtypeMode == MODE

    /** Registers the voice subtype, or clears it when the lab goes off. */
    @Suppress("DEPRECATION") // Dynamic subtypes have no replacement since API 29; a static one changes Hex with the lab off.
    fun setEnabled(context: Context, enabled: Boolean) {
        val manager = context.getSystemService(InputMethodManager::class.java) ?: return
        val subtypes: Array<InputMethodSubtype> = if (enabled) arrayOf(subtype(context)) else emptyArray()
        try {
            manager.setAdditionalInputMethodSubtypes(imeId(context), subtypes)
        } catch (_: RuntimeException) {
            // The system may not list the IME yet (a fresh install); its onCreate registers it again.
        }
    }

    private fun subtype(context: Context) = InputMethodSubtype.InputMethodSubtypeBuilder()
        .setSubtypeNameResId(R.string.voice_only)
        .setSubtypeMode(MODE)
        .setSubtypeId(SUBTYPE_ID)
        .setIsAsciiCapable(false)
        .build()

    /** The IME's id as the system names it: `ly.anoma.hex/.keyboard.HexKeyboard`. */
    private fun imeId(context: Context) = ComponentName(context, HexKeyboard::class.java).flattenToShortString()
}
