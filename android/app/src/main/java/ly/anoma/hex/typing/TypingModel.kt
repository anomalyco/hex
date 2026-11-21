package ly.anoma.hex.typing

import android.content.res.AssetManager
import java.io.File
import java.io.FileInputStream
import java.io.Reader
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.channels.FileChannel

/** HXTM v3: sorted letter-code spellings, quantized unigram costs and CSR bigram rows.
 * All large tables stay in the read-only file mapping, including when mapped inside the APK. */
internal class TypingModel private constructor(private val data: ByteBuffer, val language: String,
                                              private val validWords: ByteBuffer?, list: Reader) {
    private fun u8(pos: Int) = data.get(pos).toInt() and 255
    private fun int(pos: Int) = data.getInt(pos)
    val words = int(8)
    val pairs = int(12)
    private val blobBytes = int(16)
    val alphabet = int(20)
    private val chars = CharArray(alphabet + 1)
    val bases = IntArray(alphabet + 1)
    private val codes = HashMap<Char, Int>()
    private val starts = 28 + 8 * alphabet
    private val uni = starts + 4 * (words + 1)
    private val rows = (uni + words + 3) and -4
    private val seconds = rows + 4 * (words + 1)
    private val bi = seconds + 4 * pairs
    private val blob = bi + pairs
    val apostrophe get() = code('\'')
    fun code(c: Char) = codes[c] ?: 0
    fun character(code: Int) = chars[code]
    init {
        require(int(0) == 0x4d545848 && int(4) == 3 && words in 1..250000 && pairs in 0..2000000 &&
            blobBytes > 0 && blob.toLong() + blobBytes == data.limit().toLong())
        val lang = (24..27).map { u8(it) }.filter { it != 0 }.map { it.toChar() }.joinToString("")
        require(lang == language && int(starts) == 0 && int(starts + 4 * words) == blobBytes &&
            int(rows) == 0 && int(rows + 4 * words) == pairs)
        for (i in 0 until alphabet) {
            val p = 28 + i * 8
            val code = u8(p + 4)
            require(code in 1..alphabet && u8(p + 5) in 1..alphabet && int(p) in 1..65535)
            chars[code] = int(p).toChar(); codes[chars[code]] = code; bases[code] = u8(p + 5)
        }
    }
    private fun start(word: Int) = int(starts + 4 * word)
    fun length(word: Int) = start(word + 1) - start(word)
    fun byte(word: Int, depth: Int): Int {
        val pos = start(word) + depth
        return if (pos < start(word + 1)) u8(blob + pos) else -1
    }
    fun text(word: Int): String = buildString {
        for (at in start(word) until start(word + 1)) append(chars[u8(blob + at)])
    }
    fun frequencyCost(word: Int) = u8(uni + word)
    fun languageCost(prev: Int, word: Int): Double {
        val backoff = 0.92 + frequencyCost(word) * 0.125
        if (prev < 0) return backoff
        var a = int(rows + 4 * prev); val end = int(rows + 4 * (prev + 1)); var b = end
        while (a < b) {
            val m = (a + b) ushr 1
            if (int(seconds + 4 * m) < word) a = m + 1 else b = m
        }
        return if (a < end && int(seconds + 4 * a) == word) minOf(backoff, u8(bi + a) * 0.125) else backoff
    }
    fun bound(lo: Int, hi: Int, depth: Int, target: Int): Int {
        var a = lo; var b = hi
        while (a < b) {
            val m = (a + b) ushr 1
            if (byte(m, depth) < target) a = m + 1 else b = m
        }
        return a
    }
    // Binary-search the mapped lexicon. Only the first three levels of boundaries are cached;
    // there is no in-memory copy of the word list.
    private val shallow = HashMap<Int, IntArray>()
    fun child(lo: Int, hi: Int, depth: Int, c: Int): Long {
        val a: Int; val b: Int
        if (hi - lo == 1) return if (byte(lo, depth) == c) (lo.toLong() shl 32) or hi.toLong() else 0
        if (depth < 3 && hi - lo > 8) {
            val bounds = shallow.getOrPut(lo * 3 + depth) {
                val result = IntArray(alphabet + 2)
                var at = lo
                for (i in 1..alphabet + 1) { at = bound(at, hi, depth, i); result[i] = at }
                result
            }
            a = bounds[c]; b = bounds[c + 1]
        } else {
            a = bound(lo, hi, depth, c)
            if (a == hi || byte(a, depth) != c) return 0
            b = bound(a + 1, hi, depth, c + 1)
        }
        return if (a < b) (a.toLong() shl 32) or b.toLong() else 0
    }
    val neverSuggest = NeverSuggest(this, list)
    fun context(spelling: String) = neverSuggest.context(spelling, index(spelling))
    fun nextWords(previous: Int, swears: Boolean): List<String> {
        if (previous < 0) return completions("", swears, previous).map { text(it) }
        val best = ArrayList<Int>(4)
        for (at in int(rows + 4 * previous) until int(rows + 4 * (previous + 1))) {
            if (neverSuggest.blocks(int(seconds + 4 * at), swears, previous)) continue
            val cost = u8(bi + at)
            val i = best.indexOfFirst { u8(bi + it) > cost }
            if (i >= 0) best.add(i, at) else if (best.size < 3) best.add(at)
            if (best.size > 3) best.removeAt(3)
        }
        val result = best.mapTo(ArrayList()) { text(int(seconds + 4 * it)) }
        if (result.size < 3) for (i in completions("", swears, previous)) { val w = text(i); if (w !in result) result.add(w); if (result.size == 3) break }
        return result
    }
    private fun range(prefix: String): Long {
        var a = 0; var b = words
        for (i in prefix.indices) {
            val c = code(prefix[i]); if (c == 0) return 0
            a = bound(a, b, i, c); b = bound(a, b, i, c + 1)
            if (a == b) return 0
        }
        return (a.toLong() shl 32) or b.toLong()
    }
    // Public, newline-delimited ASCII spellings outside the word-pair model. Find a
    // literal in the mapped bytes without allocating a heap dictionary or calling an IME service.
    fun validLiteral(word: String): Boolean {
        val data = validWords ?: return false
        var lo = 0; var hi = data.limit()
        while (lo < hi) {
            var at = (lo + hi) ushr 1
            while (at > lo && data.get(at - 1) != 10.toByte()) at--
            var end = at
            while (data.get(end) != 10.toByte()) end++
            var cmp = 0
            for (i in 0 until minOf(word.length, end - at)) {
                cmp = data.get(at + i).toInt() - word[i].code
                if (cmp != 0) break
            }
            if (cmp == 0) cmp = (end - at) - word.length
            if (cmp == 0) return true
            if (cmp < 0) lo = end + 1 else hi = at
        }
        return false
    }
    fun index(word: String): Int {
        val r = range(word); val a = (r ushr 32).toInt()
        return if (r != 0L && length(a) == word.length) a else -1
    }
    // Common prefixes get reused on consecutive words. No corpus-sized heap trie.
    private data class Prefix(val text: String, val swears: Boolean, val context: Int)
    private val prefixCache = object : LinkedHashMap<Prefix, IntArray>(96, 0.75f, true) {
        override fun removeEldestEntry(eldest: Map.Entry<Prefix, IntArray>?) = size > 96
    }
    fun completions(prefix: String, swears: Boolean, previous: Int): IntArray {
        val key = Prefix(prefix, swears, neverSuggest.restrictedContext(previous))
        return prefixCache.getOrPut(key) { rankCompletions(key) }
    }
    private fun rankCompletions(key: Prefix): IntArray {
        val r = range(key.text)
        val best = IntArray(3) { -1 }
        for (word in (r ushr 32).toInt() until r.toInt()) {
            if (neverSuggest.blocks(word, key.swears, key.context)) continue
            val f = frequencyCost(word)
            for (i in best.indices) {
                if (best[i] == -1 || f < frequencyCost(best[i])) {
                    for (j in best.lastIndex downTo i+1) best[j] = best[j-1]
                    best[i] = word; break
                }
            }
        }
        val count = best.indexOf(-1)
        return if (count < 0) best else best.copyOf(count)
    }
    companion object {
        private fun mapped(channel: FileChannel, offset: Long, size: Long): ByteBuffer {
            require(size in 1L..32_000_000L)
            return channel.map(FileChannel.MapMode.READ_ONLY, offset, size).order(ByteOrder.LITTLE_ENDIAN)
        }
        private fun open(model: ByteBuffer, language: String, valid: ByteBuffer?, list: Reader): TypingModel {
            require(model.limit() >= 36 && model.getInt(20) in 1..250)
            require(valid == null || valid.get(valid.limit() - 1) == 10.toByte())
            return TypingModel(model, language, valid, list)
        }
        fun asset(assets: AssetManager, language: String): TypingModel {
            fun read(name: String) = assets.openFd("typing/" + name).use { fd ->
                FileInputStream(fd.fileDescriptor).use { mapped(it.channel, fd.startOffset, fd.declaredLength) }
            }
            return assets.open("typing/never-suggest.tsv").bufferedReader().use { list ->
                open(read(language + ".hextm"), language, if (language == "en") read("en.valid") else null, list)
            }
        }
        fun file(file: File, language: String): TypingModel {
            fun read(f: File) = FileInputStream(f).use { mapped(it.channel, 0, f.length()) }
            val valid = File(file.parentFile, "en.valid")
            return File(file.parentFile, "never-suggest.tsv").bufferedReader().use { list ->
                open(read(file), language, if (language == "en") read(valid) else null, list)
            }
        }
    }
}
