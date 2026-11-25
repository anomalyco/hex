package ly.anoma.hex.keyboard

import ly.anoma.hex.diagnostics.Core
import ly.anoma.hex.diagnostics.Diagnostics
import ly.anoma.hex.diagnostics.Run
import ly.anoma.hex.diagnostics.Sched
import ly.anoma.hex.diagnostics.cgroupSummary
import ly.anoma.hex.diagnostics.clusterSummary
import android.os.VibrationEffect
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
import ly.anoma.hex.diagnostics.FASTER_MODE
import ly.anoma.hex.diagnostics.fasterEnglishLine
import ly.anoma.hex.diagnostics.libraryLabel
import ly.anoma.hex.speech.Engine
import ly.anoma.hex.speech.fastCoreMask
import ly.anoma.hex.diagnostics.report
import ly.anoma.hex.diagnostics.taskStat
import ly.anoma.hex.diagnostics.wavSamples
import ly.anoma.hex.diagnostics.lastDictationLines
import ly.anoma.hex.diagnostics.DICTATION_ROW
import ly.anoma.hex.diagnostics.KEPT_ANY
import ly.anoma.hex.diagnostics.KEPT_FAST
import ly.anoma.hex.diagnostics.poolWords
import ly.anoma.hex.diagnostics.processorLine
import ly.anoma.hex.diagnostics.threadWords
import ly.anoma.hex.speech.DictationPlace
import ly.anoma.hex.speech.LanguageChoice
import ly.anoma.hex.speech.DictationLanguage
import ly.anoma.hex.speech.DictationTimes
import ly.anoma.hex.speech.ModelAtTick
import ly.anoma.hex.speech.RanOn
import ly.anoma.hex.speech.modelAtTick
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
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
        // A debug build's stand-in ratings (the emulator check) say so on the Processor line; a phone's line is unchanged.
        val emulator = (0..3).map { Core(it, null, 2) }
        assertEquals("Processor: Google Ranchu, 4 cores; cpu 0-3: capacity not readable, top frequency not reported " +
            "(debug core ratings: 300, 300, 1024, 1024)", processorLine("Google Ranchu", emulator, listOf(300, 300, 1024, 1024)))
        assertEquals("Processor: QTI SM8750, 8 cores; cpu 0-5: capacity 870, up to 3.53 GHz; cpu 6-7: capacity 1024, up to 4.32 GHz",
            processorLine("QTI SM8750", cores, null))
    }
    @Test fun diagnosticsCountFastCoresForTheSixThreadRun() {
        assertEquals(8, fastCores(List(6) { 870 } + List(2) { 1024 })) // Snapdragon 8 Elite
        assertEquals(2, fastCores(List(6) { 160 } + List(2) { 1024 })) // Snapdragon 695
        assertEquals(0, fastCores(listOf(1024, null)))
    }
    @Test fun dictationKeepsItsThreadsOnTheFastCores() {
        // A MediaTek phone's four little cores at 477, three at 871 and one at 1024: cores 4-7.
        assertEquals(0xF0, fastCoreMask(List(4) { 477 } + List(3) { 871 } + listOf(1024)))
        // A Snapdragon 695's six at 160 and two at 1024: cores 6 and 7.
        assertEquals(0xC0, fastCoreMask(List(6) { 160 } + List(2) { 1024 }))
        // Nothing to choose between: every core counts as fast (a Snapdragon 8 Elite's eight), none is rated, or none
        // the kernel rates above zero. No restriction is better than one bit for every core.
        assertEquals(0, fastCoreMask(List(8) { 1024 }))
        assertEquals(0, fastCoreMask(List(6) { 870 } + List(2) { 1024 }))
        assertEquals(0, fastCoreMask(listOf(0, 0, 0, 0)))
        // One core Hex can't rate leaves placement to the kernel, as before.
        assertEquals(0, fastCoreMask(listOf(477, 871, 1024, null)))
        assertEquals(0, fastCoreMask(emptyList()))
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
            "Hex's Haptics switch: on",
            "Keyboard vibration switch: this phone has none (stored value 0)",
            "Touch feedback: on, strength not set; ringer: vibrate only",
            "So Hex asks the phone to vibrate on a key press: yes",
            "Your phone says it can play: click unknown, tick unknown, strength control no",
        ), hapticsLines(HapticsFacts(true, false, "0", "1", "not set", "vibrate", true, "unknown", "unknown", false)))
        // A phone with the keyboard switch off (a Pixel, or a Galaxy S23 on One UI 9 storing 0): Hex still asks, as
        // only its own switch stops it; Android then applies touch feedback.
        val pixel = hapticsLines(HapticsFacts(true, true, "0", "0", "2", "silent", true, "yes", "yes", true))
        assertEquals("Keyboard vibration switch: off (Hex doesn't follow it)", pixel[1])
        assertEquals("Touch feedback: off, strength 2; ringer: silent", pixel[2])
        assertEquals("So Hex asks the phone to vibrate on a key press: yes", pixel[3])
        val off = hapticsLines(HapticsFacts(false, true, "not set", "1", "not set", "sound", false, "no", "no", false))
        assertEquals("Hex's Haptics switch: off", off[0])
        assertEquals("Keyboard vibration switch: not set (Hex doesn't follow it)", off[1])
        assertEquals("Touch feedback: on, strength not set; ringer: sound on", off[2])
        assertEquals("So Hex asks the phone to vibrate on a key press: no, because Hex's Haptics switch is off", off[3])
        assertEquals("Your phone says it can play: click no, tick no, strength control no (it reports no vibrator)", off[4])
        assertEquals("Keyboard vibration switch: this phone has none (stored value not set)",
            hapticsLines(HapticsFacts(true, false, "not set", "1", "not set", "sound", true, "yes", "yes", true))[1])
        assertEquals("Taps Hex asked for: 12, stopped by Hex's settings: 0 (since Hex last started)", tapCountLine(12, 0))
    }
    @Test fun diagnosticsNameWhatEachTapPlays() {
        // The same decision tap() makes: Android's effect where offered, else Hex's pulse.
        val offered = Tap.entries.map { it to it.plays(offered = true) }
        assertEquals(listOf("Key press: Android's click", "Held key lifting: Android's tick", "Dictation start: Android's click",
            "Dictation end: Android's tick"), tapLines(TapWords.English, offered))
        assertEquals("Key press: a 20 ms pulse at the phone's default strength, because this phone doesn't offer Android's click",
            tapLines(TapWords.English, listOf(Tap.Press to Tap.Press.plays(offered = false))).single())
        // A pulse at a set strength, as Hex played before #2718.
        assertEquals("Held key lifting: a 12 ms pulse at 31% strength, because this phone doesn't offer Android's tick",
            tapLines(TapWords.English, listOf(Tap.Release to TapPlay.Pulse(12, 80, VibrationEffect.EFFECT_TICK))).single())
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
        assertEquals("standard build", libraryLabel("libhexspeech.so"))
        assertEquals("newer-processor build", libraryLabel("libhexspeech_v82.so"))
        assertEquals("unknown build", libraryLabel(null))
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
        // The fast-cores row, which this build's dictation uses: pinned to cpu 4-7 in the app; in the keyboard the kernel
        // refused, so the decoding thread read back any core.
        fun run(n: Int, after: Sched, cores: Long = 0xF0) = Run("$KEPT_FAST $DICTATION_ROW", n, true, 5_812, 19_340,
            longArrayOf(20_000, 4_100_000, 1_400_000, 0, 80_000, 0, 4, cores),
            "And so, my fellow Americans.", top, after, "cpu 4-7", "cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz")
        val done = Diagnostics.Speed.Done(listOf(run(1, top), run(2, top.copy(cpuset = "/foreground"))), 11.0)
        val steady = Diagnostics.Speed.Done(listOf(run(1, top, 0), run(2, top, 0)), 11.0)
        assertEquals("""
            Hex diagnostics
            Build: Hex 0.2.2 (build 5), preview

            Last dictation (durations only, held in memory until Hex's process ends; a dictation discarded with ✕ isn't counted; nothing said or typed): none since Hex started

            Speed test in the app: bundled 11.0 s English clip, downloaded model
              same for every run: cgroup 0::/uid_*/pid_*, cpus allowed 0-7, nice 0
              threads kept ready, fast cores (what dictation uses), run 1: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads kept ready on cpu 4-7, encoder window full
                cpuset /top-app
                ran on cpu 4-7
                clocks while it ran: cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz
              threads kept ready, fast cores (what dictation uses), run 2: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads kept ready on cpu 4-7, encoder window full
                cpuset /top-app; at the end cpuset /foreground
                ran on cpu 4-7
                clocks while it ran: cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz
              text: And so, my fellow Americans. (every run)

            Speed test in the keyboard, open in another app: bundled 11.0 s English clip, downloaded model
              same for every run: cpuset /top-app, cgroup 0::/uid_*/pid_*, cpus allowed 0-7, nice 0
              threads kept ready, fast cores (what dictation uses), run 1: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads kept ready on any core, encoder window full
                ran on cpu 4-7
                clocks while it ran: cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz
              threads kept ready, fast cores (what dictation uses), run 2: 5812 ms, processor 19340 ms; encoder 4100 ms, decoder 1500 ms, other 212 ms; 4 threads kept ready on any core, encoder window full
                ran on cpu 4-7
                clocks while it ran: cpu 4-7 1.80 GHz–3.53 GHz, 40% of 20 samples below its top 3.53 GHz
              text: And so, my fellow Americans. (every run)

        """.trimIndent(), report(listOf("Build: Hex 0.2.2 (build 5), preview"), done, steady))
    }
    @Test fun diagnosticsReportTheLastDictationsSteps() {
        val timings = longArrayOf(30_000, 6_100_000, 3_300_000, 0, 80_000, 0, 4, 0xF0)
        assertEquals("""
            Last dictation (durations only, held in memory until Hex's process ends; a dictation discarded with ✕ isn't counted; nothing said or typed):
              in the keyboard, 10.4 s recorded, 9.8 s kept; standard dictation; 4 threads kept ready on cpu 4-7, encoder window full; model still loading at ✓, ready 1240 ms after
              after ✓: recording stopped 31 ms, decode started 52 ms, decode finished 9870 ms, text inserted 9884 ms
              decode 9818 ms; encoder 6100 ms, decoder 3330 ms, prompt 80 ms, other 308 ms; processor 30436 ms, 3.1× the decode's wall time

        """.trimIndent(), lastDictationLines(DictationTimes(DictationPlace.Keyboard, 10_400, 9_800, ModelAtTick.Loading, 1_240,
            true, 31, 52, 9_870, 9_884, 30_436, timings)))
        // Stopped at 1 minute without ✓, the model loaded by the decode, no text, and nothing readable from the decode.
        assertEquals("""
            Last dictation (durations only, held in memory until Hex's process ends; a dictation discarded with ✕ isn't counted; nothing said or typed):
              on Home, 60.0 s recorded, 58.2 s kept; standard dictation; model loaded after the 1-minute stop
              after the 1-minute stop: recording stopped 0 ms, decode started 1250 ms, decode finished 1300 ms, no text inserted
              decode 50 ms; processor not readable

        """.trimIndent(), lastDictationLines(DictationTimes(DictationPlace.Home, 60_000, 58_200, ModelAtTick.LoadedAfter, 0,
            false, 0, 1_250, 1_300, null, -1, null)))
        // Faster English dictation: no encoder/decoder split, its own thread count.
        assertEquals("""
            Last dictation (durations only, held in memory until Hex's process ends; a dictation discarded with ✕ isn't counted; nothing said or typed):
              in the keyboard, 11.0 s recorded, 10.6 s kept; faster English dictation; 4 threads; model loaded while you spoke
              after ✓: recording stopped 30 ms, decode started 41 ms, decode finished 231 ms, text inserted 240 ms
              decode 190 ms; processor 697 ms, 3.7× the decode's wall time

        """.trimIndent(), lastDictationLines(DictationTimes(DictationPlace.Keyboard, 11_000, 10_600, ModelAtTick.LoadedWhileSpeaking, 900,
            true, 30, 41, 231, 240, 697, null, RanOn.Faster, 4)))
        // Short of memory, today's model wrote it, and says why.
        assertTrue(lastDictationLines(DictationTimes(DictationPlace.Home, 5_000, 4_800, ModelAtTick.Loaded, 0, true, 20, 25, 2_025,
            2_030, 7_000, timings, RanOn.LowMemory, 4)).contains(
            "4.8 s kept; standard dictation, as the phone was low on memory for faster English dictation; 4 threads kept ready on cpu 4-7, encoder window full;"))
        // A prompted decode that repeated itself, decoded again without the words: both decodes' times, then the second's.
        assertEquals("""
            Last dictation (durations only, held in memory until Hex's process ends; a dictation discarded with ✕ isn't counted; nothing said or typed):
              in the keyboard, 3.0 s recorded, 2.2 s kept; standard dictation; 4 threads kept ready on any core, encoder window 320; model already loaded
              after ✓: recording stopped 40 ms, decode started 41 ms, decode finished 3421 ms, text inserted 3433 ms
              decode 3380 ms; encoder 2150 ms, decoder 1230 ms, prompt 0 ms, other 0 ms; processor 9800 ms, 2.9× the decode's wall time
              repeated text, decoded again without your words: 1180 ms of the decode

        """.trimIndent(), lastDictationLines(DictationTimes(DictationPlace.Keyboard, 3_000, 2_200, ModelAtTick.Loaded, 0,
            true, 40, 41, 3_421, 3_433, 9_800, longArrayOf(40_000, 2_150_000, 1_100_000, 90_000, 0, 320, 4, 0, 1_180_000, 1))))
        // Decoded again as the prompted decode wrote nothing, only the prompt's words, or one phrase twice in too little audio.
        assertTrue(lastDictationLines(DictationTimes(DictationPlace.Home, 2_000, 1_400, ModelAtTick.Loaded, 0, true, 20, 21, 1_321,
            1_330, 3_900, longArrayOf(10_000, 1_000_000, 90_000, 20_000, 0, 320, 4, 0, 610_000, 2))).endsWith(
            "  no text, decoded again without your words: 610 ms of the decode\n"))
        assertTrue(lastDictationLines(DictationTimes(DictationPlace.Home, 2_000, 1_400, ModelAtTick.Loaded, 0, true, 20, 21, 1_321,
            1_330, 3_900, longArrayOf(10_000, 1_000_000, 90_000, 20_000, 0, 320, 4, 0, 610_000, 3))).endsWith(
            "  only your words, decoded again without your words: 610 ms of the decode\n"))
        assertTrue(lastDictationLines(DictationTimes(DictationPlace.Home, 2_000, 1_400, ModelAtTick.Loaded, 0, true, 20, 21, 1_321,
            1_330, 3_900, longArrayOf(10_000, 1_000_000, 90_000, 20_000, 0, 320, 4, 0, 610_000, 4))).endsWith(
            "  a phrase written twice, decoded again without your words: 610 ms of the decode\n"))
        // Labs › Dictate from Quick Settings' sheet names its own place.
        assertTrue(lastDictationLines(DictationTimes(DictationPlace.QuickSettings, 5_000, 4_800, ModelAtTick.Loaded, 0, true, 20, 25,
            2_025, 2_030, 7_000, timings)).contains("\n  from Quick Settings, 5.0 s recorded, 4.8 s kept;"))
        // Where a dictation's threads were kept, in the words the report uses: no pool, a pool on any core, a pinned one,
        // and a report from a build before that slot existed.
        assertEquals("started for every graph", poolWords(-1))
        assertEquals("kept ready on any core", poolWords(0))
        assertEquals("kept ready on cpu 4-7", poolWords(0xF0))
        assertEquals("kept ready on cpu 0,6-7", poolWords(0xC1))
        assertEquals("4 threads started for every graph", threadWords(longArrayOf(0, 0, 0, 0, 0, 0, 4, -1)))
        assertEquals("4 threads", threadWords(longArrayOf(0, 0, 0, 0, 0, 0, 4)))
    }
    @Test fun diagnosticsSayWhereTheModelWasAtTheTick() {
        assertEquals(ModelAtTick.Loaded, modelAtTick(tick = 100, prepared = 50, prepareLoaded = false, decodeLoaded = false))
        assertEquals(ModelAtTick.LoadedWhileSpeaking, modelAtTick(tick = 100, prepared = 50, prepareLoaded = true, decodeLoaded = false))
        assertEquals(ModelAtTick.Loading, modelAtTick(tick = 100, prepared = 150, prepareLoaded = true, decodeLoaded = false))
        // Stopped at 1 minute: no tick, so a load that finished is a load while speaking.
        assertEquals(ModelAtTick.LoadedWhileSpeaking, modelAtTick(tick = 0, prepared = 150, prepareLoaded = true, decodeLoaded = false))
        assertEquals(ModelAtTick.LoadedAfter, modelAtTick(tick = 100, prepared = 0, prepareLoaded = false, decodeLoaded = true))
        assertEquals(ModelAtTick.Unknown, modelAtTick(tick = 100, prepared = 0, prepareLoaded = false, decodeLoaded = false))
    }
    @Test fun diagnosticsSayWhichLanguageAutomaticChose() {
        // Labs › Automatic listens for the language: the language chosen, kept or switched, every candidate's probability
        // against what a switch takes, on its own line after the decode's.
        val both = listOf(DictationLanguage.English, DictationLanguage.Spanish)
        fun last(choice: LanguageChoice?, keptMs: Long = 2_400) = lastDictationLines(DictationTimes(DictationPlace.Keyboard, 2_600, keptMs,
            ModelAtTick.Loaded, 0, true, 20, 25, 900, 910, 2_000, null, RanOn.Standard, 4, choice)).lines().dropLast(1).last()
        assertEquals("  language Español, switched from English; English 0.4%, Español 99.6%, switching takes 99%; chosen in 3.3 ms",
            last(LanguageChoice(both, 1, listOf(0.004, 0.996), 3_300)))
        assertEquals("  language English, kept the keyboard's; English 38.0%, Español 62.0%, switching takes 95%; chosen in 2.9 ms",
            last(LanguageChoice(both, 0, listOf(0.38, 0.62), 2_900), keptMs = 5_000))
        assertEquals("  language English, the only one listened for", last(LanguageChoice(listOf(DictationLanguage.English), 0, null)))
        // The lab off: the decode's line is the last, as before.
        assertTrue(last(null).startsWith("  decode 875 ms"))
    }
    @Test fun diagnosticsReportTheFasterModelsTextOnItsOwnLine() {
        val top = Sched("/top-app", "0::/uid_*/pid_*", "0-7", 0)
        val standard = Run(KEPT_ANY, 1, true, 2_676, 10_091, longArrayOf(20_000, 2_423_000, 194_000, 0, 0, 0, 4, 0),
            "And so my fellow Americans, ask not.", top, top, "cpu 0-3")
        val faster = Run(FASTER_MODE, 1, true, 190, 697, LongArray(0), "And so, my fellow Americans, ask not.", top, top, "cpu 0-3",
            "cpu 0-3 1.8–2.4 GHz", threads = 4)
        val report = report(null, Diagnostics.Speed.Done(listOf(standard, faster), 11.0), Diagnostics.Speed.Idle)
        assertTrue(report.contains("  faster English dictation, run 1: 190 ms, processor 697 ms; 4 threads\n"))
        assertTrue(report.contains("    clocks while it ran: cpu 0-3 1.8–2.4 GHz"))
        assertTrue(report.contains("  text: And so my fellow Americans, ask not. (every run)\n"))
        assertTrue(report.contains("  faster English dictation text: And so, my fellow Americans, ask not. (every run)\n"))
        assertEquals("Faster English dictation: on, downloaded; the dictation language now uses faster dictation",
            fasterEnglishLine(true, true, Engine.English))
        assertEquals("Faster English dictation: on, not downloaded; the dictation language now uses standard dictation",
            fasterEnglishLine(true, false, Engine.English))
    }
    @Test fun undoBesideANoteIsItsIconSoTheNoteKeepsTheLine() {
        // "Stopped at 1 minute" or the low-memory note shares the line with Undo: the chip drops its words, the note
        // may wrap once. Alone, Undo dictation keeps its words and the hint line stays one line.
        assertEquals(UndoLook(label = false, noteLines = 2), undoLook(shared = true))
        assertEquals(UndoLook(label = true, noteLines = 1), undoLook(shared = false))
    }
}
