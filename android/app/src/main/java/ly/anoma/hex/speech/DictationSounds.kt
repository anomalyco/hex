package ly.anoma.hex.speech

import android.content.Context
import android.media.AudioAttributes
import android.media.SoundPool
import android.os.Handler
import android.os.Looper
import android.os.SystemClock

/** Labs › Dictation sounds' sets: desktop Hex's, in its picker order. Each folder is desktop's own
 * `resources/audio/sounds/<folder>`, packed as assets at build time. Classic and Thock have no done sound. */
internal enum class SoundSet(val folder: String, val hasDone: Boolean) {
    Classic("classic", false), Signature("signature", true), Click("click", true), Soft("soft", true),
    Wood("wood", true), Analog("analog", true), Thock("thock", false);

    companion object {
        /** Desktop Hex's default, Classic, for nothing saved or a name this build doesn't know. */
        fun matching(name: String?) = entries.firstOrNull { it.name == name } ?: Classic
    }
}

/** A moment of dictation that has its own sound. */
internal enum class SoundCue(val file: String) { Start("start"), Stop("stop"), Done("done"), Cancel("cancel"), Error("error") }

/** The asset [cue] plays in [set], or null when the set has none. Classic has no error of its own: like desktop Hex
 * off macOS, it borrows Kalimba's. */
internal fun soundAsset(set: SoundSet, cue: SoundCue): String? = when {
    cue == SoundCue.Done && !set.hasDone -> null
    cue == SoundCue.Error && set == SoundSet.Classic -> "sounds/${SoundSet.Signature.folder}/error.wav"
    else -> "sounds/${set.folder}/${cue.file}.wav"
}

/** The least time between the stop sound and the text going in for the done sound to play, as on desktop: a dictation
 * whose text goes in at once is already over when stop sounds. */
internal const val DONE_AFTER_STOP_MS = 2_000L

/** The recording is zeroed for the start sound's length and this much more, so the start sound is never transcribed. */
internal const val START_GUARD_EXTRA_MS = 120L

/** How long a WAV file's audio lasts in ms, from its `fmt ` byte rate and its `data` chunk's size; null when the bytes
 * aren't a RIFF WAVE file with both. */
internal fun wavDurationMs(bytes: ByteArray): Long? {
    fun u32(at: Int): Long = (0..3).fold(0L) { sum, i -> sum or ((bytes[at + i].toLong() and 0xff) shl (8 * i)) }
    fun tag(at: Int) = String(bytes, at, 4, Charsets.US_ASCII)
    if (bytes.size < 12 || tag(0) != "RIFF" || tag(8) != "WAVE") return null
    var byteRate = 0L
    var at = 12
    while (at + 8 <= bytes.size) {
        val size = u32(at + 4)
        when (tag(at)) {
            "fmt " -> if (at + 20 <= bytes.size) byteRate = u32(at + 16)
            "data" -> return if (byteRate > 0) size * 1000 / byteRate else null
        }
        // Chunks are padded to an even length.
        val next = at + 8 + size + (size and 1)
        if (next > Int.MAX_VALUE) return null
        at = next.toInt()
    }
    return null
}

/** How long a sound asked for while its set still loads may wait before it is dropped. */
internal const val SOUND_WAIT_MS = 300L

/** The samples at the head of a recording that started at [recordingStartedMs] (uptime) to zero under the start sound:
 * all of them while the sound is [pending] (asked for, neither played nor dropped yet), then everything up to [endsAtMs]
 * (when it played, plus its length and [START_GUARD_EXTRA_MS]; 0: it never played, nothing to zero). */
internal fun startMuteSamples(pending: Boolean, endsAtMs: Long, recordingStartedMs: Long, rate: Int = 16_000): Int = when {
    pending -> Int.MAX_VALUE
    endsAtMs <= recordingStartedMs -> 0
    else -> ((endsAtMs - recordingStartedMs) * rate / 1000).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
}

/** Labs › Dictation sounds: plays the chosen set's sounds through one [SoundPool] as sonification, so silent and
 * vibrate mute them, as they do the phone's other interface sounds. Shared by the keyboard, Home and the Quick Settings
 * sheet (one process); used on the main thread only. Off, nothing is loaded and nothing plays. */
internal object DictationSounds {
    private var pool: SoundPool? = null
    private var set: SoundSet? = null
    private val ids = HashMap<SoundCue, Int>()
    private val loaded = HashSet<Int>()
    /** Sounds asked for before they loaded, by sample id: told when one plays (its uptime) or is dropped (null), and
     * dropped when no longer [wanted] by the time it loads. */
    private class Waiting(val played: ((Long?) -> Unit)?, val wanted: () -> Boolean)
    private val waiting = HashMap<Int, Waiting>()
    private var startGuardMs = 0L
    private val main = Handler(Looper.getMainLooper())

    /** Before a dictation: loads the chosen set when the lab is on and returns how long to zero the recording's start
     * for (the start sound and [START_GUARD_EXTRA_MS]); null with the lab off, which also frees the sounds. */
    fun prepare(context: Context): Long? {
        val prefs = SpeechPreferences(context)
        if (!prefs.dictationSounds) { release(); return null }
        load(context, prefs.soundSet)
        return startGuardMs + START_GUARD_EXTRA_MS
    }

    /** Plays [cue] of the loaded set, or once it has loaded if that takes under [SOUND_WAIT_MS]; a set without that
     * sound plays nothing. [played] hears when it played (uptime ms), or null when it didn't. A sound that has to wait
     * plays only if it is still [wanted] once loaded (a start sound whose dictation already stopped is dropped). */
    fun play(cue: SoundCue, wanted: () -> Boolean = { true }, played: ((Long?) -> Unit)? = null) {
        val pool = pool
        val id = ids[cue]
        when {
            pool == null || id == null -> played?.invoke(null)
            id in loaded -> { pool.play(id, 1f, 1f, 1, 0, 1f); played?.invoke(SystemClock.uptimeMillis()) }
            else -> {
                waiting.remove(id)?.played?.invoke(null)
                val wait = Waiting(played, wanted)
                waiting[id] = wait
                main.postAtTime({ if (waiting[id] === wait) { waiting.remove(id); played?.invoke(null) } }, this,
                    SystemClock.uptimeMillis() + SOUND_WAIT_MS)
            }
        }
    }

    /** The sound picker: a short dictation in [set], start, stop and done, at desktop Hex's moments. */
    fun audition(context: Context, set: SoundSet) {
        main.removeCallbacksAndMessages(this)
        load(context, set)
        play(SoundCue.Start)
        main.postAtTime({ play(SoundCue.Stop) }, this, SystemClock.uptimeMillis() + 1_400)
        main.postAtTime({ play(SoundCue.Done) }, this, SystemClock.uptimeMillis() + 2_100)
    }

    fun release() {
        main.removeCallbacksAndMessages(this)
        pool?.release()
        pool = null; set = null
        ids.clear(); loaded.clear()
        // Nothing still waiting will play now.
        val dropped = waiting.values.toList()
        waiting.clear()
        dropped.forEach { it.played?.invoke(null) }
        startGuardMs = 0
    }

    private fun load(context: Context, chosen: SoundSet) {
        if (pool != null && set == chosen) return
        release()
        val attributes = AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build()
        val next = SoundPool.Builder().setMaxStreams(2).setAudioAttributes(attributes).build()
        next.setOnLoadCompleteListener { loading, id, status ->
            if (loading !== pool || status != 0) return@setOnLoadCompleteListener
            loaded += id
            // A sound asked for while it loaded still plays: it is at most SOUND_WAIT_MS late, else it was dropped.
            waiting.remove(id)?.let {
                if (!it.wanted()) { it.played?.invoke(null); return@let }
                loading.play(id, 1f, 1f, 1, 0, 1f); it.played?.invoke(SystemClock.uptimeMillis())
            }
        }
        pool = next; set = chosen
        val assets = context.applicationContext.assets
        for (cue in SoundCue.entries) {
            val path = soundAsset(chosen, cue) ?: continue
            try {
                assets.openFd(path).use { ids[cue] = next.load(it, 1) }
                if (cue == SoundCue.Start) startGuardMs = assets.open(path).use { wavDurationMs(it.readBytes()) } ?: 0
            } catch (_: Exception) {}
        }
    }
}
