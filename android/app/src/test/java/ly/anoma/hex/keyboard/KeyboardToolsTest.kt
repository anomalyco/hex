package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyboardToolsTest {
    private fun tools(enabled: Boolean = true, secure: Boolean = false, busy: Boolean = false,
                      hasClip: Boolean = true, canUndo: Boolean = true) =
        KeyboardTools.tools(enabled = enabled, busy = busy, secure = { secure }, canUndo = { canUndo },
            clipboard = { hasClip })

    @Test fun withTheLabOffThereAreNoTools() {
        assertEquals(emptyList<StripTool>(), tools(enabled = false))
    }

    @Test fun theStripShowsAllFourWithAClipAndAnAutocorrection() {
        assertEquals(listOf(StripTool.Emoji, StripTool.Paste, StripTool.Settings, StripTool.Undo), tools())
    }

    @Test fun emojiAndSettingsAreAlwaysThere() {
        assertEquals(listOf(StripTool.Emoji, StripTool.Settings), tools(hasClip = false, canUndo = false))
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
        val clipboard = { asked++; true }
        val canUndo = { asked++; true }
        KeyboardTools.tools(enabled = false, busy = false, secure = secure, canUndo = canUndo, clipboard = clipboard)
        KeyboardTools.tools(enabled = true, busy = true, secure = secure, canUndo = canUndo, clipboard = clipboard)
        assertEquals(0, asked)
        // A password field needs only the field, not the clipboard or the editor.
        KeyboardTools.tools(enabled = true, busy = false, secure = { asked++; true }, canUndo = canUndo, clipboard = clipboard)
        assertEquals(1, asked)
        KeyboardTools.tools(enabled = true, busy = false, secure = secure, canUndo = canUndo, clipboard = clipboard)
        assertEquals(4, asked)
    }
}
