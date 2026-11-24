package ly.anoma.hex.keyboard

import org.junit.Assert.assertFalse
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
}
