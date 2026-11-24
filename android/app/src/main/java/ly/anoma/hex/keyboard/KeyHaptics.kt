package ly.anoma.hex.keyboard

import android.annotation.SuppressLint
import android.content.Context
import android.content.res.Resources
import android.media.AudioAttributes
import android.os.Build
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.provider.Settings
import java.util.concurrent.atomic.AtomicInteger

/** Hex's taps, Android's own keyboard pair (HapticFeedbackVibrationProvider: a key press is EFFECT_CLICK, a
 * release EFFECT_TICK). Each plays the phone's predefined effect where its vibrator says it has one, or a
 * short pulse at the phone's default strength where it doesn't. */
internal enum class Tap(private val predefined: Int, private val millis: Long) {
    /** A key going down, or an emoji picked: a click. */
    Press(VibrationEffect.EFFECT_CLICK, 20),
    /** A key held [KeyboardMetrics.RELEASE_TICK_MS] or more lifting: Gboard's second tick. */
    Release(VibrationEffect.EFFECT_TICK, 12),
    /** Dictation starting: a click. */
    Start(VibrationEffect.EFFECT_CLICK, 20),
    /** Dictation ending (done, cancelled or failed): a tick. */
    End(VibrationEffect.EFFECT_TICK, 12);

    fun effect(vibrator: Vibrator): VibrationEffect = when (val play = plays(vibrator)) {
        is TapPlay.Predefined -> VibrationEffect.createPredefined(play.effect)
        is TapPlay.Pulse -> VibrationEffect.createOneShot(play.millis, play.amplitude)
    }

    fun plays(vibrator: Vibrator) = plays(vibrator.areAllEffectsSupported(predefined) == Vibrator.VIBRATION_EFFECT_SUPPORT_YES)

    /** What this tap plays on a vibrator that does or doesn't offer its predefined effect; also what Labs › Diagnostics
     * names. */
    fun plays(offered: Boolean): TapPlay =
        if (offered) TapPlay.Predefined(predefined) else TapPlay.Pulse(millis, VibrationEffect.DEFAULT_AMPLITUDE, predefined)
}

internal sealed interface TapPlay {
    /** Android's predefined [effect] (VibrationEffect.EFFECT_*). */
    data class Predefined(val effect: Int) : TapPlay
    /** A one-shot pulse of [millis] at [amplitude] (1–255, or DEFAULT_AMPLITUDE), because the vibrator doesn't offer [missing]. */
    data class Pulse(val millis: Long, val amplitude: Int, val missing: Int) : TapPlay
}

/** Hex's Haptics setting on, and the system's Keyboard vibration switch too where the phone has one. */
internal fun vibrates(context: Context, haptics: Boolean) =
    haptics && keyboardVibrationAllows(keyboardVibrationSupported,
        Settings.System.getInt(context.contentResolver, "keyboard_vibration_enabled", 1))

/** Android's own rule (VibrationSettings): the stored Keyboard vibration value counts only on a phone that
 * declares the setting; elsewhere keyboard taps follow touch feedback, whatever value is stored. */
internal fun keyboardVibrationAllows(supported: Boolean, stored: Int) = !supported || stored != 0

/** Whether the phone declares Android's Keyboard vibration setting (Pixel does); a phone without the
 * resource doesn't. */
private val keyboardVibrationSupported: Boolean by lazy @SuppressLint("DiscouragedApi") {
    val system = Resources.getSystem()
    val id = system.getIdentifier("config_keyboardVibrationSettingsSupported", "bool", "android")
    id != 0 && system.getBoolean(id)
}

/** Plays [tap] on the vibrator itself rather than through View.performHapticFeedback, whose keyboard
 * effects each phone maker may tie to its own keyboard's settings (One UI does), so the Haptics switch is
 * felt on every phone; only this path plays, so a press is never felt twice. The taps are touch feedback
 * (USAGE_TOUCH; before Android 13, sonification audio attributes, which Android maps to it), so the
 * system's touch vibration setting and Do Not Disturb apply to them as to Android's own. */
internal fun tap(context: Context, haptics: Boolean, tap: Tap) {
    if (!vibrates(context, haptics)) { TapCounts.stopped.incrementAndGet(); return }
    val vibrator = context.getSystemService(VibratorManager::class.java)?.defaultVibrator ?: return
    if (!vibrator.hasVibrator()) return
    val effect = tap.effect(vibrator)
    TapCounts.asked.incrementAndGet()
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU)
        vibrator.vibrate(effect, VibrationAttributes.createForUsage(VibrationAttributes.USAGE_TOUCH))
    else @Suppress("DEPRECATION") vibrator.vibrate(effect, AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
}

/** Labs › Diagnostics' haptics counts, in memory only: taps that reached the vibrator, and taps Hex's settings stopped. */
internal object TapCounts {
    val asked = AtomicInteger()
    val stopped = AtomicInteger()
}
