package ly.anoma.hex.keyboard

import android.text.InputType
import android.view.KeyEvent
import android.view.inputmethod.EditorInfo

/** What Enter did in a field: ran the field's action, pressed the Enter key, or left a line break for the
 * keyboard to type as text. */
internal enum class EnterPress {
    Action, Key, LineBreak;

    companion object {
        /** Enter in a field with [inputType] and [imeOptions], and the editor's own [actionLabel] and [actionId]:
         * the labelled action first (unless the field asks for no Enter action), then the field's standard one
         * (Search, Go, Send, Done), else what a field without an action takes.
         *
         * A multi-line field gets its line break as text, as it always did: Android marks every multi-line field
         * IME_FLAG_NO_ENTER_ACTION, so no action runs there, and an Enter key press in a chat app or a web text
         * area can mean Send. A field that holds one line can't take a line break as text, and one with no
         * action of its own (a launcher's search box, #2864) may still be waiting for the Enter key to search, so
         * it gets the key press. So does a TYPE_NULL editor (a terminal), whose text can't be edited at all. */
        fun run(target: KeyboardTextTarget, inputType: Int, imeOptions: Int, actionLabel: CharSequence?,
                actionId: Int): EnterPress {
            if (actionLabel != null && imeOptions and EditorInfo.IME_FLAG_NO_ENTER_ACTION == 0 &&
                target.performEditorAction(actionId)) return Action
            if (target.defaultEditorAction()) return Action
            if (multiLine(inputType)) return LineBreak
            target.sendKey(KeyEvent.KEYCODE_ENTER)
            return Key
        }

        /** A text field that holds more than one line: the app's own (MULTI_LINE) or only in the keyboard's
         * full-screen editor (IME_MULTI_LINE). Every other class (number, phone, date, TYPE_NULL) holds one. */
        fun multiLine(inputType: Int) = inputType and InputType.TYPE_MASK_CLASS == InputType.TYPE_CLASS_TEXT &&
            inputType and (InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_IME_MULTI_LINE) != 0
    }
}
