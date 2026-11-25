package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyboardToolsTest {
    private fun tools(enabled: Boolean = true, secure: Boolean = false, busy: Boolean = false,
                      emoji: Boolean = true, hasClip: Boolean = true, canUndo: Boolean = true, history: Boolean = false) =
        KeyboardTools.tools(enabled = enabled, busy = busy, secure = { secure }, emoji = { emoji },
            canUndo = { canUndo }, clipboard = { hasClip }, history = { history })

    @Test fun withTheLabOffThereAreNoTools() {
        assertEquals(emptyList<StripTool>(), tools(enabled = false))
    }

    @Test fun theStripShowsAllFourWithAClipAndAnAutocorrection() {
        assertEquals(listOf(StripTool.Emoji, StripTool.Paste, StripTool.Settings, StripTool.Undo), tools())
    }

    @Test fun emojiAndSettingsShowWithoutAClipOrUndo() {
        assertEquals(listOf(StripTool.Emoji, StripTool.Settings), tools(hasClip = false, canUndo = false))
    }

    @Test fun emojiNeedsTheLettersPage() {
        assertEquals(listOf(StripTool.Paste, StripTool.Settings, StripTool.Undo), tools(emoji = false))
    }

    @Test fun pasteNeedsAClip() {
        assertTrue(StripTool.Paste !in tools(hasClip = false))
        assertTrue(StripTool.Paste in tools(hasClip = true))
    }

    @Test fun undoNeedsAnAutocorrectionToRevert() {
        assertTrue(StripTool.Undo !in tools(canUndo = false))
        assertTrue(StripTool.Undo in tools(canUndo = true))
    }

    @Test fun nothingShowsInAPasswordField() {
        assertEquals(emptyList<StripTool>(), tools(secure = true))
    }

    @Test fun nothingShowsWhileTheStripIsBusy() {
        assertEquals(emptyList<StripTool>(), tools(busy = true))
    }

    @Test fun nothingIsReadWithTheLabOffOrWhileTheStripIsBusy() {
        var asked = 0
        val secure = { asked++; false }
        val emoji = { asked++; true }
        val clipboard = { asked++; true }
        val canUndo = { asked++; true }
        val history = { asked++; true }
        KeyboardTools.tools(enabled = false, busy = false, secure = secure, emoji = emoji, canUndo = canUndo, clipboard = clipboard,
            history = history)
        KeyboardTools.tools(enabled = true, busy = true, secure = secure, emoji = emoji, canUndo = canUndo, clipboard = clipboard,
            history = history)
        assertEquals(0, asked)
        // A password field needs only the field, not its page, the clipboard or the editor.
        KeyboardTools.tools(enabled = true, busy = false, secure = { asked++; true }, emoji = emoji, canUndo = canUndo,
            clipboard = clipboard, history = history)
        assertEquals(1, asked)
        KeyboardTools.tools(enabled = true, busy = false, secure = secure, emoji = emoji, canUndo = canUndo, clipboard = clipboard,
            history = history)
        assertEquals(6, asked)
    }

    @Test fun anEmptyFieldKeepsItsLineForTheTools() {
        assertTrue(KeyboardTools.showsInEmptyField(enabled = true, before = "") { "" })
        assertTrue(KeyboardTools.showsInEmptyField(enabled = true, before = "") { null })
        assertFalse(KeyboardTools.showsInEmptyField(enabled = false, before = "") { "" })
        assertFalse(KeyboardTools.showsInEmptyField(enabled = true, before = "I") { "" })
        assertFalse(KeyboardTools.showsInEmptyField(enabled = true, before = "") { "rest" })
    }

    @Test fun theTextAfterTheCursorIsReadOnlyForAnEmptyField() {
        var asked = 0
        KeyboardTools.showsInEmptyField(enabled = false, before = "") { asked++; "" }
        KeyboardTools.showsInEmptyField(enabled = true, before = "word") { asked++; "" }
        assertEquals(0, asked)
        KeyboardTools.showsInEmptyField(enabled = true, before = "") { asked++; "" }
        assertEquals(1, asked)
    }

    @Test fun clipboardShowsAfterPasteWithClipboardHistoryOn() {
        assertEquals(listOf(StripTool.Emoji, StripTool.Paste, StripTool.Clipboard, StripTool.Settings, StripTool.Undo),
            tools(history = true))
        assertTrue(StripTool.Clipboard !in tools(history = false))
        assertEquals(emptyList<StripTool>(), tools(history = true, secure = true))
    }
}
