package ly.anoma.hex.keyboard

import android.content.ActivityNotFoundException
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.provider.Settings
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
    fun shows(lab: Boolean, subtypeMode: String?) = lab && subtypeMode == MODE

    /** The Labs toggle: registers or clears the subtype and, where Android can't enable it, opens its screen. */
    fun setEnabledFromSettings(context: Context, enabled: Boolean) {
        setEnabled(context, enabled)
        if (enabled && Build.VERSION.SDK_INT < 34) openSubtypeSettings(context)
    }

    /**
     * Registers the voice subtype, or clears it when the lab goes off. An added subtype is not enabled by
     * itself (it has no locale and is auxiliary), so Android 14 and later enables it explicitly; Android 12
     * and 13 have no such API and use [openSubtypeSettings] instead.
     */
    @Suppress("DEPRECATION") // Dynamic subtypes have no replacement; setExplicitlyEnabled… needs Android 14.
    fun setEnabled(context: Context, enabled: Boolean) {
        val manager = context.getSystemService(InputMethodManager::class.java) ?: return
        val subtypes: Array<InputMethodSubtype> = if (enabled) arrayOf(subtype(context)) else emptyArray()
        try {
            manager.setAdditionalInputMethodSubtypes(imeId(context), subtypes)
            if (Build.VERSION.SDK_INT >= 34) {
                val hashes = if (enabled) enabledSubtypes(manager, context) else intArrayOf()
                if (hashes != null) manager.setExplicitlyEnabledInputMethodSubtypes(imeId(context), hashes)
            }
        } catch (_: RuntimeException) {
            // The system may not list the IME yet (a fresh install); a later registration or the subtype
            // screen fixes it.
        }
    }

    /** Android 12 and 13 can't enable an added subtype: Android's Hex subtype screen lets the person tick it. */
    private fun openSubtypeSettings(context: Context) {
        try {
            context.startActivity(Intent(Settings.ACTION_INPUT_METHOD_SUBTYPE_SETTINGS)
                .putExtra(Settings.EXTRA_INPUT_METHOD_ID, imeId(context))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (_: ActivityNotFoundException) {
        }
    }

    /** Android 14's explicit set: Hex's full keyboard subtype and the voice subtype. Null when the IME isn't
     * listed yet, so the system's implicit defaults stay instead of dropping the full keyboard. */
    private fun enabledSubtypes(manager: InputMethodManager, context: Context): IntArray? {
        val id = imeId(context)
        val info = manager.inputMethodList.firstOrNull { it.id == id } ?: return null
        val keyboard = (0 until info.subtypeCount).map(info::getSubtypeAt).firstOrNull { it.mode != MODE }
            ?: return null
        return intArrayOf(keyboard.hashCode(), subtype(context).hashCode())
    }

    private fun subtype(context: Context) = InputMethodSubtype.InputMethodSubtypeBuilder()
        .setSubtypeNameResId(R.string.voice_only)
        .setSubtypeMode(MODE)
        .setSubtypeId(SUBTYPE_ID)
        .setIsAuxiliary(true)
        .setIsAsciiCapable(false)
        .build()

    /** The IME's id as the system names it: `ly.anoma.hex/.keyboard.HexKeyboard`. */
    private fun imeId(context: Context) = ComponentName(context, HexKeyboard::class.java).flattenToShortString()
}
