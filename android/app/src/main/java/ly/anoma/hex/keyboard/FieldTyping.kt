package ly.anoma.hex.keyboard

import android.text.InputType
import android.view.inputmethod.EditorInfo

/** What a field lets typing do: [words] fields get the model's suggestions, next words, tap correction and
 * swipes, [corrects] fields also autocorrect on space and punctuation, [secure] fields are never read and type
 * literally, and [learns] fields let Hex remember what is typed there (an undone correction, an emoji pick).
 * A field that asks for no suggestions, completions or filtering still reads words: those flags describe what
 * the app offers, not whether its keyboard may offer suggestions of its own, and only an automatic correction
 * would speak over what the field asked for. A [keyEvents] field keeps its own text on the other side (Android's
 * TYPE_NULL, a terminal's), so nothing typed can be read back or edited as text and only key presses reach it.
 * Android's IME_FLAG_NO_PERSONALIZED_LEARNING only asks that nothing typed there be learned (a browser's
 * incognito tab), so such a field types as usual and keeps no such memory. */
internal data class FieldTyping(val secure: Boolean, val words: Boolean, val corrects: Boolean, val learns: Boolean,
                                 val keyEvents: Boolean) {
    companion object {
        fun of(inputType: Int, imeOptions: Int): FieldTyping {
            val kind = inputType and InputType.TYPE_MASK_CLASS
            val variation = inputType and InputType.TYPE_MASK_VARIATION
            val secure = when (kind) {
                InputType.TYPE_CLASS_TEXT -> variation in setOf(InputType.TYPE_TEXT_VARIATION_PASSWORD,
                    InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD, InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD)
                InputType.TYPE_CLASS_NUMBER -> variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD
                else -> false
            }
            val words = kind == InputType.TYPE_CLASS_TEXT && !secure &&
                variation !in setOf(InputType.TYPE_TEXT_VARIATION_URI, InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
                    InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS)
            // A field asking for no suggestions, completions or filtering still reads words; only an automatic
            // correction is off there.
            val corrects = words && variation != InputType.TYPE_TEXT_VARIATION_FILTER &&
                inputType and (InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or InputType.TYPE_TEXT_FLAG_AUTO_COMPLETE) == 0
            return FieldTyping(secure, words, corrects, imeOptions and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING == 0,
                keyEvents = kind == InputType.TYPE_NULL)
        }
    }
}
