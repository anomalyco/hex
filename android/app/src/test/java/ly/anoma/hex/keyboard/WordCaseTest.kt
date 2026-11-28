package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Labs › Keyboard › Shift changes a word's case (#3008). */
class WordCaseTest {
    @Test fun aShiftTapCyclesLowercaseTitleAndCapitals() {
        assertEquals("Sf", WordCase.next("sf"))
        assertEquals("SF", WordCase.next("Sf"))
        assertEquals("sf", WordCase.next("SF"))
        // Any other casing goes to Title.
        assertEquals("Iphone", WordCase.next("iPhone"))
        assertEquals("Github", WordCase.next("GitHUB"))
        // A single letter's Title and capitals are one step.
        assertEquals("A", WordCase.next("a"))
        assertEquals("a", WordCase.next("A"))
        // Apostrophes stay, and letters change one for one, so the word keeps its length.
        assertEquals("Don't", WordCase.next("don't"))
        assertEquals("DON’T", WordCase.next("Don’t"))
        assertEquals("STRAßE", WordCase.next("Straße"))
        // Nothing to change: the caller leaves shift as it was.
        assertEquals("中文", WordCase.next("中文"))
    }

    @Test fun onlyCapitalsOfTwoLettersOrMoreLockShift() {
        assertTrue(WordCase.allCaps("SF"))
        assertTrue(WordCase.allCaps("DON'T"))
        assertFalse(WordCase.allCaps("I"))
        assertFalse(WordCase.allCaps("Sf"))
    }

    @Test fun findsTheWordAtTheEndOrAroundTheCursor() {
        assertEquals(WordCase.Span("sf", ""), WordCase.at("meet in sf", ""))
        assertEquals(WordCase.Span("sf", ""), WordCase.at("meet in sf", ". ok"))
        assertEquals(WordCase.Span("a", "i"), WordCase.at("say a", "i now"))
        assertEquals(WordCase.Span("don't", ""), WordCase.at("I don't", ""))
        assertEquals(WordCase.Span("quoted", ""), WordCase.at("a 'quoted", "' word"))
        assertEquals(WordCase.Span("sf", ""), WordCase.at("sf", ""))
    }

    @Test fun capitalsLockHoldsWhileTheRecasedWordGoesOn() {
        // "SF" recased at the field's 8th character: the cursor reported two letters later, then one more.
        assertTrue(WordCase.continues("meet in SF", 2))
        assertTrue(WordCase.continues("meet in SFO", 3))
        // A late cursor report: "SFOX" is already in while the editor still says three letters.
        assertTrue(WordCase.continues("meet in SFOX", 3))
        assertTrue(WordCase.continues("DON'T", 4))
        assertTrue(WordCase.continues("SF", 2))                  // the word starts the field
        // The word ended: a space, a mark, or the cursor in a later word nearer than the recased one began.
        assertFalse(WordCase.continues("meet in SF ", 3))
        assertFalse(WordCase.continues("meet in SF.", 3))
        assertFalse(WordCase.continues("meet in SF and", 7))
        assertFalse(WordCase.continues("meet in S", 0))
    }

    @Test fun shiftIsAsBeforeAwayFromAWord() {
        assertNull(WordCase.at("", ""))
        assertNull(WordCase.at("sf ", ""))
        assertNull(WordCase.at("sf.", ""))
        assertNull(WordCase.at("meet ", "sf"))
        assertNull(WordCase.at("don'", ""))
        // Not a word to recase: run against digits or an underscore.
        assertNull(WordCase.at("mp", "3"))
        assertNull(WordCase.at("snake_case", ""))
        assertNull(WordCase.at("3d", ""))
    }
}
