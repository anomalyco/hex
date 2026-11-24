package ly.anoma.hex.keyboard

import ly.anoma.hex.diagnostics.Core
import ly.anoma.hex.diagnostics.Diagnostics
import ly.anoma.hex.diagnostics.Run
import ly.anoma.hex.diagnostics.Sched
import ly.anoma.hex.diagnostics.cgroupSummary
import ly.anoma.hex.diagnostics.clusterSummary
import ly.anoma.hex.diagnostics.coresSummary
import ly.anoma.hex.diagnostics.cpuRanges
import ly.anoma.hex.diagnostics.fastCores
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
        assertEquals("cpu 0-1: capacity not readable, top frequency not readable", clusterSummary(listOf(Core(0, null, null), Core(1, null, null))))
    }
    @Test fun diagnosticsCountFastCoresForTheSixThreadRun() {
        assertEquals(8, fastCores(List(6) { 870 } + List(2) { 1024 })) // Snapdragon 8 Elite
        assertEquals(2, fastCores(List(6) { 160 } + List(2) { 1024 })) // Snapdragon 695
        assertEquals(0, fastCores(listOf(1024, null)))
    }
    @Test fun diagnosticsSayWhichCoresTheTestRanOnAndHowFast() {
        assertEquals("cpu 6-7 at 1.20 GHz–4.32 GHz", coresSummary(mapOf(6 to listOf(1_200_000L), 7 to listOf(4_320_000L, 3_000_000L))))
        assertEquals("cpu 2, frequency not readable", coresSummary(mapOf(2 to emptyList())))
        assertNull(coresSummary(emptyMap()))
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
            "And so, my fellow Americans.", top, after, "cpu 4-7 at 1.80 GHz–3.53 GHz")
        val done = Diagnostics.Speed.Done(listOf(run(1, top), run(2, top.copy(cpuset = "/foreground"))), 11.0)
        assertEquals("""
            Hex diagnostics
            Build: Hex 0.2.2 (build 5), preview

            Speed test in the app: bundled 11.0 s English clip, downloaded model
              as now, run 1: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads, encoder window full
                cpuset /top-app, cgroup 0::/uid_*/pid_*, cpus allowed 0-7, nice 0
                ran on cpu 4-7 at 1.80 GHz–3.53 GHz
              as now, run 2: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads, encoder window full
                cpuset /top-app, cgroup 0::/uid_*/pid_*, cpus allowed 0-7, nice 0; at the end cpuset /foreground, cgroup 0::/uid_*/pid_*, cpus allowed 0-7, nice 0
                ran on cpu 4-7 at 1.80 GHz–3.53 GHz
              text: And so, my fellow Americans. (every run)

            Speed test in the keyboard, open in another app: not run

        """.trimIndent(), report(listOf("Build: Hex 0.2.2 (build 5), preview"), done, Diagnostics.Speed.Waiting))
    }
}
