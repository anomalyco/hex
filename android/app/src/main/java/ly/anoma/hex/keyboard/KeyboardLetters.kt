package ly.anoma.hex.keyboard

/** Each layout's letter rows, lowercase, keyed by the layout's BCP 47 code. A new layout is a new
 * entry here (and its accents in LetterFeedback); anything unknown types on English's rows. */
internal object KeyboardLetters {
    private val rows = mapOf(
        "en" to listOf("qwertyuiop", "asdfghjkl", "zxcvbnm"),
        "es" to listOf("qwertyuiop", "asdfghjklñ", "zxcvbnm"),
        "nb" to listOf("qwertyuiopå", "asdfghjkløæ", "zxcvbnm"),
        "de" to listOf("qwertzuiopü", "asdfghjklöä", "yxcvbnm"),
    )

    fun rows(layoutCode: String) = rows[layoutCode] ?: rows.getValue("en")

    /** The 123 page and the second symbols page; Spanish puts ¿ and ¡ on the second page's last row. */
    fun numbers(layoutCode: String) = listOf("1234567890", "-/:;()$&@\"", punctuation(layoutCode, more = false))
    fun symbols(layoutCode: String) = listOf("[]{}#%^*+=", "_\\|~<>€£¥•", punctuation(layoutCode, more = true))
    private fun punctuation(layoutCode: String, more: Boolean) = if (more && layoutCode == "es") ".,¿¡'" else ".,?!'"
}
