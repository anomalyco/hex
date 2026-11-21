package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Test
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.speech.SpokenCleanup
import ly.anoma.hex.speech.SpokenText
import ly.anoma.hex.speech.SpeechOptions
import ly.anoma.hex.speech.Style

class InsertionTest {
    @Test fun replacesASelectedWordWithoutJoiningItsNeighbours() {
        assertEquals(" hello ", insertion(" hello ", 'a', 'b'))
    }
    @Test fun respectsWhitespaceAndClosingPunctuationInTheEditor() {
        assertEquals("Hello", insertion("  Hello ", '\n', '!'))
        assertEquals("hello", insertion("hello", '(', ')'))
        assertEquals(", yes", insertion(", yes", 'e', null))
    }
    @Test fun emptyResultMakesNoEdit() {
        assertEquals("", insertion(" \n ", 'a', 'b'))
    }

    @Test fun stylesChangeWhatReachesTheEditor() {
        fun render(text: String, style: Style) = insertion(
            SpokenText.format(text, SpeechOptions(style, Spoken.English, true, false)), null, null)
        assertEquals("Can you send it Thursday.", render("Can you, um, send it Tuesday, no wait, Thursday.", Style.Polished))
        assertEquals("see you soon", render("Um, see you soon.", Style.Casual))
        assertEquals("- Buy milk\n- Call Sam", render("bullet buy milk next bullet call Sam", Style.Notes))
        assertEquals("Um, Tuesday, sorry, Thursday.", render("Um, Tuesday, sorry, Thursday.", Style.Verbatim))
        assertEquals("Das ist eh egal.", SpokenText.format("Äh, das ist eh egal.",
            SpeechOptions(Style.Polished, Spoken.German, false, false)))
    }
    @Test fun removesOnlyAnchoredRepairsAndPreservesNegation() {
        assertEquals("Meet at 4pm?", SpokenCleanup.clean("Meet at 3pm, actually 4pm?", "en"))
        assertEquals("I didn't say Friday.", SpokenCleanup.clean("I didn't say Friday.", "en"))
        assertEquals("I want the red one, sorry, eventually.", SpokenCleanup.clean("I want the red one, sorry, eventually.", "en"))
        assertEquals("I sent the invoice, sorry I am late.", SpokenCleanup.clean("I sent the invoice, sorry I am late.", "en"))
        assertEquals("It isn’t blue, I mean, it is green.", SpokenCleanup.clean("It isn’t blue, I mean, it is green.", "en"))
        assertEquals("Uno\ndos", SpokenCleanup.clean("Uno\ndos", "es"))
        assertEquals("eBay works.", SpokenCleanup.clean("Um, eBay works.", "en"))
        assertEquals("Te digo que no puedo ir.", SpokenCleanup.clean("Te digo que no puedo ir.", "es"))
        assertEquals("¿El miércoles?", SpokenCleanup.clean("¿El martes, perdón, el miércoles?", "es"))
        assertEquals("Am Mittwoch.", SpokenCleanup.clean("Am Dienstag, ich meine, Mittwoch.", "de"))
        assertEquals("Det er torsdag.", SpokenCleanup.clean("Det er tirsdag, nei, torsdag.", "no"))
    }

    // The space bar (SpaceGesture, CursorSlide). These belong in a KeyboardGestureTest once new test files are agreed.
    @Test fun quickSpaceTapsThatDriftStillTypeTheirSpace() {
        for (ms in listOf(60L, 100L, 150L)) for (drift in listOf(8f, 21f, 42f, 63f)) {
            val space = SpaceGesture(start = 45f, step = 33f, stillSlop = 21f)
            space.down(500f, 50f, 0)
            for (t in 10L..ms step 10L) assertEquals(null, space.move(500f - drift * t / ms, 50f, t))
            assertEquals(true, space.up())
        }
    }
    @Test fun aSpaceSlideMovesTheCursorAtGboardsSteps() {
        val space = SpaceGesture(start = 45f, step = 33f, stillSlop = 21f)
        space.down(500f, 50f, 0)
        assertEquals(null, space.move(458f, 50f, 300))
        assertEquals(-1, space.move(455f, 50f, 310))
        assertEquals(-3, space.move(374f, 50f, 320))
        assertEquals(-15, space.move(-25f, 50f, 330))
        assertEquals(2, space.move(580f, 50f, 340))
        assertEquals(false, space.up())
    }
    @Test fun aCursorSlideStepsOverWholeCharacters() {
        val moves = ArrayList<Int>()
        val text = "ab\uD83D\uDC4D\uD83C\uDFFDcd"
        val target = object : KeyboardTextTarget {
            override val connected = true
            override fun before(length: Int) = text
            override fun after(length: Int) = ""
            override fun selected() = ""
            override fun capsMode(mask: Int) = 0
            override fun commit(text: String) = true
            override fun deleteBefore(length: Int) = true
            override fun deleteCodePointsBefore(count: Int) = true
            override fun beginBatch() {}
            override fun endBatch() {}
            override fun setSelection(start: Int, end: Int) = moves.add(start)
            override fun performEditorAction(id: Int) = true
            override fun defaultEditorAction() = true
            override fun sendKey(code: Int) {}
        }
        val slide = CursorSlide(target, text.length, text.length, readable = true)
        for (n in listOf(-1, -2, -3, -4, -5, -9, 1)) slide.moveTo(n)
        assertEquals(listOf(7, 6, 2, 1, 0, 8), moves)
    }
}
