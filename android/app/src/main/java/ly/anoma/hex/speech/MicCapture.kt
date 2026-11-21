package ly.anoma.hex.speech

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import java.util.concurrent.Executors
import kotlin.math.sqrt

internal enum class SpeechPhase { Idle, Opening, Listening, Transcribing }

// The IME calls start/finish/cancel on the main thread. All audio and inference live on
// one worker. Every posted result belongs to its original ticket, including failures.
internal class MicCapture(
    private val recognizer: Recognizer,
    private val update: (SpeechPhase, String?) -> Unit,
    private val meter: (Float) -> Unit,
    private val insert: (String) -> Unit,
) {
    private class Ticket(val options: SpeechOptions) {
        @Volatile var finish = false
        @Volatile var cancelled = false
    }
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor { Thread(it, "Hex dictation") }
    private var active: Ticket? = null
    var phase = SpeechPhase.Idle
        private set

    fun start(options: SpeechOptions) {
        if (active != null) return
        val ticket = Ticket(options)
        active = ticket
        publish(SpeechPhase.Opening)
        worker.execute {
            try {
                recordAndDecode(ticket)
            } catch (_: SecurityException) {
                complete(ticket, "Allow the microphone in Hex")
            } catch (_: Exception) {
                complete(ticket, "Microphone unavailable. Try again.")
            } catch (_: UnsatisfiedLinkError) {
                complete(ticket, "Speech model unavailable")
            }
        }
    }

    fun finish() {
        val ticket = active ?: return
        ticket.finish = true
        publish(SpeechPhase.Transcribing)
    }

    fun cancel() {
        active?.cancelled = true
        active = null
        publish(SpeechPhase.Idle)
    }

    fun close() { cancel(); worker.shutdownNow() }

    private fun publish(value: SpeechPhase, note: String? = null) {
        phase = value
        update(value, note)
    }

    private fun complete(ticket: Ticket, note: String? = null, text: String? = null) = main.post {
        if (active === ticket) {
            active = null
            publish(SpeechPhase.Idle, note)
            if (text != null) insert(text)
        }
    }

    // No audio-focus request, playback stream, device selection or communication mode.
    // Capture dies before inference starts. No audio or transcript is written to storage.
    @SuppressLint("MissingPermission")
    private fun recordAndDecode(ticket: Ticket) {
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
            .setBufferSizeInBytes(maxOf(minBuffer, block * 2 * 4)).build()
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
        if (ticket.cancelled) return
        if (voiced < rate / 10 || count < rate / 4) {
            complete(ticket, "No speech heard")
            return
        }
        main.post { if (active === ticket && phase != SpeechPhase.Transcribing) publish(SpeechPhase.Transcribing) }
        val options = ticket.options
        val raw = recognizer.decode(pcm, maxOf(count, rate + rate / 10), options.spoken.code, options.translate)
        if (ticket.cancelled) return
        when (val text = raw?.let { SpokenText.format(it, options) }) {
            null -> complete(ticket, "Couldn’t transcribe. Try again.")
            "" -> complete(ticket, "No speech heard")
            else -> complete(ticket, text = text)
        }
    }
}
