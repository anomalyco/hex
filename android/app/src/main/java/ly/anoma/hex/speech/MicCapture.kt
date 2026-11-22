package ly.anoma.hex.speech

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import java.util.concurrent.Executors
import kotlin.math.sqrt

internal enum class SpeechPhase { Idle, Opening, Listening, Transcribing }

private const val LOG_TAG = "HexSpeech"

// The IME calls start/finish/cancel on the main thread. One worker records, another transcribes,
// so a dictation can start while the one before is still transcribing (as desktop Hex queues
// them); results are inserted in the order they were spoken. Every posted result belongs to its
// original ticket, including failures.
internal class MicCapture(
    private val recognizer: Recognizer,
    private val update: (SpeechPhase, String?) -> Unit,
    private val meter: (Float) -> Unit,
    private val insert: (String, SpeechOptions) -> Unit,
) {
    private class Ticket(val options: SpeechOptions) {
        @Volatile var finish = false
        @Volatile var cancelled = false
        var done = false
        var note: String? = null
        var text: String? = null
    }
    private val main = Handler(Looper.getMainLooper())
    private val recorder = Executors.newSingleThreadExecutor { Thread(it, "Hex dictation") }
    private val transcriber = Executors.newSingleThreadExecutor { Thread(it, "Hex transcription") }
    /** The dictation holding the microphone. */
    private var active: Ticket? = null
    /** Dictations that stopped recording, oldest first, until their text is inserted. */
    private val queue = ArrayDeque<Ticket>()
    var phase = SpeechPhase.Idle
        private set
    /** Whether a new dictation can start: nothing holds the microphone. */
    val canStart get() = active == null

    fun start(options: SpeechOptions) {
        if (active != null) return
        val ticket = Ticket(options)
        active = ticket
        publish(SpeechPhase.Opening)
        recorder.execute {
            try {
                record(ticket)
            } catch (_: SecurityException) {
                complete(ticket, "Allow the microphone in Hex")
            } catch (_: Exception) {
                complete(ticket, "Microphone unavailable. Try again.")
            }
        }
    }

    fun finish() {
        val ticket = active ?: return
        ticket.finish = true
        publish(SpeechPhase.Transcribing)
    }

    /** The ✕: discards the dictation recording now, else the ones still transcribing. Earlier
     * dictations still arrive when a later one is discarded while it records. */
    fun cancelLatest() {
        val ticket = active
        if (ticket != null) {
            ticket.cancelled = true
            active = null
            publish(current())
        } else cancel()
    }

    /** Leaving the field: nothing recording or transcribing may reach another editor. */
    fun cancel() {
        active?.cancelled = true
        active = null
        queue.forEach { it.cancelled = true }
        queue.clear()
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
    private fun complete(ticket: Ticket, note: String? = null, text: String? = null) = main.post {
        if (active === ticket) active = null
        if (ticket.cancelled) { publish(current()); return@post }
        ticket.done = true; ticket.note = note; ticket.text = text
        if (ticket !in queue) queue.addLast(ticket)
        var shown: String? = null
        while (queue.firstOrNull()?.done == true) {
            val next = queue.removeFirst()
            shown = next.note ?: shown
            next.text?.let { insert(it, next.options) }
        }
        publish(current(), shown)
    }

    /** Recording stopped with audio to transcribe: queue it behind earlier dictations. */
    private fun transcribe(ticket: Ticket, pcm: ShortArray, count: Int) = main.post {
        if (active === ticket) active = null
        if (ticket.cancelled) { publish(current()); return@post }
        queue.addLast(ticket)
        publish(current())
        transcriber.execute {
            try {
                if (ticket.cancelled) return@execute
                val options = ticket.options
                val raw = recognizer.decode(pcm, count, options.spoken.code, options.translate,
                    options.vocabulary.takeIf { it.isNotEmpty() }?.joinToString(", "))
                if (ticket.cancelled) return@execute
                when (val text = raw?.let { SpokenText.format(it, options) }) {
                    null -> complete(ticket, "Couldn’t transcribe. Try again.")
                    "" -> complete(ticket, "No speech heard")
                    else -> complete(ticket, text = text)
                }
            } catch (_: UnsatisfiedLinkError) {
                complete(ticket, "Speech model unavailable")
            } catch (_: Exception) {
                complete(ticket, "Couldn’t transcribe. Try again.")
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
        var lastUpdate = 0L
        try {
            check(record.state == AudioRecord.STATE_INITIALIZED)
            if (!ticket.cancelled && !ticket.finish) {
                record.startRecording()
                check(record.recordingState == AudioRecord.RECORDSTATE_RECORDING)
                main.post { if (active === ticket && !ticket.finish) publish(SpeechPhase.Listening) }
            }
            while (!ticket.finish && !ticket.cancelled && count < pcm.size) {
                val n = record.read(pcm, count, minOf(block, pcm.size - count), AudioRecord.READ_BLOCKING)
                check(n > 0)
                var energy = 0.0
                for (i in count until count + n) energy += pcm[i].toDouble() * pcm[i]
                val rms = sqrt(energy / n) / 32768.0
                if (rms > .004) voiced += n
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
        // Only with "setprop log.tag.HexSpeech DEBUG": how much audio arrived, never the audio or its words.
        if (Log.isLoggable(LOG_TAG, Log.DEBUG)) Log.d(LOG_TAG, "captured $count samples, $voiced voiced")
        if (ticket.cancelled) return
        if (voiced < rate / 10 || count < rate / 4) {
            complete(ticket, "No speech heard")
            return
        }
        transcribe(ticket, pcm, maxOf(count, rate + rate / 10))
    }
}
