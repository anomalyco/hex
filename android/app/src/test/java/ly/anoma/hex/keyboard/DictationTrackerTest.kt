package ly.anoma.hex.keyboard

import ly.anoma.hex.speech.DictationEdit
import ly.anoma.hex.speech.DictationOutcome
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DictationTrackerTest {
    /** A field whose text the test edits as a person would, with the cursor anywhere in it. */
    private class Field(var text: String = "", var cursor: Int = 0) : KeyboardTextTarget {
        override val connected = true
        override fun before(length: Int) = text.substring(maxOf(0, cursor - length), cursor)
        override fun after(length: Int) = text.substring(cursor, minOf(text.length, cursor + length))
        override fun selected() = ""
        override fun capsMode(mask: Int) = 0
        override fun commit(text: String) = insert(text)
        override fun deleteBefore(length: Int) = delete(minOf(length, cursor))
        override fun deleteCodePointsBefore(count: Int) = delete(minOf(count, cursor))
        override fun beginBatch() {}
        override fun endBatch() {}
        override fun setSelection(start: Int, end: Int) = setCursor(start)
        override fun performEditorAction(id: Int) = true
        override fun defaultEditorAction() = true
        override fun sendKey(code: Int) {}

        fun insert(text: String): Boolean {
            this.text = this.text.substring(0, cursor) + text + this.text.substring(cursor)
            cursor += text.length
            return true
        }

        fun delete(count: Int): Boolean {
            text = text.substring(0, cursor - count) + text.substring(cursor)
            cursor -= count
            return true
        }

        fun replace(start: Int, end: Int, text: String): Boolean {
            this.text = this.text.substring(0, start) + text + this.text.substring(end)
            cursor = start + text.length
            return true
        }

        fun setCursor(at: Int): Boolean { cursor = at.coerceIn(0, text.length); return true }
    }

    private var now = 1_000L
    private val field = Field()
    private val results = ArrayList<DictationEdit>()
    private val tracker = DictationTracker(field, { now }) { results += it }

    private fun dictate(text: String) {
        val before = field.before(64)
        val after = field.after(DictationTracker.anchorLength)
        field.insert(text)
        tracker.inserted(text, text, before, after)
    }

    private fun outcomes() = results.map { it.outcome }

    @Test fun aSentDictationSettlesKeptAfterAMinute() {
        dictate("See you soon.")
        tracker.observe()
        assertTrue(tracker.isTracking)
        now += 60_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Kept), outcomes())
        assertFalse(tracker.isTracking)
    }

    @Test fun anEditIsReportedWhenTheWindowEnds() {
        dictate(" Meet at cloud nine.")
        tracker.observe()
        now += 10_000
        field.replace(9, 14, "Claude") // " Meet at Claude nine."
        tracker.observe()
        now += 51_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Edited), outcomes())
        assertEquals(listOf("Claude"), results.single().pairs.map { it.meant })
    }

    @Test fun aNewDictationSettlesTheOneBeforeIt() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        field.replace(8, 13, "Claude")
        now += 5_000
        tracker.settle()
        assertEquals(listOf(DictationOutcome.Edited), outcomes())
        assertFalse(tracker.isTracking)
        dictate("Another one.")
        tracker.observe()
        tracker.finish()
        assertEquals(listOf(DictationOutcome.Edited, DictationOutcome.Kept), outcomes())
    }

    private val longDictation = " one two three four five six seven eight nine ten eleven twelve thirteen fourteen."

    @Test fun anEditInsideTheDictationIsFoundWithTextAfterIt() {
        field.text = "Hi. "
        field.cursor = 4
        dictate(longDictation)
        field.insert(" Thanks.")
        field.replace(field.text.indexOf("thirteen"), field.text.indexOf("thirteen") + 8, "thirteenth")
        field.setCursor(field.text.indexOf("five"))
        tracker.observe()
        now += 61_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Edited), outcomes())
        assertEquals(listOf("thirteenth"), results.single().pairs.map { it.meant })
    }

    @Test fun anEditInsideTheDictationIsFoundWithoutTextAfterIt() {
        dictate(longDictation)
        field.replace(field.text.indexOf("thirteen"), field.text.indexOf("thirteen") + 8, "thirteenth")
        field.setCursor(field.text.indexOf("five"))
        tracker.observe()
        now += 61_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Edited), outcomes())
        assertEquals(listOf("thirteenth"), results.single().pairs.map { it.meant })
    }

    @Test fun typingAfterADictationAtTheFieldStartIsNotAnEdit() {
        dictate("Meet at cloud nine.")
        field.insert(" and then we can talk about the rest of the plan tomorrow.")
        tracker.observe()
        now += 61_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Kept), outcomes())
        assertTrue(results.single().pairs.isEmpty())
    }

    @Test fun aCursorInsideALongDictationDoesNotCutItsEnd() {
        dictate("one two three four five six seven eight nine ten eleven twelve thirteen fourteen.")
        field.setCursor(field.text.indexOf("nine"))
        tracker.observe()
        now += 61_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Kept), outcomes())
        assertTrue(results.single().pairs.isEmpty())
    }

    @Test fun aShortAfterAnchorRepeatedInsideTheDictationStillFindsItsEnd() {
        field.text = "Hi  "
        field.cursor = 3
        dictate(" one two three four five six seven eight nine cloud eleven twelve thirteen fourteen fifteen " +
            "sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive")
        field.replace(field.text.indexOf("cloud"), field.text.indexOf("cloud") + 5, "Claude")
        field.setCursor(field.text.indexOf("seven"))
        tracker.observe()
        now += 61_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Edited), outcomes())
        assertEquals(listOf("Claude"), results.single().pairs.map { it.meant })
    }

    @Test fun aRepeatedNewlineAfterAnchorFindsTheEndOfALongList() {
        field.text = "List:\n\n"
        field.cursor = 6
        val lines = (1..20).joinToString("\n") { if (it == 20) "line20 cloud" else "line" + it }
        dictate(lines)
        field.replace(field.text.indexOf("cloud"), field.text.indexOf("cloud") + 5, "Claude")
        field.setCursor(field.text.indexOf("line15"))
        tracker.observe()
        now += 61_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Edited), outcomes())
        assertEquals(listOf("Claude"), results.single().pairs.map { it.meant })
    }

    @Test fun deletingAndDictatingAgainIsARedictation() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        tracker.keyboardRemoved() // Hex's Delete key takes the whole dictation out of an empty field
        field.delete(field.text.length)
        tracker.observe()
        now += 5_000
        dictate("Meet at Claude nine.")
        assertEquals(listOf(DictationOutcome.Redictated), outcomes())
        assertEquals(listOf("Claude"), results.single().pairs.map { it.meant })
    }

    @Test fun undoingADictationInAnEmptyFieldStillAllowsARedictation() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        field.delete(field.text.length)
        tracker.undone()
        tracker.observe()
        now += 5_000
        dictate("Meet at Claude nine.")
        assertEquals(listOf(DictationOutcome.Redictated), outcomes())
    }

    @Test fun aFieldAnAppClearedIsNotARedictation() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        field.delete(field.text.length) // the app clears the field, as a chat app does on send
        tracker.observe()
        now += 5_000
        dictate("Meet at Claude nine.")
        assertEquals(listOf(DictationOutcome.Kept), outcomes())
        assertTrue(results.none { it.outcome == DictationOutcome.Redictated })
    }

    @Test fun aFieldAnAppClearedKeepsTheFixItSaw() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        field.replace(8, 13, "Claude")
        tracker.observe()
        field.setCursor(field.text.length)
        field.delete(field.text.length) // the app clears the field, as a chat app does on send
        tracker.observe()
        tracker.finish()
        assertEquals(listOf(DictationOutcome.Edited), outcomes())
        assertEquals(listOf("Claude"), results.single().pairs.map { it.meant })
    }

    @Test fun aFieldAnAppClearedIsJudgedBeforeTheNextMessage() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        field.replace(8, 13, "Claude")
        tracker.observe()
        field.setCursor(field.text.length)
        field.delete(field.text.length) // the app clears the field, as a chat app does on send
        tracker.observe()
        field.insert("ok") // the next message must not be read as this dictation's span
        tracker.observe()
        tracker.finish()
        assertEquals(listOf(DictationOutcome.Edited), outcomes())
        assertEquals(listOf("Claude"), results.single().pairs.map { it.meant })
    }

    @Test fun sendingTheFieldIsNoRedictation() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        tracker.sent() // Enter, Send, Go, Done or Next: the field goes on without the dictation
        assertFalse(tracker.isTracking)
        now += 5_000
        dictate("Meet at Claude nine.")
        assertEquals(listOf(DictationOutcome.Kept), outcomes())
        assertTrue(results.none { it.outcome == DictationOutcome.Redictated })
    }

    @Test fun sendingTheFieldKeepsTheFixItSaw() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        field.replace(8, 13, "Claude")
        tracker.observe()
        tracker.sent()
        assertEquals(listOf(DictationOutcome.Edited), outcomes())
        assertEquals(listOf("Claude"), results.single().pairs.map { it.meant })
        assertFalse(tracker.isTracking)
    }

    @Test fun anEditAfterTheMinuteIsNotFoldedIn() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        now += 61_000
        field.replace(8, 13, "Claude")
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Kept), outcomes())
        assertTrue(results.single().pairs.isEmpty())
    }

    @Test fun aDictationEmptiedAndRetypedAfterTheMinuteIsNotAnEdit() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        field.delete(field.text.length)
        now += 61_000
        field.insert("Meet at Claude nine.")
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Kept), outcomes())
        assertTrue(results.single().pairs.isEmpty())
    }

    @Test fun aDictationEmptiedBeforeTheMinuteAndRetypedAfterItIsNoEdit() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        now += 50_000
        tracker.keyboardRemoved()
        field.delete(field.text.length)
        tracker.observe()
        now += 15_000
        field.insert("Meet at Claude nine.") // re-typed after the window, not the dictation coming back
        tracker.observe()
        now += 10_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Deleted), outcomes())
        assertTrue(results.none { it.outcome == DictationOutcome.Edited })
    }

    @Test fun aDeletionWithoutADictationAfterItIsReportedDeleted() {
        dictate("Meet at noon.")
        tracker.observe()
        tracker.keyboardRemoved() // Hex's Delete key took it
        field.delete(field.text.length)
        tracker.observe()
        now += 21_000
        tracker.observe()
        assertEquals(listOf(DictationOutcome.Deleted), outcomes())
        assertFalse(tracker.isTracking)
    }

    @Test fun aDictationTheFieldNeverShowedReportsNothing() {
        tracker.inserted(null, "Meet at noon.", "Hi Sam,", " Thanks")
        field.text = "Something else entirely"
        field.cursor = field.text.length
        tracker.observe()
        now += 61_000
        tracker.observe()
        tracker.finish()
        assertTrue(results.isEmpty())
        assertFalse(tracker.isTracking)
    }

    @Test fun undoDictationIsReportedUndone() {
        dictate("Meet at noon.")
        tracker.observe()
        field.delete(field.text.length)
        tracker.undone()
        tracker.finish()
        assertEquals(listOf(DictationOutcome.Undone), outcomes())
    }
}
