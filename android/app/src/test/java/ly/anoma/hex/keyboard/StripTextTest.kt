package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Test

class StripTextTest {
    /** A word ten times as wide as its size, in the same units as the space it is given. */
    private fun fit(available: Float) = fitTextSize(available, StripText.SIZE, StripText.MIN) { it * 10 }

    @Test fun aWordThatFitsKeepsTheStripSize() {
        assertEquals(StripText.SIZE, fit(StripText.SIZE * 10), 0f)
    }
    @Test fun aLongWordShrinksOnlyAsFarAsItMust() {
        assertEquals(13.5f, fit(135f), 0.01f)
    }
    @Test fun aWordWithNoRoomStopsAtTheMinimum() {
        assertEquals(StripText.MIN, fit(StripText.MIN * 10 - 1), 0.01f)
    }
    @Test fun noRoomAtAllStillGivesTheMinimum() {
        assertEquals(StripText.MIN, fit(0f), 0f)
    }
}
