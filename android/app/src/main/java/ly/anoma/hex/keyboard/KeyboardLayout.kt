package ly.anoma.hex.keyboard

import android.text.InputType
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

/** What the key beside ?123 types on the letters page: Gboard's @ in email fields and / in web
 * addresses, else a comma. */
internal enum class FieldKind(val mark: String) {
    Text(","), Email("@"), Url("/");
    companion object {
        fun of(inputType: Int): FieldKind {
            if (inputType and InputType.TYPE_MASK_CLASS != InputType.TYPE_CLASS_TEXT) return Text
            return when (inputType and InputType.TYPE_MASK_VARIATION) {
                InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS, InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS -> Email
                InputType.TYPE_TEXT_VARIATION_URI -> Url
                else -> Text
            }
        }
    }
}

/** What the keys show: one page of one layout, with shift, the enter key and the field's kind. */
internal data class KeyboardPage(
    val language: Spoken,
    val symbols: Boolean,
    val more: Boolean,
    val shifted: Boolean,
    val locked: Boolean,
    val enter: EnterKey,
    val kind: FieldKind = FieldKind.Text,
    val pad: Pad? = null,
    /** Labs › Keyboard › Number row: the letter pages draw 1 to 0 above their letters too. */
    val numberRow: Boolean = false,
    /** Labs › Keyboard › Local currency: the phone's symbol, read once for the page, that the 123 page shows
     * in place of $, or null for $. */
    val currency: Char? = null,
) {
    val lines: List<String> get() {
        val code = language.layoutCode
        return when {
            !symbols -> KeyboardLetters.rows(code)
            more -> KeyboardLetters.symbols(code, currency)
            else -> KeyboardLetters.numbers(code, currency)
        }
    }

    /** The ten digits above the letter rows while the number row shows; ?123 and the pads keep their own. */
    val digits: String? get() = if (numberRow && !symbols && pad == null) KeyboardLetters.DIGITS else null

    /** Row 1's digit corner hint, or none while the number row already shows the digit. */
    fun hint(row: Int, index: Int): Char? = if (digits != null) null else KeyboardLetters.digit(row, index)
}

/** Sizes in dp, measured from Gboard on a Pixel 8 emulator (API 36, 411 dp wide, default
 * height) in the same field: row 1's keycaps start 52.2 dp below the keyboard's top, keycaps
 * are 47 dp tall on a 58.9 dp row pitch with 5.33 dp between them, and row 4 ends 12.2 dp
 * above the system's bar. Taps type row 1 from 2 dp above its cell, and the gaps and edges type
 * the nearest key, as Gboard's do. Widths are in cells: a tenth of the row for the bottom row and
 * 10-letter rows, an eleventh where a layout has 11 letters. */
internal object KeyboardMetrics {
    const val SIDE = 1.9f
    const val GAP = 5.33f
    const val STRIP = 44.25f
    /** Row 1's touch targets reach this far above its cell, into the strip (Gboard's reach about 2 dp). */
    const val ROW1_REACH = 2f
    /** Where the strip's words and the Hex mark centre, from its top (Gboard's mic: 21.7 dp). */
    const val STRIP_CENTER = 21.7f
    const val ROW = 58.9f
    const val KEY_HEIGHT = 47f
    /** Under row 4's cell, above the system's bar: with half a row gap it is Gboard's 12.2 dp. */
    const val BOTTOM = 6.25f
    // Turned, Gboard's rows are 43 dp apart with 39 dp keycaps (it goes full screen; Hex doesn't).
    const val STRIP_LANDSCAPE = 38f
    const val STRIP_CENTER_LANDSCAPE = 19f
    const val ROW_LANDSCAPE = 43f
    const val KEY_HEIGHT_LANDSCAPE = 39f
    const val BOTTOM_LANDSCAPE = 2f
    const val CORNER = 7f
    /** The Hex key's target at the strip's trailing end: its centre is where Gboard's mic is. */
    const val HEX_KEY = 42.7f
    /** The Hex key's circle when pressed or dictating, and the recording bar's height. */
    const val PILL = 34f
    const val MARK = 24f
    const val MENU_ROWS = 4
    /** A slide on space moves the cursor one character after this much travel, then one more each step
     * (Gboard: about 45 px and 33 px on a 2.625 density Pixel 8). */
    const val CURSOR_START = 17f
    const val CURSOR_STEP = 12.6f
    /** A slide left from delete selects a word after this much travel, one more each step; a press
     * that went further than the cancel distance deletes nothing on its own (Gboard: about 140 px,
     * 95 px, and under 107 px on the Pixel 8). Held still, delete repeats every 50 ms after 400 ms. */
    const val DELETE_START = 53f
    const val DELETE_STEP = 36f
    const val DELETE_CANCEL = 32f
    const val DELETE_REPEAT_DELAY = 400L
    const val DELETE_REPEAT_EVERY = 50L
    /** A key held this long shows what it holds (Gboard's long press: between 300 and 350 ms). */
    const val HOLD_MS = 300L
    /** Gboard's pads: 53.3 dp keys on the letters' 58.9 dp rows (a 5.7 dp gap between rows). */
    const val PAD_ROW_GAP = 5.7f
    /** Gboard ticks again as a key lifts when it was held at least this long. */
    const val RELEASE_TICK_MS = 200L

    // The bottom row, in tenths: ?123, comma, emoji, space, period, enter (Gboard's with its emoji key).
    const val MODE = 1.5f
    const val COMMA = 1f
    const val EMOJI = 1f
    const val SPACE = 4f
    const val PERIOD = 1f
    const val ENTER = 1.5f
    /** The emoji grid's cell, as Gboard's (a 45 dp row pitch, nine across in portrait). */
    const val EMOJI_CELL = 45f
    const val EMOJI_CELL_LANDSCAPE = 38f
    /** The emoji picker's own category bar above the grid, and a category's title in it. */
    const val EMOJI_HEADER = 42f
    const val EMOJI_TITLE = 24f
    /** Gboard's emoji panel stands this much taller than its keyboard (Pixel 8 emulator: 325 px). */
    const val EMOJI_GROW = 124f

    /** Cells across the letter rows: the longer of rows 1 and 2. */
    fun columns(lines: List<String>) = maxOf(lines[0].length, lines[1].length)
    /** Shift and delete (or the symbol pages' switch) beside row 3's keys, in that row's cells. */
    fun sideWidth(page: KeyboardPage, lines: List<String>): Float =
        if (page.symbols) 1.5f else (columns(lines) - lines[2].length) / 2f
}
