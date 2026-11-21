package ly.anoma.hex.keyboard

import android.view.inputmethod.EditorInfo
import ly.anoma.hex.speech.Spoken

/** The enter key: the editor's own label, else one of the standard actions, else plain return. */
internal data class EnterKey(val label: String?, val action: Int) {
    companion object {
        const val RETURN = EditorInfo.IME_ACTION_NONE
        fun from(info: EditorInfo?): EnterKey {
            val options = info?.imeOptions ?: 0
            if (options and EditorInfo.IME_FLAG_NO_ENTER_ACTION != 0) return EnterKey(null, RETURN)
            return EnterKey(info?.actionLabel?.toString(), options and EditorInfo.IME_MASK_ACTION)
        }
    }
}

/** What the keys show: one page of one layout, with shift and the enter key. */
internal data class KeyboardPage(
    val language: Spoken,
    val symbols: Boolean,
    val more: Boolean,
    val shifted: Boolean,
    val locked: Boolean,
    val enter: EnterKey,
) {
    val lines: List<String> get() {
        val code = language.layoutCode
        return when {
            !symbols -> KeyboardLetters.rows(code)
            more -> KeyboardLetters.symbols(code)
            else -> KeyboardLetters.numbers(code)
        }
    }
}

/** Sizes in dp and key widths in letter widths. */
internal object KeyboardMetrics {
    const val STRIP = 44
    const val KEY_HEIGHT = 50
    const val KEY_HEIGHT_LANDSCAPE = 40
    const val SIDE = 3
    const val BOTTOM = 4
    const val CAP_INSET_X = 2
    const val CAP_INSET_Y = 3
    const val CORNER = 8
    const val MENU_ROWS = 4

    // The bottom row, left to right.
    const val MODE = 1.45f
    const val GLOBE = 1.05f
    const val HEX = 1.35f
    const val SPACE = 3.5f
    const val PERIOD = 1f
    const val ENTER = 1.65f
    /** Shift and delete (or the symbol pages' switch) beside row 3's keys. */
    fun sideWidth(page: KeyboardPage, lines: List<String>): Float {
        val columns = maxOf(lines[0].length, lines[1].length)
        return if (page.symbols) 1.5f else (columns - lines[2].length) / 2f
    }
}
