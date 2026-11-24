package ly.anoma.hex.speech

import kotlin.math.sqrt

/** Faster English dictation's model sometimes runs two sentences together ("back of the bus We should", "Wednesday My
 * brother") while still capitalising the second one's first word. Where that word is one English writes in lowercase
 * (in the English typing model and not on its capitals list, so not a name, and not "I") and the speaker paused before
 * it, the first sentence gets its period back. Only the faster engine's text goes through this, before Hex's shared
 * formatting; the dictation's last period stays Polished's job.
 *
 * On every English clip of the benchmark (README › Faster English dictation) this restored 9 of the 10 sentence ends
 * the model dropped and put no period inside a sentence for any pause from 60 to 150 ms; [PAUSE_MS] is that band's
 * middle. Without the pause, words that are also names ("on Main Street", "at Target", "in New York") got stray
 * periods; a capital alone after a pause added nothing on these clips, so names are left as they are. */
internal object SentenceEnds {
    const val PAUSE_MS = 100
    /** RMS a quiet 20 ms frame stays under: MicCapture's voiced test, −48 dBFS. */
    private const val QUIET = .004
    private const val FRAME = 320
    private val I_FORMS = setOf("i", "i'm", "i'll", "i've", "i'd")

    /** One word of the model's output and where it starts and ends, in ms of the audio it was given. */
    data class Word(val text: String, val startMs: Int, val endMs: Int)

    /** [decoded] is the faster engine's output (Parakeet.decode): its text, then a "start\tend\tword" line per word.
     * Returns the text with its run-together sentences split, or the text as it came when its words don't spell it
     * exactly. [lowercase] says whether English writes a word, given in lowercase, in lowercase. */
    fun restore(decoded: String, pcm: ShortArray, count: Int, lowercase: (String) -> Boolean): String {
        val lines = decoded.split('\n')
        val text = lines.first().trim()
        val words = lines.drop(1).mapNotNull { line ->
            val parts = line.split('\t', limit = 3)
            if (parts.size != 3) null else Word(parts[2], parts[0].toIntOrNull() ?: return@mapNotNull null,
                parts[1].toIntOrNull() ?: return@mapNotNull null)
        }
        if (words.isEmpty() || words.joinToString(" ") { it.text } != text) return text
        return words.mapIndexed { i, word ->
            val next = words.getOrNull(i + 1)
            if (next != null && endsSentence(word, next, pcm, count, lowercase)) word.text + "." else word.text
        }.joinToString(" ")
    }

    private fun endsSentence(word: Word, next: Word, pcm: ShortArray, count: Int, lowercase: (String) -> Boolean): Boolean {
        if (word.text.lastOrNull()?.let { it in ".?!,;:" } != false) return false
        val first = next.text.firstOrNull() ?: return false
        if (!first.isUpperCase()) return false
        val letters = next.text.filter(Char::isLetter)
        if (letters.length > 1 && letters.all(Char::isUpperCase)) return false
        val plain = next.text.lowercase().filter { it in 'a'..'z' || it in '0'..'9' || it == '\'' }
        if (plain.isEmpty() || plain in I_FORMS || !lowercase(plain)) return false
        return quietMs(pcm, count, word.startMs, next.startMs) >= PAUSE_MS
    }

    /** The longest run of quiet 20 ms frames between [fromMs] and [toMs] (16 kHz). */
    fun quietMs(pcm: ShortArray, count: Int, fromMs: Int, toMs: Int): Int {
        var best = 0; var run = 0
        var at = fromMs * 16
        val end = minOf(toMs * 16, count)
        while (at < end - FRAME) {
            var energy = 0.0
            for (i in at until at + FRAME) energy += pcm[i].toDouble() * pcm[i]
            if (sqrt(energy / FRAME) / 32768.0 < QUIET) { run += 20; best = maxOf(best, run) } else run = 0
            at += FRAME
        }
        return best
    }
}
