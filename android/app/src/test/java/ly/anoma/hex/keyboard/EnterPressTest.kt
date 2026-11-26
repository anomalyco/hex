package ly.anoma.hex.keyboard

import android.text.InputType
import android.view.KeyEvent
import android.view.inputmethod.EditorInfo
import org.junit.Assert.assertEquals
import org.junit.Test

class EnterPressTest {
    /** An editor that records what Enter did: [default] is whether Android ran the field's standard action. */
    private class Editor(val default: Boolean = false, val labelled: Boolean = true) : KeyboardTextTarget {
        val actions = ArrayList<Int>()
        val keys = ArrayList<Int>()
        var defaults = 0
        override val connected = true
        override fun before(length: Int) = ""
        override fun after(length: Int) = ""
        override fun selected() = ""
        override fun capsMode(mask: Int) = 0
        override fun commit(text: String) = true
        override fun deleteBefore(length: Int) = true
        override fun deleteCodePointsBefore(count: Int) = true
        override fun beginBatch() {}
        override fun endBatch() {}
        override fun setSelection(start: Int, end: Int) = true
        override fun performEditorAction(id: Int): Boolean { actions.add(id); return labelled }
        override fun defaultEditorAction(): Boolean { defaults++; return default }
        override fun sendKey(code: Int) { keys.add(code) }
    }
    private val text = InputType.TYPE_CLASS_TEXT
    private val multiLine = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE

    @Test fun aMultiLineFieldWithoutAnActionStillTakesALineBreak() {
        // Android marks every multi-line field IME_FLAG_NO_ENTER_ACTION, so its default action never runs.
        val notes = Editor()
        assertEquals(EnterPress.LineBreak, EnterPress.run(notes, multiLine, EditorInfo.IME_FLAG_NO_ENTER_ACTION, null, 0))
        assertEquals(emptyList<Int>(), notes.keys)
        // The Google app's search box: TEXT|MULTI_LINE|NO_SUGGESTIONS.
        val search = Editor()
        assertEquals(EnterPress.LineBreak, EnterPress.run(search, multiLine or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS,
            EditorInfo.IME_FLAG_NO_ENTER_ACTION or EditorInfo.IME_ACTION_SEARCH, null, 0))
        assertEquals(emptyList<Int>(), search.keys)
        // Multiple lines only in the keyboard's full-screen editor still take one as text.
        assertEquals(EnterPress.LineBreak,
            EnterPress.run(Editor(), text or InputType.TYPE_TEXT_FLAG_IME_MULTI_LINE, EditorInfo.IME_ACTION_NONE, null, 0))
    }

    @Test fun aFieldWithAnActionRunsIt() {
        val search = Editor(default = true)
        assertEquals(EnterPress.Action, EnterPress.run(search, text, EditorInfo.IME_ACTION_SEARCH, null, 0))
        assertEquals(1, search.defaults)
        assertEquals(emptyList<Int>(), search.keys)
        // The editor's own label runs its own action id, before the standard one.
        val chat = Editor()
        assertEquals(EnterPress.Action, EnterPress.run(chat, multiLine, EditorInfo.IME_ACTION_SEND, "Post", 42))
        assertEquals(listOf(42), chat.actions)
        assertEquals(0, chat.defaults)
        // A label in a field that asks for no Enter action is not run.
        val noted = Editor()
        assertEquals(EnterPress.LineBreak, EnterPress.run(noted, multiLine, EditorInfo.IME_FLAG_NO_ENTER_ACTION, "Post", 42))
        assertEquals(emptyList<Int>(), noted.actions)
    }

    @Test fun aOneLineFieldWithoutAnActionGetsTheEnterKey() {
        // A launcher's search box (#2864): one line, no action of its own, waiting for the key to search.
        val launcher = Editor()
        assertEquals(EnterPress.Key, EnterPress.run(launcher, text, EditorInfo.IME_ACTION_NONE, null, 0))
        assertEquals(listOf(KeyEvent.KEYCODE_ENTER), launcher.keys)
        val flagged = Editor()
        assertEquals(EnterPress.Key, EnterPress.run(flagged, text, EditorInfo.IME_FLAG_NO_ENTER_ACTION, null, 0))
        assertEquals(listOf(KeyEvent.KEYCODE_ENTER), flagged.keys)
        // A terminal (TYPE_NULL) keeps getting the key, MULTI_LINE flag or not.
        val terminal = Editor()
        assertEquals(EnterPress.Key, EnterPress.run(terminal, InputType.TYPE_NULL or InputType.TYPE_TEXT_FLAG_MULTI_LINE, 0, null, 0))
        assertEquals(listOf(KeyEvent.KEYCODE_ENTER), terminal.keys)
    }
}
