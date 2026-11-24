package ly.anoma.hex.keyboard

/** Each layout's letter rows, lowercase, keyed by the layout's BCP 47 code. A new layout is a new
 * entry here (and its accents in LetterFeedback); anything unknown types on English's rows. */
internal object KeyboardLetters {
    private val rows = mapOf(
        "en" to listOf("qwertyuiop", "asdfghjkl", "zxcvbnm"),
        "es" to listOf("qwertyuiop", "asdfghjklñ", "zxcvbnm"),
        "nb" to listOf("qwertyuiopå", "asdfghjkløæ", "zxcvbnm"),
        "de" to listOf("qwertzuiopü", "asdfghjklöä", "yxcvbnm"),
        // Gboard's French AZERTY (Pixel 8 emulator): ten keys edge to edge on rows 1 and 2, and an
        // apostrophe key after n on row 3, between shift and delete.
        "fr" to listOf("azertyuiop", "qsdfghjklm", "wxcvbn'"),
        "it" to listOf("qwertyuiop", "asdfghjkl", "zxcvbnm"),
    )

    /** The number row's digits, in the order Gboard's row 1 holds them. */
    const val DIGITS = "1234567890"

    fun rows(layoutCode: String) = rows[layoutCode] ?: rows.getValue("en")

    /** Row 1's digits, 1 to 0 on its first ten keys, as Gboard draws and holds them. */
    fun digit(row: Int, index: Int): Char? = if (row == 0 && index < 10) DIGITS[index] else null

    /** Gboard's ?123 page (Pixel 8 emulator, API 36), top to bottom: its ten digits, ten marks, and the
     * seven keys between the page switch and delete. [currency] is Labs › Keyboard › Local currency's
     * symbol, in $'s place, or null for $. */
    fun numbers(currency: Char? = null) =
        listOf(DIGITS, "@#${currency ?: '$'}_&-+()/", "*\"':;!?")

    /** Gboard's =< page: the marks the ?123 page has no room for, on the same seven-key last row. A
     * local currency that page took $'s place with gives up its own slot here for $; one Hex has no slot
     * for takes only ¥'s, so £, ¢ and € stay drawn and nothing is duplicated. $ is always on one of the
     * two pages. */
    fun symbols(currency: Char? = null): List<String> {
        val money = when {
            currency == null -> "£¢€¥"
            currency in "£¢€¥" -> "£¢€¥".replace(currency, '$')
            else -> "£¢€$"
        }
        return listOf("~`|•√π÷×§Δ", "${money}^°={}\\", "%©®™✓[]")
    }
}
