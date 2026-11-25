package ly.anoma.hex.keyboard

/** Add only the separators missing at the cursor (or on either side of a selection). */
internal fun insertion(transcript: String, before: Char?, after: Char?): String {
    val words = transcript.trim()
    if (words.isEmpty()) return ""
    val opening = "([{“‘"
    val closing = ".,!?:;)]}”’"
    val left = before != null && !before.isWhitespace() && before !in opening && words.first() !in closing
    val right = after != null && !after.isWhitespace() && after !in closing && words.last() !in opening
    return (if (left) " " else "") + words + (if (right) " " else "")
}

/** A dictation going in at the cursor: [insertion]'s spacing around [before], and, when it starts a line
 * below a signed dictation ([standalone]), the line break that puts it there. Its spacing treats that
 * line break as the previous character, so it adds no space in front; the trailing space is unchanged. */
internal fun dictationInsertion(transcript: String, before: String?, after: Char?, standalone: Boolean): String {
    val spaced = insertion(transcript, if (standalone) '\n' else before?.lastOrNull(), after)
    return if (spaced.isEmpty() || !standalone) spaced else "\n$spaced"
}

/** The field a dictation's text may go into. Every field Hex starts in gets a new number; the same field restarting in
 * place (the same app and view: an app resetting it or changing its type) keeps it, and the field finishing retires it.
 * A dictation keeps the number it was spoken in, and its text goes in only while that number is current, the editor is
 * connected and the field isn't a password one, whether or not the keyboard shows: an app hiding the keyboard keeps its
 * field, so the text lands where it was spoken, and never in another field or app. */
internal class DictationField {
    var current = 0
        private set
    private var live = false
    private var app: String? = null
    private var view = 0

    /** A field started; true when it is the same one restarting, which keeps its number. */
    fun started(restarting: Boolean, app: String?, view: Int): Boolean {
        val same = live && restarting && app == this.app && view == this.view
        if (!same) current++
        live = true; this.app = app; this.view = view
        return same
    }

    fun finished() { current++; live = false }

    /** Whether a text spoken in field [spokenIn] goes in now. */
    fun takes(spokenIn: Int, connected: Boolean, secure: Boolean) = live && spokenIn == current && connected && !secure

    companion object {
        /** A hold on the Hex key that Android cancelled: the keyboard going away under the finger finishes the dictation,
         * as the hide itself does; a cancel with the keyboard still up discards it. */
        fun cancelledHoldFinishes(keyboardShown: Boolean) = !keyboardShown
    }
}
