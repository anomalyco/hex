package ly.anoma.hex.typing

import java.nio.ByteBuffer

/** How often the books write each word with a capital (`<lang>.capitals`, beside the model): one
 * `word<TAB>percent` line per lowercase word, sorted by its UTF-8 bytes, memory-mapped and searched
 * in place. It lists the model's words written with a capital at least half the time, and names
 * outside the model (`benchmark/typing/build-capitals.py`); German's is iOS's `de.capitals`. */
internal class Capitals(private val data: ByteBuffer) {
    /** The capitalised share (50–100) of `word`, lowercase, or -1 when the list doesn't have it. */
    fun percent(word: String): Int {
        val line = find(data, word)
        if (line == -1L) return -1
        var value = 0
        for (i in (line ushr 32).toInt() + 1 until line.toInt()) value = value * 10 + (data.get(i) - ZERO)
        return value
    }

    private companion object {
        const val ZERO: Byte = 48
    }
}

/** The model's words that have a spelling of their own (`<lang>.spellings`, beside the model, written by
 * `benchmark/typing/build-typing-model.py`): one `word<TAB>Spelling` line per lowercase word, sorted
 * as `.capitals` is and searched in place the same way ("chatgpt" -> "ChatGPT", "figma" -> "Figma"). */
internal class Spellings(private val data: ByteBuffer) {
    /** `word`'s spelling, `word` lowercase, or null when the list doesn't have it. */
    fun of(word: String): String? {
        val line = find(data, word)
        if (line == -1L) return null
        val from = (line ushr 32).toInt() + 1
        val bytes = ByteArray(line.toInt() - from)
        for (i in bytes.indices) bytes[i] = data.get(from + i)
        return String(bytes, Charsets.UTF_8)
    }
}

private const val NEWLINE: Byte = 10
private const val TAB: Byte = 9

/** `word`'s line in a sorted `word<TAB>value` list, as (its tab shl 32) or its newline; -1 when absent. */
private fun find(data: ByteBuffer, word: String): Long {
    val key = word.toByteArray(Charsets.UTF_8)
    var lo = 0
    var hi = data.limit()
    while (lo < hi) {
        var at = (lo + hi) ushr 1
        while (at > lo && data.get(at - 1) != NEWLINE) at--
        var tab = at
        while (data.get(tab) != TAB) tab++
        var end = tab
        while (data.get(end) != NEWLINE) end++
        var cmp = 0
        for (i in 0 until minOf(key.size, tab - at)) {
            cmp = (data.get(at + i).toInt() and 255) - (key[i].toInt() and 255)
            if (cmp != 0) break
        }
        if (cmp == 0) cmp = (tab - at) - key.size
        if (cmp == 0) return (tab.toLong() shl 32) or end.toLong()
        if (cmp < 0) lo = end + 1 else hi = at
    }
    return -1
}
