package ly.anoma.hex.speech

/** The model a dictation runs on: today's ([Standard]) or Labs › Faster English dictation's ([English]). */
internal enum class Engine { Standard, English }

/** Faster English dictation takes English, and only English, decided from the options captured when the dictation
 * started, so changing the language before it finishes doesn't move it. Paste in English (translating Spanish) stays
 * with today's model: the faster one only writes the English it hears. */
internal fun engineFor(options: SpeechOptions): Engine =
    if (options.fastEnglish && options.spoken == DictationLanguage.English && !options.translate) Engine.English else Engine.Standard

/** What the faster model did with a dictation. */
internal sealed interface EnglishResult {
    data class Text(val text: String) : EnglishResult
    /** It couldn't run: no verified file (missing, or failing its SHA-256) or a failed load; [memory] when Android was
     * short of memory for it. */
    data class Unavailable(val memory: Boolean) : EnglishResult
    /** It ran and failed, or the person discarded the dictation while it ran. */
    data object Failed : EnglishResult
}

/** A dictation's text (null when it failed), and whether a shortage of memory sent it to today's model. */
internal data class Decoded(val text: String?, val lowMemory: Boolean = false)

/** Runs the faster model on [pcm] when [engine] says so. When it can't, or it fails, the same audio goes to today's
 * model, so nothing said is lost; only a dictation the person discarded ([abandoned]) is not tried again. */
internal fun transcribe(engine: Engine, pcm: ShortArray, count: Int, abandoned: () -> Boolean,
                        english: (ShortArray, Int) -> EnglishResult, standard: (ShortArray, Int) -> String?): Decoded {
    if (engine == Engine.Standard) return Decoded(standard(pcm, count))
    return when (val result = english(pcm, count)) {
        is EnglishResult.Text -> Decoded(result.text)
        is EnglishResult.Unavailable -> Decoded(standard(pcm, count), result.memory)
        EnglishResult.Failed -> if (abandoned()) Decoded(null) else Decoded(standard(pcm, count))
    }
}

/** Custom vocabulary for today's model: Whisper's initial prompt, as it has always been. The faster model takes the
 * terms themselves. */
internal fun whisperPrompt(vocabulary: List<String>) = vocabulary.takeIf { it.isNotEmpty() }?.joinToString(", ")

/** A dictation shows one note. The 1-minute stop ([recording]) explains words that are missing, so it wins. */
internal fun dictationNote(recording: String?, engine: String?) = recording ?: engine
