package ly.anoma.hex.speech

import android.content.Context
import android.util.Log
import ly.anoma.hex.BuildConfig
import java.time.LocalDate
import java.util.Locale

/** Why a dictation didn't count towards Labs › Pick the English model for this phone, in the order the rule checks. */
internal enum class NotCounted { OtherLanguage, TooShort, ModelLoading, Retried }

/** What Labs › Pick the English model for this phone keeps on the phone: durations and counts only, never text, audio or a
 * language. [ratios]: the last [ModelChoice.SAMPLES] counted dictations' seconds of decoding per second of speech kept,
 * oldest first; [counted]: how many counted; [englishInARow]: dictations in a row that would go to faster English dictation
 * if it were on, up to [ModelChoice.WINDOW]; [skipped]: how many didn't count, by reason; [decidedDay] (epoch day) and
 * [decidedMedian]: when it chose faster English dictation, and on what. */
internal data class ModelChoiceState(val ratios: List<Double> = emptyList(), val counted: Int = 0, val englishInARow: Int = 0,
                                     val skipped: Map<NotCounted, Int> = emptyMap(), val decidedDay: Long? = null,
                                     val decidedMedian: Double? = null) {
    val decided get() = decidedDay != null
}

/** One dictation as the rule sees it: whether it would go to faster English dictation were it on, the speech kept, the
 * decode's wall time, the model's state at ✓ and whether hexspeech.cpp decoded it again without the person's words. */
internal class ChoiceSample(val englishRoutable: Boolean, val keptMs: Long, val decodeMs: Long, val model: ModelAtTick,
                            val retried: Boolean)

/** What opening Hex does about the rule's choice. */
internal enum class OpenAction { Nothing, TurnOnAndDownload, Download }

/** Labs › Pick the English model for this phone (off by default): Hex times English dictations on the standard model and,
 * when they are slow on this phone, turns on faster English dictation and downloads its model the next time Hex is opened,
 * waiting for Wi-Fi. One way: once chosen it stays chosen, and only the person turns it off. The keyboard only records
 * ([after]); acting is [whenOpened]'s, from a Hex screen. */
internal object ModelChoice {
    /** Seconds of decoding per second of speech, on the median of the last [SAMPLES]: at or above it, it is slow here. */
    const val THRESHOLD = 0.5
    const val SAMPLES = 5
    /** English-only: this many dictations in a row would all go to faster English dictation. */
    const val WINDOW = 20
    /** Shorter dictations are mostly fixed costs (an emulator's 0.37 at 12.6 s is 0.6 on short clips). */
    const val MIN_KEPT_MS = 6_000L

    /** [times] as the rule sees it, or null when it isn't measured at all: faster English dictation was on when it started
     * ([SpeechOptions.fastEnglish]), another model than the standard one wrote it, or no text went in. [scale] multiplies
     * the decode's time (debug builds' HexDecodeScale; 1 elsewhere). */
    fun sample(times: DictationTimes, options: SpeechOptions, scale: Double = 1.0): ChoiceSample? {
        if (options.fastEnglish || times.ran != RanOn.Standard || times.insertedMs == null) return null
        val retried = times.timings?.let { it.size >= 10 && it[8] > 0 } ?: false
        return ChoiceSample(engineFor(options.copy(fastEnglish = true)) == Engine.English, times.keptMs,
            ((times.decodeEndedMs - times.decodeStartedMs) * scale).toLong(), times.model, retried)
    }

    /** Why [s] doesn't count, or null when it does. A model that finished loading before ✓ (already loaded, or loaded while
     * the person spoke) counts: the decode's wall time holds no loading. */
    fun notCounted(s: ChoiceSample): NotCounted? = when {
        !s.englishRoutable -> NotCounted.OtherLanguage
        s.keptMs < MIN_KEPT_MS -> NotCounted.TooShort
        s.model != ModelAtTick.Loaded && s.model != ModelAtTick.LoadedWhileSpeaking -> NotCounted.ModelLoading
        s.retried -> NotCounted.Retried
        else -> null
    }

    /** Whether a dictation is measured now: the lab is on, faster English dictation wasn't turned off by hand, and nothing is
     * decided yet. */
    fun measures(labOn: Boolean, offByHand: Boolean, state: ModelChoiceState) = labOn && !offByHand && !state.decided

    /** The state after [s], on [today] (epoch day): the one place the rule decides. */
    fun after(state: ModelChoiceState, s: ChoiceSample, today: Long): ModelChoiceState {
        if (state.decided) return state
        val why = notCounted(s)
        val inARow = if (s.englishRoutable) minOf(WINDOW, state.englishInARow + 1) else 0
        val next = if (why != null) state.copy(englishInARow = inARow, skipped = state.skipped + (why to (state.skipped[why] ?: 0) + 1))
            else state.copy(englishInARow = inARow, counted = state.counted + 1,
                ratios = (state.ratios + round(s.decodeMs.toDouble() / s.keptMs)).takeLast(SAMPLES))
        val median = median(next.ratios)
        return if (next.ratios.size >= SAMPLES && next.englishInARow >= WINDOW && median != null && median >= THRESHOLD)
            next.copy(decidedDay = today, decidedMedian = median) else next
    }

    /** What opening Hex does ([ModelChoiceStore.opened], from MainActivity's onResume only): nothing with the lab off,
     * before a choice, or after faster English dictation was turned off by hand; else turn faster English dictation on and
     * download its model, or carry on a download that isn't running ([downloading]) or done. */
    fun whenOpened(labOn: Boolean, state: ModelChoiceState, fastEnglish: Boolean, offByHand: Boolean,
                   installed: Boolean, downloading: Boolean): OpenAction = when {
        !labOn || !state.decided || offByHand -> OpenAction.Nothing
        !fastEnglish -> OpenAction.TurnOnAndDownload
        !installed && !downloading -> OpenAction.Download
        else -> OpenAction.Nothing
    }

    /** Faster English dictation's by-hand off after the lab is switched: turning it on is a new request, so an earlier off
     * (perhaps from before the lab existed) no longer holds it back; turning it off changes nothing. */
    fun offByHandWhenLabTurned(on: Boolean, offByHand: Boolean) = if (on) false else offByHand

    /** The same after faster English dictation's own switch or Delete download: off is the person's choice, kept for good. */
    fun offByHandWhenFasterEnglishTurned(on: Boolean, offByHand: Boolean) = if (on) offByHand else true

    fun median(values: List<Double>): Double? {
        if (values.isEmpty()) return null
        val sorted = values.sorted()
        val mid = sorted.size / 2
        return if (sorted.size % 2 == 1) sorted[mid] else (sorted[mid - 1] + sorted[mid]) / 2
    }

    private fun round(value: Double) = Math.round(value * 100) / 100.0

    fun ratiosText(ratios: List<Double>) = ratios.joinToString(",") { String.format(Locale.ROOT, "%.2f", it) }
    fun parseRatios(text: String?) = text.orEmpty().split(',').mapNotNull { it.trim().toDoubleOrNull() }.takeLast(SAMPLES)
}

/** Labs › Pick the English model for this phone's numbers, in their own private file ([FILE]), which turning the lab off
 * deletes. The switch itself and faster English dictation's by-hand off are settings, in "speech". */
internal class ModelChoiceStore(context: Context) {
    companion object {
        const val FILE = "modelChoice"
        /** Debug builds only: "adb shell setprop log.tag.HexDecodeScale DEBUG" counts each decode as 10× as long, VERBOSE as
         * half, so an emulator can act out a slow or a fast phone. */
        fun decodeScale(): Double = if (!BuildConfig.DEBUG) 1.0 else when {
            Log.isLoggable("HexDecodeScale", Log.VERBOSE) -> 0.5
            Log.isLoggable("HexDecodeScale", Log.DEBUG) -> 10.0
            else -> 1.0
        }
    }
    private val app = context.applicationContext
    private val prefs = app.getSharedPreferences(FILE, Context.MODE_PRIVATE)
    private val speech = SpeechPreferences(app)

    val state: ModelChoiceState get() = ModelChoiceState(
        ModelChoice.parseRatios(prefs.getString("ratios", null)), prefs.getInt("counted", 0), prefs.getInt("englishInARow", 0),
        NotCounted.entries.associateWith { prefs.getInt(it.key, 0) }.filterValues { it > 0 },
        prefs.getLong("decidedDay", -1).takeIf { it >= 0 }, prefs.getString("decidedMedian", null)?.toDoubleOrNull())

    /** A dictation finished: measured only while the lab is on and nothing is decided. Durations and counts only. */
    fun dictated(times: DictationTimes, options: SpeechOptions) {
        val now = state
        if (!ModelChoice.measures(speech.pickModel, speech.fastEnglishOffByHand, now)) return
        val sample = ModelChoice.sample(times, options, decodeScale()) ?: return
        val next = ModelChoice.after(now, sample, LocalDate.now().toEpochDay())
        val edit = prefs.edit().putString("ratios", ModelChoice.ratiosText(next.ratios)).putInt("counted", next.counted)
            .putInt("englishInARow", next.englishInARow)
        NotCounted.entries.forEach { edit.putInt(it.key, next.skipped[it] ?: 0) }
        if (next.decidedDay != null) edit.putLong("decidedDay", next.decidedDay)
            .putString("decidedMedian", String.format(Locale.ROOT, "%.2f", next.decidedMedian ?: 0.0))
        edit.apply()
    }

    /** Hex was opened ([MainActivity]'s onResume, never the keyboard or an [OfflineScreen]): acts on a choice. The download
     * is [ModelStore.english]'s, which waits for Wi-Fi on a network that charges for data. */
    fun opened() {
        val store = ModelStore.english
        val action = ModelChoice.whenOpened(speech.pickModel, state, speech.fastEnglish, speech.fastEnglishOffByHand,
            store.installed(app) != null, store.state.value.let { it is ModelStore.State.Loading || it == ModelStore.State.WaitingForWifi })
        if (action == OpenAction.TurnOnAndDownload) speech.fastEnglish = true
        if (action != OpenAction.Nothing) store.download(app)
    }

    /** The lab turned off: its numbers go, file and all. A setting or download it already made stays, as Labs shows it. */
    fun clear() {
        prefs.edit().clear().commit()
        app.deleteSharedPreferences(FILE)
    }

    private val NotCounted.key get() = when (this) {
        NotCounted.OtherLanguage -> "otherLanguage"
        NotCounted.TooShort -> "tooShort"
        NotCounted.ModelLoading -> "modelLoading"
        NotCounted.Retried -> "retried"
    }
}
