package ly.anoma.hex.keyboard

import android.content.Context
import android.media.AudioManager
import android.provider.Settings
import kotlin.math.pow

/** The key click each key plays as it goes down. With Labs › Keyboard › Key click volume off ([percent] null) it is
 * Android's own level-less click, which itself follows Touch sounds. On, it is played at [scalar]'s level; Android's
 * volume overload doesn't check Touch sounds, so Hex checks it here first. The system volume still scales both. */
internal object KeyClick {
    const val MIN = 5
    const val MAX = 100
    const val STEP = 5
    const val DEFAULT = 50
    /** Each 10% below 100 is this many dB quieter, so 50% is -15 dB and 5% is -28.5 dB. */
    private const val DB_PER_TEN = 3f

    fun clamp(percent: Int) = percent.coerceIn(MIN, MAX)

    /** The raw amplitude scalar [AudioManager.playSoundEffect] takes for [percent]: 1 at 100%, 3 dB less per 10%. */
    fun scalar(percent: Int): Float = 10f.pow((clamp(percent) - MAX) / 10f * DB_PER_TEN / 20f)

    fun play(context: Context, sound: Int, percent: Int?) {
        val audio = context.getSystemService(AudioManager::class.java) ?: return
        if (percent == null) { audio.playSoundEffect(sound); return }
        if (touchSounds(context)) audio.playSoundEffect(sound, scalar(percent))
    }

    /** Labs › Key click volume's slider: one standard click at the level shown, through the same gate as the keys, so
     * it sounds exactly as they will (nothing with Touch sounds off). */
    fun preview(context: Context, percent: Int) = play(context, AudioManager.FX_KEYPRESS_STANDARD, percent)

    /** Android's Touch sounds (Sound & vibration › Touch sounds), on unless the phone says off. */
    private fun touchSounds(context: Context) =
        Settings.System.getInt(context.contentResolver, Settings.System.SOUND_EFFECTS_ENABLED, 1) != 0
}
