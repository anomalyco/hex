package ly.anoma.hex.typing

import java.text.Normalizer

/** Labs › Keyboard › Emoji search: the emoji panel's English search over each emoji's name and keywords, as the
 * iPhone panel's (EmojiLibrary.search), from the generated asset ("emoji<TAB>name words|keywords", in the iPhone
 * catalog's order). Nothing typed into it is kept. */
internal class EmojiSearch private constructor(private val emoji: List<String>, private val names: List<List<String>>,
                                               private val keywords: List<List<String>>) {
    val size get() = emoji.size

    /** The first [count] emoji in catalog order, the smileys first: what an empty search offers after your recents. */
    fun first(count: Int): List<String> = emoji.take(count)

    /** [value]'s English name ("red heart"), for TalkBack, or null when it is not in the index. */
    fun name(value: String): String? = index[value]?.let { names[it].joinToString(" ") }

    /** Whether [value] is in the index (an emoji this phone can draw). */
    operator fun contains(value: String) = value in index

    private val index = emoji.withIndex().associate { (i, e) -> e to i }

    /** The emoji [query] finds, best first: each of its words must start a word of the emoji's name or keywords. A
     * whole name word scores 4, a name prefix 3, a whole keyword 2 and a keyword prefix 1; among equals a short name
     * the query fills ("red heart") beats a long one it only touches, then [boost] (your use, 0 to 1), then catalog
     * order. The iPhone's ranking, term for term. */
    fun search(query: String, limit: Int = 80, boost: (String) -> Float = { 0f }): List<String> {
        val terms = words(query).sortedByDescending { it.length }
        if (terms.isEmpty()) return emptyList()
        val scored = ArrayList<Pair<Int, Float>>()
        for (i in emoji.indices) {
            var total = 0f
            var all = true
            for (term in terms) {
                var best = 0
                for (word in names[i]) if (word.startsWith(term)) { best = maxOf(best, if (word.length == term.length) 4 else 3); if (best == 4) break }
                if (best < 3) for (word in keywords[i]) if (word.startsWith(term)) { best = maxOf(best, if (word.length == term.length) 2 else 1); if (best == 2) break }
                if (best == 0) { all = false; break }
                total += best
            }
            if (!all) continue
            val nameWords = maxOf(1, names[i].size)
            val fill = 0.3f * minOf(terms.size, nameWords) / nameWords
            scored.add(i to total + fill + 0.6f * minOf(1f, boost(emoji[i])))
        }
        scored.sortWith(compareByDescending<Pair<Int, Float>> { it.second }.thenBy { it.first })
        return scored.take(limit).map { emoji[it.first] }
    }

    companion object {
        const val ASSET = "emoji/search-en.txt"

        /** The emoji people send most, roughly (iPhone Hex's EmojiSuggester.defaults): before you have used any they
         * lead an empty search, and among equal matches they rank as half your most used. */
        val EVERYDAY = listOf("😂", "❤️", "👍", "😊", "🙏", "😍", "🎉", "🔥", "😭", "🥰", "😘", "😅")

        /** Reads [lines], skipping comments and blanks; [drawable] keeps only emoji the device can draw (the keyboard
         * passes `Paint.hasGlyph`), and the first line for an emoji wins. */
        fun parse(lines: Sequence<String>, drawable: (String) -> Boolean = { true }): EmojiSearch {
            val emoji = ArrayList<String>(2048); val names = ArrayList<List<String>>(2048); val keywords = ArrayList<List<String>>(2048)
            val seen = HashSet<String>()
            for (line in lines) {
                if (line.isEmpty() || line[0] == '#') continue
                val tab = line.indexOf('\t')
                if (tab <= 0 || tab == line.lastIndex) continue
                val e = line.substring(0, tab)
                if (e in seen || !drawable(e)) continue
                seen.add(e)
                val text = line.substring(tab + 1)
                val bar = text.indexOf('|').let { if (it < 0) text.length else it }
                emoji.add(e)
                names.add(text.substring(0, bar).split(' ').filter { it.isNotEmpty() })
                keywords.add(if (bar < text.length) text.substring(bar + 1).split(' ').filter { it.isNotEmpty() } else emptyList())
            }
            return EmojiSearch(emoji, names, keywords)
        }

        private val marks = Regex("\\p{Mn}+")

        /** Folded words, as the iPhone's EmojiText.words: accents dropped, lowercased, curly apostrophes straightened,
         * split on anything but letters, digits and apostrophes, an apostrophe's tail dropped ("that's" → "that"). */
        fun words(text: String): List<String> =
            marks.replace(Normalizer.normalize(text.replace('\u2019', '\''), Normalizer.Form.NFD), "").lowercase()
                .split(Regex("[^\\p{L}\\p{N}']+"))
                .mapNotNull { part -> part.substringBefore('\'').takeIf { it.isNotEmpty() } }
    }
}
