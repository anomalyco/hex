package ly.anoma.hex.typing

import android.content.ContentResolver
import android.provider.UserDictionary

/** A word list sorted by letter codes, searched letter by letter: the model's, or your own. */
internal interface Lexicon {
    val size: Int
    fun length(word: Int): Int
    /** The words in [lo, hi), which share their first [depth] letters, whose next letter is [code],
     * as (first shl 32) or end; 0 when none. */
    fun child(lo: Int, hi: Int, depth: Int, code: Int): Long
}

/** Your own words and text shortcuts from Android's personal dictionary (`UserDictionary.Words`),
 * as iOS reads `UILexicon`: the decoder reads taps as them (a word you added is a word, scored as a
 * moderately common one), the strip completes them, a word typed in lowercase takes the capitals you
 * gave it ("github" -> "GitHub") unless it is also an ordinary word ("will"), and a shortcut is
 * replaced by its phrase (`replacement`). Words in the model's alphabet only; nothing is written. */
internal class PersonalWords(private val model: TypingModel, words: Collection<String>, shortcuts: Map<String, String>) : Lexicon {
    private val codes: Array<ByteArray>
    private val texts: Array<String>
    private val lookup = HashMap<String, Int>()
    // Lowercase shortcut -> phrase as stored.
    private val shortcuts = shortcuts.mapKeys { it.key.lowercase() }

    init {
        val byCodes = LinkedHashMap<String, Pair<ByteArray, String>>()
        for (word in words) {
            val lower = word.lowercase()
            if (lower.length !in 2..TypingWord.MAX_LENGTH || lower.any { !it.isLetter() && it != '\'' }) continue
            val encoded = ByteArray(lower.length)
            var ok = true
            for ((i, c) in lower.withIndex()) { val code = model.code(c); if (code == 0) { ok = false; break }; encoded[i] = code.toByte() }
            if (ok) byCodes.putIfAbsent(lower, encoded to word)
        }
        val sorted = byCodes.values.sortedWith { a, b -> compare(a.first, b.first) }
        codes = Array(sorted.size) { sorted[it].first }
        texts = Array(sorted.size) { sorted[it].second }
        for ((i, t) in texts.withIndex()) lookup[t.lowercase()] = i
    }

    val isEmpty get() = codes.isEmpty() && shortcuts.isEmpty()
    /** Your words, spelled your way. */
    val words: List<String> get() = texts.asList()
    override val size get() = codes.size
    override fun length(word: Int) = codes[word].size
    private fun byte(word: Int, depth: Int) = if (depth < codes[word].size) codes[word][depth].toInt() and 255 else -1
    override fun child(lo: Int, hi: Int, depth: Int, code: Int): Long {
        var a = lo; var b = hi
        while (a < b) { val m = (a + b) ushr 1; if (byte(m, depth) < code) a = m + 1 else b = m }
        if (a == hi || byte(a, depth) != code) return 0
        var c = a + 1; var d = hi
        while (c < d) { val m = (c + d) ushr 1; if (byte(m, depth) <= code) c = m + 1 else d = m }
        return (a.toLong() shl 32) or c.toLong()
    }
    /** Your word with these letters, or -1. */
    fun index(spelling: String) = lookup[spelling.lowercase()] ?: -1
    /** Your spelling of a word, capitals and all. */
    fun text(word: Int) = texts[word]
    /** Whether `spelling` is one of your words or a shortcut's phrase, in any case. */
    fun owns(spelling: String) = index(spelling) >= 0 || shortcuts.values.any { it.equals(spelling, ignoreCase = true) }

    /** Your words that start with `prefix` and are longer, in your spelling. */
    fun completions(prefix: String): List<String> {
        val lower = prefix.lowercase()
        return texts.filter { it.length > prefix.length && it.lowercase().startsWith(lower) }.sortedBy { it.length }
    }

    /** Your phrase for a shortcut typed as [typed], cased as Apple's keyboard writes it (#2153, #2173):
     * a shortcut typed with a leading capital, but not all capitals, capitalises the phrase's first
     * letter; stored capitals always stay. */
    fun shortcut(typed: String): String? {
        val stored = shortcuts[typed.lowercase()] ?: return null
        val letters = typed.filter { it.isLetter() }
        val leading = typed.first().isUpperCase() && (letters.length < 2 || letters != letters.uppercase())
        return if (leading && stored.first().isLowerCase()) stored.replaceFirstChar { it.uppercase() } else stored
    }

    /** The text replacement whose shortcut ends `before` (the text before the cursor), as space, a
     * closing mark or return finds it (iOS AutocorrectEngine.textReplacement): the longest one at a word
     * boundary, digits and dots allowed ("ad2", "s.o"), after a bracket or comma ("(myem", "hi,myem"),
     * never the end of a longer word ("xmyem"). Null when none ends the text. */
    fun replacement(before: String): Correction? {
        if (shortcuts.isEmpty() || before.lastOrNull()?.isWhitespace() != false) return null
        val chunk = before.takeLast(64).takeLastWhile { !it.isWhitespace() }
        fun joins(c: Char) = c.isLetterOrDigit() || c == '\'' || c == '\u2019'
        for (start in chunk.indices) {
            if (start > 0 && joins(chunk[start - 1]) && joins(chunk[start])) continue
            val typed = chunk.substring(start)
            shortcut(typed)?.let { return Correction(typed, it) }
        }
        return null
    }

    companion object {
        private fun compare(a: ByteArray, b: ByteArray): Int {
            for (i in 0 until minOf(a.size, b.size)) {
                val d = (a[i].toInt() and 255) - (b[i].toInt() and 255)
                if (d != 0) return d
            }
            return a.size - b.size
        }
        private const val LIMIT = 5_000

        /** Whether a personal dictionary entry's locale ("en_US", "nb_NO", none for every language)
         * applies to the model's language; Bokmål also takes "no". */
        fun applies(locale: String?, language: String) = locale.isNullOrEmpty() ||
            locale.substringBefore('_').lowercase().let { it == language || (language == "nb" && it == "no") }

        /** The personal dictionary's words and shortcuts for the model's language (or every language).
         * Android lets only an enabled input method or spell checker read it, with no permission, and
         * answers anyone else with nothing; any failure reads as nothing. Read it off the main thread. */
        fun read(resolver: ContentResolver, model: TypingModel): PersonalWords {
            val words = ArrayList<String>()
            val shortcuts = HashMap<String, String>()
            try {
                resolver.query(UserDictionary.Words.CONTENT_URI,
                    arrayOf(UserDictionary.Words.WORD, UserDictionary.Words.SHORTCUT, UserDictionary.Words.LOCALE),
                    null, null, null)?.use { cursor ->
                    while (cursor.moveToNext() && words.size + shortcuts.size < LIMIT) {
                        if (!applies(cursor.getString(2), model.language)) continue
                        val word = cursor.getString(0)?.trim().orEmpty()
                        if (word.isEmpty()) continue
                        val shortcut = cursor.getString(1)?.trim().orEmpty()
                        if (shortcut.isNotEmpty() && !shortcut.equals(word, ignoreCase = true)) shortcuts[shortcut] = word
                        else words.add(word)
                    }
                }
            } catch (_: RuntimeException) {
                return PersonalWords(model, emptyList(), emptyMap())
            }
            return PersonalWords(model, words, shortcuts)
        }
    }
}
