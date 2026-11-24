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

    fun effect(vibrator: Vibrator): VibrationEffect =
        if (vibrator.areAllEffectsSupported(predefined) == Vibrator.VIBRATION_EFFECT_SUPPORT_YES) VibrationEffect.createPredefined(predefined)
        else VibrationEffect.createOneShot(millis, VibrationEffect.DEFAULT_AMPLITUDE)
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
    if (!vibrates(context, haptics)) return
    val vibrator = context.getSystemService(VibratorManager::class.java)?.defaultVibrator ?: return
    if (!vibrator.hasVibrator()) return
    val effect = tap.effect(vibrator)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU)
        vibrator.vibrate(effect, VibrationAttributes.createForUsage(VibrationAttributes.USAGE_TOUCH))
    else @Suppress("DEPRECATION") vibrator.vibrate(effect, AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
}
