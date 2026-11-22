package ly.anoma.hex.keyboard

import ly.anoma.hex.typing.SwipeTyped

/** The word a swipe just typed, until anything else changes the text (iOS `FullKeyboardEngine.SwipedWord`):
 * delete takes it back whole, with the space it added, and the strip offers its other readings, any of
 * which takes its place. */
internal class SwipedWords(private val target: KeyboardTextTarget) {
    /** What it put in (a space when one was needed, then the word), the word, and its other readings. */
    private class Swiped(val inserted: String, val word: String, val alternatives: List<String>)
    private var swiped: Swiped? = null

    val active get() = swiped != null
    /** Its other readings, cased as the word is, best first. */
    val alternatives get() = swiped?.alternatives.orEmpty()

    /** Types a finished swipe's word, after a space when [typed] asks for one; false when the editor took nothing. */
    fun type(typed: SwipeTyped): Boolean {
        swiped = null
        val inserted = (if (typed.space) " " else "") + typed.word
        if (!target.commit(inserted)) return false
        swiped = Swiped(inserted, typed.word, typed.alternatives.take(3))
        return true
    }

    /** Puts [text], one of its other readings, in the word's place; the word joins the others. False when
     * [text] isn't one of them, or the word is no longer right before the cursor. */
    fun replace(text: String): Boolean {
        val s = swiped ?: return false
        if (text !in s.alternatives) return false
        swiped = null
        if (!before(s.word)) return false
        target.beginBatch()
        val done = try { target.deleteBefore(s.word.length) && target.commit(text) } finally { target.endBatch() }
        if (done) swiped = Swiped(s.inserted.dropLast(s.word.length) + text, text, (listOf(s.word) + s.alternatives.filter { it != text }).take(3))
        return done
    }

    /** Delete right after a swipe: the whole word and the space it added go, when they're still right
     * before the cursor; false (nothing deleted) otherwise. */
    fun takeBack(): Boolean {
        val s = swiped ?: return false
        swiped = null
        return before(s.inserted) && target.deleteBefore(s.inserted.length)
    }

    /** Whether what it typed is still right before the cursor, with nothing selected. */
    fun inPlace() = swiped?.let { before(it.inserted) } ?: false

    fun forget() { swiped = null }

    private fun before(text: String) = target.selected().isNullOrEmpty() && target.before(text.length) == text
}
