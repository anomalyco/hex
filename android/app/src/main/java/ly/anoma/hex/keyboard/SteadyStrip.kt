package ly.anoma.hex.keyboard

import ly.anoma.hex.typing.Suggestion

/** Keeps the strip's words where they are while one word is typed (#3017). Each letter brings two strips,
 * completions at once and, after a pause, the fix with the near words; laid out on their own they moved a
 * word the person was about to tap ("gonna" jumped when "gone" came in). A word already shown keeps its
 * slot while the same word is typed; only what was typed, the fix and the emoji have fixed places. Before
 * the pause, a word the model doesn't know shows what was typed on the left, the middle for the fix and
 * the likeliest completion on the right, and the pause's strip keeps what was typed on the left even when
 * it has no fix for it. Otherwise only places change, never which words the strip offers. */
internal class SteadyStrip {
    private var shown: List<Suggestion?>? = null
    private var token = ""

    /** Another strip took the line (next words, a swipe's readings, nothing): the next word starts afresh. */
    fun forget() { shown = null; token = "" }

    /** [slots], in the typing model's order, for the word [token] being typed, as the strip shows them left
     * to right ([Suggestion.SLOTS], null for an empty slot). [typed] is what was typed as its slot: [early]
     * (the pause's strip is still to come) lays an unknown word out with it on the left, and the pause's strip
     * keeps it there once this word showed it, so the left slot doesn't swap between what was typed and a
     * completion with each letter. */
    fun arrange(token: String, slots: List<Suggestion>, typed: Suggestion? = null, early: Boolean = false): List<Suggestion?> {
        val last = shown
        val same = last != null && sameWord(this.token, token)
        val keepsTyped = early || (same && last.orEmpty().any { it?.kind == Suggestion.Kind.Typed })
        // Only beside a word: an empty strip stays empty (the tools can show there), and an emoji alone stays alone.
        val placed = if (typed != null && keepsTyped && slots.none { it.kind == Suggestion.Kind.Typed } &&
            slots.any { it.kind != Suggestion.Kind.Emoji }) withTyped(typed, slots)
            else Suggestion.place(slots)
        val out = if (same) keep(last.orEmpty(), placed) else placed
        shown = out; this.token = token
        return out
    }

    private companion object {
        val FIXED = setOf(Suggestion.Kind.Typed, Suggestion.Kind.Fix, Suggestion.Kind.Emoji)

        /** The same word, a letter typed or deleted since: one is the start of the other. */
        fun sameWord(a: String, b: String) = a.isNotEmpty() && b.isNotEmpty() &&
            (a.startsWith(b, ignoreCase = true) || b.startsWith(a, ignoreCase = true))

        /** An unknown word: what was typed on the left and the likeliest completion on the right, where a fix
         * won't take it; the next one waits in the middle a fix comes to. With an emoji in the last slot, the
         * likeliest completion has only the middle. */
        fun withTyped(typed: Suggestion, slots: List<Suggestion>): List<Suggestion?> {
            val emoji = slots.firstOrNull { it.kind == Suggestion.Kind.Emoji }
            val words = slots.filter { it.kind != Suggestion.Kind.Emoji }
            return if (emoji != null) listOf(typed, words.getOrNull(0), emoji)
            else listOf(typed, words.getOrNull(1), words.getOrNull(0))
        }

        /** [next] with each word that [last] showed back in its slot, unless what was typed, the fix or the
         * emoji now has it. The rest take their own place if it is free, else the first free one, likeliest first. */
        fun keep(last: List<Suggestion?>, next: List<Suggestion?>): List<Suggestion?> {
            val out = arrayOfNulls<Suggestion>(Suggestion.SLOTS)
            for (i in out.indices) next.getOrNull(i)?.takeIf { it.kind in FIXED }?.let { out[i] = it }
            // Without what was typed the likeliest word is in the middle and the next on the left (Suggestion.place).
            val order = if (next.any { it?.kind == Suggestion.Kind.Typed }) listOf(0, 1, 2) else listOf(1, 0, 2)
            val moved = ArrayList<Pair<Int, Suggestion>>()
            for (i in order) {
                val s = next.getOrNull(i)?.takeIf { it.kind !in FIXED } ?: continue
                val was = last.indexOfFirst { it != null && it.kind != Suggestion.Kind.Typed &&
                    it.kind != Suggestion.Kind.Emoji && it.text.equals(s.text, ignoreCase = true) }
                if (was in out.indices && out[was] == null) out[was] = s else moved.add(i to s)
            }
            for ((i, s) in moved) {
                val at = if (out[i] == null) i else out.indices.firstOrNull { out[it] == null } ?: continue
                out[at] = s
            }
            return out.toList()
        }
    }
}
