package ly.anoma.hex.keyboard

import android.inputmethodservice.InputMethodService

/** Everything the keyboard engine may read from or do to the editor. The IME passes its live
 * InputConnection; a test can pass a fake. Each call reaches the field focused at that moment. */
internal interface KeyboardTextTarget {
    val connected: Boolean
    fun before(length: Int): String?
    fun after(length: Int): String?
    fun selected(): String?
    fun capsMode(mask: Int): Int
    fun commit(text: String): Boolean
    fun deleteBefore(length: Int): Boolean
    fun deleteCodePointsBefore(count: Int): Boolean
    /** Deletes [length] characters after the cursor; false where the target can't. */
    fun deleteAfter(length: Int): Boolean = false
    fun beginBatch()
    fun endBatch()
    fun setSelection(start: Int, end: Int): Boolean
    fun performEditorAction(id: Int): Boolean
    fun defaultEditorAction(): Boolean
    fun sendKey(code: Int)
}

/** The current editor's InputConnection, looked up on every call: it changes with each field. */
internal class InputConnectionTarget(private val ime: InputMethodService) : KeyboardTextTarget {
    private val ic get() = ime.currentInputConnection
    override val connected get() = ic != null
    override fun before(length: Int) = ic?.getTextBeforeCursor(length, 0)?.toString()
    override fun after(length: Int) = ic?.getTextAfterCursor(length, 0)?.toString()
    override fun selected() = ic?.getSelectedText(0)?.toString()
    override fun capsMode(mask: Int) = ic?.getCursorCapsMode(mask) ?: 0
    override fun commit(text: String) = ic?.commitText(text, 1) ?: false
    override fun deleteBefore(length: Int) = ic?.deleteSurroundingText(length, 0) ?: false
    override fun deleteCodePointsBefore(count: Int) = ic?.deleteSurroundingTextInCodePoints(count, 0) ?: false
    override fun deleteAfter(length: Int) = ic?.deleteSurroundingText(0, length) ?: false
    override fun beginBatch() { ic?.beginBatchEdit() }
    override fun endBatch() { ic?.endBatchEdit() }
    override fun setSelection(start: Int, end: Int) = ic?.setSelection(start, end) ?: false
    override fun performEditorAction(id: Int) = ic?.performEditorAction(id) ?: false
    override fun defaultEditorAction() = ime.sendDefaultEditorAction(true)
    override fun sendKey(code: Int) = ime.sendDownUpKeyEvents(code)
}
