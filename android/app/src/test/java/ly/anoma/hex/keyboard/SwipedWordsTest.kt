package ly.anoma.hex.keyboard

import ly.anoma.hex.typing.SwipeTyped
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** What a swipe puts in the editor, and how delete and the strip's other readings take it back. */
class SwipedWordsTest {
    /** An editor holding [text] with the cursor at its end (or at [cursor]). */
    private class Editor(var text: String) : KeyboardTextTarget {
        var cursor = text.length
        var selection = ""
        override val connected = true
        override fun before(length: Int) = text.substring(maxOf(0, cursor - length), cursor)
        override fun after(length: Int) = text.substring(cursor, minOf(text.length, cursor + length))
        override fun selected() = selection
        override fun capsMode(mask: Int) = 0
        override fun commit(text: String): Boolean {
            this.text = this.text.substring(0, cursor) + text + this.text.substring(cursor); cursor += text.length; return true
        }
        override fun deleteBefore(length: Int): Boolean {
            text = text.substring(0, cursor - length) + text.substring(cursor); cursor -= length; return true
        }
        override fun deleteCodePointsBefore(count: Int) = deleteBefore(count)
        override fun beginBatch() {}
        override fun endBatch() {}
        override fun setSelection(start: Int, end: Int) = true
        override fun performEditorAction(id: Int) = true
        override fun defaultEditorAction() = true
        override fun sendKey(code: Int) {}
    }

    @Test fun theWordGoesInAfterTheSpaceTheCoreAsksFor() {
        val editor = Editor("hello")
        val swiped = SwipedWords(editor)
        assertTrue(swiped.type(SwipeTyped(true, "world", arrayOf("word", "worlds", "wired", "would"))))
        assertEquals("hello world", editor.text)
        assertEquals(listOf("word", "worlds", "wired"), swiped.alternatives)
        val after = Editor("(")
        assertTrue(SwipedWords(after).type(SwipeTyped(false, "Hi", emptyArray())))
        assertEquals("(Hi", after.text)
    }

    @Test fun deleteTakesTheWholeWordBackWithItsSpace() {
        val editor = Editor("hello")
        val swiped = SwipedWords(editor)
        swiped.type(SwipeTyped(true, "world", arrayOf("word")))
        assertTrue(swiped.takeBack())
        assertEquals("hello", editor.text)
        // Only once: the next delete is an ordinary one.
        assertFalse(swiped.active)
        assertFalse(swiped.takeBack())
        assertEquals("hello", editor.text)
    }

    @Test fun anotherReadingTakesTheWordsPlaceAndTheWordJoinsTheOthers() {
        val editor = Editor("hello")
        val swiped = SwipedWords(editor)
        swiped.type(SwipeTyped(true, "world", arrayOf("word", "worlds")))
        assertTrue(swiped.replace("word"))
        assertEquals("hello word", editor.text)
        assertEquals(listOf("world", "worlds"), swiped.alternatives)
        // Delete then takes back the reading picked, with the swipe's space.
        assertTrue(swiped.takeBack())
        assertEquals("hello", editor.text)
    }

    @Test fun aWordThatIsNoLongerBeforeTheCursorStays() {
        val editor = Editor("hello")
        val swiped = SwipedWords(editor)
        swiped.type(SwipeTyped(true, "world", arrayOf("word")))
        editor.cursor = 2 // a tap in the text
        assertFalse(swiped.inPlace())
        assertFalse(swiped.takeBack())
        assertEquals("hello world", editor.text)

        val selected = Editor("hello")
        val other = SwipedWords(selected)
        other.type(SwipeTyped(true, "world", arrayOf("word")))
        selected.selection = "world"
        assertFalse(other.replace("word"))
        assertEquals("hello world", selected.text)
    }

    @Test fun onlyTheSwipesOwnReadingsReplaceIt() {
        val editor = Editor("hello")
        val swiped = SwipedWords(editor)
        swiped.type(SwipeTyped(true, "world", arrayOf("word")))
        assertFalse(swiped.replace("there"))
        assertEquals("hello world", editor.text)
        assertTrue(swiped.active)
    }
}
