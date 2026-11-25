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

    // Labs › Keyboard › Clipboard history (ClipboardHistory.kt). These would move, as they are, to their own
    // ClipboardHistoryTest if a new test file is agreed.

    private val hour = ClipboardRules.LIFETIME_MS
    private fun keeps(text: String? = "a copy", isText: Boolean = true, sensitive: Boolean = false, own: Boolean = false,
                      secureField: Boolean = false, noLearning: Boolean = false) =
        ClipboardRules.keeps(text, isText, sensitive, own, secureField, noLearning)

    @Test fun withTheLabOffTheClipboardIsNeverListenedToOrRead() {
        var listens = 0; var reads = 0; var forgets = 0
        val keeper = ClipboardKeeper(enabled = { false }, listen = { listens++ }, read = { reads++ }, forget = { forgets++ })
        keeper.sync(); keeper.copied(); keeper.copied(); keeper.stop()
        assertEquals(listOf(0, 0, 0), listOf(listens, reads, forgets))
        assertFalse(keeper.listening)
    }

    @Test fun onTheListenerComesAndTurningItOffRemovesItAndForgetsEverything() {
        var on = true
        val calls = mutableListOf<String>()
        val keeper = ClipboardKeeper(enabled = { on }, listen = { calls += if (it) "add" else "remove" },
            read = { calls += "read" }, forget = { calls += "forget" })
        keeper.sync(); keeper.sync(); keeper.copied()
        on = false
        keeper.copied() // a copy announced as the lab turns off is not read
        keeper.sync(); keeper.copied()
        assertEquals(listOf("add", "read", "remove", "forget"), calls)
    }

    @Test fun theKeyboardGoingAwayRemovesTheListenerButKeepsTheTexts() {
        val calls = mutableListOf<String>()
        val keeper = ClipboardKeeper(enabled = { true }, listen = { calls += if (it) "add" else "remove" },
            read = { calls += "read" }, forget = { calls += "forget" })
        keeper.sync(); keeper.stop(); keeper.copied()
        assertEquals(listOf("add", "remove"), calls)
    }

    @Test fun aPlainCopyIsKept() = assertTrue(keeps())

    @Test fun whatIsNeverKept() {
        assertFalse("marked sensitive", keeps(sensitive = true))
        assertFalse("Hex's own copy", keeps(own = true))
        assertFalse("copied with a password field open", keeps(secureField = true))
        assertFalse("copied in a field that asks not to be learned from", keeps(noLearning = true))
        assertFalse("not text", keeps(isText = false))
        assertFalse("no text", keeps(text = null))
        assertFalse("blank", keeps(text = " \n\t"))
        assertFalse("too long", keeps(text = "x".repeat(ClipboardRules.MAX_LENGTH + 1)))
        assertTrue("as long as it may be", keeps(text = "x".repeat(ClipboardRules.MAX_LENGTH)))
    }

    @Test fun aRepeatedCopyMovesToTheTopAndKeepsItsPin() {
        val start = listOf(CopiedText("b", 20), CopiedText("a", 10, pinned = true))
        val again = ClipboardRules.add(start, "a", 30)
        assertEquals(listOf(CopiedText("a", 30, pinned = true), CopiedText("b", 20)), again)
        assertEquals(listOf("c", "b"), ClipboardRules.add(listOf(CopiedText("b", 20)), "c", 30).map { it.text })
    }

    @Test fun anUnpinnedTextGoesAfterAnHourAndAPinnedOneStays() {
        val items = listOf(CopiedText("old", 0), CopiedText("new", hour / 2), CopiedText("kept", 0, pinned = true))
        assertEquals(listOf("kept", "new"), ClipboardRules.current(items, hour).map { it.text })
        assertEquals(listOf("kept", "old", "new").sorted(), ClipboardRules.current(items, hour - 1).map { it.text }.sorted())
    }

    @Test fun aTextDatedInTheFutureByAClockSetBackGoes() {
        assertEquals(emptyList<CopiedText>(), ClipboardRules.current(listOf(CopiedText("later", 5_000)), 1_000))
    }

    @Test fun twentyRecentAndTwentyPinnedAreKeptPinnedFirst() {
        val recent = (1..25).map { CopiedText("r$it", it.toLong()) }
        val pinned = (1..25).map { CopiedText("p$it", it.toLong(), pinned = true) }
        val kept = ClipboardRules.current(recent + pinned, 30)
        assertEquals((25 downTo 6).map { "p$it" } + (25 downTo 6).map { "r$it" }, kept.map { it.text })
    }

    @Test fun thePanelShowsATextsOwnLinesWithoutRunsOfBlankOnes() {
        assertEquals("first\nsecond\nthird", ClipboardRules.preview("  first\n\n\n  second\nthird \n"))
    }
}
