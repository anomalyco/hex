package ly.anoma.hex.keyboard

import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.KeyPoint
import ly.anoma.hex.typing.SpatialDecoder
import ly.anoma.hex.typing.Suggestion
import ly.anoma.hex.typing.TypingWord

/** Labs › Keyboard › Fix earlier words (#3019): with the cursor back in a word already finished, by deleting back into it
 * or tapping it, the strip offers what autocorrect changed it from and other words its letters could be that fit the
 * words on both sides better. A tap replaces the whole word. Off, none of this is read or kept. */
internal class EarlierWords {
    /** An autocorrection made in this field: [original] as typed became [replacement]. One word each. */
    data class Fix(val language: String, val original: String, val replacement: String)

    /** The word around the cursor: [left] before it and [right] after it, as written. [previous] and [next] are the words
     * on either side, lowercase with a straight apostrophe and none at their end, or null after punctuation or at the
     * text's ends. */
    data class Spot(val left: String, val right: String, val previous: String?, val next: String?) {
        val word get() = left + right
    }

    /** What the strip offers for [spot]: the original of a fix first, then the other readings. [fix] is the fix whose
     * original is offered, so choosing it blocks that fix as Delete's undo does. */
    data class Offer(val spot: Spot, val words: List<String>, val fix: Fix?)

    private val fixes = ArrayDeque<Fix>()
    /** The offer the strip shows, while it does. */
    var shown: Offer? = null

    /** An autocorrection went in. Only one word replacing one word is kept, the last [KEPT] of them. */
    fun fixed(language: String, original: String, replacement: String) {
        if (original.isEmpty() || replacement.isEmpty() || ' ' in original || ' ' in replacement) return
        fixes.removeAll { it.replacement == replacement }
        fixes.addLast(Fix(language, original, replacement))
        while (fixes.size > KEPT) fixes.removeFirst()
    }

    /** That fix was undone, or its original chosen: it is no longer offered. */
    fun forget(fix: Fix) { fixes.remove(fix) }

    /** A new field: its fixes go with it. */
    fun clear() { fixes.clear(); shown = null }

    /** What to offer for the word around the cursor, or null: [decoder] reads the other words, [grid] its keys. The strip
     * has [Suggestion.SLOTS] slots, so at most that many words in all, the fix's original first. */
    fun offer(spot: Spot, language: String, decoder: SpatialDecoder, grid: KeyGrid, swears: Boolean): Offer? {
        val word = spot.word
        val fix = fixes.lastOrNull { it.language == language && it.replacement == word }
        val model = decoder.model
        // A word with capitals inside it ("iPhone", "GitHub") or all capitals is written that way on purpose.
        val readings = if (word.drop(1).any { it.isUpperCase() }) emptyList() else
            decoder.alternatives(word, grid, model.context(spot.previous.orEmpty()), spot.next?.let(model::index) ?: -1, swears)
        val words = LinkedHashSet<String>()
        fix?.let { words.add(it.original) }
        for (r in readings) if (words.size < Suggestion.SLOTS) words.add(cased(word, r))
        words.remove(word)
        if (words.isEmpty()) return null
        return Offer(spot, words.toList(), fix?.takeIf { it.original in words })
    }

    companion object {
        /** How many fixes a field keeps. */
        const val KEPT = 20

        private fun wordChar(c: Char) = c.isLetter() || c == '\'' || c == '\u2019'

        /** The word the cursor is in or right after, from the text [before] and [after] it, or null: no word there, or one
         * that is part of a handle, path, number, address or link (a digit, @, #, /, _, - or a dot joined to it). */
        fun spot(before: String, after: String): Spot? {
            var left = before.takeLastWhile(::wordChar)
            var right = after.takeWhile(::wordChar)
            // An apostrophe at either end is a quote, not the word's.
            val leadingQuotes = left.takeWhile { !it.isLetter() }.length
            left = left.drop(leadingQuotes)
            if (left.isEmpty() && right.isNotEmpty() && !right.first().isLetter()) return null
            // Right after a closing quote ("'teh'", "dogs'", "dogs’") the cursor is past the word, not in it: offering
            // there would replace the quote too.
            if (right.isEmpty() && left.lastOrNull()?.isLetter() == false) return null
            right = right.dropLastWhile { !it.isLetter() }
            val word = left + right
            if (word.isEmpty() || !word.first().isLetter()) return null
            val rest = before.dropLast(left.length)
            val tail = after.drop(right.length)
            val joinedBefore = rest.lastOrNull()?.let { it.isDigit() || it in JOINED }
            val joinedAfter = tail.firstOrNull()?.let { it.isDigit() || it in JOINED || (it == '.' && tail.getOrNull(1)?.isLetterOrDigit() == true) }
            if (joinedBefore == true || joinedAfter == true) return null
            val previous = TypingWord.previousWord(rest).takeIf { word ->
                word.any { it.isLetter() } && rest.trimEnd().lastOrNull()?.let(::wordChar) == true
            }
            val nextText = tail.trimStart(' ')
            val next = nextText.takeWhile(::wordChar).takeIf { it.isNotEmpty() && nextText.length < tail.length }
                ?.lowercase()?.replace('\u2019', '\'')?.trimEnd('\'')
            return Spot(left, right, previous, next)
        }

        // Marks that make a word part of something else: a handle, a hashtag, a path, a file or variable name, an address.
        private const val JOINED = "@#/\\_-:=+&*<>[]{}|~`$%"

        /** [reading], the model's lowercase word, as [word] was written: all capitals, or a capital first letter. */
        fun cased(word: String, reading: String) = when {
            word.length >= 2 && word.all { !it.isLetter() || it.isUpperCase() } -> reading.uppercase()
            word.first().isUpperCase() -> reading.replaceFirstChar { it.uppercase() }
            else -> reading
        }

        /** Keys for reading a word's letters when the keyboard has measured none yet: [rows] drawn at an even pitch, each
         * row centred, as the letters are. Only how near the keys are to each other matters. */
        fun grid(rows: List<String>): KeyGrid {
            val pitchX = 39.5; val pitchY = 56.0
            val width = rows.maxOf { it.length }
            return KeyGrid(buildMap {
                for ((row, letters) in rows.withIndex()) for ((i, c) in letters.withIndex()) if (c.isLetter())
                    put(c, KeyPoint(pitchX * (width / 2.0 + i - (letters.length - 1) / 2.0), pitchY * (row + .5)))
            }, pitchX, pitchY)
        }
    }
}
