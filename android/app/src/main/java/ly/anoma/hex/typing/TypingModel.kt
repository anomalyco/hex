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
                                              private val validWords: ByteBuffer?, list: Reader,
                                              val capitals: Capitals? = null, val spellings: Spellings? = null) : Lexicon {
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
    override val size get() = words
    override fun length(word: Int) = start(word + 1) - start(word)
    fun byte(word: Int, depth: Int): Int {
        val pos = start(word) + depth
        return if (pos < start(word + 1)) u8(blob + pos) else -1
    }
    fun text(word: Int): String = buildString {
        for (at in start(word) until start(word + 1)) append(chars[u8(blob + at)])
    }
    fun frequencyCost(word: Int) = u8(uni + word)
    /** -ln P(word), in nats. */
    fun unigramCost(word: Int) = frequencyCost(word) * 0.125
    /** -ln P(word | prev) from the stored pair, in nats, or -1 when the model has no such pair. */
    fun pairCost(prev: Int, word: Int): Double = pair(prev, word).let { if (it >= 0) u8(bi + it) * 0.125 else -1.0 }
    /** The word's letter codes into [out], at most its size; returns how many there are. Reads the mapping only. */
    fun codes(word: Int, out: IntArray): Int {
        val from = start(word); val n = minOf(start(word + 1) - from, out.size)
        for (i in 0 until n) out[i] = u8(blob + from + i)
        return n
    }
    fun languageCost(prev: Int, word: Int): Double {
        val backoff = 0.92 + frequencyCost(word) * 0.125
        val pair = pair(prev, word)
        return if (pair >= 0) minOf(backoff, u8(bi + pair) * 0.125) else backoff
    }
    // The stored pair's slot, or -1.
    private fun pair(prev: Int, word: Int): Int {
        if (prev < 0) return -1
        var a = int(rows + 4 * prev); val end = int(rows + 4 * (prev + 1)); var b = end
        while (a < b) {
            val m = (a + b) ushr 1
            if (int(seconds + 4 * m) < word) a = m + 1 else b = m
        }
        return if (a < end && int(seconds + 4 * a) == word) a else -1
    }
    /** -ln P(word | previous) with the unigram backoff, or null when the model lacks the word. */
    fun cost(word: String, previous: String?): Double? {
        val w = index(word)
        return if (w < 0) null else languageCost(previous?.let(::index) ?: -1, w)
    }
    fun hasPair(first: String, second: String): Boolean {
        val a = index(first); val b = index(second)
        return a >= 0 && b >= 0 && pair(a, b) >= 0
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
    override fun child(lo: Int, hi: Int, depth: Int, c: Int): Long {
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
    /** The three likeliest allowed words after `previous`; [fill] tops a short list up with the
     * commonest words. */
    fun nextWords(previous: Int, swears: Boolean, fill: Boolean = true): List<String> {
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
        if (fill && result.size < 3) for (i in completions("", swears, previous)) { val w = text(i); if (w !in result) result.add(w); if (result.size == 3) break }
        return result
    }
    /** The two likeliest words that complete a 1–3 letter `prefix` after `previous` (a word index, or
     * -1), by the word pairs with the unigram backoff, at most 8 letters longer (iOS
     * TypingDecoderModel.completions(of:after:), #2125). Walks only the prefix's range and the previous
     * word's pair row, which share their sort order. */
    fun completionsAfter(prefix: String, previous: Int, swears: Boolean): IntArray {
        if (prefix.length !in 1..3) return IntArray(0)
        val r = range(prefix)
        if (r == 0L) return IntArray(0)
        val lo = (r ushr 32).toInt(); val hi = r.toInt()
        var link = 0; var end = 0
        if (previous >= 0) {
            link = int(rows + 4 * previous); end = int(rows + 4 * (previous + 1))
            var upper = end
            while (link < upper) {
                val mid = (link + upper) ushr 1
                if (int(seconds + 4 * mid) < lo) link = mid + 1 else upper = mid
            }
        }
        val context = neverSuggest.restrictedContext(previous)
        val best = IntArray(2) { -1 }; val ranks = IntArray(2) { Int.MAX_VALUE }
        var next = if (link < end) int(seconds + 4 * link) else hi
        for (i in lo until hi) {
            val unigram = u8(uni + i)
            var cost = unigram + 7 // 0.92 nats of unigram backoff, in eighths of a nat.
            if (i == next) {
                cost = minOf(cost, u8(bi + link))
                link++
                next = if (link < end) int(seconds + 4 * link) else hi
            }
            val rank = cost * 256 + unigram
            if (length(i) <= prefix.length || length(i) > prefix.length + 8 || rank >= ranks[1]) continue
            if (neverSuggest.blocks(i, swears, context)) continue
            if (rank < ranks[0]) { best[1] = best[0]; ranks[1] = ranks[0]; best[0] = i; ranks[0] = rank }
            else { best[1] = i; ranks[1] = rank }
        }
        return best.filter { it >= 0 }.toIntArray()
    }
    // exp(-cost) for each stored unigram cost byte (cost = byte / 8 nats).
    private val frequency = DoubleArray(256) { kotlin.math.exp(-it / 8.0) }
    private var wordStart: DoubleArray? = null
    private var lastContinuation: Pair<String, DoubleArray?>? = null
    /** How much of the word list's frequency goes on from `prefix` with each letter, by letter code
     * (iOS TypingDecoderModel.continuations): null when no word starts with it, or every one ends
     * there. The word start's, the costliest (every word), and the last prefix's are kept. */
    fun continuations(prefix: String): DoubleArray? {
        if (prefix.isEmpty()) wordStart?.let { return it }
        lastContinuation?.let { if (it.first == prefix) return it.second }
        val r = range(prefix)
        val mass = if (r == 0L) null else {
            val depth = prefix.length
            val out = DoubleArray(alphabet + 1)
            var any = false
            for (i in (r ushr 32).toInt() until r.toInt()) {
                val code = byte(i, depth)
                if (code <= 0 || code == apostrophe) continue
                out[code] += frequency[u8(uni + i)]
                any = true
            }
            out.takeIf { any }
        }
        if (prefix.isEmpty()) wordStart = mass else lastContinuation = prefix to mass
        return mass
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
    // Public, newline-delimited lowercase spellings outside the word-pair model, sorted by their UTF-8
    // bytes (`<lang>.valid`), each alone (English) or with a tab and its cost in eighths of a nat. Find a
    // literal in the mapped bytes without allocating a heap dictionary or calling an IME service.
    // -1 when it isn't listed, its cost, or [UNCOUNTED].
    private fun validEntry(word: String): Int {
        val data = validWords ?: return -1
        val bytes = word.encodeToByteArray()
        var lo = 0; var hi = data.limit()
        while (lo < hi) {
            var at = (lo + hi) ushr 1
            while (at > lo && data.get(at - 1) != 10.toByte()) at--
            var end = at
            while (data.get(end) != 10.toByte()) end++
            var key = at
            while (key < end && data.get(key) != 9.toByte()) key++
            var cmp = 0
            for (i in 0 until minOf(bytes.size, key - at)) {
                cmp = (data.get(at + i).toInt() and 0xff) - (bytes[i].toInt() and 0xff)
                if (cmp != 0) break
            }
            if (cmp == 0) cmp = (key - at) - bytes.size
            if (cmp == 0) {
                if (key == end) return UNCOUNTED
                var cost = 0
                for (i in key + 1 until end) cost = cost * 10 + (data.get(i) - 48)
                return cost
            }
            if (cmp < 0) lo = end + 1 else hi = at
        }
        return -1
    }
    fun validLiteral(word: String) = validEntry(word) >= 0
    /** What a listed spelling outside the model costs as a word, -ln P with the model's backoff, from
     * how often the books use it; null when it isn't listed or its list has no counts (English). */
    fun validCost(word: String): Double? = validEntry(word).takeIf { it in 0..255 }?.let { 0.92 + it * 0.125 }
    /** Whether a spelling outside the model is a real word: one the language's validity list has
     * (SCOWL for English, the books' attested spellings for the others), or a name the capitals
     * list has. */
    fun known(word: String) = validLiteral(word) || (capitals?.percent(word) ?: -1) >= 0
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
        private const val UNCOUNTED = 256
        private fun mapped(channel: FileChannel, offset: Long, size: Long): ByteBuffer {
            require(size in 1L..32_000_000L)
            return channel.map(FileChannel.MapMode.READ_ONLY, offset, size).order(ByteOrder.LITTLE_ENDIAN)
        }
        private fun open(model: ByteBuffer, language: String, valid: ByteBuffer?, list: Reader, capitals: ByteBuffer?,
                         spellings: ByteBuffer?): TypingModel {
            require(model.limit() >= 36 && model.getInt(20) in 1..250)
            for (sidecar in listOfNotNull(valid, capitals, spellings)) require(sidecar.get(sidecar.limit() - 1) == 10.toByte())
            return TypingModel(model, language, valid, list, capitals?.let(::Capitals), spellings?.let(::Spellings))
        }
        fun asset(assets: AssetManager, language: String): TypingModel {
            fun read(name: String) = assets.openFd("typing/" + name).use { fd ->
                FileInputStream(fd.fileDescriptor).use { mapped(it.channel, fd.startOffset, fd.declaredLength) }
            }
            val names = assets.list("typing").orEmpty()
            val capitals = if (language + ".capitals" in names) read(language + ".capitals") else null
            val valid = if (language + ".valid" in names) read(language + ".valid") else null
            val spellings = if (language + ".spellings" in names) read(language + ".spellings") else null
            return assets.open("typing/never-suggest.tsv").bufferedReader().use { list ->
                open(read(language + ".hextm"), language, valid, list, capitals, spellings)
            }
        }
        fun file(file: File, language: String): TypingModel {
            fun read(f: File) = FileInputStream(f).use { mapped(it.channel, 0, f.length()) }
            val valid = File(file.parentFile, language + ".valid").takeIf { it.isFile }?.let(::read)
            val capitals = File(file.parentFile, language + ".capitals").takeIf { it.isFile }?.let(::read)
            val spellings = File(file.parentFile, language + ".spellings").takeIf { it.isFile }?.let(::read)
            return File(file.parentFile, "never-suggest.tsv").bufferedReader().use { list ->
                open(read(file), language, valid, list, capitals, spellings)
            }
        }
    }
}
