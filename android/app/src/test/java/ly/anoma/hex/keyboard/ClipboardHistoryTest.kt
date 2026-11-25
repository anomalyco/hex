package ly.anoma.hex.keyboard

import android.content.ClipDescription
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Labs › Keyboard › Clipboard history (ClipboardHistory.kt): what is never kept, how long the rest stays, pins, the
 * limits, Clear, and that off means no listener, no read and nothing kept. */
class ClipboardHistoryTest {
    private val hour = ClipboardRules.LIFETIME_MS
    private fun keeps(text: String? = "a copy", isText: Boolean = true, sensitive: Boolean = false, own: Boolean = false,
                      secureField: Boolean = false, noLearning: Boolean = false) =
        ClipboardRules.keeps(text, isText, sensitive, own, secureField, noLearning)
    /** A description's extras as the store reads them: one boolean per key. */
    private fun extras(vararg set: String): (String) -> Boolean = { it in set }

    @Test fun aPlainCopyIsKept() = assertTrue(keeps())

    // What is never kept, one case each.

    @Test fun aClipMarkedSensitiveWithAndroid13sConstantIsNotKept() {
        // ClipDescription.EXTRA_IS_SENSITIVE (Android 13 and later) is the key the store reads.
        assertEquals(ClipDescription.EXTRA_IS_SENSITIVE, ClipboardRules.SENSITIVE)
        assertTrue(ClipboardRules.sensitive(extras(ClipDescription.EXTRA_IS_SENSITIVE)))
        assertFalse(keeps(sensitive = true))
    }

    @Test fun aClipMarkedSensitiveWithTheLiteralKeyOlderPasswordManagersSetIsNotKept() {
        // Below Android 13 password managers put the key as a string (KeePassDX; Bitwarden does both).
        assertTrue(ClipboardRules.sensitive(extras("android.content.extra.IS_SENSITIVE")))
        assertFalse(ClipboardRules.sensitive(extras()))
        assertFalse(ClipboardRules.sensitive(extras(ClipboardRules.OWN)))
    }

    @Test fun aCopyMadeWhileAPasswordFieldIsOpenIsNotKept() = assertFalse(keeps(secureField = true))

    @Test fun aCopyMadeInAFieldThatAsksNotToBeLearnedFromIsNotKept() = assertFalse(keeps(noLearning = true))

    @Test fun aClipThatIsNotTextIsNotKept() = assertFalse(keeps(isText = false))

    @Test fun aClipWithNoTextIsNotKept() = assertFalse(keeps(text = null))

    @Test fun blankTextIsNotKept() {
        assertFalse(keeps(text = ""))
        assertFalse(keeps(text = " \n\t"))
    }

    @Test fun textOverTheLengthLimitIsNotKept() {
        assertFalse(keeps(text = "x".repeat(ClipboardRules.MAX_LENGTH + 1)))
        assertTrue(keeps(text = "x".repeat(ClipboardRules.MAX_LENGTH)))
    }

    @Test fun hexsOwnCopiesAreNotKept() {
        assertTrue(ClipboardRules.own(extras(ClipboardRules.OWN)))
        assertFalse(ClipboardRules.own(extras(ClipboardRules.SENSITIVE)))
        assertFalse(keeps(own = true))
    }

    // The lab off.

    @Test fun withTheLabOffTheClipboardIsNeverListenedToOrRead() {
        var listens = 0; var reads = 0; var forgets = 0
        val keeper = ClipboardKeeper(enabled = { false }, listen = { listens++ }, read = { reads++ }, forget = { forgets++ })
        keeper.sync(); keeper.copied(); keeper.copied(); keeper.stop()
        assertEquals(listOf(0, 0, 0), listOf(listens, reads, forgets))
        assertFalse(keeper.listening)
    }

    @Test fun turningItOffRemovesTheListenerAndDeletesEverything() {
        var on = true
        val calls = mutableListOf<String>()
        val keeper = ClipboardKeeper(enabled = { on }, listen = { calls += if (it) "add" else "remove" },
            read = { calls += "read" }, forget = { calls += "forget" })
        keeper.sync(); keeper.sync(); keeper.copied()
        on = false
        keeper.copied() // a copy announced as the lab turns off is not read
        keeper.sync(); keeper.copied()
        assertEquals(listOf("add", "read", "remove", "forget"), calls)
        assertFalse(keeper.listening)
    }

    @Test fun theKeyboardGoingAwayRemovesTheListenerButKeepsTheTexts() {
        val calls = mutableListOf<String>()
        val keeper = ClipboardKeeper(enabled = { true }, listen = { calls += if (it) "add" else "remove" },
            read = { calls += "read" }, forget = { calls += "forget" })
        keeper.sync(); keeper.stop(); keeper.copied()
        assertEquals(listOf("add", "remove"), calls)
    }

    // How long texts stay, pins, limits and Clear.

    @Test fun anUnpinnedTextGoesAnHourAfterItWasCopied() {
        val items = listOf(CopiedText("old", 0), CopiedText("new", hour / 2))
        assertEquals(listOf("new", "old"), ClipboardRules.current(items, hour - 1).map { it.text })
        assertEquals(listOf("new"), ClipboardRules.current(items, hour).map { it.text })
    }

    @Test fun theTimerIsSetForTheOldestUnpinnedTextAndMovesOnOnceItIsDue() {
        val items = listOf(CopiedText("pin", 0, pinned = true), CopiedText("new", hour / 2), CopiedText("old", hour / 4))
        assertEquals(hour / 4 + hour, ClipboardRules.nextExpiry(items, hour / 2))
        // At that moment "old" is due; the pin stays and the timer moves to "new".
        assertEquals(listOf("pin", "new"), ClipboardRules.current(items, hour / 4 + hour).map { it.text })
        assertEquals(hour / 2 + hour, ClipboardRules.nextExpiry(items, hour / 4 + hour))
    }

    @Test fun noTimerWhileNothingUnpinnedIsKept() {
        assertNull(ClipboardRules.nextExpiry(emptyList(), 0))
        assertNull(ClipboardRules.nextExpiry(listOf(CopiedText("pin", 0, pinned = true)), 2 * hour))
        assertNull(ClipboardRules.nextExpiry(listOf(CopiedText("old", 0)), hour))
    }

    @Test fun aPinnedTextOutlivesTheHour() {
        val items = listOf(CopiedText("kept", 0, pinned = true), CopiedText("old", 0))
        assertEquals(listOf("kept"), ClipboardRules.current(items, 30 * 24 * hour).map { it.text })
    }

    @Test fun aTextDatedInTheFutureByAClockSetBackGoes() {
        assertEquals(emptyList<CopiedText>(), ClipboardRules.current(listOf(CopiedText("later", 5_000)), 1_000))
    }

    @Test fun pinningAndUnpinningMoveATextBetweenTheTwoGroups() {
        val items = listOf(CopiedText("b", 20), CopiedText("a", 10))
        val pinned = ClipboardRules.pin(items, "a", true)
        assertEquals(listOf(CopiedText("a", 10, pinned = true), CopiedText("b", 20)), pinned)
        assertEquals(listOf(CopiedText("b", 20), CopiedText("a", 10)), ClipboardRules.pin(pinned, "a", false))
    }

    @Test fun aRepeatedCopyMovesToTheTopAndKeepsItsPin() {
        val start = listOf(CopiedText("b", 20), CopiedText("a", 10, pinned = true))
        assertEquals(listOf(CopiedText("a", 30, pinned = true), CopiedText("b", 20)), ClipboardRules.add(start, "a", 30))
        assertEquals(listOf("c", "b"), ClipboardRules.add(listOf(CopiedText("b", 20)), "c", 30).map { it.text })
    }

    @Test fun twentyRecentAndTwentyPinnedAreKeptPinnedFirst() {
        val recent = (1..25).map { CopiedText("r$it", it.toLong()) }
        val pinned = (1..25).map { CopiedText("p$it", it.toLong(), pinned = true) }
        val kept = ClipboardRules.current(recent + pinned, 30)
        assertEquals((25 downTo 6).map { "p$it" } + (25 downTo 6).map { "r$it" }, kept.map { it.text })
    }

    @Test fun theTwentyFirstCopyPushesOutTheOldest() {
        val items = (1..20).fold(emptyList<CopiedText>()) { list, i -> ClipboardRules.add(list, "t$i", i.toLong()) }
        val after = ClipboardRules.add(items, "t21", 21)
        assertEquals(20, after.size)
        assertFalse(after.any { it.text == "t1" })
        assertEquals("t21", after.first().text)
    }

    @Test fun deleteRemovesOneTextPinnedOrNot() {
        val items = listOf(CopiedText("a", 2, pinned = true), CopiedText("b", 1))
        assertEquals(listOf("b"), ClipboardRules.delete(items, "a").map { it.text })
        assertEquals(listOf("a"), ClipboardRules.delete(items, "b").map { it.text })
    }

    @Test fun clearLeavesThePinnedTexts() {
        val items = listOf(CopiedText("p", 1, pinned = true), CopiedText("a", 3), CopiedText("b", 2))
        assertEquals(listOf(CopiedText("p", 1, pinned = true)), ClipboardRules.clear(items))
    }

    @Test fun thePanelShowsATextsOwnLinesWithoutBlankOnes() {
        assertEquals("first\nsecond\nthird", ClipboardRules.preview("  first\n\n\n  second\nthird \n"))
    }
}
