package ly.anoma.hex.typing

/** The emoji a shortcode can name (Labs › Keyboard › Emoji shortcodes), read from the bundled
 * `emoji/shortcodes-en.txt` (android/scripts/build-emoji-shortcodes.py, from iPhone Hex's English
 * emoji index): each emoji's family and search words (its name's words, "|", its keywords), and
 * the names and aliases a word or phrase is exactly. Immutable once parsed. */
internal class EmojiShortcodeIndex private constructor(
    private val emoji: List<String>,
    private val families: IntArray,
    /** Each emoji's search words; those before [nameWords] are its name's. */
    private val searchWords: List<Array<String>>,
    private val nameWords: IntArray,
    private val words: Map<String, List<Hit>>,
    private val phrases: Map<String, List<Hit>>,
) {
    data class Hit(val entry: Int, val weight: Float)

    val size get() = emoji.size
    fun emoji(entry: Int) = emoji[entry]

    /** Emoji whose names differ only by who does it ("man running", "woman running") share a
     * family, so the strip shows one of them; -1 for none. */
    fun family(entry: Int) = families[entry]

    /** The emoji a folded word or phrase names exactly, as written in the index. */
    fun hits(word: String): List<Hit> = (if (' ' in word) phrases[word] else words[word]).orEmpty()

    /** The emoji whose name or keywords each of [query]'s words starts: whole name words first,
     * then name prefixes, then keywords, each in the index's order, a short name the query fills
     * ("red heart") before a long one it only touches. iPhone Hex's EmojiLibrary.search. */
    fun search(query: String, limit: Int): List<Int> {
        val terms = query.lowercase().split(Regex("[^a-z0-9]+")).filter { it.isNotEmpty() }
            .sortedByDescending { it.length }
        if (terms.isEmpty()) return emptyList()
        val scored = ArrayList<Pair<Int, Float>>()
        for (entry in emoji.indices) {
            val all = searchWords[entry]
            val names = nameWords[entry]
            var total = 0f
            var matched = true
            for (term in terms) {
                var best = 0f
                for ((i, word) in all.withIndex()) {
                    if (!word.startsWith(term)) continue
                    best = maxOf(best, (if (word.length == term.length) 2f else 1f) + (if (i < names) 2f else 0f))
                    if (best == 4f) break
                }
                if (best == 0f) { matched = false; break }
                total += best
            }
            if (!matched) continue
            val fill = 0.3f * minOf(terms.size, maxOf(names, 1)) / maxOf(names, 1)
            scored.add(entry to total + fill)
        }
        scored.sortWith(compareByDescending<Pair<Int, Float>> { it.second }.thenBy { it.first })
        return scored.take(limit).map { it.first }
    }

    companion object {
        const val ASSET = "emoji/shortcodes-en.txt"

        /** Reads [lines]. [drawable] keeps only emoji the device can draw (the keyboard passes
         * `Paint.hasGlyph`, so an emoji newer than the phone's font never shows as tofu); an emoji
         * left out keeps its line number, so the tables still point at the right ones. */
        fun parse(lines: Sequence<String>, drawable: (String) -> Boolean = { true }): EmojiShortcodeIndex {
            val emoji = ArrayList<String>(2048)
            val families = ArrayList<Int>(2048)
            val search = ArrayList<Array<String>>(2048)
            val names = ArrayList<Int>(2048)
            // The file's entry numbers to ours; -1 for an emoji this phone doesn't draw.
            val remap = ArrayList<Int>(2048)
            val words = HashMap<String, List<Hit>>(1024)
            val phrases = HashMap<String, List<Hit>>(2048)
            val tables = ArrayList<Triple<Char, String, String>>(4096)
            for (line in lines) {
                if (line.length < 3 || line[1] != '\t') continue
                when (line[0]) {
                    'E' -> {
                        val fields = line.substring(2).split('\t')
                        if (fields.size != 4) continue
                        if (!drawable(fields[0])) { remap.add(-1); continue }
                        remap.add(emoji.size)
                        emoji.add(fields[0])
                        families.add(fields[2].toIntOrNull() ?: -1)
                        val name = fields[3].substringBefore('|').split(' ').filter { it.isNotEmpty() }
                        val keywords = fields[3].substringAfter('|', "").split(' ').filter { it.isNotEmpty() }
                        search.add((name + keywords).toTypedArray())
                        names.add(name.size)
                    }
                    'W', 'P' -> {
                        val tab = line.indexOf('\t', 2)
                        if (tab > 2) tables.add(Triple(line[0], line.substring(2, tab), line.substring(tab + 1)))
                    }
                }
            }
            // The tables come after every emoji, but they are read only once all are numbered.
            for ((kind, key, postings) in tables) {
                val hits = postings.split(',').mapNotNull { posting ->
                    val colon = posting.indexOf(':')
                    val file = posting.substring(0, maxOf(colon, 0)).toIntOrNull() ?: return@mapNotNull null
                    val entry = remap.getOrNull(file)?.takeIf { it >= 0 } ?: return@mapNotNull null
                    Hit(entry, posting.substring(colon + 1).toFloatOrNull() ?: return@mapNotNull null)
                }
                if (hits.isNotEmpty()) (if (kind == 'W') words else phrases)[key] = hits
            }
            return EmojiShortcodeIndex(emoji, families.toIntArray(), search, names.toIntArray(), words, phrases)
        }
    }
}

/** Labs › Keyboard › Emoji shortcodes, as iPhone Hex's: the ":name" right before the cursor and the
 * emoji it names. A token is a colon and a name after whitespace or at the start of the field, so a
 * colon inside a word ("abc:pizza") or a time ("10:30") is not one. An exact name or alias comes
 * first, then the names and keywords the name starts, one emoji per family. */
internal object EmojiShortcode {
    /** The shortest name a token holds: two letters or digits, so ":pi" suggests and ":p" is just a colon. */
    const val MINIMUM_NAME = 2
    /** The strip shows at most this many. */
    const val LIMIT = 3
    /** Names and aliases weigh this much or more; keywords less (iPhone Hex's strong line). */
    private const val STRONG = 0.85f

    /** A ":name" right before the cursor: [name] as typed, [text] the whole token a pick takes back. */
    data class Token(val name: String) {
        val text get() = ":$name"
        /** The name in the index's words: lowercase, with "_", "+" and "-" as spaces
         * ("sweat_smile" is "sweat smile" there). */
        val lookup get() = name.lowercase().split('_', '+', '-').filter { it.isNotEmpty() }.joinToString(" ")
    }

    /** The token at the end of [before], or null. The name starts with an ASCII letter or digit and
     * holds at least [MINIMUM_NAME] of them, then only those, "_", "+" and "-", so chat emoticons
     * (":-D", ":+1") stay text. */
    fun token(before: String?): Token? {
        if (before == null) return null
        var start = before.length
        while (start > 0 && nameCharacter(before[start - 1])) start--
        if (start == 0 || before[start - 1] != ':') return null
        if (start >= 2 && !before[start - 2].isWhitespace()) return null
        val name = before.substring(start)
        if (name.isEmpty() || !alphanumeric(name[0]) || name.count(::alphanumeric) < MINIMUM_NAME) return null
        return Token(name)
    }

    private fun alphanumeric(c: Char) = c in 'a'..'z' || c in 'A'..'Z' || c in '0'..'9'
    private fun nameCharacter(c: Char) = alphanumeric(c) || c == '_' || c == '+' || c == '-'

    /** The strip's emoji for [token], best first: the exact names and aliases, then the names and
     * keywords it starts, one per family. */
    fun matches(token: Token, index: EmojiShortcodeIndex, limit: Int = LIMIT): List<String> {
        val picked = ArrayList<Int>(limit)
        val families = HashSet<Int>()
        fun take(entry: Int) {
            if (picked.size >= limit || entry in picked) return
            val family = index.family(entry)
            if (family >= 0 && !families.add(family)) return
            picked.add(entry)
        }
        exact(token, index).forEach(::take)
        if (picked.size < limit) index.search(token.lookup, limit + 8).forEach(::take)
        return picked.map(index::emoji)
    }

    /** The emoji a name typed in full names, for its closing ":": only a name or alias, never a prefix. */
    fun completion(token: Token, index: EmojiShortcodeIndex): String? = exact(token, index).firstOrNull()?.let(index::emoji)

    private fun exact(token: Token, index: EmojiShortcodeIndex): List<Int> {
        val name = token.lookup
        if (name.isEmpty()) return emptyList()
        return index.hits(name).filter { it.weight >= STRONG }
            .sortedWith(compareByDescending<EmojiShortcodeIndex.Hit> { it.weight }.thenBy { it.entry })
            .map { it.entry }
    }
}
