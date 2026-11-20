package ly.anoma.hex.typing

import java.io.Reader

/** These are output flags, not dictionary deletions. Index only the small curated list;
 * beam search and completion scans check a byte without decoding every spelling. */
internal class NeverSuggest(model: TypingModel, source: Reader) {
    private val wordFlags = ByteArray(model.words)
    private val spellings = HashMap<String, Byte>() // Also protects literals outside HXTM.
    private class Following {
        val ids = HashMap<Int, Byte>()
        val spellings = HashMap<String, Byte>()
    }
    private val following = HashMap<Int, Following>()
    private val unknownStarts = HashMap<String, Int>()

    init {
        source.buffered().useLines { lines ->
            for (line in lines) {
                if (line.isBlank() || line.startsWith('#')) continue
                val (language, kind, spelling) = line.split('\t')
                if (language != "*" && language != model.language) continue
                val flag: Byte = when (kind) {
                    "swear" -> SWEAR
                    "slur" -> SLUR
                    else -> error("Unknown never-suggest category: $kind")
                }
                val space = spelling.indexOf(' ')
                if (space < 0) {
                    spellings[spelling] = flag
                    val id = model.index(spelling)
                    if (id >= 0) wordFlags[id] = flag
                } else {
                    // Flag the exact pair without banning its ordinary component words.
                    val start = spelling.substring(0, space)
                    val end = spelling.substring(space + 1)
                    val id = model.index(start).takeIf { it >= 0 }
                        ?: unknownStarts.getOrPut(start) { -2 - unknownStarts.size }
                    val after = following.getOrPut(id) { Following() }
                    after.spellings[end] = flag
                    val next = model.index(end)
                    if (next >= 0) after.ids[next] = flag
                }
            }
        }
    }

    // Negative IDs still use HXTM's unknown-word probability, including a typed
    // phrase starter outside the model. Most previous words share the same prefix cache.
    fun context(spelling: String, id: Int) = if (id >= 0) id else unknownStarts[spelling] ?: -1
    fun restrictedContext(previous: Int) = if (previous in following) previous else -1
    private fun denied(flag: Byte, swears: Boolean) = flag == SLUR || (!swears && flag == SWEAR)
    fun blocksFollowing(word: Int, swears: Boolean, previous: Int) =
        previous != -1 && denied(following[previous]?.ids?.get(word) ?: 0, swears)
    fun blocks(word: Int, swears: Boolean, previous: Int = -1) =
        denied(wordFlags[word], swears) || blocksFollowing(word, swears, previous)
    fun blocks(spelling: String, swears: Boolean, previous: Int) =
        denied(spellings[spelling] ?: 0, swears) ||
            (previous != -1 && denied(following[previous]?.spellings?.get(spelling) ?: 0, swears))

    private companion object {
        const val SWEAR: Byte = 1
        const val SLUR: Byte = 2
    }
}
