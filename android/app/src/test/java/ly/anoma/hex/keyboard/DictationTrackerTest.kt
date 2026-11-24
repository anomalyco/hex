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

    @Test fun deletingAndDictatingAgainIsARedictation() {
        dictate("Meet at cloud nine.")
        tracker.observe()
        field.delete(field.text.length)
        tracker.observe()
        now += 5_000
        dictate("Meet at Claude nine.")
        assertEquals(listOf(DictationOutcome.Redictated), outcomes())
        assertEquals(listOf("Claude"), results.single().pairs.map { it.meant })
    }

    @Test fun aDeletionWithoutADictationAfterItIsReportedDeleted() {
        dictate("Meet at noon.")
        tracker.observe()
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
