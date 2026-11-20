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
