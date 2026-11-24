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
