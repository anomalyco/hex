package ly.anoma.hex.speech

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DictationSoundsTest {
    private val sounds = File("../../resources/audio/sounds")

    @Test fun everySetsSoundIsDesktopsOwnFile() {
        for (set in SoundSet.entries) for (cue in SoundCue.entries) {
            val asset = soundAsset(set, cue) ?: continue
            assertTrue("$set $cue", File(sounds, asset.removePrefix("sounds/")).isFile)
        }
    }

    @Test fun classicAndThockHaveNoDoneAndClassicBorrowsKalimbasError() {
        assertNull(soundAsset(SoundSet.Classic, SoundCue.Done))
        assertNull(soundAsset(SoundSet.Thock, SoundCue.Done))
        assertEquals("sounds/signature/error.wav", soundAsset(SoundSet.Classic, SoundCue.Error))
        assertEquals("sounds/thock/error.wav", soundAsset(SoundSet.Thock, SoundCue.Error))
    }

    @Test fun startSoundsLastTheirLength() {
        // Classic's is 44.1 kHz stereo, the others 48 kHz mono.
        assertEquals(271L, wavDurationMs(File(sounds, "classic/start.wav").readBytes()))
        assertEquals(116L, wavDurationMs(File(sounds, "signature/start.wav").readBytes()))
        for (set in SoundSet.entries) {
            val ms = wavDurationMs(File(sounds, "${set.folder}/start.wav").readBytes())
            assertTrue("$set $ms", ms != null && ms in 50..500)
        }
    }

    @Test fun notAWavHasNoLength() {
        assertNull(wavDurationMs(ByteArray(0)))
        assertNull(wavDurationMs("RIFF\u0000\u0000\u0000\u0000WAVEdata".toByteArray()))
        // A fmt chunk cut off before its byte rate's last byte.
        val head = "RIFF\u0000\u0000\u0000\u0000WAVEfmt \u0010\u0000\u0000\u0000".toByteArray()
        assertNull(wavDurationMs(head + ByteArray(11)))
    }

    @Test fun theStartSoundIsZeroedWhereverItPlays() {
        // Still to play or be dropped: everything read so far is zeroed.
        assertEquals(Int.MAX_VALUE, startMuteSamples(pending = true, endsAtMs = 0, recordingStartedMs = 1_000))
        // Never played: nothing is.
        assertEquals(0, startMuteSamples(pending = false, endsAtMs = 0, recordingStartedMs = 1_000))
        // Classic's 271 ms played 300 ms late (its set still loading): zeroed to 300 + 271 + 120 ms after recording began.
        val classicLate = startMuteSamples(false, 1_000 + 300 + 271 + START_GUARD_EXTRA_MS, 1_000)
        assertEquals(16 * 691, classicLate)
        // Played at once: its length and the guard.
        assertEquals(16 * (97 + 120), startMuteSamples(false, 1_000 + 97 + START_GUARD_EXTRA_MS, 1_000))
    }

    @Test fun anUnknownSetIsDesktopsDefault() {
        assertEquals(SoundSet.Classic, SoundSet.matching(null))
        assertEquals(SoundSet.Classic, SoundSet.matching("Gone"))
        assertEquals(SoundSet.Thock, SoundSet.matching("Thock"))
    }
}
