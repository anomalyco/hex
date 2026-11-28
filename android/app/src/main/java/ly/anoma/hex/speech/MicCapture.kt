package ly.anoma.hex.speech

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.sqrt
import kotlinx.coroutines.flow.MutableStateFlow

internal enum class SpeechPhase { Idle, Opening, Listening, Transcribing }

private const val LOG_TAG = "HexSpeech"

/** Audio Whisper hears before the first voiced 40 ms block and after the last. A word's soft edges (an
 * initial "h" or "f", a final "s" or "t", a quiet speaker's last syllable) can sit under the voiced test's
 * −48 dBFS for a few hundred milliseconds. The 40 benchmark clips and 15 long ones kept the same text with
 * 0.3, 0.4 and 0.5 s after the speech; the longer end costs nothing measurable, so it gets 0.5 s. */
internal const val SPEECH_LEAD_MS = 300
internal const val SPEECH_TAIL_MS = 500

/** The part of a recording Whisper transcribes: from [SPEECH_LEAD_MS] before the speech ([first], -1 for none:
 * all of it) to [SPEECH_TAIL_MS] after it ([last], where it ends), within the [count] samples recorded ([SpeechEdges]).
 * Quiet around the speech only slows Whisper (3 s of speech in a 60 s recording took 8.5× as long) and
 * invites stray words ("you", a repeated sentence); pauses between words stay. */
internal fun speechSpan(first: Int, last: Int, count: Int, rate: Int = 16_000): IntRange =
    if (first < 0) 0 until count
    else maxOf(0, first - rate * SPEECH_LEAD_MS / 1000) until minOf(count, last + rate * SPEECH_TAIL_MS / 1000)

/** Voiced 40 ms blocks less than SPEECH_GAP_MS apart form a stretch, which is speech once it holds SPEECH_MIN_MS of
 * voice. A click, a knock or the tap on ✓ (one or two loud blocks) away from the speech never is, so it doesn't stretch
 * what Whisper hears; any word is longer. */
internal const val SPEECH_GAP_MS = 300
internal const val SPEECH_MIN_MS = 120

/** Where speech starts ([first], -1 for none yet) and ends ([last]) in a recording, fed its blocks in order. */
internal class SpeechEdges(private val rate: Int = 16_000) {
    var first = -1
        private set
    var last = 0
        private set
    private var start = -1
    private var end = 0
    private var voice = 0
    fun block(at: Int, n: Int, voiced: Boolean) {
        if (!voiced) return
        if (start < 0 || at - end > rate * SPEECH_GAP_MS / 1000) { start = at; voice = 0 }
        voice += n; end = at + n
        if (voice >= rate * SPEECH_MIN_MS / 1000) { if (first < 0) first = start; last = end }
    }
}

/** Names a dictation's decode to whisper.cpp, in this process (Home and the keyboard share the model). */
private val jobs = AtomicLong()

/** Where a dictation was made, for Labs › Diagnostics' last dictation. */
internal enum class DictationPlace { Keyboard, Home, QuickSettings, SpeechRequest }

/** The speech model when ✓ was tapped: loaded before the dictation, loaded while the person spoke, still loading
 * (ready [DictationTimes.modelReadyMs] after ✓), loaded by the decode itself after ✓, or not known. */
internal enum class ModelAtTick { Loaded, LoadedWhileSpeaking, Loading, LoadedAfter, Unknown }

/** [ModelAtTick] from the times (System.nanoTime) of ✓ ([tick]) and of the model's load ahead of the dictation
 * ([prepared], 0 when it never finished), whether that load read the model ([prepareLoaded]), and whether the
 * decode had to load it ([decodeLoaded]). */
internal fun modelAtTick(tick: Long, prepared: Long, prepareLoaded: Boolean, decodeLoaded: Boolean) = when {
    decodeLoaded -> ModelAtTick.LoadedAfter
    prepared == 0L -> ModelAtTick.Unknown
    prepareLoaded && tick != 0L && prepared > tick -> ModelAtTick.Loading
    prepareLoaded -> ModelAtTick.LoadedWhileSpeaking
    else -> ModelAtTick.Loaded
}

/** The last dictation that reached a speech model, for Labs › Diagnostics: durations only, never what was said or typed.
 * Times are ms after ✓ (after recording stopped when it stopped at 1 minute, [tick] false); [insertedMs] is null
 * when no text went in. [timings] are whisper.cpp's (Whisper.timings; null when today's model didn't run, since the
 * faster model gives no split), [cpuMs] the process's processor time during the decode (-1: unreadable), [ran] the
 * model that wrote the text, [threads] the threads it ran on (0: not known) and [choice] the language Labs › Automatic
 * listens for the language chose (null with the lab off). */
internal class DictationTimes(val place: DictationPlace, val recordedMs: Long, val keptMs: Long, val model: ModelAtTick,
                              val modelReadyMs: Long, val tick: Boolean, val stoppedMs: Long, val decodeStartedMs: Long,
                              val decodeEndedMs: Long, val insertedMs: Long?, val cpuMs: Long, val timings: LongArray?,
                              val ran: RanOn = RanOn.Standard, val threads: Int = 0, val choice: LanguageChoice? = null)

/** Held in memory only, so it goes when Hex's process ends; Home and the keyboard share the process. */
internal val lastDictation = MutableStateFlow<DictationTimes?>(null)

// The IME calls start/finish/cancel on the main thread. One worker records, another transcribes,
// so a dictation can start while the one before is still transcribing (as desktop Hex queues
// them); results are inserted in the order they were spoken. Every posted result belongs to its
// original ticket, including failures.
internal class MicCapture(
    /** Reads Labs › Dictation sounds at each start. */
    private val context: Context,
    private val recognizer: Recognizer,
    private val place: DictationPlace,
    private val update: (SpeechPhase, String?) -> Unit,
    private val meter: (Float) -> Unit,
    /** The text, what was heard, the options it was written with and the field number [start] was given. */
    private val insert: (String, String, SpeechOptions, Int) -> Unit,
) {
    private class Ticket(val options: SpeechOptions, val field: Int) {
        val job = jobs.incrementAndGet()
        @Volatile var finish = false
        @Volatile var cancelled = false
        /** The keyboard's field went away: its notes aren't shown in the next one. */
        var left = false
        var done = false
        var note: String? = null
        var text: String? = null
        /** What the model heard, before formatting: Labs › Learn from my fixes follows the raw words. */
        var heard: String? = null
        /** [options] in the language Labs › Automatic listens for the language chose for it, once decoded. */
        var written: SpeechOptions? = null
        // Labs › Diagnostics' last dictation: System.nanoTime of each step (0: not reached) and sample counts.
        @Volatile var tickAt = 0L
        var preparedAt = 0L
        var prepareLoaded = false
        var stoppedAt = 0L
        var recorded = 0
        var kept = 0
        var spent: SpeechModel.Spent? = null
        // Labs › Dictation sounds, as it was when the dictation started: whether it plays them, the start sound's length
        // plus START_GUARD_EXTRA_MS, whether the start sound is still to play or be dropped, when the zeroed head of the
        // recording ends (uptime ms: when the start sound played plus that guard; 0: nothing to zero), whether the start
        // sound played, when the stop sound played (uptime ms, 0: not) and whether the dictation failed (the error sound).
        var sounds = false
        var guardMs = 0L
        @Volatile var startPending = false
        @Volatile var muteEndsAt = 0L
        @Volatile var startSounded = false
        var stopSoundAt = 0L
        var failed = false
    }
    private val main = Handler(Looper.getMainLooper())
    private val recorder = Executors.newSingleThreadExecutor { Thread(it, "Hex dictation") }
    private val transcriber = Executors.newSingleThreadExecutor { Thread(it, "Hex transcription") }
    /** The dictation holding the microphone. */
    private var active: Ticket? = null
    /** Dictations that stopped recording, oldest first, until their text is inserted. */
    private val queue = ArrayDeque<Ticket>()
    /** The dictation Whisper is decoding, so discarding it stops the decode. */
    @Volatile private var decoding: Ticket? = null
    var phase = SpeechPhase.Idle
        private set
    /** Whether a new dictation can start: nothing holds the microphone. */
    val canStart get() = active == null

    /** [field]: the keyboard's number for the field it is spoken in, handed back with the text. */
    fun start(options: SpeechOptions, field: Int = 0) {
        if (active != null) return
        val ticket = Ticket(options, field)
        DictationSounds.prepare(context)?.let { ticket.sounds = true; ticket.guardMs = it; ticket.startPending = true }
        active = ticket
        publish(SpeechPhase.Opening)
        // The model loads while the person speaks, not after they let go.
        transcriber.execute {
            try {
                if (!ticket.cancelled) { ticket.prepareLoaded = recognizer.prepare(ticket.options); ticket.preparedAt = System.nanoTime() }
            } catch (_: Throwable) {}
        }
        recorder.execute {
            try {
                record(ticket)
            } catch (_: SecurityException) {
                complete(ticket, "Allow the microphone in Hex", failed = true)
            } catch (_: Exception) {
                complete(ticket, "Microphone unavailable. Try again.", failed = true)
            }
        }
    }

    /** ✓, a hold's release, or the keyboard hiding with its field still there: the microphone stops now and what
     * was heard is transcribed. */
    fun finish() {
        val ticket = active ?: return
        if (ticket.tickAt == 0L) ticket.tickAt = System.nanoTime()
        ticket.finish = true
        publish(SpeechPhase.Transcribing)
    }

    /** The ✕: discards the dictation recording now, else the ones still transcribing. Earlier
     * dictations still arrive when a later one is discarded while it records. */
    fun cancelLatest() {
        val ticket = active
        if (ticket != null) {
            // Like the stop sound, only once the start sound has played.
            if (ticket.sounds && ticket.startSounded) DictationSounds.play(SoundCue.Cancel)
            ticket.cancelled = true
            active = null
            publish(current())
        } else cancel(sound = true)
    }

    /** The keyboard's field went away: a recording stops as with ✓ and is still transcribed, and nothing of it is shown
     * in the next field; [insert] gets each text with its old field number, so the keyboard keeps it rather than type it. */
    fun leave() {
        queue.forEach { it.left = true }
        active?.let { it.left = true; finish() }
    }

    /** Nothing recording or transcribing goes in: Next keyboard, Home or the Quick Settings sheet closing. A decode in progress stops too, or the next dictation's load and decode would wait for it. */
    fun cancel(sound: Boolean = false) {
        // Labs › Dictation sounds: the cancel sound only when the person discards a dictation (✕, Cancel, Home's pages)
        // and something was discarded; Hex closing, the screen going off or a field going away discard silently.
        // A recording counts once its start sound played, like the stop sound; a transcribing one already sounded both.
        val discarded = active?.takeIf { it.startSounded } ?: queue.firstOrNull()
        if (sound && discarded?.sounds == true) DictationSounds.play(SoundCue.Cancel)
        active?.cancelled = true
        active = null
        queue.forEach { it.cancelled = true }
        queue.clear()
        decoding?.takeIf { it.cancelled }?.let { recognizer.abort(it.job) }
        publish(SpeechPhase.Idle)
    }

    fun close() { cancel(); recorder.shutdownNow(); transcriber.shutdownNow() }

    private fun current() = when {
        active != null -> phase.takeIf { it == SpeechPhase.Opening || it == SpeechPhase.Listening } ?: SpeechPhase.Listening
        queue.isNotEmpty() -> SpeechPhase.Transcribing
        else -> SpeechPhase.Idle
    }

    private fun publish(value: SpeechPhase, note: String? = null) {
        phase = value
        update(value, note)
    }

    /** A dictation ends: recording stopped with nothing to transcribe, or its text is ready. */
    private fun complete(ticket: Ticket, note: String? = null, text: String? = null, heard: String? = null,
                         written: SpeechOptions? = null, failed: Boolean = false) = main.post {
        if (active === ticket) active = null
        if (ticket.cancelled) { publish(current()); return@post }
        ticket.done = true; ticket.note = note; ticket.text = text; ticket.heard = heard; ticket.written = written
        ticket.failed = failed
        if (ticket !in queue) queue.addLast(ticket)
        var shown: String? = null
        while (queue.firstOrNull()?.done == true) {
            val next = queue.removeFirst()
            if (!next.left) shown = next.note ?: shown
            next.text?.let { insert(it, next.heard ?: it, next.written ?: next.options, next.field) }
            sound(next)
            next.spent?.let {
                val times = durations(next, it, if (next.text != null) System.nanoTime() else 0L)
                lastDictation.value = times
                // Labs › Pick the English model for this phone: numbers only, and never at a dictation's cost.
                try { recognizer.timed(times, next.options) } catch (_: Exception) {}
            }
        }
        publish(current(), shown)
    }

    /** Labs › Dictation sounds once [ticket]'s outcome is shown: the error sound for a failure, and done for text that
     * went in [DONE_AFTER_STOP_MS] or more after the stop sound. Neither plays while another dictation records, which
     * would hear it. */
    private fun sound(ticket: Ticket) {
        if (!ticket.sounds || active != null) return
        when {
            ticket.failed -> DictationSounds.play(SoundCue.Error)
            ticket.text != null && ticket.stopSoundAt != 0L &&
                SystemClock.uptimeMillis() - ticket.stopSoundAt >= DONE_AFTER_STOP_MS -> DictationSounds.play(SoundCue.Done)
        }
    }

    /** The start sound played at [at] (uptime ms), or never (null): the recorder zeroes up to its end and the guard, and
     * stops zeroing everything it reads. */
    private fun startPlayed(ticket: Ticket, at: Long?) {
        if (at != null) { ticket.muteEndsAt = at + ticket.guardMs; ticket.startSounded = true }
        ticket.startPending = false
    }

    /** [ticket]'s durations once its text went in at [inserted] (0: no text). Arithmetic only. */
    private fun durations(ticket: Ticket, spent: SpeechModel.Spent, inserted: Long): DictationTimes {
        val base = if (ticket.tickAt != 0L) ticket.tickAt else ticket.stoppedAt
        fun ms(at: Long) = (at - base) / 1_000_000
        return DictationTimes(place, ticket.recorded * 1000L / 16_000, ticket.kept * 1000L / 16_000,
            modelAtTick(ticket.tickAt, ticket.preparedAt, ticket.prepareLoaded, spent.loaded),
            if (ticket.preparedAt != 0L) ms(ticket.preparedAt) else 0,
            ticket.tickAt != 0L, ms(ticket.stoppedAt), ms(spent.startedAt), ms(spent.endedAt),
            if (inserted != 0L) ms(inserted) else null, spent.cpuMs, spent.timings, spent.ran, spent.threads, spent.choice)
    }

    /** Recording stopped with audio to transcribe: queue it behind earlier dictations. [note] goes with its text. */
    private fun transcribe(ticket: Ticket, pcm: ShortArray, count: Int, note: String?) = main.post {
        if (active === ticket) active = null
        if (ticket.cancelled) { publish(current()); return@post }
        queue.addLast(ticket)
        publish(current())
        transcriber.execute {
            // Set before the check: cancel() either sees it and stops the decode, or this sees the ticket cancelled.
            decoding = ticket
            try {
                if (ticket.cancelled) return@execute
                val heard = recognizer.decode(pcm, count, ticket.options, ticket.job)
                ticket.spent = recognizer.spent(ticket.job)
                if (ticket.cancelled) return@execute
                // Labs › Automatic listens for the language: formatted and inserted in the language it was written in.
                val options = heard.spoken?.let(ticket.options::writtenIn) ?: ticket.options
                when (val text = heard.text?.let { SpokenText.format(it, options) }) {
                    null -> complete(ticket, "Couldn’t transcribe. Try again.", failed = true)
                    "" -> complete(ticket, "No speech heard")
                    else -> complete(ticket, dictationNote(note, heard.note), text, heard.text, options)
                }
            } catch (_: UnsatisfiedLinkError) {
                complete(ticket, "Speech model unavailable", failed = true)
            } catch (_: Exception) {
                complete(ticket, "Couldn’t transcribe. Try again.", failed = true)
            } finally {
                decoding = null
            }
        }
    }

    // No audio-focus request, playback stream, device selection or communication mode.
    // Capture dies before inference starts. No audio or transcript is written to storage.
    @SuppressLint("MissingPermission")
    private fun record(ticket: Ticket) {
        if (ticket.cancelled) return
        val rate = 16_000
        val block = rate / 25
        val minBuffer = AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        check(minBuffer > 0)
        val pcm = ShortArray(rate * 60)
        val record = AudioRecord.Builder()
            .setAudioSource(MediaRecorder.AudioSource.MIC)
            .setAudioFormat(AudioFormat.Builder().setSampleRate(rate).setChannelMask(AudioFormat.CHANNEL_IN_MONO)
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
            // Two seconds, so the reader can fall behind while an earlier dictation transcribes.
            .setBufferSizeInBytes(maxOf(minBuffer, rate * 2 * 2)).build()
        var count = 0
        var voiced = 0
        val edges = SpeechEdges(rate)
        var lastUpdate = 0L
        var recordingStarted = 0L
        try {
            check(record.state == AudioRecord.STATE_INITIALIZED)
            if (!ticket.cancelled && !ticket.finish) {
                record.startRecording()
                recordingStarted = SystemClock.uptimeMillis()
                check(record.recordingState == AudioRecord.RECORDSTATE_RECORDING)
                main.post {
                    if (active === ticket && !ticket.finish) {
                        publish(SpeechPhase.Listening)
                        // Labs › Dictation sounds: the microphone is open. What it hears of this is zeroed below.
                        // A start sound still waiting for its set is dropped once the dictation stopped or was discarded,
                        // so it never plays after the microphone closed.
                        if (ticket.sounds) DictationSounds.play(SoundCue.Start,
                            wanted = { active === ticket && !ticket.finish && !ticket.cancelled }) { at -> startPlayed(ticket, at) }
                    } else ticket.startPending = false
                }
            }
            while (!ticket.finish && !ticket.cancelled && count < pcm.size) {
                val n = record.read(pcm, count, minOf(block, pcm.size - count), AudioRecord.READ_BLOCKING)
                check(n > 0)
                // The start sound and START_GUARD_EXTRA_MS after it are silence to Whisper (0 with the lab off).
                val muted = startMuteSamples(ticket.startPending, ticket.muteEndsAt, recordingStarted, rate)
                if (count < muted) pcm.fill(0, count, minOf(count + n, muted))
                var energy = 0.0
                for (i in count until count + n) energy += pcm[i].toDouble() * pcm[i]
                val rms = sqrt(energy / n) / 32768.0
                if (rms > .004) voiced += n
                edges.block(count, n, rms > .004)
                count += n
                val now = SystemClock.elapsedRealtime()
                if (now - lastUpdate >= 75) {
                    lastUpdate = now
                    main.post { if (active === ticket && phase == SpeechPhase.Listening) meter(rms.toFloat()) }
                }
            }
        } finally {
            try {
                if (record.recordingState == AudioRecord.RECORDSTATE_RECORDING) record.stop()
            } finally { record.release() }
        }
        ticket.stoppedAt = System.nanoTime()
        // Labs › Dictation sounds: the stop sound once the microphone is closed, so the recording never holds it; a
        // dictation whose start sound never played (stopped before the microphone opened) stays silent.
        if (ticket.sounds && !ticket.cancelled) main.post {
            if (ticket.startSounded && !ticket.cancelled) { ticket.stopSoundAt = SystemClock.uptimeMillis(); DictationSounds.play(SoundCue.Stop) }
        }
        ticket.recorded = count
        val span = speechSpan(edges.first, edges.last, count, rate)
        // Only with "setprop log.tag.HexSpeech DEBUG": how much audio arrived, never the audio or its words.
        if (Log.isLoggable(LOG_TAG, Log.DEBUG)) Log.d(LOG_TAG, "captured $count samples, $voiced voiced, kept ${span.first}..${span.last + 1}")
        if (ticket.cancelled) return
        if (voiced < rate / 10 || count < rate / 4) {
            complete(ticket, "No speech heard")
            return
        }
        // Whisper hears the speech and its margins, at least 1.1 s (zeros after it, as before).
        val kept = span.last + 1 - span.first
        ticket.kept = kept
        if (span.first > 0) pcm.copyInto(pcm, 0, span.first, span.last + 1)
        val padded = maxOf(kept, rate + rate / 10)
        pcm.fill(0, kept, padded)
        transcribe(ticket, pcm, padded, if (count == pcm.size) "Stopped at 1 minute" else null)
    }
}
