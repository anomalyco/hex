package ly.anoma.hex.speech

import android.app.ActivityManager
import android.content.Context
import android.content.res.AssetManager
import android.util.Log
import ly.anoma.hex.BuildConfig
import ly.anoma.hex.typing.TypingModel
import java.io.File
import java.time.LocalDate
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/** A dictation's text (null when it failed), the note that goes with it, if any, and the language Labs › Automatic listens
 * for the language chose for it among several ([spoken]; null when it chose none). */
internal class Heard(val text: String?, val note: String? = null, val spoken: DictationLanguage? = null)

internal interface Recognizer {
    /** Loads the model [options] dictate with ahead of a dictation, while it records (on the transcription thread).
     * Returns whether it read a model, false when the one it needs was already loaded. */
    fun prepare(options: SpeechOptions): Boolean = false
    /** [job] names this decode, so [abort] can stop it. */
    fun decode(pcm: ShortArray, count: Int, options: SpeechOptions, job: Long = 0): Heard
    /** Stops [job]'s decode, from any thread, if it is running: the person discarded it. */
    fun abort(job: Long) {}
    /** [job]'s decode times for Labs › Diagnostics, once it has run; null for another or none. */
    fun spent(job: Long): SpeechModel.Spent? = null
}

/** Shown when a shortage of memory sent an English dictation to today's model; at most once a day. */
internal const val LOW_MEMORY_NOTE = "Low on memory: used standard dictation."

internal class LocalSpeech(private val app: Context) : Recognizer {
    override fun prepare(options: SpeechOptions) = SpeechModel.prepare(app, engineFor(options))
    override fun decode(pcm: ShortArray, count: Int, options: SpeechOptions, job: Long): Heard {
        val decoded = SpeechModel.decode(app, pcm, count, options, job)
        return Heard(decoded.text, if (decoded.lowMemory && lowMemoryNoteDue()) LOW_MEMORY_NOTE else null,
            decoded.choice?.takeIf { it.probabilities != null }?.language)
    }
    override fun abort(job: Long) = SpeechModel.abort(job)
    override fun spent(job: Long) = SpeechModel.spent(job)

    private fun lowMemoryNoteDue(): Boolean {
        val prefs = app.getSharedPreferences("speech", Context.MODE_PRIVATE)
        val today = LocalDate.now().toEpochDay()
        if (prefs.getLong("lowMemoryNoteDay", -1) == today) return false
        prefs.edit().putLong("lowMemoryNoteDay", today).apply()
        return true
    }
}

/** The one speech model in Hex's process, shared by the keyboard and Home. It loads when a dictation
 * starts (the downloaded model, else the bundled one, or Labs › Faster English dictation's for English),
 * stays loaded between dictations and is freed after [KEEP_MS] without one, or when Android asks for
 * memory. Nothing runs while it waits. Loading one model frees the other. */
internal object SpeechModel {
    private const val KEEP_MS = 120_000L
    private val timer = Executors.newSingleThreadScheduledExecutor { Thread(it, "Hex speech model").apply { isDaemon = true } }
    private var pending: ScheduledFuture<*>? = null
    private var handle = 0L
    /** The loaded model: the downloaded file's path, "" for the bundled one, null for none. */
    private var loaded: String? = null
    /** Faster English dictation's session, while it is the model loaded. */
    private var english = 0L
    /** Whether libhexenglish.so has been loaded in this process: until then nothing touches [Parakeet]. */
    @Volatile private var englishUsed = false
    /** Android's available memory below which an English dictation keeps today's model, when that is the one loaded,
     * rather than load another: loading is what gets the keyboard's process killed. */
    private const val SHORT_OF_MEMORY = 300_000_000L

    /** Debug builds only: core ratings that stand in for the kernel's, so an emulator (which rates none) can take the
     * pinned branch once. MainActivity sets it from `--es cpu_capacity 300,300,1024,1024` before anything reads them. */
    @Volatile var capacityOverride: List<Int?>? = null

    /** The kernel's cpu_capacity for each core, or null for one it doesn't rate (an emulator's). */
    private val capacities: List<Int?> by lazy {
        capacityOverride?.takeIf { BuildConfig.DEBUG } ?: (0 until Runtime.getRuntime().availableProcessors()).map {
            runCatching { File("/sys/devices/system/cpu/cpu$it/cpu_capacity").readText().trim().toInt() }.getOrNull()
        }
    }

    /** One thread per fast core, at most four: the work is split evenly, so a slow core holds up the
     * rest. A core is fast with at least half the strongest core's capacity as the kernel rates it
     * (cpu_capacity: 2 of 8 on a Snapdragon 695, all 8 on an 8 Elite); unrated cores all count. */
    internal val threads: Int by lazy {
        val rated = capacities.filterNotNull()
        val fast = if (rated.size == capacities.size && rated.max() > 0) rated.count { it * 2 >= rated.max() } else capacities.size
        fast.coerceIn(1, 4)
    }

    /** The cores [threads] counts, for the pool the model keeps ([open]): 0 when Hex can't tell them apart. */
    internal val poolMask: Long by lazy { fastCoreMask(capacities) }

    /** Where dictation's threads are kept, for Labs › Diagnostics: the loaded model's pool (-1 for none, 0 for any core,
     * else a bit per core), or, before a model loads, where its pool would be (the same check, the kernel asked). */
    @Synchronized fun dictationCores(): Long =
        (if (handle != 0L) Whisper.poolCores(handle) else -1L).takeIf { it >= 0 } ?: Whisper.usableCores(poolMask, threads)

    /** The debug ratings standing in for the kernel's, or null (always null outside debug builds). */
    val debugRatings: List<Int?>? get() = capacityOverride?.takeIf { BuildConfig.DEBUG }

    /** Dictation's pool, made on the calling thread (the transcription thread) when the model has none, as after the
     * speed test, which leaves none of its own behind. A no-op when it is already there. */
    private fun ensurePool() { if (handle != 0L) Whisper.threadPool(handle, threads, poolMask) }

    /** Models opened since the process started (today's or the faster one), so a caller can tell whether a load read one. */
    private var opened = 0L

    @Synchronized fun prepare(app: Context, engine: Engine): Boolean {
        val before = opened
        if (engine == Engine.Standard || openEnglish(app) != null) { load(app); ensurePool() }
        keep()
        return opened != before
    }

    /** A dictation decode's times for Labs › Diagnostics: its start and end (System.nanoTime), the process's processor
     * time over it (-1: unreadable), whisper.cpp's timings when today's model ran, copied before the speed test can
     * replace them, whether it loaded a model itself, the model that wrote the text, the threads it ran on and the language
     * Labs › Automatic listens for the language chose. Only [decode] writes it; the speed test's [measure] never does. */
    class Spent(val job: Long, val startedAt: Long, val endedAt: Long, val cpuMs: Long, val timings: LongArray?, val loaded: Boolean,
                val ran: RanOn = RanOn.Standard, val threads: Int = 0, val choice: LanguageChoice? = null)
    @Volatile private var spent: Spent? = null
    fun spent(job: Long) = spent?.takeIf { it.job == job }
    private fun cpuMs() = runCatching { android.os.Process.getElapsedCpuTime() }.getOrDefault(-1L)
    /** When this decode's first model call began (System.nanoTime, 0: none yet) and the processor time then; a load
     * before it isn't counted, as before. [whisperRan]: today's model decoded, so whisper.cpp's timings are this one's. */
    private var decodeAt = 0L
    private var decodeCpu = -1L
    private var whisperRan = false
    private fun decodeStarts() { if (decodeAt == 0L) { decodeAt = System.nanoTime(); decodeCpu = cpuMs() } }

    /** The last decode stopped by [abort]: its empty result says nothing about the model. */
    @Volatile private var abandoned = 0L

    @Synchronized fun decode(app: Context, pcm: ShortArray, count: Int, options: SpeechOptions, job: Long): Decoded {
        val before = opened
        decodeAt = 0L; decodeCpu = -1L; whisperRan = false
        val decoded = transcribe(engineFor(options), pcm, count, { abandoned == job },
            english = { audio, n -> openEnglish(app) ?: decodeEnglish(app, audio, n, options.recognitionHints, job) },
            standard = { audio, n -> decodeStandard(app, audio, n, options.languages.joinToString(",") { it.code }, options.translate,
                whisperPrompt(options.recognitionHints), job) })
        val ended = System.nanoTime()
        val started = if (decodeAt != 0L) decodeAt else ended
        val processor = cpuMs().let { if (it < 0 || decodeCpu < 0) -1L else it - decodeCpu }
        val choice = languageChoice(options.candidates)
        spent = Spent(job, started, ended, processor, if (whisperRan) runCatching { Whisper.timings() }.getOrNull() else null,
            opened != before, decoded.ran, threads, choice)
        keep()
        return decoded.copy(choice = choice)
    }

    /** What Labs › Automatic listens for the language chose this decode, read from hexspeech.cpp: null with the lab off
     * (no [candidates]), or when today's model didn't run or chose nothing (it failed first); one candidate is no choice. */
    private fun languageChoice(candidates: List<DictationLanguage>): LanguageChoice? {
        if (candidates.size <= 1) return candidates.firstOrNull()?.let { LanguageChoice(candidates, 0, null) }
        val made = if (whisperRan) runCatching { Whisper.language() }.getOrNull() else null
        val chosen = made?.takeIf { it.size >= 2 + candidates.size }?.get(0)?.toInt() ?: return null
        if (chosen !in candidates.indices) return null
        return LanguageChoice(candidates, chosen, candidates.indices.map { made[2 + it] / 1e6 }, made[1])
    }

    private fun decodeEnglish(app: Context, pcm: ShortArray, count: Int, vocabulary: List<String>, job: Long): EnglishResult {
        decodeStarts()
        return Parakeet.decode(english, pcm, count, vocabulary.takeIf { it.isNotEmpty() }?.toTypedArray(), job)
            ?.decodeToString()?.let { decoded ->
                // A formatting step never costs a dictation: if it fails, the model's text goes out as it came.
                try {
                    SentenceEnds.restore(decoded, pcm, count) { word -> lowercaseEnglish(app, word) }
                } catch (e: Exception) {
                    Log.w("HexSpeech", "sentence ends skipped: ${e.javaClass.simpleName}")
                    decoded.substringBefore('\n')
                }
            }?.let { text ->
                try {
                    SpokenNumbers.digits(text, vocabulary)
                } catch (e: Exception) {
                    Log.w("HexSpeech", "numbers as digits skipped: ${e.javaClass.simpleName}")
                    text
                }
            }?.trim()
            ?.let(EnglishResult::Text) ?: EnglishResult.Failed
    }

    /** English's typing model, memory-mapped from the APK, for [SentenceEnds]: opened on the first English dictation. */
    private var englishWords: TypingModel? = null
    private fun lowercaseEnglish(app: Context, word: String): Boolean {
        val model = englishWords ?: TypingModel.asset(app.assets, "en").also { englishWords = it }
        return model.index(word) >= 0 && (model.capitals?.percent(word) ?: -1) < 0
    }

    private fun decodeStandard(app: Context, pcm: ShortArray, count: Int, language: String, translate: Boolean, prompt: String?, job: Long): String? {
        val small = load(app)
        ensurePool()
        decodeStarts()
        // whisper.cpp's timings belong to this decode only once one ran; with no model they are an earlier one's.
        if (handle != 0L) whisperRan = true
        var bytes = if (handle != 0L) Whisper.decode(handle, small, pcm, count, threads, language, translate, prompt, job) else null
        // Unusable downloaded weights fall back to the bundled model.
        if (bytes == null && small && abandoned != job) {
            unload()
            if (open(app, null)) { whisperRan = true; bytes = Whisper.decode(handle, false, pcm, count, threads, language, translate, prompt, job) }
        }
        return bytes?.decodeToString()?.trim()
    }

    /** Labs › Diagnostics' speed test: decodes [pcm] as English on [threads] threads, as a dictation does, the model
     * already loaded, and returns the text and whisper.cpp's timings, or null when it failed. [pool] threads are kept
     * ready with the model for this decode, on the cores [mask] names (0: any core), or none (0 threads: new ones for
     * every graph, ggml's default). Each run makes its own pool on the calling thread (the test's), so its workers take
     * that thread's name, which is how the core sampler finds them, and frees it afterwards: no worker the test made,
     * pinned or not, outlives the run, and dictation makes its own pool again on its own thread when it next starts. */
    @Synchronized fun measure(app: Context, pcm: ShortArray, threads: Int, pool: Int, mask: Long, job: Long): Measured? {
        val small = load(app)
        if (handle == 0L) return null
        try {
            Whisper.threadPool(handle, 0, 0)
            if (pool > 0 && !Whisper.threadPool(handle, pool, mask)) return null
            val cpu = android.os.Process.getElapsedCpuTime()
            val started = System.nanoTime()
            val bytes = Whisper.decode(handle, small, pcm, pcm.size, threads, "en", false, null, job)
            val wall = (System.nanoTime() - started) / 1_000_000
            val processor = android.os.Process.getElapsedCpuTime() - cpu
            val timings = Whisper.timings()
            keep()
            return if (bytes == null || timings == null) null
                else Measured(small, wall, processor, timings, bytes.decodeToString().trim())
        } finally {
            // This run's pool goes, whatever the decode did: its workers exit (they were pinned on a fast-cores run), and the
            // thread that measured had its own affinity put back by decode. Dictation's next start makes its pool again.
            Whisper.threadPool(handle, 0, 0)
        }
    }
    /** Labs › Diagnostics' speed test with Faster English dictation's model, as an English dictation with that switch on
     * runs it (its own thread count, SentenceEnds included); null when that model isn't downloaded or failed. Opening it
     * frees today's model, as a dictation would; the timing starts once it is open. */
    @Synchronized fun measureEnglish(app: Context, pcm: ShortArray, job: Long): Measured? {
        if (ModelStore.english.installed(app) == null || openEnglish(app) != null) return null
        val cpu = android.os.Process.getElapsedCpuTime()
        val started = System.nanoTime()
        val result = decodeEnglish(app, pcm, pcm.size, emptyList(), job)
        val wall = (System.nanoTime() - started) / 1_000_000
        val processor = android.os.Process.getElapsedCpuTime() - cpu
        keep()
        return (result as? EnglishResult.Text)?.let { Measured(true, wall, processor, LongArray(0), it.text) }
    }
    /** [wallMs] and [cpuMs] (the whole process's processor time) cover the decode alone. */
    class Measured(val downloaded: Boolean, val wallMs: Long, val cpuMs: Long, val timings: LongArray, val text: String)

    /** Ends [job]'s decode at whisper.cpp's next check (after an encoder pass or a token). Not under the lock,
     * which that decode holds. */
    fun abort(job: Long) { abandoned = job; Whisper.abort(job); if (englishUsed) Parakeet.abort(job) }

    /** Frees the model off the caller's thread (it may wait for a dictation in progress). */
    fun releaseSoon() { timer.execute(::release) }

    @Synchronized private fun release() { pending?.cancel(false); pending = null; unload(); freeEnglish() }

    /** Returns whether the downloaded model is the one loaded. */
    private fun load(app: Context): Boolean {
        val path = ModelStore.small.installed(app)?.absolutePath
        if (handle != 0L && loaded == (path ?: "")) return path != null
        unload(); freeEnglish()
        if (path != null && open(app, path)) return true
        open(app, null)
        return false
    }

    private fun open(app: Context, path: String?): Boolean {
        handle = Whisper.load(app.assets, path)
        opened++
        loaded = if (handle != 0L) path ?: "" else null
        // One pool, reused by every decode: ggml otherwise starts and joins one for every graph, each encoder pass and
        // each decoded token, and those short-lived threads land on slow cores.
        if (handle != 0L) Whisper.threadPool(handle, threads, poolMask)
        return handle != 0L
    }

    private fun unload() { if (handle != 0L) Whisper.free(handle); handle = 0L; loaded = null }

    /** Opens Faster English dictation's model, freeing today's; null when it is ready, else why it can't run. Its file is
     * hashed once per process ([ModelStore.verified]): one that is missing or fails goes unused, silently. */
    private fun openEnglish(app: Context): EnglishResult.Unavailable? {
        if (english != 0L) return null
        val file = ModelStore.english.verified(app) ?: return EnglishResult.Unavailable(Unready.NoFile)
        if (handle != 0L && shortOfMemory(app)) return EnglishResult.Unavailable(Unready.LowMemory)
        unload()
        englishUsed = true
        english = Parakeet.load(file.absolutePath, threads)
        opened++
        return if (english != 0L) null
            else EnglishResult.Unavailable(if (Parakeet.outOfMemory()) Unready.LowMemory else Unready.LoadFailed)
    }

    private fun freeEnglish() { if (english != 0L) Parakeet.free(english); english = 0L }

    private fun shortOfMemory(app: Context): Boolean {
        // Debug builds only: "adb shell setprop log.tag.HexLowMemory DEBUG" acts out a shortage, to see the fallback.
        if (BuildConfig.DEBUG && Log.isLoggable("HexLowMemory", Log.DEBUG)) return true
        val info = ActivityManager.MemoryInfo()
        app.getSystemService(ActivityManager::class.java)?.getMemoryInfo(info) ?: return false
        return info.lowMemory || info.availMem < SHORT_OF_MEMORY
    }

    private fun keep() {
        pending?.cancel(false)
        pending = timer.schedule(::release, KEEP_MS, TimeUnit.MILLISECONDS)
    }
}

/** One bit per core the kernel rates at least half as strong as the strongest (cpu_capacity), the rule [SpeechModel.threads]
 * counts: a decode then runs its workers, and itself, on those cores instead of landing on slow ones.
 * 0, no restriction, when a core is unrated, when none has a positive rating, or when every core counts as fast (an 8
 * Elite: the rule can't tell them apart). Cores past 63 can't be named. */
internal fun fastCoreMask(capacities: List<Int?>): Long {
    if (capacities.isEmpty() || capacities.size > 64) return 0
    val rated = capacities.map { it ?: return 0 }
    val top = rated.max()
    if (top <= 0) return 0
    val fast = rated.withIndex().filter { (_, capacity) -> capacity * 2 >= top }
    if (fast.size == rated.size) return 0
    return fast.fold(0L) { mask, (core, _) -> mask or (1L shl core) }
}

internal object Whisper {
    init { System.loadLibrary("hexspeech") }
    external fun load(assets: AssetManager, file: String?): Long
    external fun free(handle: Long)
    external fun decode(handle: Long, small: Boolean, pcm: ShortArray, count: Int, threads: Int,
                        language: String, translate: Boolean, prompt: String?, job: Long): ByteArray?
    external fun abort(job: Long)
    /** The last decode's timings, for Labs › Diagnostics (hexspeech.cpp says what each is). */
    external fun timings(): LongArray?
    /** What the last decode chose among the languages it was given (hexspeech.cpp says what each value is). */
    external fun language(): LongArray?
    /** Keeps [threads] worker threads ready with the model [handle] for its decodes, or none (0, ggml's default: new
     * threads for every graph), on the cores [fastCores] has a bit for (0: no restriction, and also what a mask leaves
     * fewer cores than [threads] of). The pool is freed with the model. */
    external fun threadPool(handle: Long, threads: Int, fastCores: Long): Boolean
    /** The file this library loaded from, for Labs › Diagnostics. */
    external fun library(): String?
    /** The cores of [fast] a pool of [threads] would be kept on from this thread: 0 (any core) when fewer than [threads]
     * are usable or the kernel refuses to pin to them. */
    external fun usableCores(fast: Long, threads: Int): Long
    /** The cores model [handle]'s pool is kept on: -1 for no pool, 0 for any core. */
    external fun poolCores(handle: Long): Long
}
