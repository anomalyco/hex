package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyboardToolsTest {
    private fun tools(enabled: Boolean = true, secure: Boolean = false, busy: Boolean = false,
                      hasClip: Boolean = true, canUndo: Boolean = true) =
        KeyboardTools.tools(enabled = enabled, secure = secure, busy = busy, canUndo = { canUndo }, clipboard = { hasClip })

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

    @Test fun theClipboardAndTheEditorAreOnlyAskedWhenTheToolsCouldShow() {
        var asked = 0
        val clipboard = { asked++; true }
        val canUndo = { asked++; true }
        KeyboardTools.tools(enabled = false, secure = false, busy = false, canUndo = canUndo, clipboard = clipboard)
        KeyboardTools.tools(enabled = true, secure = true, busy = false, canUndo = canUndo, clipboard = clipboard)
        KeyboardTools.tools(enabled = true, secure = false, busy = true, canUndo = canUndo, clipboard = clipboard)
        assertEquals(0, asked)
        KeyboardTools.tools(enabled = true, secure = false, busy = false, canUndo = canUndo, clipboard = clipboard)
        assertEquals(2, asked)
    }
}
