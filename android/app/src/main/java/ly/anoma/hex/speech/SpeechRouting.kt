package ly.anoma.hex.speech

/** The model a dictation runs on: today's ([Standard]) or Labs › Faster English dictation's ([English]). */
internal enum class Engine { Standard, English }

/** Faster English dictation takes English, and only English, decided from the options captured when the dictation
 * started, so changing the language before it finishes doesn't move it. Paste in English (translating Spanish) stays
 * with today's model: the faster one only writes the English it hears. So does Automatic choosing among several
 * languages (Labs › Automatic listens for the language): which one was spoken is known only once today's model heard it. */
internal fun engineFor(options: SpeechOptions): Engine =
    if (options.fastEnglish && options.spoken == DictationLanguage.English && !options.translate && options.candidates.size <= 1)
        Engine.English else Engine.Standard

/** Why the faster model couldn't run: no verified file (missing, or failing its SHA-256), Android short of memory for
 * it (before loading it, or its load ran out), or a load that failed otherwise. */
internal enum class Unready { NoFile, LowMemory, LoadFailed }

/** What the faster model did with a dictation. */
internal sealed interface EnglishResult {
    data class Text(val text: String) : EnglishResult
    data class Unavailable(val why: Unready) : EnglishResult
    /** It ran and failed, or the person discarded the dictation while it ran. */
    data object Failed : EnglishResult
}

/** The model that wrote a dictation, for Labs › Diagnostics' last dictation: today's, the faster one, or today's after the
 * faster one couldn't run ([Unready] says why) or failed. */
internal enum class RanOn { Standard, Faster, NoFile, LowMemory, LoadFailed, FasterFailed }

/** What Labs › Automatic listens for the language chose for a dictation among [candidates] (the keyboard's language first):
 * the one it was written in ([chosen], an index), each candidate's probability among them and the choice's microseconds.
 * [probabilities] is null when there was one candidate, so nothing was chosen. */
internal class LanguageChoice(val candidates: List<DictationLanguage>, val chosen: Int, val probabilities: List<Double>?,
                              val micros: Long = 0) {
    val language get() = candidates[chosen]
}

/** A dictation's text (null when it failed), the model that wrote it and, with Labs › Automatic listens for the language,
 * what it chose. */
internal data class Decoded(val text: String?, val ran: RanOn = RanOn.Standard, val choice: LanguageChoice? = null) {
    /** A shortage of memory sent it to today's model. */
    val lowMemory get() = ran == RanOn.LowMemory
}

/** Runs the faster model on [pcm] when [engine] says so. When it can't, or it fails, the same audio goes to today's
 * model, so nothing said is lost; only a dictation the person discarded ([abandoned]) is not tried again. */
internal fun transcribe(engine: Engine, pcm: ShortArray, count: Int, abandoned: () -> Boolean,
                        english: (ShortArray, Int) -> EnglishResult, standard: (ShortArray, Int) -> String?): Decoded {
    if (engine == Engine.Standard) return Decoded(standard(pcm, count))
    return when (val result = english(pcm, count)) {
        is EnglishResult.Text -> Decoded(result.text, RanOn.Faster)
        is EnglishResult.Unavailable -> Decoded(standard(pcm, count), when (result.why) {
            Unready.NoFile -> RanOn.NoFile
            Unready.LowMemory -> RanOn.LowMemory
            Unready.LoadFailed -> RanOn.LoadFailed
        })
        EnglishResult.Failed -> if (abandoned()) Decoded(null, RanOn.Faster) else Decoded(standard(pcm, count), RanOn.FasterFailed)
    }
}

/** Custom vocabulary for today's model: Whisper's initial prompt, as it has always been. The faster model takes the
 * terms themselves. */
internal fun whisperPrompt(vocabulary: List<String>) = vocabulary.takeIf { it.isNotEmpty() }?.joinToString(", ")

/** A dictation shows one note. The 1-minute stop ([recording]) explains words that are missing, so it wins. */
internal fun dictationNote(recording: String?, engine: String?) = recording ?: engine
