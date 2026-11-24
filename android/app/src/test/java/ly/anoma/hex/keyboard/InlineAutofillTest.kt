package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Test

class InlineAutofillTest {
    @Test fun pinnedSuggestionsAreKeptApartAndInAndroidsOrder() {
        assertEquals(InlineAutofill.Split(listOf(0, 1, 2), emptyList()),
            InlineAutofill.split(listOf(false, false, false)))
        assertEquals(InlineAutofill.Split(listOf(0, 2), listOf(1)),
            InlineAutofill.split(listOf(false, true, false)))
        assertEquals(InlineAutofill.Split(listOf(1), listOf(0, 2)),
            InlineAutofill.split(listOf(true, false, true)))
        assertEquals(InlineAutofill.Split(emptyList(), listOf(0, 1, 2)),
            InlineAutofill.split(listOf(true, true, true)))
        assertEquals(InlineAutofill.Split(emptyList(), emptyList()), InlineAutofill.split(emptyList()))
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
