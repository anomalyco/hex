package ly.anoma.hex.keyboard

import java.util.Locale
import ly.anoma.hex.speech.SpeechPhase
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Labs › Keyboard › Voice input mode's decisions, without Android: the view, the one automatic listen, the
 * delayed switch back, the write, and the subtype's locale. */
class VoiceInputTest {
    @Test fun theVoiceBarNeedsTheLabAndTheVoiceSubtype() {
        assertTrue(VoiceInput.active(lab = true, subtypeMode = VoiceInput.MODE))
        assertFalse(VoiceInput.active(lab = false, subtypeMode = VoiceInput.MODE))
        assertFalse(VoiceInput.active(lab = true, subtypeMode = "keyboard"))
        assertFalse(VoiceInput.active(lab = true, subtypeMode = null))
    }

    @Test fun enteringTheVoiceSubtypeArmsOneListen() {
        assertTrue(armsVoice(VoiceInput.MODE, null))
        assertTrue(armsVoice(VoiceInput.MODE, "keyboard"))
        assertFalse(armsVoice(VoiceInput.MODE, VoiceInput.MODE))
        assertFalse(armsVoice("keyboard", null))
        assertFalse(armsVoice(null, null))
    }

    @Test fun aNewClientInTheSameVoiceSubtypeDoesNotArmAgain() {
        // onUnbindInput keeps the handled mode, so a field in another app starts without the microphone opening.
        val handledAfterEntry = VoiceInput.MODE
        assertFalse(armsVoice(VoiceInput.MODE, handledAfterEntry))
    }

    @Test fun theSwitchWaitsUntilNothingIsPending() {
        assertTrue(voiceCanLeave(leaving = true, phase = SpeechPhase.Idle))
        for (phase in listOf(SpeechPhase.Opening, SpeechPhase.Listening, SpeechPhase.Transcribing)) {
            assertFalse(voiceCanLeave(leaving = true, phase = phase))
        }
        assertFalse(voiceCanLeave(leaving = false, phase = SpeechPhase.Idle))
    }

    @Test fun androidIsWrittenOnlyWhenItsListDisagrees() {
        val english = VoiceSubtype("en_US", "en-US")
        assertTrue(subtypeSyncNeeded(on = true, wanted = english, registered = null))
        assertFalse(subtypeSyncNeeded(on = true, wanted = english, registered = english))
        assertTrue(subtypeSyncNeeded(on = true, wanted = english, registered = VoiceSubtype("en", "en")))
        assertTrue(subtypeSyncNeeded(on = false, wanted = english, registered = english))
        assertFalse(subtypeSyncNeeded(on = false, wanted = english, registered = null))
    }

    @Test fun theSubtypeLanguageIsLanguageOrLanguageCountry() {
        assertEquals("zh_CN" to "zh-CN", subtypeLocale(Locale.forLanguageTag("zh-Hans-CN")))
        assertEquals("en" to "en", subtypeLocale(Locale.forLanguageTag("en")))
        assertEquals("en_US" to "en-US", subtypeLocale(Locale.forLanguageTag("en-US")))
        assertEquals("es_419" to "es-419", subtypeLocale(Locale.forLanguageTag("es-419")))
        assertEquals("sr_RS" to "sr-RS", subtypeLocale(Locale.forLanguageTag("sr-Latn-RS")))
    }
}
