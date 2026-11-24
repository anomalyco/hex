package ly.anoma.hex.keyboard

import ly.anoma.hex.speech.DictationEdit
import ly.anoma.hex.speech.DictationEdits
import ly.anoma.hex.speech.DictationText

/** Follows a dictation Hex inserted for a minute and reports what became of it, holding its text only in memory.
 * iPhone Hex's DictationEditTracker over [KeyboardTextTarget]: the dictation is found again by ~32-character
 * anchors, never by position. A span that was never seen in place reports nothing. */
internal class DictationTracker(
    private val target: KeyboardTextTarget,
    private val now: () -> Long,
    private val onResult: (DictationEdit) -> Unit,
) {
    companion object {
        const val editWindow = 60_000L
        const val redictationWindow = 20_000L
        const val anchorLength = 32
        /** How often the keyboard checks a dictation still being followed. */
        const val checkInterval = 10_000L
    }

    private class Tracked(val heard: String?, val inserted: String, val anchorBefore: String, val anchorAfter: String,
                          val insertedAt: Long) {
        var latest = inserted
        var located = false
        var emptiedAt: Long? = null
        var cleared = false
        var byPerson = false
        var undone = false
    }

    private var tracked: Tracked? = null
    /** A dictation removed, held for the redictation window in case another one follows. */
    private var removed: Pair<Tracked, Long>? = null

    val isTracking get() = tracked != null || removed != null

    /** A dictation just went in at the cursor; [before] and [after] are the text around it before the insertion. */
    fun inserted(heard: String?, inserted: String, before: String?, after: String?) {
        // Judge the one before it from the last place it was seen: never from text that now ends in this dictation.
        val previous = tracked
        tracked = null
        previous?.let { settle(it, now()) }
        val at = now()
        val words = DictationText.words(inserted).size
        if (words == 0 || words > DictationEdits.maxWords) return
        val pending = removed
        if (pending != null && at - pending.second > redictationWindow) {
            removed = null
            emitRemoval(pending.first)
        } else if (pending != null) {
            removed = null
            DictationEdits.redictation(pending.first.heard, pending.first.inserted, inserted)?.let(onResult)
        }
        tracked = Tracked(heard, inserted, before.orEmpty().takeLast(anchorLength), after.orEmpty().take(anchorLength), at)
    }

    /** The text changed or the cursor moved: find the dictation again, or settle once it can't be followed. */
    fun observe() {
        val at = now()
        val pending = removed
        if (pending != null && at - pending.second > redictationWindow) {
            removed = null
            emitRemoval(pending.first)
        }
        val current = tracked ?: return
        // The window ends before the text is read again: a dictation is judged by what it looked like while
        // it was still being followed, never by edits made after its minute.
        if (current.emptiedAt == null && at - current.insertedAt >= editWindow) {
            tracked = null
            settle(current, at)
            return
        }
        if (!update(current)) { tracked = null; settle(current, at); return }
        // An app that cleared the field is judged from the last text it showed now, before the next message
        // typed into that field can be taken as this dictation's span.
        if (current.cleared) { tracked = null; settle(current, at); return }
        val emptiedAt = current.emptiedAt
        if (emptiedAt != null && at - emptiedAt > redictationWindow) { tracked = null; settle(current, at) }
    }

    /** A new dictation is starting: report the one before it from the text as it is now. */
    fun settle() {
        val current = tracked ?: return
        tracked = null
        val at = now()
        if (update(current)) settle(current, at)
    }

    /** The field or keyboard is going away: report from the last place the dictation was seen. */
    fun finish() {
        val current = tracked
        tracked = null
        current?.let { settle(it, now()) }
        flush(removed?.first)
    }

    /** Undo dictation took it back. */
    fun undone() {
        val current = tracked ?: return
        current.latest = ""
        current.located = true
        current.undone = true
        current.emptiedAt = now()
    }

    /** Hex's own Delete took text (the key, its held repeat, a delete slide or a swipe): the person removed
     * it, so a field that then empties was deleted by them and the next dictation is a redictation. */
    fun keyboardRemoved() {
        tracked?.byPerson = true
    }

    /** The person sent the field (Enter, Send, Go, Done, Next): the dictation is judged from the last text it
     * showed, never held for the next dictation to be judged against. */
    fun sent() {
        val current = tracked
        tracked = null
        removed = null
        if (current == null || !current.located) return
        if (DictationText.words(current.latest).isNotEmpty())
            DictationEdits.classify(current.heard, current.inserted, current.latest, current.undone)?.let(onResult)
        else emitRemoval(current)
    }

    /** Updates the dictation's place from the text around the cursor; false when it can no longer be followed
     * and had been seen before. Until it is first seen the host may simply be behind, so it keeps waiting. */
    private fun update(current: Tracked): Boolean {
        // Past its minute the dictation takes no new span: the text there now is not what it was, even when
        // the field was emptied and filled again since.
        val expired = now() - current.insertedAt >= editWindow
        if (expired && current.emptiedAt == null) return true
        // The whole dictation may sit on either side of the cursor, edited a little; read far enough on both
        // sides for its text and anchors, so a cursor inside it never cuts a word at the window's edge.
        val reach = current.inserted.length + anchorLength + 200
        val before = target.before(reach)
        val after = target.after(reach)
        if (before == null && after == null) return !current.located
        val currentBefore = before.orEmpty()
        val currentAfter = after.orEmpty()
        // A short read is the field's edge. A full one only says the window stopped there, so an empty anchor
        // must not be read as the start or end of the field.
        val startEdge = before != null && currentBefore.length < reach
        val endEdge = after != null && currentAfter.length < reach
        if (currentBefore.isEmpty() && currentAfter.isEmpty()) {
            if (!current.located) return true
            current.emptiedAt = current.emptiedAt ?: now()
            // Hex's own Delete or Undo emptied it, so the person removed it and a redictation may follow. An app
            // that clears the whole field (its own send button) is no deletion by them: the last text it showed is
            // still judged when this settles, and no later dictation is compared against it.
            current.cleared = !current.undone && !current.byPerson
            if (!current.cleared) current.latest = ""
            return true
        }
        val text = currentBefore + currentAfter
        val starts = when {
            current.anchorBefore.isNotEmpty() -> occurrences(text, current.anchorBefore).map { it + current.anchorBefore.length }
            startEdge -> listOf(0)
            else -> emptyList()
        }
        val ends = when {
            current.anchorAfter.isNotEmpty() -> occurrences(text, current.anchorAfter)
            endEdge -> listOf(text.length)
            else -> emptyList()
        }
        var bestStart = -1
        var bestEnd = -1
        var bestMiss = Int.MAX_VALUE
        // A short anchor (" ", "\n", ".") repeats inside the dictation, so every end is considered, but only
        // the few nearest the span's last known end: never a fixed first-16 window that stops inside it.
        for (start in starts) {
            val target = start + current.latest.length
            val found = ends.binarySearch(target)
            val at = if (found >= 0) found else -found - 1
            for (index in maxOf(0, at - 2)..minOf(ends.lastIndex, at + 2)) {
                val end = ends[index]
                if (end < start) continue
                val miss = kotlin.math.abs(end - target)
                if (miss < bestMiss) { bestMiss = miss; bestStart = start; bestEnd = end }
            }
        }
        if (bestStart < 0 || bestEnd - bestStart > current.inserted.length * 4 + 200) return !current.located
        val span = text.substring(bestStart, bestEnd)
        val hasWords = DictationText.words(span).isNotEmpty()
        if (hasWords) {
            // Text that appeared after the window is not this dictation coming back.
            if (expired) return true
            current.latest = span
            current.located = true
            current.emptiedAt = null
            current.cleared = false
            current.byPerson = false
            current.undone = false
        } else if (current.located) {
            current.latest = span
            current.emptiedAt = current.emptiedAt ?: now()
        }
        return true
    }

    private fun settle(current: Tracked, at: Long) {
        if (!current.located) return
        if (DictationText.words(current.latest).isNotEmpty()) {
            DictationEdits.classify(current.heard, current.inserted, current.latest, current.undone)?.let(onResult)
            return
        }
        // A field cleared as a whole by something other than the person is no deletion by them, so it is
        // reported at once and never held for the next dictation to be judged against.
        if (current.cleared) { emitRemoval(current); return }
        val emptiedAt = current.emptiedAt ?: at
        if (at - emptiedAt > redictationWindow) emitRemoval(current) else removed = current to emptiedAt
    }

    private fun emitRemoval(current: Tracked) {
        DictationEdits.classify(current.heard, current.inserted, "", current.undone)?.let(onResult)
    }

    private fun flush(current: Tracked?) {
        if (current == null) return
        removed = null
        emitRemoval(current)
    }

    private fun occurrences(text: String, anchor: String): List<Int> {
        val starts = ArrayList<Int>()
        var from = 0
        while (true) {
            val at = text.indexOf(anchor, from)
            if (at < 0) return starts
            starts += at
            from = at + 1
        }
    }
}
