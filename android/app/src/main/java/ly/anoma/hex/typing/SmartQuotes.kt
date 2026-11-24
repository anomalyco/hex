package ly.anoma.hex.typing

/** Straight quotes as typographic ones, the one place the keyboard turns ' and " into ’ ‘ “ ”
 * (Labs › Keyboard › Smart quotes). Every path asks this object: a quote key, a word a suggestion,
 * autocorrection or swipe commits, and dictated text. Off, nothing calls it.
 *
 * A straight quote opens after nothing, whitespace or an opening bracket or quote and closes
 * otherwise, so "this" becomes “this” and doesn't becomes doesn’t. A lone ' follows the same rule,
 * with ’ for an apostrophe after a word or digit and ‘ where a quotation opens. */
internal object SmartQuotes {
    const val APOSTROPHE: Char = '\u2019'
    const val OPENING_SINGLE: Char = '\u2018'
    const val OPENING_DOUBLE: Char = '\u201C'
    const val CLOSING_DOUBLE: Char = '\u201D'

    /** Marks a quote opens after: brackets, and the opening quotes themselves. */
    private const val OPENING = "([{“‘"

    /** The quote [quote] (a straight ' or ") as it goes in after [before], the character right before
     * the cursor (null at the field's start or in an empty field). Anything but a straight quote is
     * returned unchanged. */
    fun typed(quote: Char, before: Char?): Char {
        val opens = before == null || before.isWhitespace() || before in OPENING
        return when (quote) {
            '\'' -> if (opens) OPENING_SINGLE else APOSTROPHE
            '"' -> if (opens) OPENING_DOUBLE else CLOSING_DOUBLE
            else -> quote
        }
    }

    /** A word a suggestion, autocorrection or swipe commits: its straight apostrophes are
     * apostrophes, wherever they sit in the word ("don't" -> "don’t", "'em" -> "’em"). */
    fun word(text: String): String =
        if ('\'' !in text) text else text.replace('\'', APOSTROPHE)

    /** [text], as dictated or otherwise inserted whole, with each straight quote placed by [typed]
     * after the character before it: an apostrophe inside a word becomes ’, a pair of straight
     * quotes becomes “ ” or ‘ ’, and text that is already curly is left alone. */
    fun text(text: String, before: Char?): String {
        if (text.none { it == '\'' || it == '"' }) return text
        val out = StringBuilder(text.length + 8)
        var previous = before
        for (c in text) {
            val placed = if (c == '\'' || c == '"') typed(c, previous) else c
            out.append(placed)
            previous = placed
        }
        return out.toString()
    }
}
