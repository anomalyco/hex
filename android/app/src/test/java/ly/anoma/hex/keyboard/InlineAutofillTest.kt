package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InlineAutofillTest {
    @Test fun withTheLabOffNoInlineSuggestionsAreRequested() {
        assertFalse(InlineAutofill.requests(lab = false))
        assertTrue(InlineAutofill.requests(lab = true))
    }

    @Test fun pinnedSuggestionsGoLastInAndroidsOrder() {
        assertEquals(listOf(0, 1, 2), InlineAutofill.ordered(listOf(false, false, false)))
        assertEquals(listOf(0, 2, 1), InlineAutofill.ordered(listOf(false, true, false)))
        assertEquals(listOf(1, 2, 0), InlineAutofill.ordered(listOf(true, false, false)))
        assertEquals(listOf(0, 1, 2), InlineAutofill.ordered(listOf(true, true, true)))
        assertEquals(emptyList<Int>(), InlineAutofill.ordered(emptyList()))
    }

    @Test fun aChipIsNoTallerThanTheStrip() {
        assertEquals(30, InlineAutofill.chipHeight(strip = 108, density = 1f))
        assertEquals(20, InlineAutofill.chipHeight(strip = 20, density = 1f))
        assertEquals(60, InlineAutofill.chipHeight(strip = 500, density = 2f))
    }

    @Test fun chipsAreTappableAndFitALogin() {
        assertEquals(56..240, InlineAutofill.chipWidths(1f))
        assertEquals(112..480, InlineAutofill.chipWidths(2f))
    }
}
