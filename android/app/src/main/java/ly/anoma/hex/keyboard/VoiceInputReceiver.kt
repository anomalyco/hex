package ly.anoma.hex.keyboard

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import ly.anoma.hex.typing.TypingPreferences

/** Android drops an IME's additional subtypes when its package is updated, and the subtype's language has to
 * follow the phone's: both put the voice subtype back from the saved toggle. */
internal class VoiceInputReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_MY_PACKAGE_REPLACED, Intent.ACTION_LOCALE_CHANGED ->
                VoiceInput.apply(context, TypingPreferences(context).voiceInput)
        }
    }
}
