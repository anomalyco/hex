package ly.anoma.hex.keyboard

/** A slide left from delete, as on Gboard (black box, Pixel 8 emulator, API 36): past [start] the
 * word before the cursor is selected, one more every [step]; sliding back selects fewer, and lifting
 * deletes what is selected, or nothing. A press that moved more than [cancel] left types no delete. */
internal class DeleteGesture(private val start: Float, private val step: Float, private val cancel: Float, private val stillSlop: Float) {
    private var originX = 0f
    private var farthest = 0f
    private var moved = 0f
    fun down(x: Float) { originX = x; farthest = 0f; moved = 0f }
    /** How many words the finger asks for now. */
    fun move(x: Float): Int {
        val distance = originX - x
        farthest = maxOf(farthest, distance)
        moved = maxOf(moved, kotlin.math.abs(distance))
        return if (distance < start) 0 else ((distance - start) / step).toInt() + 1
    }
    /** Held still enough to repeat (Gboard stops repeating once the finger moves). */
    val still get() = moved < stillSlop
    /** Lifted without selecting: whether it still counts as a tap on delete. */
    val tap get() = farthest < cancel
}

/** Selects whole words back from the cursor while a delete slide runs. Words are what Option-Delete
 * takes on a Mac: the spaces and punctuation before the cursor go with the word in front of them. */
internal class DeleteSlide(private val target: KeyboardTextTarget, private val cursor: Int, before: String) {
    private val lengths = wordLengths(before)
    var words = 0
        private set

    fun select(count: Int) {
        val next = count.coerceIn(0, lengths.size)
        if (next == words) return
        words = next
        target.setSelection(if (next == 0) cursor else cursor - lengths[next - 1], cursor)
    }

    companion object {
        /** The UTF-16 length of the last character of [before] as people see it (a grapheme cluster). */
        fun lastCharacter(before: String): Int {
            if (before.isEmpty()) return 0
            val it = java.text.BreakIterator.getCharacterInstance().apply { setText(before) }
            return before.length - it.preceding(before.length)
        }
        /** Cumulative UTF-16 lengths of the last words of [before], nearest first. */
        fun wordLengths(before: String): IntArray {
            val out = ArrayList<Int>()
            var i = before.length
            while (i > 0) {
                while (i > 0 && !isWordCharacter(before, i)) i = previous(before, i)
                while (i > 0 && isWordCharacter(before, i)) i = previous(before, i)
                out.add(before.length - i)
            }
            return out.toIntArray()
        }
        private fun previous(text: String, i: Int) = if (i >= 2 && Character.isLowSurrogate(text[i - 1]) && Character.isHighSurrogate(text[i - 2])) i - 2 else i - 1
        /** Letters, digits, emoji and the apostrophe inside "don't" belong to a word. */
        private fun isWordCharacter(text: String, end: Int): Boolean {
            val c = text.codePointBefore(end)
            return Character.isLetterOrDigit(c) || c == '_'.code || c == '\''.code || c == '’'.code ||
                Character.getType(c) == Character.OTHER_SYMBOL.toInt() || Character.getType(c) == Character.NON_SPACING_MARK.toInt()
        }
    }
}

/** Labs › Keyboard › Delete words when held, as SwiftKey's held delete: after the usual hold
 * ([KeyboardMetrics.DELETE_REPEAT_DELAY]) the word before the cursor goes at once, then one more word after
 * [FIRST_GAP] ms, each next gap [SPEEDUP] shorter than the one before, never under [SHORTEST_GAP] ms:
 * 500, 400, 320, 256… ms. A word is what a delete slide takes ([DeleteSlide.wordLengths]). */
internal object HeldDeleteWords {
    const val FIRST_GAP = 500L
    const val SPEEDUP = 0.2
    const val SHORTEST_GAP = 90L
    /** The gap before the word after one that waited [gap] ms. */
    fun next(gap: Long): Long = maxOf(SHORTEST_GAP, Math.round(gap * (1 - SPEEDUP)))
}
