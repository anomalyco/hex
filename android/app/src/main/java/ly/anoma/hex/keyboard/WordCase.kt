package ly.anoma.hex.keyboard

/** Labs › Keyboard › Shift changes a word's case (#3008), as Microsoft SwiftKey's shift does: a tap at a word cycles
 * it lowercase → Title → ALL CAPS → lowercase, and a word cased any other way ("iPhone") goes to Title. Each step
 * changes letters one for one, so the word keeps its length and the cursor its place ("ß" stays "ß" in capitals). */
internal object WordCase {
    /** The word around the cursor: [before] is its part before the cursor (at least one letter), [after] the rest. */
    data class Span(val before: String, val after: String) {
        val word get() = before + after
    }

    private fun inWord(c: Char) = c.isLetter() || c == '\'' || c == '\u2019'

    /** The word the cursor is at the end of or inside, read from the text either side of it; null with no letter right
     * before the cursor (before a word's first letter, after its space or mark), so shift is as before there. An
     * apostrophe belongs to the word only between letters ("don't", not "'quoted'"). A word run against a digit or
     * an underscore ("mp3", "snake_case") is left alone: it is no word to recase. */
    fun at(before: String, after: String): Span? {
        var start = before.length
        while (start > 0 && inWord(before[start - 1])) start--
        var head = before.substring(start)
        while (head.isNotEmpty() && !head[0].isLetter()) { head = head.drop(1); start++ }
        if (head.isEmpty() || !head.last().isLetter()) return null
        var end = 0
        while (end < after.length && inWord(after[end])) end++
        var tail = after.substring(0, end)
        while (tail.isNotEmpty() && !tail.last().isLetter()) { tail = tail.dropLast(1); end-- }
        if (start > 0 && joined(before[start - 1])) return null
        if (end < after.length && joined(after[end])) return null
        return Span(head, tail)
    }

    private fun joined(c: Char) = c.isLetterOrDigit() || c == '_'

    private fun lower(word: String) = word.map { it.lowercaseChar() }.joinToString("")
    private fun upper(word: String) = word.map { it.uppercaseChar() }.joinToString("")
    private fun title(word: String): String {
        val first = word.indexOfFirst { it.isLetter() }
        if (first < 0) return word
        return word.mapIndexed { i, c -> if (i == first) c.titlecaseChar() else c.lowercaseChar() }.joinToString("")
    }

    /** The word's next case. A step that would leave it as it is (a single letter's Title and ALL CAPS) is skipped. */
    fun next(word: String): String {
        val steps = listOf(lower(word), title(word), upper(word))
        val at = steps.indexOf(word)
        if (at < 0) return steps[1]
        for (n in 1..steps.size) {
            val candidate = steps[(at + n) % steps.size]
            if (candidate != word) return candidate
        }
        return word
    }

    /** Whether the live text [before] the cursor still ends in the word a recase locked shift for, which started
     * [length] characters before the cursor as last known: a run of letters (and apostrophes) ending in a letter, at
     * least that long. The cursor may be reported late, after more letters went in, so a longer run still counts; a
     * shorter one means a space, a mark or a cursor moved elsewhere ended the word. A run filling all of [before] counts
     * too: the field starts there, or the word is longer than was read. */
    fun continues(before: String, length: Int): Boolean {
        if (length <= 0 || before.lastOrNull()?.isLetter() != true) return false
        var start = before.length
        while (start > 0 && inWord(before[start - 1])) start--
        return before.length - start >= length
    }

    /** Whether [word] is in capitals throughout, with two or more letters: shift stays locked after it. */
    fun allCaps(word: String) = word.count { it.isLetter() } >= 2 && word == upper(word) && word != lower(word)
}
