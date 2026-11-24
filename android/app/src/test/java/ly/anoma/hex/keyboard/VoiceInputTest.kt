package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Labs › Keyboard › Voice input mode: the voice bar replaces the keys only in Hex's "voice" subtype with the
 * lab on; off, or in the full keyboard subtype, the keyboard is the one it always was. */
class VoiceInputTest {
    @Test fun theVoiceBarNeedsTheLabAndTheVoiceSubtype() {
        assertTrue(VoiceInput.active(lab = true, subtypeMode = VoiceInput.MODE))
        assertFalse(VoiceInput.active(lab = false, subtypeMode = VoiceInput.MODE))
        assertFalse(VoiceInput.active(lab = true, subtypeMode = "keyboard"))
        assertFalse(VoiceInput.active(lab = true, subtypeMode = null))
        assertFalse(VoiceInput.active(lab = true, subtypeMode = "VOICE"))
    }

    @Test fun theSubtypeIsAndroidsVoiceMode() {
        assertEquals("voice", VoiceInput.MODE)
    }
}
