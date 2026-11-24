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
        if (!update(current)) { tracked = null; settle(current, at); return }
        val emptiedAt = current.emptiedAt
        when {
            emptiedAt != null && at - emptiedAt > redictationWindow -> { tracked = null; emitRemoval(current) }
            emptiedAt == null && at - current.insertedAt >= editWindow -> { tracked = null; settle(current, at) }
        }
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

    /** Updates the dictation's place from the text around the cursor; false when it can no longer be followed
     * and had been seen before. Until it is first seen the host may simply be behind, so it keeps waiting. */
    private fun update(current: Tracked): Boolean {
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
            current.latest = ""
            current.emptiedAt = current.emptiedAt ?: now()
            return true
        }
        val text = currentBefore + currentAfter
        val starts = when {
            current.anchorBefore.isNotEmpty() -> occurrences(text, current.anchorBefore).map { it + current.anchorBefore.length }
            startEdge -> listOf(0)
            else -> emptyList()
        }.take(16)
        val ends = when {
            current.anchorAfter.isNotEmpty() -> occurrences(text, current.anchorAfter)
            endEdge -> listOf(text.length)
            else -> emptyList()
        }.take(16)
        var bestStart = -1
        var bestEnd = -1
        var bestMiss = Int.MAX_VALUE
        for (start in starts) for (end in ends) {
            if (start > end) continue
            val miss = kotlin.math.abs(end - start - current.inserted.length)
            if (miss < bestMiss) { bestMiss = miss; bestStart = start; bestEnd = end }
        }
        if (bestStart < 0 || bestEnd - bestStart > current.inserted.length * 4 + 200) return !current.located
        val span = text.substring(bestStart, bestEnd)
        val hasWords = DictationText.words(span).isNotEmpty()
        if (hasWords) {
            current.latest = span
            current.located = true
            current.emptiedAt = null
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
