package ly.anoma.hex.keyboard

import ly.anoma.hex.QuickDictation
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VoiceOnlyTest {
    @Test fun withTheLabOffTheKeyboardIsUnchanged() {
        assertFalse(VoiceOnly.shows(lab = false, subtypeMode = VoiceOnly.MODE))
        assertFalse(VoiceOnly.shows(lab = false, subtypeMode = "keyboard"))
    }

    @Test fun theVoiceSubtypeWithTheLabOnShowsTheBar() {
        assertTrue(VoiceOnly.shows(lab = true, subtypeMode = VoiceOnly.MODE))
    }

    @Test fun aRestartAfterALeaveKeepsTheBarIdle() {
        assertTrue(VoiceOnly.startsOnShow(restarting = false, left = false))
        assertTrue(VoiceOnly.startsOnShow(restarting = true, left = false))
        assertTrue(VoiceOnly.startsOnShow(restarting = false, left = true))
        assertFalse(VoiceOnly.startsOnShow(restarting = true, left = true))
    }

    @Test fun theFullKeyboardSubtypeKeepsTheKeys() {
        assertFalse(VoiceOnly.shows(lab = true, subtypeMode = "keyboard"))
        assertFalse(VoiceOnly.shows(lab = true, subtypeMode = null))
        assertFalse(VoiceOnly.shows(lab = true, subtypeMode = ""))
        assertFalse(VoiceOnly.shows(lab = true, subtypeMode = "Voice"))
    }

    // Labs › Keyboard › Voice bar from the Hex menu (#2869).
    @Test fun theKeysCollapseOnlyWithTheLabOnAndVoiceOnlyChosen() {
        assertTrue(VoiceOnly.collapses(lab = true, collapsed = true, secure = false))
        assertFalse(VoiceOnly.collapses(lab = true, collapsed = false, secure = false))
        // Off, the keyboard is unchanged even if the menu collapsed it before the lab went off.
        assertFalse(VoiceOnly.collapses(lab = false, collapsed = true, secure = false))
    }

    @Test fun aPasswordFieldAlwaysGetsItsKeys() {
        assertFalse(VoiceOnly.collapses(lab = true, collapsed = true, secure = true))
    }

    // Labs › Dictate from Quick Settings, the other way to dictate without Hex as the keyboard.
    @Test fun theQuickSettingsSheetClosesOnceItsTextIsCopied() {
        // Android 13 and later show their own copied preview: the sheet closes at once.
        assertEquals(0L, QuickDictation.closeAfterCopyMs(noteShown = false, sdk = 33))
        assertEquals(0L, QuickDictation.closeAfterCopyMs(noteShown = false, sdk = 36))
        // Android 12 shows none: the sheet says Copied for a moment.
        assertEquals(QuickDictation.COPIED_MS, QuickDictation.closeAfterCopyMs(noteShown = false, sdk = 31))
        assertEquals(QuickDictation.COPIED_MS, QuickDictation.closeAfterCopyMs(noteShown = false, sdk = 32))
    }

    @Test fun aNoteKeepsTheQuickSettingsSheetOpen() {
        // The 1-minute stop or low memory explains the copied text: the person closes the sheet.
        assertNull(QuickDictation.closeAfterCopyMs(noteShown = true, sdk = 36))
        assertNull(QuickDictation.closeAfterCopyMs(noteShown = true, sdk = 31))
    }
}
