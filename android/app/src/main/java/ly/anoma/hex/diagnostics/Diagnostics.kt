package ly.anoma.hex.diagnostics

import android.annotation.SuppressLint
import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.res.Resources
import android.media.AudioAttributes
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.provider.Settings
import android.view.HapticFeedbackConstants
import android.view.View
import java.io.File
import java.util.Locale
import java.util.concurrent.Executors
import kotlinx.coroutines.flow.MutableStateFlow
import ly.anoma.hex.BuildConfig
import ly.anoma.hex.MainActivity
import ly.anoma.hex.R
import ly.anoma.hex.keyboard.StripNotice
import ly.anoma.hex.keyboard.Tap
import ly.anoma.hex.keyboard.TapCounts
import ly.anoma.hex.keyboard.TapPlay
import ly.anoma.hex.keyboard.tap
import ly.anoma.hex.keyboard.vibrates
import ly.anoma.hex.speech.DictationPlace
import ly.anoma.hex.speech.DictationTimes
import ly.anoma.hex.speech.Engine
import ly.anoma.hex.speech.ModelAtTick
import ly.anoma.hex.speech.ModelStore
import ly.anoma.hex.speech.RanOn
import ly.anoma.hex.speech.SpeechPreferences
import ly.anoma.hex.speech.engineFor
import ly.anoma.hex.speech.SpeechModel
import ly.anoma.hex.speech.Whisper
import ly.anoma.hex.speech.lastDictation
import ly.anoma.hex.typing.TypingPreferences

/** Labs › Diagnostics (Hex Preview and debug builds, [BuildConfig.DIAGNOSTICS]): a plain-text report a tester copies and
 * pastes wherever they like. It holds this phone's model, Android version, memory and processor, Hex's choices on it, a
 * speed test on a bundled clip, how long the last dictation's steps took and what the vibrator supports: nothing typed or said, no identifiers, no list of apps,
 * and nothing is sent. Nothing here runs until a button on the page is tapped. */
internal object Diagnostics {
    /** The speed test's clip: whisper.cpp's samples/jfk.wav (public domain), 11 s of English, never anything recorded. */
    private const val CLIP = "speed-test.wav"
    /** Names the test's thread; the native threads it starts inherit the name, which is how the core sampler finds them. */
    private const val THREAD = "Hex speed test"
    private const val RUNS = 2

    /** A way to run the clip: [threads] threads, [pool] of them kept ready with the model (0: new ones per graph) on
     * the cores [mask] names (0: any core), or with Faster English dictation's model ([english]). */
    private class Mode(val label: String, val threads: Int, val pool: Int, val mask: Long = 0, val english: Boolean = false)

    /** Each way to keep the threads, named by what it does so reports from different builds read the same: ggml's
     * default of new threads for every graph, a kept pool on any core, the same pool on the fast cores (only where Hex
     * can tell them apart: at least half the strongest core's cpu_capacity), and six kept ready on a processor whose
     * kernel rates six or more cores fast. The row a dictation uses in this build says so ([DICTATION_ROW]). */
    private fun modes(c: Context): List<Mode> {
        val threads = SpeechModel.threads
        val capacities = (0 until Runtime.getRuntime().availableProcessors())
            .map { read("/sys/devices/system/cpu/cpu$it/cpu_capacity")?.toIntOrNull() }
        val dictation = SpeechModel.dictationCores()
        fun label(name: String, used: Boolean) = if (used) "$name $DICTATION_ROW" else name
        return listOfNotNull(Mode(label(NEW_THREADS, dictation < 0), threads, 0),
            Mode(label(KEPT_ANY, dictation == 0L), threads, threads),
            Mode(label(KEPT_FAST, dictation > 0), threads, threads, SpeechModel.poolMask).takeIf { SpeechModel.poolMask != 0L },
            Mode(KEPT_SIX, 6, 6, SpeechModel.poolMask).takeIf { fastCores(capacities) >= 6 },
            // Labs › Faster English dictation's model, once it is downloaded, whether or not its switch is on.
            Mode(FASTER_MODE, threads, 0, english = true).takeIf { ModelStore.english.installed(c) != null })
    }

    sealed interface Speed {
        data object Idle : Speed
        /** Run in the keyboard was tapped: the test starts the next time the keyboard opens in another app. */
        data object Waiting : Speed
        data class Running(val done: Int, val total: Int) : Speed
        data class Done(val runs: List<Run>, val clipSeconds: Double) : Speed
        data object Failed : Speed
    }

    val facts = MutableStateFlow<List<String>?>(null)
    val app = MutableStateFlow<Speed>(Speed.Idle)
    val keyboard = MutableStateFlow<Speed>(Speed.Idle)
    private val worker = Executors.newSingleThreadExecutor { Thread(it, THREAD) }
    private val main by lazy { Handler(Looper.getMainLooper()) }
    private var jobs = 0L

    val busy get() = app.value is Speed.Running || keyboard.value is Speed.Running

    fun check(context: Context) {
        val c = context.applicationContext
        worker.execute { facts.value = collect(c) }
    }

    fun runInApp(context: Context) {
        if (busy) return
        val c = context.applicationContext
        app.value = Speed.Running(0, RUNS * modes(c).size)
        worker.execute { if (facts.value == null) facts.value = collect(c); speedTest(c, app) }
    }

    fun runInKeyboard() { if (!busy) keyboard.value = Speed.Waiting }

    /** From the keyboard's onStartInputView: runs the armed test while the keyboard is open in another app, the way a
     * dictation runs, and says so in the strip. Never in Hex's own fields, where Hex's window is in front, or a password. */
    fun keyboardShown(context: Context, editor: String?, secure: Boolean, show: (StripNotice?) -> Unit) {
        if (!BuildConfig.DIAGNOSTICS || keyboard.value != Speed.Waiting || secure || editor == null || editor == context.packageName) return
        val c = context.applicationContext
        keyboard.value = Speed.Running(0, RUNS * modes(c).size)
        val close = c.getString(R.string.diagnostics_dismiss)
        show(StripNotice(c.getString(R.string.diagnostics_strip_running), close, tap = {}, dismiss = {}))
        worker.execute {
            if (facts.value == null) facts.value = collect(c)
            speedTest(c, keyboard)
            main.post {
                show(StripNotice(c.getString(R.string.diagnostics_strip_done), close, tap = {
                    c.startActivity(Intent(c, MainActivity::class.java).putExtra(MainActivity.EXTRA_PAGE, MainActivity.PAGE_DIAGNOSTICS)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP))
                }, dismiss = {}))
            }
        }
    }

    /** What a key press plays, through KeyHaptics with its gate (Hex's Haptics switch). */
    fun testTick(context: Context) = tap(context, TypingPreferences(context).haptics, Tap.Press)

    /** A plainly stronger pulse on the same touch channel, without Hex's gates: felt here but not with Test tick means the
     * tick arrives too weak, or Hex's switch stops it; felt with neither means the phone stops touch vibration itself. */
    fun strongPulse(context: Context) {
        val vibrator = context.getSystemService(VibratorManager::class.java)?.defaultVibrator ?: return
        val effect = VibrationEffect.createOneShot(80, if (vibrator.hasAmplitudeControl()) 255 else VibrationEffect.DEFAULT_AMPLITUDE)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU)
            vibrator.vibrate(effect, VibrationAttributes.createForUsage(VibrationAttributes.USAGE_TOUCH))
        else @Suppress("DEPRECATION") vibrator.vibrate(effect, AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
    }

    fun report() = report(facts.value, app.value, keyboard.value, tapCounts(), lastDictation.value)

    /** KeyHaptics' counts since Hex's process started, read live: the keyboard shares the process. */
    fun tapCounts() = tapCountLine(TapCounts.asked.get(), TapCounts.stopped.get())

    /** Android's own keyboard tap on [view], which Android's settings control, for comparison with Hex's. */
    fun androidTap(view: View) = view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)

    /** What each of Hex's taps plays on this phone, decided by KeyHaptics' own Tap.plays. */
    fun tapPlays(context: Context): List<Pair<Tap, TapPlay>> {
        val vibrator = context.getSystemService(VibratorManager::class.java)?.defaultVibrator ?: return emptyList()
        return Tap.entries.map { it to it.plays(vibrator) }
    }

    /** The page's words for [tapLines], in the person's language. */
    fun tapWords(c: Context) = TapWords(
        mapOf(Tap.Press to c.getString(R.string.diagnostics_tap_press), Tap.Release to c.getString(R.string.diagnostics_tap_release),
            Tap.Start to c.getString(R.string.diagnostics_tap_start), Tap.End to c.getString(R.string.diagnostics_tap_end)),
        mapOf(VibrationEffect.EFFECT_CLICK to c.getString(R.string.diagnostics_tap_click),
            VibrationEffect.EFFECT_TICK to c.getString(R.string.diagnostics_tap_tick)),
        c.getString(R.string.diagnostics_tap_predefined), c.getString(R.string.diagnostics_tap_pulse),
        c.getString(R.string.diagnostics_tap_default_strength), c.getString(R.string.diagnostics_tap_strength))

    private fun speedTest(c: Context, into: MutableStateFlow<Speed>) {
        val pcm = runCatching { c.assets.open(CLIP).use { wavSamples(it.readBytes()) } }.getOrNull()
        if (pcm == null) { into.value = Speed.Failed; return }
        val runs = mutableListOf<Run>()
        val modes = modes(c)
        // Each mode once, then each again, so warming up and heating up don't favour one.
        for (number in 1..RUNS) for (mode in modes) {
            into.value = Speed.Running(runs.size, RUNS * modes.size)
            val before = sched()
            val sampler = CoreSampler(THREAD, cores(), parseCpuList(before.allowed)).also { it.start() }
            // Negative jobs: no dictation's ✕ (MicCapture counts up from 1) can stop this decode.
            val measured = if (mode.english) SpeechModel.measureEnglish(c, pcm, -(++jobs))
                else SpeechModel.measure(c, pcm, mode.threads, mode.pool, mode.mask, -(++jobs))
            val (cores, clocks) = sampler.stop()
            if (measured == null) { into.value = Speed.Failed; return }
            runs += Run(mode.label, number, measured.downloaded, measured.wallMs, measured.cpuMs, measured.timings, measured.text,
                before, sched(), cores, clocks, if (mode.english) SpeechModel.threads else 0)
        }
        into.value = Speed.Done(runs, pcm.size / 16_000.0)
    }

    private fun collect(c: Context): List<String> {
        val memory = ActivityManager.MemoryInfo().also { c.getSystemService(ActivityManager::class.java).getMemoryInfo(it) }
        val cores = cores()
        val channel = when (BuildConfig.BUILD_TYPE) { "preview" -> "preview"; "release" -> "play"; else -> BuildConfig.BUILD_TYPE }
        val library = runCatching { libraryLabel(Whisper.library()) }.getOrDefault("speech library didn't load")
        return listOf(
            "Build: Hex ${BuildConfig.VERSION_NAME} (build ${BuildConfig.VERSION_CODE}), $channel",
            // Build.ID is the public build number every phone on that release shares; Build.DISPLAY can carry a self-built
            // ROM's builder name, so it isn't used.
            "Phone: ${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}), " +
                "build ${Build.ID}, security patch ${Build.VERSION.SECURITY_PATCH}",
            processorLine("${Build.SOC_MANUFACTURER} ${Build.SOC_MODEL}", cores, SpeechModel.debugRatings),
            "Memory: ${gb(memory.totalMem)} total, ${gb(memory.availMem)} available" + if (memory.lowMemory) ", low" else "",
            "Speech: ${SpeechModel.threads} threads, $library, downloaded model " +
                (if (ModelStore.small.installed(c) != null) "installed" else "not installed") +
                "; dictation threads: ${SpeechModel.threads} ${runCatching { poolWords(SpeechModel.dictationCores()) }.getOrDefault("not known")}",
            fasterEnglishLine(SpeechPreferences(c).fastEnglish, ModelStore.english.installed(c) != null,
                engineFor(SpeechPreferences(c).options(null))),
        ) + haptics(c)
    }

    private fun haptics(c: Context): List<String> {
        val haptics = TypingPreferences(c).haptics
        val vibrator = c.getSystemService(VibratorManager::class.java)?.defaultVibrator
        fun system(name: String) = runCatching { Settings.System.getString(c.contentResolver, name) ?: "not set" }.getOrDefault("not readable")
        @SuppressLint("DiscouragedApi")
        val declared = Resources.getSystem().getIdentifier("config_keyboardVibrationSettingsSupported", "bool", "android")
            .let { if (it == 0) "not declared" else if (Resources.getSystem().getBoolean(it)) "yes" else "no" }
        fun support(effect: Int) = when (vibrator?.areAllEffectsSupported(effect)) {
            Vibrator.VIBRATION_EFFECT_SUPPORT_YES -> "yes"
            Vibrator.VIBRATION_EFFECT_SUPPORT_NO -> "no"
            else -> "unknown"
        }
        val ringer = when (c.getSystemService(AudioManager::class.java)?.ringerMode) {
            AudioManager.RINGER_MODE_NORMAL -> "sound"; AudioManager.RINGER_MODE_VIBRATE -> "vibrate"
            AudioManager.RINGER_MODE_SILENT -> "silent"; else -> "unknown"
        }
        return hapticsLines(HapticsFacts(haptics, declared == "yes", system("keyboard_vibration_enabled"),
            system(Settings.System.HAPTIC_FEEDBACK_ENABLED), system("haptic_feedback_intensity"), ringer,
            vibrator?.hasVibrator() == true, support(VibrationEffect.EFFECT_CLICK), support(VibrationEffect.EFFECT_TICK),
            vibrator?.hasAmplitudeControl() == true)) + tapLines(TapWords.English, tapPlays(c))
    }

    private fun cores() = (0 until Runtime.getRuntime().availableProcessors()).map {
        Core(it, read("/sys/devices/system/cpu/cpu$it/cpu_capacity")?.toIntOrNull(),
            read("/sys/devices/system/cpu/cpu$it/cpufreq/cpuinfo_max_freq")?.toLongOrNull())
    }

    private fun sched() = Sched(read("/proc/thread-self/cpuset") ?: read("/proc/self/cpuset"),
        read("/proc/self/cgroup")?.let(::cgroupSummary),
        read("/proc/thread-self/status")?.lineSequence()?.firstOrNull { it.startsWith("Cpus_allowed_list:") }?.substringAfter(':')?.trim(),
        runCatching { Process.getThreadPriority(Process.myTid()) }.getOrNull())

    private fun gb(bytes: Long) = String.format(Locale.ROOT, "%.1f GB", bytes / 1e9)
}

internal fun read(path: String) = runCatching { File(path).readText().trim() }.getOrNull()

/** One speed test run: wall and processor time in ms, and whisper.cpp's timings (Whisper.timings). */
internal class Run(val mode: String, val number: Int, val downloaded: Boolean, val wallMs: Long, val cpuMs: Long,
                   val timings: LongArray, val text: String, val before: Sched, val after: Sched, val cores: String?,
                   val clocks: String? = null, val threads: Int = 0)

/** Where the test's thread was scheduled: its cpuset, the process's cgroups, the cores it may use and its nice value. */
internal data class Sched(val cpuset: String?, val cgroup: String?, val allowed: String?, val nice: Int?)

internal class Core(val index: Int, val capacity: Int?, val maxKhz: Long?)

/** Samples the run every 150 ms on its own thread, so a clock cap during the decode shows up: the current clock of
 * each core the test's thread may use, and which of them the test's threads (found by [name]) are running on. Each
 * sample reads a few small files, about a millisecond of processor time a second. */
private class CoreSampler(private val name: String, private val cores: List<Core>, allowed: Set<Int>) {
    private val watched = cores.map { it.index }.filter { allowed.isEmpty() || it in allowed }
    private val ticks = mutableListOf<Tick>()
    @Volatile private var running = true
    private val thread = Thread({
        while (running) {
            sample()
            try { Thread.sleep(150) } catch (_: InterruptedException) { break }
        }
    }, "Hex core sampler")

    fun start() = thread.start()
    /** The cores the test ran on, and its clocks there. */
    fun stop(): Pair<String?, String?> {
        running = false; thread.interrupt(); thread.join()
        val ran = ticks.flatMap { it.running }.toSet()
        return (if (ran.isEmpty()) null else "cpu ${cpuRanges(ran)}") to clockSummary(cores, watched.toSet(), ticks)
    }

    private fun sample() {
        val busy = HashSet<Int>()
        for (task in File("/proc/self/task").listFiles().orEmpty()) {
            val (comm, state, cpu) = taskStat(read("${task.path}/stat") ?: continue) ?: continue
            if (comm == name && state == 'R') busy += cpu
        }
        ticks += Tick(busy, watched.associateWith { read("/sys/devices/system/cpu/cpu$it/cpufreq/scaling_cur_freq")?.toLongOrNull() })
    }
}

/** One sample: the cores the test's threads were running on, and each watched core's clock in kHz (null: unreadable). */
internal class Tick(val running: Set<Int>, val khz: Map<Int, Long?>)

/** Per cluster of [watched] cores: the clock range while the test's threads ran there, and the share of those samples
 * below the cluster's top frequency, which a clock cap shows up in. */
internal fun clockSummary(cores: List<Core>, watched: Set<Int>, ticks: List<Tick>): String? {
    if (ticks.isEmpty()) return null
    return clusters(cores.filter { it.index in watched }).joinToString("; ") { group ->
        val indices = group.map { it.index }.toSet()
        val used = ticks.filter { tick -> tick.running.any { it in indices } }
        val khz = used.flatMap { tick -> tick.running.filter { it in indices }.mapNotNull { tick.khz[it] } }.filter { frequency(it) != null }
        val name = "cpu ${cpuRanges(indices)}"
        val top = group.first().maxKhz?.takeIf { frequency(it) != null }
        when {
            used.isEmpty() -> "$name not used"
            khz.isEmpty() -> "$name frequency not reported"
            else -> {
                val range = if (khz.min() == khz.max()) "${frequency(khz.min())}" else "${frequency(khz.min())}–${frequency(khz.max())}"
                "$name $range, " + if (top == null) "top frequency not reported"
                    else "${khz.count { it < top } * 100 / khz.size}% of ${khz.size} samples below its top ${frequency(top)}"
            }
        }
    }
}

/** A /proc task stat's name, state and the core it last ran on (field 39), or null. */
internal fun taskStat(stat: String): Triple<String, Char, Int>? {
    val open = stat.indexOf('('); val close = stat.lastIndexOf(')')
    if (open < 0 || close < open) return null
    val fields = stat.substring(close + 1).trim().split(' ')
    val cpu = fields.getOrNull(36)?.toIntOrNull() ?: return null
    return Triple(stat.substring(open + 1, close), fields[0].firstOrNull() ?: return null, cpu)
}

/** Cores the kernel rates at least half the strongest core's capacity; 0 when any core's capacity is unreadable. */
internal fun fastCores(capacities: List<Int?>): Int {
    if (capacities.isEmpty() || capacities.any { it == null }) return 0
    val top = capacities.maxOf { it!! }
    return if (top <= 0) 0 else capacities.count { it!! * 2 >= top }
}

/** What the report says about haptics: Hex's switch, Android's settings and what the vibrator says it plays. */
internal class HapticsFacts(val hex: Boolean, val keyboardSettingHere: Boolean, val keyboardValue: String,
                            val touchFeedback: String, val touchStrength: String, val soundMode: String, val vibrator: Boolean,
                            val click: String, val tick: String, val strength: Boolean)

internal fun hapticsLines(h: HapticsFacts): List<String> {
    fun yes(value: Boolean) = if (value) "yes" else "no"
    fun onOff(stored: String) = when (stored) { "1" -> "on"; "0" -> "off"; else -> stored }
    // Shown for information: Hex doesn't follow Android's Keyboard vibration value (KeyHaptics' vibrates()).
    val keyboard = if (h.keyboardSettingHere) "${onOff(h.keyboardValue)} (Hex doesn't follow it)"
        else "this phone has none (stored value ${h.keyboardValue})"
    val ringer = when (h.soundMode) { "sound" -> "sound on"; "vibrate" -> "vibrate only"; else -> h.soundMode }
    // The same rule as KeyHaptics' vibrates(): Hex's own switch alone.
    val stopper = if (h.hex) null else "Hex's Haptics switch is off"
    return listOf(
        "Hex's Haptics switch: ${if (h.hex) "on" else "off"}",
        "Keyboard vibration switch: $keyboard",
        "Touch feedback: ${onOff(h.touchFeedback)}, strength ${h.touchStrength}; ringer: $ringer",
        "So Hex asks the phone to vibrate on a key press: " + if (stopper == null) "yes" else "no, because $stopper",
        "Your phone says it can play: click ${h.click}, tick ${h.tick}, strength control ${yes(h.strength)}" +
            if (h.vibrator) "" else " (it reports no vibrator)",
    )
}

/** The words [tapLines] uses: each tap's name, each predefined effect's name, and the patterns that join them. */
internal class TapWords(val taps: Map<Tap, String>, val effects: Map<Int, String>, val predefined: String, val pulse: String,
                        val defaultStrength: String, val strength: String) {
    companion object {
        /** The copied report's words, the same in every language like the rest of the report. */
        val English = TapWords(
            mapOf(Tap.Press to "Key press", Tap.Release to "Held key lifting", Tap.Start to "Dictation start", Tap.End to "Dictation end"),
            mapOf(VibrationEffect.EFFECT_CLICK to "click", VibrationEffect.EFFECT_TICK to "tick"),
            "%1\$s: Android's %2\$s", "%1\$s: a %2\$d ms pulse %3\$s, because this phone doesn't offer Android's %4\$s",
            "at the phone's default strength", "at %1\$d%% strength")
    }
}

/** Each of Hex's taps and what it plays here, in plain words: "Key press: Android’s click", or a pulse and why. */
internal fun tapLines(words: TapWords, plays: List<Pair<Tap, TapPlay>>): List<String> = plays.map { (tap, play) ->
    val name = words.taps[tap] ?: tap.name
    when (play) {
        is TapPlay.Predefined -> words.predefined.format(name, words.effects[play.effect] ?: "effect ${play.effect}")
        is TapPlay.Pulse -> words.pulse.format(name, play.millis,
            if (play.amplitude == VibrationEffect.DEFAULT_AMPLITUDE) words.defaultStrength
            else words.strength.format(Locale.ROOT, (play.amplitude * 100 + 127) / 255),
            words.effects[play.missing] ?: "effect ${play.missing}")
    }
}

/** A count stuck at 0 while typing means Hex's gate stopped the taps; a climbing count with nothing felt means Android or
 * the phone dropped them. */
internal fun tapCountLine(asked: Int, stopped: Int) =
    "Taps Hex asked for: $asked, stopped by Hex's settings: $stopped (since Hex last started)"

/** "0-3,6" for 0, 1, 2, 3 and 6. */
internal fun cpuRanges(cpus: Collection<Int>): String {
    val sorted = cpus.toSortedSet().toList()
    val parts = mutableListOf<String>()
    var i = 0
    while (i < sorted.size) {
        var j = i
        while (j + 1 < sorted.size && sorted[j + 1] == sorted[j] + 1) j++
        parts += if (i == j) "${sorted[i]}" else "${sorted[i]}-${sorted[j]}"
        i = j + 1
    }
    return parts.joinToString(",")
}

/** Neighbouring cores with the same capacity and top frequency, as the kernel reports them. */
internal fun clusters(cores: List<Core>): List<List<Core>> {
    val groups = mutableListOf<MutableList<Core>>()
    for (core in cores) {
        val last = groups.lastOrNull()?.last()
        if (last != null && last.index + 1 == core.index && last.capacity == core.capacity && last.maxKhz == core.maxKhz) groups.last() += core
        else groups += mutableListOf(core)
    }
    return groups
}

/** "0-3,6" read back as 0, 1, 2, 3 and 6; empty for null or nonsense. */
internal fun parseCpuList(text: String?): Set<Int> = text.orEmpty().split(',').flatMap { part ->
    val bounds = part.trim().split('-').map { it.trim().toIntOrNull() ?: return@flatMap emptyList() }
    when (bounds.size) { 1 -> bounds; 2 -> (bounds[0]..bounds[1]).toList(); else -> emptyList() }
}.toSet()

/** The Processor line; a debug build's stand-in core ratings say so, so its report can't pass for a phone's. */
internal fun processorLine(soc: String, cores: List<Core>, debugRatings: List<Int?>?) =
    "Processor: $soc, ${cores.size} cores; ${clusterSummary(cores)}" +
        (debugRatings?.let { r -> " (debug core ratings: ${r.joinToString(", ") { it?.toString() ?: "none" }})" } ?: "")

internal fun clusterSummary(cores: List<Core>): String =
    clusters(cores).joinToString("; ") { group ->
        val first = group.first()
        "cpu ${cpuRanges(group.map { it.index })}: capacity ${first.capacity ?: "not readable"}, " +
            (first.maxKhz?.let(::frequency)?.let { "up to $it" } ?: "top frequency not reported")
    }

/** The process's cgroups on one line, without the uid and pid numbers Android puts in their paths. */
internal fun cgroupSummary(text: String) = text.lines().map { it.trim() }.filter { it.isNotEmpty() }
    .joinToString(" ") { it.replace(Regex("(uid|pid)_\\d+"), "$1_*") }

/** The speed test's mode with Faster English dictation's model. */
internal const val FASTER_MODE = "faster English dictation"

/** The speed test's rows, the same words in every build from 2412 on. */
internal const val NEW_THREADS = "new threads every graph"
internal const val KEPT_ANY = "threads kept ready, any core"
internal const val KEPT_FAST = "threads kept ready, fast cores"
internal const val KEPT_SIX = "threads kept ready, 6"
/** Marks the row this build's dictation uses. */
internal const val DICTATION_ROW = "(what dictation uses)"

/** Where a decode's threads were kept (hexspeech.cpp's last timing: -1 none, 0 any core, else a bit per core). */
internal fun poolWords(cores: Long) = when {
    cores < 0 -> "started for every graph"
    cores == 0L -> "kept ready on any core"
    else -> "kept ready on cpu " + cpuRanges((0 until 64).filter { cores and (1L shl it) != 0L })
}

/** "4 threads kept ready on cpu 4-7" from a decode's timings, or just the count when an older build left no cores. */
internal fun threadWords(t: LongArray) = "${t[6]} threads" + if (t.size >= 8) " " + poolWords(t[7]) else ""

/** Labs › Faster English dictation in the report: the switch, its download, and what the dictation language now uses,
 * "faster" or "standard". The only line of the report that says "faster". */
internal fun fasterEnglishLine(on: Boolean, downloaded: Boolean, engine: Engine) =
    "Faster English dictation: ${if (on) "on" else "off"}, ${if (downloaded) "downloaded" else "not downloaded"}; " +
        "the dictation language now uses ${if (engine == Engine.English && downloaded) "faster" else "standard"} dictation"

/** Which build of the speech library runs, in words that can't be read as Faster English dictation (whose own line says
 * "faster") or name an engine. */
internal fun libraryLabel(file: String?) = when {
    file == null -> "unknown build"
    file.contains("v82") -> "newer-processor build"
    file.startsWith("libhexspeech") -> "standard build"
    else -> "other build"
}

/** A PCM16 mono 16 kHz WAV's samples, or null for anything else. */
internal fun wavSamples(bytes: ByteArray): ShortArray? {
    fun u16(at: Int) = (bytes[at].toInt() and 0xff) or ((bytes[at + 1].toInt() and 0xff) shl 8)
    fun u32(at: Int) = u16(at).toLong() or (u16(at + 2).toLong() shl 16)
    if (bytes.size < 12 || String(bytes, 0, 4, Charsets.US_ASCII) != "RIFF" || String(bytes, 8, 4, Charsets.US_ASCII) != "WAVE") return null
    var at = 12
    var format = false
    while (at + 8 <= bytes.size) {
        val id = String(bytes, at, 4, Charsets.US_ASCII)
        val size = u32(at + 4)
        val body = at + 8
        if (size > bytes.size - body) return null
        when (id) {
            "fmt " -> format = size >= 16 && u16(body) == 1 && u16(body + 2) == 1 && u32(body + 4) == 16_000L && u16(body + 14) == 16
            "data" -> return if (!format) null else ShortArray(size.toInt() / 2) { (u16(body + it * 2)).toShort() }
        }
        at = body + size.toInt() + (size.toInt() and 1)
    }
    return null
}

/** A frequency from cpufreq, which reports kHz: GHz with two decimals, MHz under 1 GHz, or null for a value under
 * 1 MHz (zero, or an emulator's 1 or 2), which no real core runs at. */
internal fun frequency(khz: Long): String? = when {
    khz < 1_000 -> null
    khz < 1_000_000 -> "${khz / 1_000} MHz"
    else -> String.format(Locale.ROOT, "%.2f GHz", khz / 1e6)
}

/** One run on one line, then where it was scheduled and the cores it ran on. */
internal fun runLines(run: Run, shared: Set<String> = emptySet()): String = buildString {
    append("${run.mode}, run ${run.number}: ${run.wallMs} ms, processor ${run.cpuMs} ms")
    val t = run.timings
    if (t.size >= 7) {
        val encoder = t[1] / 1000
        val decoder = (t[0] + t[2] + t[3] + t[4]) / 1000
        append("; encoder $encoder ms, decoder $decoder ms, other ${maxOf(0, run.wallMs - encoder - decoder)} ms")
        append("; ${threadWords(t)}, encoder window ${if (t[5] == 0L) "full" else "${t[5]}"}")
    } else if (run.threads > 0) append("; ${run.threads} threads")
    val before = schedParts(run.before).filterKeys { it !in shared }
    val after = schedParts(run.after).filterKeys { it !in shared }
    if (before.isNotEmpty()) {
        append("\n    ").append(schedLine(before))
        if (after != before) append("; at the end ").append(schedLine(after))
    }
    append("\n    ran on ").append(run.cores ?: "cores not readable")
    if (run.clocks != null) append("\n    clocks while it ran: ").append(run.clocks)
}

/** Where a run was scheduled, by name, in the report's order. */
private fun schedParts(s: Sched) = linkedMapOf("cpuset" to (s.cpuset ?: "not readable"), "cgroup" to (s.cgroup ?: "not readable"),
    "cpus allowed" to (s.allowed ?: "not readable"), "nice" to (s.nice?.toString() ?: "not readable"))

private fun schedLine(parts: Map<String, String>) = parts.entries.joinToString(", ") { "${it.key} ${it.value}" }

/** The model that wrote a dictation, in plain words; for a fallback, why. The faster model gives no encoder/decoder
 * split, and after it failed the decode's time includes its try. */
internal fun ranOn(ran: RanOn) = when (ran) {
    RanOn.Standard -> "standard dictation"
    RanOn.Faster -> "faster English dictation"
    RanOn.NoFile -> "standard dictation, as faster English dictation's download is missing or failed its check"
    RanOn.LowMemory -> "standard dictation, as the phone was low on memory for faster English dictation"
    RanOn.LoadFailed -> "standard dictation, as faster English dictation's model didn't load"
    RanOn.FasterFailed -> "standard dictation, after faster English dictation failed (its try is in the decode's time)"
}

/** The last real dictation's durations (MicCapture's [DictationTimes]), or that there was none since Hex started. */
internal fun lastDictationLines(t: DictationTimes?): String = buildString {
    append("Last dictation (durations only, held in memory until Hex's process ends; a dictation discarded with ✕ isn't counted; nothing said or typed):")
    if (t == null) { append(" none since Hex started\n"); return@buildString }
    append('\n')
    val mark = if (t.tick) "✓" else "the 1-minute stop"
    append(if (t.place == DictationPlace.Keyboard) "  in the keyboard" else "  on Home")
    append(String.format(Locale.ROOT, ", %.1f s recorded, %.1f s kept", t.recordedMs / 1000.0, t.keptMs / 1000.0))
    append("; ").append(ranOn(t.ran))
    val timings = t.timings?.takeIf { it.size >= 7 }
    if (timings != null) append("; ${threadWords(timings)}, encoder window ${if (timings[5] == 0L) "full" else "${timings[5]}"}")
    else if (t.threads > 0) append("; ${t.threads} threads")
    append("; model ").append(when (t.model) {
        ModelAtTick.Loaded -> "already loaded"
        ModelAtTick.LoadedWhileSpeaking -> "loaded while you spoke"
        ModelAtTick.Loading -> "still loading at $mark, ready ${t.modelReadyMs} ms after"
        ModelAtTick.LoadedAfter -> "loaded after $mark"
        ModelAtTick.Unknown -> "state not known"
    }).append('\n')
    append("  after $mark: ")
    append("recording stopped ${t.stoppedMs} ms, decode started ${t.decodeStartedMs} ms, decode finished ${t.decodeEndedMs} ms, ")
    append(t.insertedMs?.let { "text inserted $it ms" } ?: "no text inserted").append('\n')
    val wall = t.decodeEndedMs - t.decodeStartedMs
    append("  decode $wall ms")
    if (timings != null) {
        val encoder = timings[1] / 1000
        val decoder = (timings[0] + timings[2] + timings[3]) / 1000
        val prompt = timings[4] / 1000
        append("; encoder $encoder ms, decoder $decoder ms, prompt $prompt ms, other ${maxOf(0, wall - encoder - decoder - prompt)} ms")
    }
    append("; processor ").append(if (t.cpuMs < 0) "not readable" else "${t.cpuMs} ms")
    if (t.cpuMs >= 0 && wall > 0) append(String.format(Locale.ROOT, ", %.1f× the decode's wall time", t.cpuMs.toDouble() / wall))
    append('\n')
    // hexspeech.cpp decoded it again without the custom vocabulary or learned words (its againWhy).
    if (timings != null && timings.size >= 10 && timings[8] > 0) {
        append("  ").append(when (timings[9]) { 1L -> "repeated text"; 2L -> "no text"; 3L -> "only your words"; else -> "text unusable" })
        append(", decoded again without your words: ${timings[8] / 1000} ms of the decode\n")
    }
}

/** The text the page shows and Copy copies. */
internal fun report(facts: List<String>?, app: Diagnostics.Speed, keyboard: Diagnostics.Speed, taps: String? = null,
                    last: DictationTimes? = null): String = buildString {
    append("Hex diagnostics\n")
    facts?.forEach { append(it).append('\n') }
    if (taps != null) append(taps).append('\n')
    append('\n').append(lastDictationLines(last))
    for ((title, speed) in listOf("Speed test in the app" to app, "Speed test in the keyboard, open in another app" to keyboard)) {
        append('\n').append(title).append(": ")
        when (speed) {
            Diagnostics.Speed.Idle, Diagnostics.Speed.Waiting -> append("not run\n")
            is Diagnostics.Speed.Running -> append("running\n")
            Diagnostics.Speed.Failed -> append("the speech model didn't load\n")
            is Diagnostics.Speed.Done -> {
                val first = speed.runs.first()
                append(String.format(Locale.ROOT, "bundled %.1f s English clip, ", speed.clipSeconds))
                append(if (first.downloaded) "downloaded model\n" else "bundled model\n")
                // Scheduling that every run shares, at its start and end, is said once; a run lists only what differs.
                val all = speed.runs.flatMap { listOf(schedParts(it.before), schedParts(it.after)) }
                val shared = all.first().filter { (name, value) -> all.all { it[name] == value } }
                if (shared.isNotEmpty()) append("  same for every run: ").append(schedLine(shared)).append('\n')
                speed.runs.forEach { append("  ").append(runLines(it, shared.keys)).append('\n') }
                // Each model's text once: the faster model's punctuation needn't match today's.
                val (standard, faster) = speed.runs.partition { it.mode != FASTER_MODE }
                for ((label, runs) in listOf("text" to standard, "$FASTER_MODE text" to faster)) {
                    if (runs.isEmpty()) continue
                    append("  ").append(label).append(": ").append(runs.first().text)
                    append(if (runs.all { it.text == runs.first().text }) " (every run)\n" else " (runs differ)\n")
                }
            }
        }
    }
}
