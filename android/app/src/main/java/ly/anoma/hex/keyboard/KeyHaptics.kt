package ly.anoma.hex.keyboard

import android.content.Context
import android.media.AudioAttributes
import android.os.Build
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.provider.Settings
import java.util.concurrent.atomic.AtomicInteger

/** Hex's taps. Each plays the phone's predefined effect where its vibrator says it has one, or a short,
 * light pulse where it doesn't. */
internal enum class Tap(private val predefined: Int, private val millis: Long) {
    /** A key going down, or an emoji picked: a light tick. */
    Press(VibrationEffect.EFFECT_TICK, 12),
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

    fun plays(vibrator: Vibrator) =
        plays(vibrator.areAllEffectsSupported(predefined) == Vibrator.VIBRATION_EFFECT_SUPPORT_YES, vibrator.hasAmplitudeControl())

    /** What this tap plays on a vibrator that does or doesn't offer its predefined effect and amplitude control; also
     * what Labs › Diagnostics names. */
    fun plays(offered: Boolean, amplitude: Boolean): TapPlay =
        if (offered) TapPlay.Predefined(predefined)
        else TapPlay.Pulse(millis, if (amplitude) LIGHT else VibrationEffect.DEFAULT_AMPLITUDE, predefined)

    private companion object { const val LIGHT = 80 }
}

internal sealed interface TapPlay {
    /** Android's predefined [effect] (VibrationEffect.EFFECT_*). */
    data class Predefined(val effect: Int) : TapPlay
    /** A one-shot pulse of [millis] at [amplitude] (1–255, or DEFAULT_AMPLITUDE), because the vibrator doesn't offer [missing]. */
    data class Pulse(val millis: Long, val amplitude: Int, val missing: Int) : TapPlay
}

/** Hex's Haptics setting and the system's Keyboard vibration both on. */
internal fun vibrates(context: Context, haptics: Boolean) =
    haptics && Settings.System.getInt(context.contentResolver, "keyboard_vibration_enabled", 1) != 0

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
