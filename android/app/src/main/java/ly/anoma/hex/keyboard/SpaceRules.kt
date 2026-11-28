package ly.anoma.hex.keyboard

/** Where spaces go, as Gboard puts them (black box, Pixel 8 emulator, API 36), and iPhone Hex where
 * Gboard differs for the worse. */
internal object SpaceRules {
    /** Two spaces this close together after a word become ". " (Gboard: 0.87 s did, 1.07 s didn't). */
    const val DOUBLE_SPACE_MS = 950L

    /** Marks that take the space a picked suggestion typed ("Hello" picked, then "," gives "Hello,"). */
    const val CLAUSE_MARKS = ",.!?;:)"

    /** What goes against the word before rather than after a space a pick held (iPhone Hex's joiners).
     * The closing typographic quote and apostrophe (Labs › Keyboard › Smart quotes) join as their
     * straight forms do; the opening forms never do, so a long-pressed “ keeps the space before it. */
    const val JOINERS = ",.!?;:)]}'-/\"»…’”"

    /** Whether [mark], typed right after a picked suggestion's space, takes that space: a clause mark anywhere, and
     * in a URL field any joiner too ("github" picked, then "." or "/" gives "github." or "github/"). */
    fun takesPickedSpace(mark: Char, url: Boolean) = mark in CLAUSE_MARKS || (url && mark in JOINERS)

    /** Whether [mark], taking a picked suggestion's space, puts it back after itself with Labs › Keyboard › Space after
     * punctuation on ("here" picked, then "," gives "here, "): a clause mark outside a URL field, which keeps joining. */
    fun movesPickedSpace(mark: Char, url: Boolean) = !url && mark in CLAUSE_MARKS

    /** How much text before the cursor Labs › Keyboard › Closing quotes take the space reads for its paragraph. */
    const val QUOTE_LOOKBACK = 1000

    /** Whether a closing double quote typed after [before] takes the one space that ends it (Labs › Keyboard ›
     * Closing quotes take the space, #3001): a quote is open in the paragraph before that space, an odd number
     * of " or more “ than ”. The space must follow something other than whitespace or the opening quote itself,
     * so `say " "` and a run of spaces stay as typed. The paragraph is what [before] holds after its last line
     * break, so one longer than [QUOTE_LOOKBACK] is counted from where the read starts. */
    fun closingQuoteTakesSpace(before: String): Boolean {
        if (before.length < 2 || before.last() != ' ') return false
        val last = before[before.length - 2]
        if (last.isWhitespace() || last == '"' || last == '“') return false
        val paragraph = before.substring(before.lastIndexOf('\n') + 1, before.length - 1)
        return paragraph.count { it == '"' } % 2 == 1 || paragraph.count { it == '“' } > paragraph.count { it == '”' }
    }

    /** Whether ". " may replace the space that ends [before]: it follows a letter or a digit. */
    fun periodFits(before: String) = before.length >= 2 && before.last() == ' ' &&
        before[before.length - 2].let { it.isLetterOrDigit() }

    /** Whether the text after the cursor already separates the word before it from what follows: a
     * space (not a line break) or a closing mark. A picked word then holds its space (iPhone Hex
     * #2214): Gboard steps over the space instead, and a word typed next joins the one after it. */
    fun separates(after: String?): Boolean {
        val first = after?.firstOrNull() ?: return false
        if (first.isWhitespace()) return first != '\n'
        return first in ".,!?;:)]}\"'»…’”"
    }
}
