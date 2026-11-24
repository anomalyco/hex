package ly.anoma.hex.speech

import android.content.Context
import android.content.res.AssetManager
import java.io.File
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

internal interface Recognizer {
    /** Loads the model ahead of a dictation, while it records (on the transcription thread). */
    fun prepare() {}
    /** [job] names this decode, so [abort] can stop it. */
    fun decode(pcm: ShortArray, count: Int, language: String, translate: Boolean, prompt: String? = null, job: Long = 0): String?
    /** Stops [job]'s decode, from any thread, if it is running: the person discarded it. */
    fun abort(job: Long) {}
}

internal class LocalSpeech(private val app: Context) : Recognizer {
    override fun prepare() = SpeechModel.prepare(app)
    override fun decode(pcm: ShortArray, count: Int, language: String, translate: Boolean, prompt: String?, job: Long): String? =
        SpeechModel.decode(app, pcm, count, language, translate, prompt, job)
    override fun abort(job: Long) = SpeechModel.abort(job)
}

/** The one speech model in Hex's process, shared by the keyboard and Home. It loads when a dictation
 * starts (the downloaded model, else the bundled one), stays loaded between dictations and is freed
 * after [KEEP_MS] without one, or when Android asks for memory. Nothing runs while it waits. */
internal object SpeechModel {
    private const val KEEP_MS = 120_000L
    private val timer = Executors.newSingleThreadScheduledExecutor { Thread(it, "Hex speech model").apply { isDaemon = true } }
    private var pending: ScheduledFuture<*>? = null
    private var handle = 0L
    /** The loaded model: the downloaded file's path, "" for the bundled one, null for none. */
    private var loaded: String? = null

    /** One thread per fast core, at most four: the work is split evenly, so a slow core holds up the
     * rest. A core is fast with at least half the strongest core's capacity as the kernel rates it
     * (cpu_capacity: 2 of 8 on a Snapdragon 695, all 8 on an 8 Elite); unrated cores all count. */
    internal val threads: Int by lazy {
        val cores = Runtime.getRuntime().availableProcessors()
        val capacity = (0 until cores).mapNotNull {
            runCatching { File("/sys/devices/system/cpu/cpu$it/cpu_capacity").readText().trim().toInt() }.getOrNull()
        }
        val fast = if (capacity.size == cores && capacity.max() > 0) capacity.count { it * 2 >= capacity.max() } else cores
        fast.coerceIn(1, 4)
    }

    @Synchronized fun prepare(app: Context) { load(app); keep() }

    /** The last decode stopped by [abort]: its empty result says nothing about the model. */
    @Volatile private var abandoned = 0L

    @Synchronized fun decode(app: Context, pcm: ShortArray, count: Int, language: String, translate: Boolean, prompt: String?, job: Long): String? {
        val small = load(app)
        var bytes = if (handle != 0L) Whisper.decode(handle, small, pcm, count, threads, language, translate, prompt, job) else null
        // Unusable downloaded weights fall back to the bundled model.
        if (bytes == null && small && abandoned != job) {
            unload()
            if (open(app, null)) bytes = Whisper.decode(handle, false, pcm, count, threads, language, translate, prompt, job)
        }
        keep()
        return bytes?.decodeToString()?.trim()
    }

    /** Labs › Diagnostics' speed test: decodes [pcm] as English on [threads] threads, as a dictation does, the model
     * already loaded, and returns the text and whisper.cpp's timings, or null when it failed. */
    @Synchronized fun measure(app: Context, pcm: ShortArray, threads: Int, job: Long): Measured? {
        val small = load(app)
        if (handle == 0L) return null
        val cpu = android.os.Process.getElapsedCpuTime()
        val started = System.nanoTime()
        val bytes = Whisper.decode(handle, small, pcm, pcm.size, threads, "en", false, null, job)
        val wall = (System.nanoTime() - started) / 1_000_000
        val processor = android.os.Process.getElapsedCpuTime() - cpu
        val timings = Whisper.timings()
        keep()
        return if (bytes == null || timings == null) null
            else Measured(small, wall, processor, timings, bytes.decodeToString().trim())
    }
    /** [wallMs] and [cpuMs] (the whole process's processor time) cover the decode alone. */
    class Measured(val downloaded: Boolean, val wallMs: Long, val cpuMs: Long, val timings: LongArray, val text: String)

    /** Ends [job]'s decode at whisper.cpp's next check (after an encoder pass or a token). Not under the lock,
     * which that decode holds. */
    fun abort(job: Long) { abandoned = job; Whisper.abort(job) }

    /** Frees the model off the caller's thread (it may wait for a dictation in progress). */
    fun releaseSoon() { timer.execute(::release) }

    @Synchronized private fun release() { pending?.cancel(false); pending = null; unload() }

    /** Returns whether the downloaded model is the one loaded. */
    private fun load(app: Context): Boolean {
        val path = ModelStore.installed(app)?.absolutePath
        if (handle != 0L && loaded == (path ?: "")) return path != null
        unload()
        if (path != null && open(app, path)) return true
        open(app, null)
        return false
    }

    private fun open(app: Context, path: String?): Boolean {
        handle = Whisper.load(app.assets, path)
        loaded = if (handle != 0L) path ?: "" else null
        return handle != 0L
    }

    private fun unload() { if (handle != 0L) Whisper.free(handle); handle = 0L; loaded = null }

    private fun keep() {
        pending?.cancel(false)
        pending = timer.schedule(::release, KEEP_MS, TimeUnit.MILLISECONDS)
    }
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
    /** The file this library loaded from, for Labs › Diagnostics. */
    external fun library(): String?
}
