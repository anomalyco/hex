package ly.anoma.hex.typing

import java.nio.ByteBuffer

/** How often the books write each word with a capital (`<lang>.capitals`, beside the model): one
 * `word<TAB>percent` line per lowercase word, sorted by its UTF-8 bytes, memory-mapped and searched
 * in place. It lists the model's words written with a capital at least half the time, and names
 * outside the model (`benchmark/typing/build-capitals.py`); German's is iOS's `de.capitals`. */
internal class Capitals(private val data: ByteBuffer) {
    /** The capitalised share (50–100) of `word`, lowercase, or -1 when the list doesn't have it. */
    fun percent(word: String): Int {
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
            if (cmp == 0) {
                var value = 0
                for (i in tab + 1 until end) value = value * 10 + (data.get(i) - ZERO)
                return value
            }
            if (cmp < 0) lo = end + 1 else hi = at
        }
        return -1
    }

    private companion object {
        const val NEWLINE: Byte = 10
        const val TAB: Byte = 9
        const val ZERO: Byte = 48
    }
}
