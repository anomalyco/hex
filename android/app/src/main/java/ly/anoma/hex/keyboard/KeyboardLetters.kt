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

    fun rows(layoutCode: String) = rows[layoutCode] ?: rows.getValue("en")

    /** Row 1's digits, 1 to 0 on its first ten keys, as Gboard draws and holds them. */
    fun digit(row: Int, index: Int): Char? = if (row == 0 && index < 10) "1234567890"[index] else null

    /** The 123 page and the second symbols page; Spanish puts ¿ and ¡ on the second page's last row. */
    fun numbers(layoutCode: String) = listOf("1234567890", "-/:;()$&@\"", punctuation(layoutCode, more = false))
    fun symbols(layoutCode: String) = listOf("[]{}#%^*+=", "_\\|~<>€£¥•", punctuation(layoutCode, more = true))
    private fun punctuation(layoutCode: String, more: Boolean) = if (more && layoutCode == "es") ".,¿¡'" else ".,?!'"
}
