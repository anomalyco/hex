package ly.anoma.hex.keyboard

import kotlin.math.log10
import org.junit.Assert.assertEquals
import org.junit.Test

/** Labs › Keyboard › Key click volume (#3005): the slider's percent as the level the click is played at. */
class KeyClickTest {
    private fun db(percent: Int) = 20 * log10(KeyClick.scalar(percent))

    @Test fun fullIsTheUnscaledClick() = assertEquals(1f, KeyClick.scalar(100), 1e-6f)

    @Test fun everyTenPercentBelowFullIsThreeDecibelsQuieter() {
        assertEquals(-3f, db(90), 1e-3f)
        assertEquals(-15f, db(50), 1e-3f)
        assertEquals(-28.5f, db(5), 1e-3f)
    }

    @Test fun theSliderStaysBetweenFiveAndAHundred() {
        assertEquals(5, KeyClick.clamp(0))
        assertEquals(100, KeyClick.clamp(140))
        assertEquals(KeyClick.scalar(5), KeyClick.scalar(-20), 0f)
    }
}
