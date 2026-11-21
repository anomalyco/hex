package ly.anoma.hex.keyboard

import android.text.InputType
import ly.anoma.hex.R

/** Gboard's pads (black box, Pixel 8 emulator, API 36): number fields, PINs, phone numbers and their
 * symbols, dates and times, and the number pad Gboard's symbol page opens with its 1234 key. */
internal enum class Pad {
    Number, Pin, Phone, PhoneSymbols, Date, Time, DateTime, Symbols;

    companion object {
        /** The pad a field asks for, or null for the letters. One-time codes are number or PIN fields. */
        fun of(inputType: Int): Pad? {
            val variation = inputType and InputType.TYPE_MASK_VARIATION
            return when (inputType and InputType.TYPE_MASK_CLASS) {
                InputType.TYPE_CLASS_NUMBER -> if (variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD) Pin else Number
                InputType.TYPE_CLASS_PHONE -> Phone
                InputType.TYPE_CLASS_DATETIME -> when (variation) {
                    InputType.TYPE_DATETIME_VARIATION_DATE -> Date
                    InputType.TYPE_DATETIME_VARIATION_TIME -> Time
                    else -> DateTime
                }
                else -> null
            }
        }
    }
}

/** What a pad key does. */
internal sealed interface PadAction {
    data class Text(val text: String) : PadAction
    data class Go(val pad: Pad) : PadAction
    data object Space : PadAction
    data object Delete : PadAction
    data object Enter : PadAction
    /** The symbol pad's ABC and !?#: back to the letters, or to the symbol page. */
    data object Letters : PadAction
    data object SymbolsPage : PadAction
}

/** One key: its label (or a string resource for words), what it does, its place and width in the row's
 * cells, whether it wears the modifier colour, the letters beside a phone digit, and what holding it types. */
internal data class PadKey(val label: String, val action: PadAction, val start: Float, val width: Float = 1f,
                           val modifier: Boolean = false, val letters: String? = null, val hold: String? = null,
                           val word: Int = 0) {
    companion object {
        fun text(s: String, start: Float, width: Float = 1f, modifier: Boolean = false, letters: String? = null, hold: String? = null) =
            PadKey(s, PadAction.Text(s), start, width, modifier, letters, hold)
    }
}

internal object KeyPads {
    /** Four columns of Gboard's number pad, or the ten cells of its symbol pad. */
    fun cells(pad: Pad) = if (pad == Pad.Symbols) 10f else 4f

    /** The symbol pad's operators, in a column of their own beside the digits, four showing at once. */
    val operators = listOf("+", "-", "*", "/", "(", ")")

    /** Rows top to bottom. The symbol pad's first three rows leave its operator column's cells empty. */
    fun rows(pad: Pad): List<List<PadKey>> {
        fun digits(row: String, start: Float = 0f, width: Float = 1f, letters: List<String?> = listOf(null, null, null)) =
            row.mapIndexed { i, c -> PadKey.text(c.toString(), start + i * width, width, letters = letters[i]) }
        val right = listOf(PadKey.text("-", 3f, modifier = true), PadKey("", PadAction.Space, 3f, modifier = true),
            PadKey("", PadAction.Delete, 3f, modifier = true))
        fun numbers(first: PadKey) = listOf(
            digits("123") + right[0], digits("456") + right[1], digits("789") + right[2],
            listOf(first, PadKey.text("0", 1f), PadKey.text(".", 2f), PadKey("", PadAction.Enter, 3f, modifier = true)))
        return when (pad) {
            Pad.Number -> numbers(PadKey.text(",", 0f))
            Pad.Date -> numbers(PadKey.text("/", 0f))
            Pad.Time -> numbers(PadKey.text(":", 0f))
            Pad.DateTime -> numbers(PadKey("/ :", PadAction.Text("/"), 0f, hold = ":"))
            // A PIN pad has no punctuation: three columns, centred.
            Pad.Pin -> listOf(digits("123", 0.5f), digits("456", 0.5f), digits("789", 0.5f),
                listOf(PadKey("", PadAction.Delete, 0.5f, modifier = true), PadKey.text("0", 1.5f),
                    PadKey("", PadAction.Enter, 2.5f, modifier = true)))
            Pad.Phone -> listOf(
                digits("123", letters = listOf(null, "ABC", "DEF")) + right[0],
                digits("456", letters = listOf("GHI", "JKL", "MNO")) + right[1],
                digits("789", letters = listOf("PQRS", "TUV", "WXYZ")) + right[2],
                listOf(PadKey("*#", PadAction.Go(Pad.PhoneSymbols), 0f), PadKey.text("0", 1f, letters = "+", hold = "+"),
                    PadKey.text(".", 2f), PadKey("", PadAction.Enter, 3f, modifier = true)))
            // Pause and Wait are the dialler's "," and ";".
            Pad.PhoneSymbols -> listOf(
                listOf(PadKey.text("(", 0f), PadKey.text("/", 1f), PadKey.text(")", 2f), right[0]),
                listOf(PadKey.text("N", 0f), PadKey("", PadAction.Text(","), 1f, word = R.string.keyboard_pause), PadKey.text(",", 2f), right[1]),
                listOf(PadKey.text("*", 0f), PadKey("", PadAction.Text(";"), 1f, word = R.string.keyboard_wait), PadKey.text("#", 2f), right[2]),
                listOf(PadKey("", PadAction.Go(Pad.Phone), 0f, word = R.string.keyboard_page_numbers), PadKey.text("+", 1f), PadKey.text(".", 2f),
                    PadKey("", PadAction.Enter, 3f, modifier = true)))
            Pad.Symbols -> {
                val w = 7f / 3
                listOf(digits("123", 1.5f, w) + PadKey.text("%", 8.5f, 1.5f, modifier = true),
                    digits("456", 1.5f, w) + PadKey("", PadAction.Space, 8.5f, 1.5f, modifier = true),
                    digits("789", 1.5f, w) + PadKey("", PadAction.Delete, 8.5f, 1.5f, modifier = true),
                    listOf(PadKey("", PadAction.Letters, 0f, 1.5f, modifier = true, word = R.string.keyboard_letters_label), PadKey.text(",", 1.5f, modifier = true),
                        PadKey("", PadAction.SymbolsPage, 2.5f, 4f / 3, word = R.string.keyboard_pad_symbols), PadKey.text("0", 2.5f + 4f / 3, w),
                        PadKey.text("=", 2.5f + 4f / 3 + w, 4f / 3), PadKey.text(".", 7.5f, 1f, modifier = true),
                        PadKey("", PadAction.Enter, 8.5f, 1.5f, modifier = true)))
            }
        }
    }
}
