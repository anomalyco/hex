package ly.anoma.hex.keyboard

import android.content.res.Configuration
import android.content.res.Resources

/** One row's height in dp: [pitch] from its top to the next row's top, and its keycaps' [key] height. */
internal data class RowSize(val pitch: Float, val key: Float) {
    val gap get() = pitch - key
}

/** The rows while the number row shows: its [digits] row, and the [letters] rows under it. */
internal data class NumberRows(val letters: RowSize, val digits: RowSize)

/** How tall Gboard's rows are (Gboard 15.1, Pixel 8 emulator, API 36, black box on 13 screen sizes): its keycaps
 * and row step follow the screen's smallest width in three steps and nothing else, not its height, density or font
 * size. From 400 dp wide, 46.9 dp keys every 59 dp (a Pixel 8); from 360, 40.3 every 50.3 (most Samsung and
 * 360-393 dp phones); below, 37.7 every 48 (a foldable's cover screen). Turned, 39.2 every 43. */
internal object KeyRows {
    fun of(smallestWidthDp: Float, landscape: Boolean): RowSize = when {
        landscape -> RowSize(KeyboardMetrics.ROW_LANDSCAPE, KeyboardMetrics.KEY_HEIGHT_LANDSCAPE)
        smallestWidthDp >= 400f -> RowSize(KeyboardMetrics.ROW, KeyboardMetrics.KEY_HEIGHT)
        smallestWidthDp >= 360f -> RowSize(50.3f, 40.3f)
        else -> RowSize(48f, 37.7f)
    }

    /** The rows with Labs › Keyboard › Number row on, as Gboard's (its Number row, same screens): the letter rows
     * close up to [NUMBER_ROW_LETTERS] of their step with the same keys, and the number row's step is
     * [NUMBER_ROW] of theirs upright ([NUMBER_ROW_LANDSCAPE] turned), with the letters' new gap. A Pixel 8's
     * letters go every 56.4 dp and its digits every 52.6 dp (43.1 dp keys): the keyboard grows by 45 dp, not a
     * whole 59 dp row (Gboard: 47, its bottom row 2 dp taller too), and turned by 30 dp, not 43. [letters] is how the rows are without it. */
    fun withNumberRow(letters: RowSize, landscape: Boolean): NumberRows {
        val pitch = letters.pitch * NUMBER_ROW_LETTERS
        val gap = maxOf(pitch - letters.key, MIN_GAP)
        val digits = letters.pitch * if (landscape) NUMBER_ROW_LANDSCAPE else NUMBER_ROW
        return NumberRows(RowSize(pitch, pitch - gap), RowSize(digits, digits - gap))
    }
    const val NUMBER_ROW_LETTERS = 0.956f
    const val NUMBER_ROW = 0.892f
    const val NUMBER_ROW_LANDSCAPE = 0.814f
    private const val MIN_GAP = 2f

    fun of(resources: Resources): RowSize {
        val config = resources.configuration
        return of(config.smallestScreenWidthDp.toFloat(), config.orientation == Configuration.ORIENTATION_LANDSCAPE)
    }

    /** The keycap height a key's labels are sized for, in dp ([KeyLabels]): the row's own upright. Turned,
     * Gboard's letters stay larger than its 39 dp keys alone would give (17.5 dp capitals). */
    const val LANDSCAPE_LABEL_BASE = 43.2f
    fun labelBase(resources: Resources): Float =
        if (resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE) LANDSCAPE_LABEL_BASE else of(resources).key
}
