package ly.anoma.hex.typing

import android.content.Context
import android.content.SharedPreferences
import android.util.Base64

/** Labs › Keyboard › Adaptive touch on this phone: its averages as one table of fixed size ([AdaptiveTouchModel.BYTES],
 * about 31 KB as text) in the private "adaptiveTouch" preferences, which never go into a backup (the app's
 * allowBackup is off), and the one [AdaptiveTouch] the keyboard and Settings share in this process. No letters,
 * words, times or order of taps, and nothing leaves the phone. */
internal object AdaptiveTouchStore {
    const val FILE = "adaptiveTouch"
    private const val MODEL = "model"
    private var shared: AdaptiveTouch? = null

    /** This process's Adaptive touch, read from the phone the first time. */
    fun get(context: Context): AdaptiveTouch {
        shared?.let { return it }
        val prefs = prefs(context)
        val saved = prefs.getString(MODEL, null)?.let { runCatching { Base64.decode(it, Base64.NO_WRAP) }.getOrNull() }
        val model = AdaptiveTouchModel.from(saved)
        // A damaged table, or one of another version, is dropped rather than half read.
        if (model == null && prefs.contains(MODEL)) prefs.edit().remove(MODEL).apply()
        return AdaptiveTouch(model ?: AdaptiveTouchModel()).also { touch ->
            touch.onSave = { bytes -> prefs.edit().putString(MODEL, Base64.encodeToString(bytes, Base64.NO_WRAP)).apply() }
            shared = touch
        }
    }

    /** Whether the phone keeps any averages: Reset touch learning stays reachable with the lab off. */
    fun stored(context: Context) = prefs(context).contains(MODEL)

    /** Reset touch learning and Delete what Hex learned: the averages go from memory and from the phone at once. */
    fun clear(context: Context) {
        shared?.reset()
        prefs(context).edit().remove(MODEL).apply()
    }

    private fun prefs(context: Context): SharedPreferences =
        context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)
}
