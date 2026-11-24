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
import java.io.File
import java.util.Locale
import java.util.concurrent.Executors
import kotlinx.coroutines.flow.MutableStateFlow
import ly.anoma.hex.BuildConfig
import ly.anoma.hex.MainActivity
import ly.anoma.hex.R
import ly.anoma.hex.keyboard.StripNotice
import ly.anoma.hex.keyboard.Tap
import ly.anoma.hex.keyboard.tap
import ly.anoma.hex.keyboard.vibrates
import ly.anoma.hex.speech.ModelStore
import ly.anoma.hex.speech.SpeechModel
import ly.anoma.hex.speech.Whisper
import ly.anoma.hex.typing.TypingPreferences

/** Labs › Diagnostics (Hex Preview and debug builds, [BuildConfig.DIAGNOSTICS]): a plain-text report a tester copies and
 * pastes wherever they like. It holds this phone's model, Android version, memory and processor, Hex's choices on it, a
 * speed test on a bundled clip and what the vibrator supports: nothing typed or said, no identifiers, no list of apps,
 * and nothing is sent. Nothing here runs until a button on the page is tapped. */
internal object Diagnostics {
    /** The speed test's clip: whisper.cpp's samples/jfk.wav (public domain), 11 s of English, never anything recorded. */
    private const val CLIP = "speed-test.wav"
    /** Names the test's thread; the native threads it starts inherit the name, which is how the core sampler finds them. */
    private const val THREAD = "Hex speed test"
    private const val RUNS = 2

    /** A way to run the clip: [threads] threads, [pool] of them kept ready with the model (0: new ones per graph). */
    private class Mode(val label: String, val threads: Int, val pool: Int)

    /** Today's threads, the same kept ready, and six kept ready on a processor whose kernel rates six or more cores
     * fast (at least half the strongest core's cpu_capacity, Hex's own rule). */
    private fun modes(): List<Mode> {
        val threads = SpeechModel.threads
        val capacities = (0 until Runtime.getRuntime().availableProcessors())
            .map { read("/sys/devices/system/cpu/cpu$it/cpu_capacity")?.toIntOrNull() }
        return listOfNotNull(Mode("as now", threads, 0), Mode("threads kept ready", threads, threads),
            Mode("threads kept ready, 6", 6, 6).takeIf { fastCores(capacities) >= 6 })
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
        app.value = Speed.Running(0, RUNS * modes().size)
        worker.execute { if (facts.value == null) facts.value = collect(c); speedTest(c, app) }
    }

    fun runInKeyboard() { if (!busy) keyboard.value = Speed.Waiting }

    /** From the keyboard's onStartInputView: runs the armed test while the keyboard is open in another app, the way a
     * dictation runs, and says so in the strip. Never in Hex's own fields, where Hex's window is in front, or a password. */
    fun keyboardShown(context: Context, editor: String?, secure: Boolean, show: (StripNotice?) -> Unit) {
        if (!BuildConfig.DIAGNOSTICS || keyboard.value != Speed.Waiting || secure || editor == null || editor == context.packageName) return
        val c = context.applicationContext
        keyboard.value = Speed.Running(0, RUNS * modes().size)
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

    /** What a key press plays, through KeyHaptics with its gates (Hex's Haptics switch, the phone's Keyboard vibration). */
    fun testTick(context: Context) = tap(context, TypingPreferences(context).haptics, Tap.Press)

    /** A plainly stronger pulse on the same touch channel, without Hex's gates: felt here but not with Test tick means the
     * tick arrives too weak, or a gate stops it; felt with neither means the phone stops touch vibration itself. */
    fun strongPulse(context: Context) {
        val vibrator = context.getSystemService(VibratorManager::class.java)?.defaultVibrator ?: return
        val effect = VibrationEffect.createOneShot(80, if (vibrator.hasAmplitudeControl()) 255 else VibrationEffect.DEFAULT_AMPLITUDE)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU)
            vibrator.vibrate(effect, VibrationAttributes.createForUsage(VibrationAttributes.USAGE_TOUCH))
        else @Suppress("DEPRECATION") vibrator.vibrate(effect, AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
    }

    fun report() = report(facts.value, app.value, keyboard.value)

    private fun speedTest(c: Context, into: MutableStateFlow<Speed>) {
        val pcm = runCatching { c.assets.open(CLIP).use { wavSamples(it.readBytes()) } }.getOrNull()
        if (pcm == null) { into.value = Speed.Failed; return }
        val runs = mutableListOf<Run>()
        val modes = modes()
        // Each mode once, then each again, so warming up and heating up don't favour one.
        for (number in 1..RUNS) for (mode in modes) {
            into.value = Speed.Running(runs.size, RUNS * modes.size)
            val before = sched()
            val sampler = CoreSampler(THREAD).also { it.start() }
            // Negative jobs: no dictation's ✕ (MicCapture counts up from 1) can stop this decode.
            val measured = SpeechModel.measure(c, pcm, mode.threads, mode.pool, -(++jobs))
            val cores = sampler.stop()
            if (measured == null) { into.value = Speed.Failed; return }
            runs += Run(mode.label, number, measured.downloaded, measured.wallMs, measured.cpuMs, measured.timings, measured.text,
                before, sched(), cores)
        }
        into.value = Speed.Done(runs, pcm.size / 16_000.0)
    }

    private fun collect(c: Context): List<String> {
        val memory = ActivityManager.MemoryInfo().also { c.getSystemService(ActivityManager::class.java).getMemoryInfo(it) }
        val cores = (0 until Runtime.getRuntime().availableProcessors()).map {
            Core(it, read("/sys/devices/system/cpu/cpu$it/cpu_capacity")?.toIntOrNull(),
                read("/sys/devices/system/cpu/cpu$it/cpufreq/cpuinfo_max_freq")?.toLongOrNull()?.takeIf { it > 0 })
        }
        val channel = when (BuildConfig.BUILD_TYPE) { "preview" -> "preview"; "release" -> "play"; else -> BuildConfig.BUILD_TYPE }
        val library = runCatching { libraryLabel(Whisper.library()) }.getOrDefault("didn't load")
        return listOf(
            "Build: Hex ${BuildConfig.VERSION_NAME} (build ${BuildConfig.VERSION_CODE}), $channel",
            "Phone: ${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}), firmware ${Build.DISPLAY}",
            "Processor: ${Build.SOC_MANUFACTURER} ${Build.SOC_MODEL}, ${cores.size} cores; ${clusterSummary(cores)}",
            "Memory: ${gb(memory.totalMem)} total, ${gb(memory.availMem)} available" + if (memory.lowMemory) ", low" else "",
            "Speech: ${SpeechModel.threads} threads, $library library, downloaded model " +
                if (ModelStore.installed(c) != null) "installed" else "not installed",
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
        return listOf(
            "Haptics: Hex's switch ${if (haptics) "on" else "off"}, key ticks play: ${if (vibrates(c, haptics)) "yes" else "no"}",
            "Haptics: keyboard_vibration_enabled ${system("keyboard_vibration_enabled")}, phone declares that setting: $declared, " +
                "haptic_feedback_enabled ${system(Settings.System.HAPTIC_FEEDBACK_ENABLED)}, " +
                "haptic_feedback_intensity ${system("haptic_feedback_intensity")}, sound mode $ringer",
            "Vibrator: " + if (vibrator == null) "none" else "has vibrator ${yes(vibrator.hasVibrator())}, amplitude control " +
                "${yes(vibrator.hasAmplitudeControl())}, tick ${support(VibrationEffect.EFFECT_TICK)}, click ${support(VibrationEffect.EFFECT_CLICK)}",
        )
    }

    private fun sched() = Sched(read("/proc/thread-self/cpuset") ?: read("/proc/self/cpuset"),
        read("/proc/self/cgroup")?.let(::cgroupSummary),
        read("/proc/thread-self/status")?.lineSequence()?.firstOrNull { it.startsWith("Cpus_allowed_list:") }?.substringAfter(':')?.trim(),
        runCatching { Process.getThreadPriority(Process.myTid()) }.getOrNull())

    private fun yes(value: Boolean) = if (value) "yes" else "no"
    private fun gb(bytes: Long) = String.format(Locale.ROOT, "%.1f GB", bytes / 1e9)
}

internal fun read(path: String) = runCatching { File(path).readText().trim() }.getOrNull()

/** One speed test run: wall and processor time in ms, and whisper.cpp's timings (Whisper.timings). */
internal class Run(val mode: String, val number: Int, val downloaded: Boolean, val wallMs: Long, val cpuMs: Long,
                   val timings: LongArray, val text: String, val before: Sched, val after: Sched, val cores: String?)

/** Where the test's thread was scheduled: its cpuset, the process's cgroups, the cores it may use and its nice value. */
internal data class Sched(val cpuset: String?, val cgroup: String?, val allowed: String?, val nice: Int?)

internal class Core(val index: Int, val capacity: Int?, val maxKhz: Long?)

/** Watches the test's threads (by [name]) every 250 ms: the cores they run on and those cores' current frequency. */
private class CoreSampler(private val name: String) {
    private val seen = HashMap<Int, MutableList<Long>>()
    @Volatile private var running = true
    private val thread = Thread({
        while (running) {
            sample()
            try { Thread.sleep(250) } catch (_: InterruptedException) { break }
        }
    }, "Hex core sampler")

    fun start() = thread.start()
    fun stop(): String? { running = false; thread.interrupt(); thread.join(); return coresSummary(seen) }

    private fun sample() {
        for (task in File("/proc/self/task").listFiles().orEmpty()) {
            val (comm, state, cpu) = taskStat(read("${task.path}/stat") ?: continue) ?: continue
            if (comm != name || state != 'R') continue
            val samples = seen.getOrPut(cpu) { mutableListOf() }
            read("/sys/devices/system/cpu/cpu$cpu/cpufreq/scaling_cur_freq")?.toLongOrNull()?.takeIf { it > 0 }?.let(samples::add)
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
internal fun clusterSummary(cores: List<Core>): String {
    val groups = mutableListOf<MutableList<Core>>()
    for (core in cores) {
        val last = groups.lastOrNull()?.last()
        if (last != null && last.capacity == core.capacity && last.maxKhz == core.maxKhz) groups.last() += core
        else groups += mutableListOf(core)
    }
    return groups.joinToString("; ") { group ->
        val first = group.first()
        "cpu ${cpuRanges(group.map { it.index })}: capacity ${first.capacity ?: "not readable"}, " +
            (first.maxKhz?.let { "up to ${ghz(it)}" } ?: "top frequency not readable")
    }
}

/** The cores the test's threads were seen on and the frequencies read there, or null when none were seen. */
internal fun coresSummary(seen: Map<Int, List<Long>>): String? {
    if (seen.isEmpty()) return null
    val khz = seen.values.flatten()
    return "cpu ${cpuRanges(seen.keys)}" + if (khz.isEmpty()) ", frequency not readable"
        else if (khz.min() == khz.max()) " at ${ghz(khz.min())}" else " at ${ghz(khz.min())}–${ghz(khz.max())}"
}

/** The process's cgroups on one line, without the uid and pid numbers Android puts in their paths. */
internal fun cgroupSummary(text: String) = text.lines().map { it.trim() }.filter { it.isNotEmpty() }
    .joinToString(" ") { it.replace(Regex("(uid|pid)_\\d+"), "$1_*") }

/** People read "standard" or "faster" for the speech library, never an engine's name. */
internal fun libraryLabel(file: String?) = when {
    file == null -> "unknown"
    file.contains("v82") -> "faster"
    file.startsWith("libhexspeech") -> "standard"
    else -> "other"
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

/** Gigahertz from the kernel's kHz; a value under 10 MHz (an emulator's) as it is. */
internal fun ghz(khz: Long) = if (khz < 10_000) "$khz kHz" else String.format(Locale.ROOT, "%.2f GHz", khz / 1e6)

/** One run on one line, then where it was scheduled and the cores it ran on. */
internal fun runLines(run: Run): String = buildString {
    append("${run.mode}, run ${run.number}: ${run.wallMs} ms, processor ${run.cpuMs} ms")
    val t = run.timings
    if (t.size >= 7) {
        val encoder = t[1] / 1000
        val decoder = (t[0] + t[2] + t[3] + t[4]) / 1000
        append("; encoder $encoder ms, decoder $decoder ms, other ${maxOf(0, run.wallMs - encoder - decoder)} ms")
        append("; ${t[6]} threads, encoder window ${if (t[5] == 0L) "full" else "${t[5]}"}")
    }
    append("\n    ").append(schedLine(run.before))
    if (run.after != run.before) append("; at the end ").append(schedLine(run.after))
    append("\n    ran on ").append(run.cores ?: "cores not readable")
}

private fun schedLine(s: Sched) = "cpuset ${s.cpuset ?: "not readable"}, cgroup ${s.cgroup ?: "not readable"}, " +
    "cpus allowed ${s.allowed ?: "not readable"}, nice ${s.nice ?: "not readable"}"

/** The text the page shows and Copy copies. */
internal fun report(facts: List<String>?, app: Diagnostics.Speed, keyboard: Diagnostics.Speed): String = buildString {
    append("Hex diagnostics\n")
    facts?.forEach { append(it).append('\n') }
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
                speed.runs.forEach { append("  ").append(runLines(it)).append('\n') }
                append("  text: ").append(first.text)
                append(if (speed.runs.all { it.text == first.text }) " (every run)\n" else " (runs differ)\n")
            }
        }
    }
}
