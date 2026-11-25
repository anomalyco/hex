package ly.anoma.hex.speech

import android.view.Window
import android.view.WindowManager

/** A dictation's window keeps the screen on while it opens, listens or transcribes (the flag needs no permission),
 * so the display timing out can't hide the IME and stop it early. Idle lets the screen sleep normally. */
internal fun Window?.keepAwake(phase: SpeechPhase) {
    val window = this ?: return
    val on = phase != SpeechPhase.Idle
    val flag = WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
    // Adding or clearing an unchanged flag would relayout the window.
    if ((window.attributes.flags and flag != 0) == on) return
    if (on) window.addFlags(flag) else window.clearFlags(flag)
}
