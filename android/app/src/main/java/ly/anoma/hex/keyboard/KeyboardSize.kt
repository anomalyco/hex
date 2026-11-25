package ly.anoma.hex.keyboard

import android.content.res.Resources

/** Settings › Keyboard › Height, named as Gboard named its heights before it replaced them with Resize. Each step
 * is the rows' step as a share of Normal's, the screen's own ([KeyRows]); the gaps between rows stay, so only the
 * keys grow or shrink, as Gboard's Resize does (Gboard 15.1, emulator: from 0.69 to 1.62 of the step, its gaps
 * always 12 dp). The strip, the keys' widths and the space under the bottom row never change. Saved by [key]. */
internal enum class KeyboardHeight(val key: String, val share: Float) {
    ExtraShort("extra_short", 0.75f),
    Short("short", 0.83f),
    MidShort("mid_short", 0.92f),
    Normal("normal", 1f),
    MidTall("mid_tall", 1.08f),
    Tall("tall", 1.17f),
    ExtraTall("extra_tall", 1.25f);

    /** [rows] at this height: the step scaled, the gap under the keys kept. */
    fun of(rows: RowSize): RowSize {
        val pitch = rows.pitch * share
        return RowSize(pitch, pitch - rows.gap)
    }

    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key } ?: Normal
    }
}

/** The rows one keyboard view was built with: this screen's ([KeyRows]) at [height]. */
internal class KeySize private constructor(val height: KeyboardHeight, val rows: RowSize) {
    companion object {
        fun of(resources: Resources, height: KeyboardHeight) = KeySize(height, height.of(KeyRows.of(resources)))
    }
}
