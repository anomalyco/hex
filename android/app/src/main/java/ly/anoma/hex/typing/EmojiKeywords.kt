package ly.anoma.hex.typing

import ly.anoma.hex.speech.Spoken

/** English words that name an emoji (Labs › Keyboard › Emoji suggestions), folded the way the
 * iPhone keyboard folds them. Built from the generated asset, so a lookup is one whole-word,
 * case-insensitive map read: "smile" -> 😊, "haha" -> 😂, "heart" -> ❤️. */
internal class EmojiKeywords(private val words: Map<String, String>) {
    /** [word]'s emoji for a typing model in [language], or null. The index is English only
     * ([Spoken.English.layoutCode], the language of `en.hextm`), so another layout's model never
     * offers an emoji for a word that merely looks English. The current word is matched whole,
     * never as a prefix, so continuing to type keeps the word. */
    fun match(word: String, language: String): String? =
        if (language == Spoken.English.layoutCode) lookup(word) else null

    private fun lookup(word: String): String? = if (word.isEmpty()) null else words[word.lowercase()]

    companion object {
        const val ASSET = "emoji/suggestions-en.txt"

        /** Reads [lines] ("word<TAB>emoji"), skipping comments and blanks. The first spelling wins,
         * so a bad duplicate never changes an established word. [drawable] keeps only emoji the
         * device can draw (the keyboard passes `Paint.hasGlyph`); it defaults to keeping all. */
        fun parse(lines: Sequence<String>, drawable: (String) -> Boolean = { true }): EmojiKeywords {
            val words = HashMap<String, String>(4096)
            for (line in lines) {
                if (line.isEmpty() || line[0] == '#') continue
                val tab = line.indexOf('\t')
                if (tab <= 0 || tab == line.lastIndex) continue
                val emoji = line.substring(tab + 1)
                if (drawable(emoji)) words.putIfAbsent(line.substring(0, tab), emoji)
            }
            return EmojiKeywords(words)
        }
    }
}
