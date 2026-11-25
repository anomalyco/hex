package ly.anoma.hex.typing

/** One letter typed with a tap, for Labs › Keyboard › Adaptive touch: where the finger went down (the keys' pixels),
 * the letter its key typed, and, when it retyped a letter just erased at the same place in the word, that erased tap. */
internal data class TouchTap(val x: Float, val y: Float, val letter: Char, val usable: Boolean,
                             val erasedX: Float = 0f, val erasedY: Float = 0f, val erasedLetter: Char? = null)

/** What Adaptive touch keeps of what you type, in memory only (iOS AdaptiveTouchSpans): the word being typed and the
 * word before it, as taps and the letters they ended as, until the next word shows it stood. Anything it can't follow
 * (the cursor moved, the text changed elsewhere, a swipe, another key plane, [TIMEOUT]) is forgotten, and a tap
 * teaches only with evidence of the letter you meant:
 *
 * - a word left as typed (weak), and only letters the key under the finger typed;
 * - a suggestion of the same length picked in its place, changing at most two letters;
 * - a word kept as typed over an autocorrection, by undoing it or tapping it, and a letter erased and retyped at the
 *   same place (strong): the erased tap is labelled with the letter that replaced it.
 *
 * An autocorrection left in place teaches nothing: a model can't confirm itself. */
internal class AdaptiveTouchSpans {
    enum class Outcome {
        /** Left as typed. */
        Typed,
        /** Kept as typed over an autocorrection. */
        Kept,
        /** A suggestion of the same length went in its place. */
        Picked,
        /** Autocorrect replaced it: nothing to learn, kept only for its undo. */
        Corrected,
    }

    /** A finished word: its taps, the letters it ended as (none when corrected), and whether it was erased and retyped
     * back to its length, changing at most two letters. */
    class Span(val taps: List<TouchTap>, val labels: CharArray, val outcome: Outcome, val geometry: Long, val at: Double,
               val retyped: Boolean)

    private val word = ArrayList<TouchTap>(WORD_LIMIT)
    private val pending = ArrayList<Span>(1)
    private val erased = ArrayList<Pair<Int, TouchTap>>(ERASED_LIMIT)
    private var wordGeometry = 0L
    private var wordKept = false
    private var wordBroken = false
    // The word's length when the current run of erasing began.
    private var eraseBase = -1
    private var lastAt = 0.0

    val isEmpty get() = word.isEmpty() && pending.isEmpty()

    fun forget() {
        word.clear(); erased.clear(); pending.clear()
        resetWord()
    }

    private fun resetWord() { wordKept = false; wordBroken = false; eraseBase = -1 }

    /** A letter key typed [tap]'s letter. */
    fun typed(tap: TouchTap, geometry: Long, now: Double) {
        if (word.isNotEmpty() && (geometry != wordGeometry || now - lastAt > TIMEOUT)) {
            word.clear(); erased.clear(); resetWord()
        }
        if (word.isEmpty()) wordGeometry = geometry
        lastAt = now
        if (word.size >= WORD_LIMIT) { wordBroken = true; return }
        val i = erased.indexOfLast { it.first == word.size }
        word.add(if (i < 0) tap else {
            val gone = erased.removeAt(i).second
            tap.copy(erasedX = gone.x, erasedY = gone.y, erasedLetter = gone.letter)
        })
    }

    /** A letter went into the word without a tap (a held accent): the word teaches nothing. */
    fun untapped() { if (word.isNotEmpty()) wordBroken = true }

    /** Delete is about to remove the last character of [before]. */
    fun willDelete(before: String?) {
        val last = before?.lastOrNull() ?: return forget()
        if (last.isLetter()) {
            if (word.isEmpty()) return forget()
            if (eraseBase < 0) eraseBase = word.size
            var tap = word.removeAt(word.size - 1)
            wordKept = false
            // A retyped letter erased again: its first attempt is the touch that meant it.
            tap.erasedLetter?.let { tap = TouchTap(tap.erasedX, tap.erasedY, it, true) }
            if (!tap.usable) return
            if (erased.size == ERASED_LIMIT) erased.removeAt(0)
            erased.add(word.size to tap)
        } else {
            val span = pending.lastOrNull()
            if (last == ' ' && word.isEmpty() && span != null && (span.outcome == Outcome.Typed || span.outcome == Outcome.Kept) &&
                endsWithWord(before.dropLast(1), span.labels)) {
                // Back into the word just finished: it is being typed again.
                pending.removeAt(pending.size - 1)
                word.addAll(span.taps)
                wordGeometry = span.geometry
                erased.clear(); resetWord()
                wordKept = span.outcome == Outcome.Kept
            } else forget()
        }
    }

    /** Undo put back [original], what was typed, in place of the autocorrection just made. */
    fun undone(original: String) {
        val span = pending.lastOrNull()
        if (word.isNotEmpty() || span == null || span.outcome != Outcome.Corrected || !matches(span.taps, original)) return forget()
        pending.removeAt(pending.size - 1)
        word.addAll(span.taps)
        wordGeometry = span.geometry
        erased.clear(); resetWord()
        wordKept = true
    }

    /** The word being typed ended as [token] (what the field shows; null when it isn't a word) and, when autocorrect
     * replaced it, [corrected]; or, with [picked], a suggestion went in its place ([overCorrection]: it is what was
     * typed, over the fix space would make). Returns the word before, if this one shows it stood: [before], the text
     * before the cursor still ending in this word, has it just before; none when autocorrect is revising it
     * ([previousRevised], "well" to "we'll"). */
    fun finish(token: String?, corrected: String?, picked: String?, overCorrection: Boolean, previousRevised: Boolean,
               before: String?, now: Double): List<Span> {
        if (word.isEmpty()) return emptyList()
        val previous = withoutLastWord(before.orEmpty())
        val stood = pending.filter { !previousRevised && now - it.at <= TIMEOUT && endsWithWord(previous, it.labels) }
        pending.clear()
        span(token, corrected, picked, overCorrection, now)?.let(pending::add)
        word.clear(); erased.clear(); resetWord()
        return stood
    }

    /** The text changed without the keys, or the cursor moved: what can't be followed any more goes. */
    fun hostChanged(before: String?, token: String?) {
        if (word.isNotEmpty()) {
            if (wordBroken || token == null || !matches(word, token)) forget()
        } else {
            val span = pending.lastOrNull() ?: return
            if (!endsWithWord(trailingSeparatorsRemoved(before.orEmpty()), span.labels)) forget()
        }
    }

    private fun span(token: String?, corrected: String?, picked: String?, overCorrection: Boolean, now: Double): Span? {
        if (wordBroken || token == null || !matches(word, token)) return null
        val typed = CharArray(word.size) { word[it].letter }
        var outcome = if (wordKept) Outcome.Kept else Outcome.Typed
        var labels = typed
        if (corrected != null) {
            outcome = Outcome.Corrected
            labels = CharArray(0)
        } else if (picked != null) {
            val letters = picked.lowercase().toCharArray()
            if (letters.contentEquals(typed)) {
                if (overCorrection) outcome = Outcome.Kept
            } else {
                if (letters.size != typed.size || !letters.all { it.isLetter() } ||
                    letters.indices.count { letters[it] != typed[it] } > CHANGED_LETTER_LIMIT) return null
                outcome = Outcome.Picked
                labels = letters
            }
        }
        val changes = word.count { it.erasedLetter != null && it.erasedLetter != it.letter }
        val retyped = eraseBase == word.size && changes <= CHANGED_LETTER_LIMIT
        return Span(ArrayList(word), labels, outcome, wordGeometry, now, retyped)
    }

    companion object {
        const val WORD_LIMIT = 32
        const val ERASED_LIMIT = 8
        /** How long a word waits for the next to show it stood, in seconds. */
        const val TIMEOUT = 30.0
        const val CHANGED_LETTER_LIMIT = 2

        /** Whether [taps] typed exactly [text]'s letters, ignoring case. */
        fun matches(taps: List<TouchTap>, text: String): Boolean {
            val lower = text.lowercase()
            return lower.length == taps.size && lower.indices.all { lower[it] == taps[it].letter }
        }

        /** [text] without the word it ends with, and the spaces and marks before that word. */
        fun withoutLastWord(text: String): String =
            trailingSeparatorsRemoved(text.trimEnd { it.isLetter() || it == '\'' || it == '\u2019' })

        fun trailingSeparatorsRemoved(text: String) = text.trimEnd { !it.isLetter() }

        /** Whether [text] ends with the word [letters] (ignoring case), standing alone. */
        fun endsWithWord(text: String, letters: CharArray): Boolean {
            if (letters.isEmpty() || text.length < letters.size) return false
            val tail = text.takeLast(letters.size).lowercase()
            if (tail.length != letters.size || !tail.indices.all { tail[it] == letters[it] }) return false
            return text.length == letters.size || !text[text.length - letters.size - 1].isLetter()
        }
    }
}
