package ly.anoma.hex.speech

/** Labs › Signature: the one line added to the end of a keyboard dictation before it goes in. */
internal object Signature {
    /** The signature a new install starts with. */
    const val DEFAULT_TEXT = "Sent from Hex"

    /** The longest signature a person may save, in characters. */
    const val MAX_CHARS = 200

    /** Why a saved signature is refused, if it is. Empty is allowed and appends nothing. */
    enum class Problem { Line, Length }

    /** The text with [signature] on its own line at the end, or null when there is nothing to append: the lab
     * is off, the text is empty after trimming, the signature is empty after trimming, or the text already ends
     * with it on a line of its own. The saved signature is trimmed, so its own edges never add blank lines. */
    fun append(text: String, signature: String, enabled: Boolean): String? {
        if (!enabled || text.isBlank()) return null
        val line = signature.trim()
        if (line.isEmpty() || alreadySigned(text.trim(), line)) return null
        return "$text\n$line"
    }

    /** Whether the text already carries the signature as a line of its own: it is only the signature, or the
     * signature follows a line break with nothing but spaces or tabs in between. A signature that merely ends
     * a line of prose, such as "Hex" in "I love Hex", does not make the line signed. */
    private fun alreadySigned(text: String, signature: String): Boolean {
        if (text == signature) return true
        if (!text.endsWith(signature)) return false
        val end = text.dropLast(signature.length).trimEnd(' ', '\t').lastOrNull()
        return end == '\n' || end == '\r'
    }

    /** Why a saved signature is refused, if it is: it is one line of at most [MAX_CHARS] characters. */
    fun textProblem(text: String): Problem? {
        val trimmed = text.trim()
        if (trimmed.any { Character.isISOControl(it) || it == '\u2028' || it == '\u2029' }) return Problem.Line
        if (trimmed.codePointCount(0, trimmed.length) > MAX_CHARS) return Problem.Length
        return null
    }
}
