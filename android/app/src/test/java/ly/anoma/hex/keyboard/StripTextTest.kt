package ly.anoma.hex.keyboard

import ly.anoma.hex.diagnostics.Core
import ly.anoma.hex.diagnostics.Diagnostics
import ly.anoma.hex.diagnostics.Run
import ly.anoma.hex.diagnostics.Sched
import ly.anoma.hex.diagnostics.cgroupSummary
import ly.anoma.hex.diagnostics.clusterSummary
import ly.anoma.hex.diagnostics.HapticsFacts
import ly.anoma.hex.diagnostics.TapWords
import ly.anoma.hex.diagnostics.tapLines
import ly.anoma.hex.diagnostics.Tick
import ly.anoma.hex.diagnostics.hapticsLines
import ly.anoma.hex.diagnostics.tapCountLine
import ly.anoma.hex.diagnostics.clockSummary
import ly.anoma.hex.diagnostics.parseCpuList
import ly.anoma.hex.diagnostics.cpuRanges
import ly.anoma.hex.diagnostics.fastCores
import ly.anoma.hex.diagnostics.frequency
import ly.anoma.hex.diagnostics.libraryLabel
import ly.anoma.hex.diagnostics.report
import ly.anoma.hex.diagnostics.taskStat
import ly.anoma.hex.diagnostics.wavSamples
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
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

    // Labs › Diagnostics, whose keyboard speed test reports through the strip's notice line.

    @Test fun diagnosticsCoreListsJoinRuns() {
        assertEquals("0-3,6", cpuRanges(listOf(6, 2, 0, 1, 3)))
        assertEquals("7", cpuRanges(listOf(7)))
    }
    @Test fun diagnosticsGroupNeighbouringCoresWithTheSameCapacity() {
        // A Snapdragon 8 Elite: six performance cores, then two prime cores.
        val cores = (0..5).map { Core(it, 870, 3_532_800) } + (6..7).map { Core(it, 1024, 4_320_000) }
        assertEquals("cpu 0-5: capacity 870, up to 3.53 GHz; cpu 6-7: capacity 1024, up to 4.32 GHz", clusterSummary(cores))
        assertEquals("cpu 0-1: capacity not readable, top frequency not reported", clusterSummary(listOf(Core(0, null, null), Core(1, null, null))))
        // An emulator's cpufreq says 2 (kHz): no real core runs at that.
        assertEquals("cpu 0-3: capacity 1024, top frequency not reported", clusterSummary((0..3).map { Core(it, 1024, 2) }))
    }
    @Test fun diagnosticsCountFastCoresForTheSixThreadRun() {
        assertEquals(8, fastCores(List(6) { 870 } + List(2) { 1024 })) // Snapdragon 8 Elite
        assertEquals(2, fastCores(List(6) { 160 } + List(2) { 1024 })) // Snapdragon 695
        assertEquals(0, fastCores(listOf(1024, null)))
    }
    @Test fun diagnosticsGiveFrequenciesFromKilohertz() {
        assertEquals("2.00 GHz", frequency(2_000_000))
        assertEquals("4.32 GHz", frequency(4_320_000))
        assertEquals("1.00 GHz", frequency(1_000_000))
        assertEquals("999 MHz", frequency(999_999))
        assertEquals("300 MHz", frequency(300_000))
        assertNull(frequency(2))
        assertNull(frequency(0))
    }
    @Test fun diagnosticsSayWhatThePhoneDoesWithHaptics() {
        // A realme on Android 16 that only feels the system's own keyboard-switch vibration.
        assertEquals(listOf(
            "Hex Haptics: on, so key presses ask to vibrate: yes",
            "Keyboard vibration setting: on this phone no, value not set",
            "Touch feedback: 1, strength not set, sound mode vibrate",
            "Your phone says it can play: click unknown, tick unknown, strength control no",
        ), hapticsLines(HapticsFacts(true, true, false, "not set", "1", "not set", "vibrate", true, "unknown", "unknown", false)))
        assertEquals("Your phone says it can play: click no, tick no, strength control no (it reports no vibrator)",
            hapticsLines(HapticsFacts(false, false, true, "0", "0", "0", "silent", false, "no", "no", false)).last())
        assertEquals("Taps Hex asked for: 12, stopped by Hex's settings: 0 (since Hex last started)", tapCountLine(12, 0))
    }
    @Test fun diagnosticsNameWhatEachTapPlays() {
        // The same decision tap() makes: Android's effect where offered, else Hex's pulse.
        val offered = Tap.entries.map { it to it.plays(offered = true, amplitude = true) }
        assertEquals(listOf("Key press: Android's tick", "Held key lifting: Android's tick", "Dictation start: Android's click",
            "Dictation end: Android's tick"), tapLines(TapWords.English, offered))
        assertEquals("Key press: a 12 ms pulse at 31% strength, because this phone doesn't offer Android's tick",
            tapLines(TapWords.English, listOf(Tap.Press to Tap.Press.plays(offered = false, amplitude = true))).single())
        assertEquals("Dictation start: a 20 ms pulse at the phone's default strength, because this phone doesn't offer Android's click",
            tapLines(TapWords.English, listOf(Tap.Start to Tap.Start.plays(offered = false, amplitude = false))).single())
    }
    @Test fun diagnosticsReadCpuLists() {
        assertEquals(setOf(0, 1, 2, 3, 6), parseCpuList("0-3,6"))
        assertEquals(emptySet<Int>(), parseCpuList(null))
    }
    @Test fun diagnosticsShowAClockCapDuringTheRun() {
        // A OnePlus 13 as the study describes it: performance cores capped from 3.53 to 2.40 GHz after a moment,
        // prime cores never used.
        val cores = (0..5).map { Core(it, 870, 3_532_800) } + (6..7).map { Core(it, 1024, 4_320_000) }
        val ticks = listOf(3_532_800L, 2_400_000L, 2_400_000L, 2_400_000L).map { khz ->
            Tick(setOf(2, 3), (0..7).associateWith { if (it < 6) khz else 1_000_000L })
        } + Tick(emptySet(), (0..7).associateWith { 300_000L })
        assertEquals("cpu 0-5 2.40 GHz–3.53 GHz, 75% of 8 samples below its top 3.53 GHz; cpu 6-7 not used",
            clockSummary(cores, (0..7).toSet(), ticks))
        // Only the cores the thread may use are watched.
        assertEquals("cpu 0-5 2.40 GHz–3.53 GHz, 75% of 8 samples below its top 3.53 GHz", clockSummary(cores, (0..5).toSet(), ticks))
        // An emulator's 1 and 2 kHz, and a phone that hides its clocks.
        val emulator = (0..3).map { Core(it, 1024, 2) }
        assertEquals("cpu 0-3 frequency not reported", clockSummary(emulator, (0..3).toSet(), listOf(Tick(setOf(1), (0..3).associateWith { 2L }))))
        assertEquals("cpu 0-3 frequency not reported", clockSummary(emulator, (0..3).toSet(), listOf(Tick(setOf(1), (0..3).associateWith { null }))))
        assertNull(clockSummary(emulator, (0..3).toSet(), emptyList()))
    }
    @Test fun diagnosticsReadATaskStatWhoseNameHasSpacesAndBrackets() {
        val fields = (4..52).joinToString(" ") { if (it == 39) "5" else "0" }
        assertEquals(Triple("Hex speed (test)", 'R', 5), taskStat("1234 (Hex speed (test)) R $fields"))
        assertNull(taskStat("1234 (short) S 1 2"))
    }
    @Test fun diagnosticsDropTheUidAndPidFromCgroups() {
        assertEquals("0::/uid_*/pid_* 4:cpuset:/top-app", cgroupSummary("0::/uid_10234/pid_5678\n4:cpuset:/top-app\n"))
    }
    @Test fun diagnosticsNameTheSpeechLibraryPlainly() {
        assertEquals("standard", libraryLabel("libhexspeech.so"))
        assertEquals("faster", libraryLabel("libhexspeech_v82.so"))
        assertEquals("unknown", libraryLabel(null))
    }
    @Test fun diagnosticsReadTheClipPastOtherChunks() {
        fun le(value: Int, bytes: Int) = ByteArray(bytes) { (value shr (8 * it)).toByte() }
        fun wav(channels: Int) = "RIFF".toByteArray() + le(0, 4) + "WAVE".toByteArray() +
            "fmt ".toByteArray() + le(16, 4) + le(1, 2) + le(channels, 2) + le(16_000, 4) + le(32_000 * channels, 4) +
            le(2 * channels, 2) + le(16, 2) + "LIST".toByteArray() + le(3, 4) + byteArrayOf(1, 2, 3, 0) +
            "data".toByteArray() + le(4, 4) + le(-2, 2) + le(300, 2)
        assertArrayEquals(shortArrayOf(-2, 300), wavSamples(wav(1)))
        assertNull(wavSamples(wav(2)))
        assertNull(wavSamples("not a wav file".toByteArray()))
    }
    @Test fun diagnosticsReportEachRunAndWhereItRan() {
        val top = Sched("/top-app", "0::/uid_*/pid_*", "0-7", 0)
        fun run(n: Int, after: Sched) = Run("as now", n, true, 5_812, 19_340, longArrayOf(20_000, 4_100_000, 1_400_000, 0, 80_000, 0, 4),
            "And so, my fellow Americans.", top, after, "cpu 4-7", "cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz")
        val done = Diagnostics.Speed.Done(listOf(run(1, top), run(2, top.copy(cpuset = "/foreground"))), 11.0)
        val steady = Diagnostics.Speed.Done(listOf(run(1, top), run(2, top)), 11.0)
        assertEquals("""
            Hex diagnostics
            Build: Hex 0.2.2 (build 5), preview

            Speed test in the app: bundled 11.0 s English clip, downloaded model
              same for every run: cgroup 0::/uid_*/pid_*, cpus allowed 0-7, nice 0
              as now, run 1: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads, encoder window full
                cpuset /top-app
                ran on cpu 4-7
                clocks while it ran: cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz
              as now, run 2: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads, encoder window full
                cpuset /top-app; at the end cpuset /foreground
                ran on cpu 4-7
                clocks while it ran: cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz
              text: And so, my fellow Americans. (every run)

            Speed test in the keyboard, open in another app: bundled 11.0 s English clip, downloaded model
              same for every run: cpuset /top-app, cgroup 0::/uid_*/pid_*, cpus allowed 0-7, nice 0
              as now, run 1: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads, encoder window full
                ran on cpu 4-7
                clocks while it ran: cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz
              as now, run 2: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads, encoder window full
                ran on cpu 4-7
                clocks while it ran: cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz
              text: And so, my fellow Americans. (every run)

        """.trimIndent(), report(listOf("Build: Hex 0.2.2 (build 5), preview"), done, steady))
    }
}
