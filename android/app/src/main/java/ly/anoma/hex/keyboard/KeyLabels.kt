package ly.anoma.hex.keyboard

import kotlin.math.max
import kotlin.math.min

/** A label's shape per pixel of text size, read from its paint: [advance] is how wide its text runs (the wider of
 * its upper and lower case, so shift never makes it overflow), [ascent] and [descent] its line box above and below
 * the baseline, [cap] its capitals' height. */
internal data class TextShape(val advance: Float, val ascent: Float, val descent: Float, val cap: Float)

/** One keycap's label and corner hint, in px: their text sizes and the hint's distance from the top and end. */
internal data class KeyLabelSizes(val label: Float, val hint: Float, val hintTop: Float, val hintEnd: Float)

/** How big a key's label and corner hint are drawn. As Gboard's (Gboard 15.1, Pixel 8 emulator, black box): they
 * follow the keyboard, never the system's font size, which Gboard ignores at 1.3 and 2.0 alike. Every size is
 * given for Gboard's letter keycap on a Pixel 8, [REF_WIDTH] by [REF_HEIGHT] dp, and scales by the smallest of
 * three: the keycap's width against [REF_WIDTH]; the screen's own keycap height ([KeyRows], its "base") against
 * [REF_HEIGHT], so a 360 dp phone's letters are its 40.3 dp keys' share (Gboard's capitals are 0.41 of its keycap on
 * every screen); and, when Settings makes the rows shorter, [STRETCH] times the keycap's height against
 * [REF_HEIGHT] (Gboard's capitals never pass 0.48 of a shortened key, and never grow on a taller one). A label
 * never runs past its keycap's sides, and a letter never touches its corner hint: it shrinks until one clears the
 * other, sideways or below. */
internal object KeyLabels {
    const val REF_WIDTH = 35.43f
    const val REF_HEIGHT = 47f
    const val STRETCH = 1.17f
    /** Hex's row-1 digit and symbol hint at the reference keycap, and its distance from the top and end. */
    const val HINT = 9.5f
    const val HINT_TOP = 3f
    const val HINT_END = 4.5f
    /** Room kept clear at each side of a label, and between a letter and its hint. */
    const val SIDE = 3f
    const val CLEAR = 1f

    /** Sizes for a keycap [width] by [height] px at [density] on a screen whose keycaps are [base] dp tall, for a
     * label [nominal] dp at the reference keycap and shaped [label]; [hint] is the corner hint's shape, or null. */
    fun sizes(width: Float, height: Float, density: Float, base: Float, nominal: Float, label: TextShape?, hint: TextShape?): KeyLabelSizes {
        val scale = min(min(width / (REF_WIDTH * density), base / REF_HEIGHT), STRETCH * height / (REF_HEIGHT * density))
        val hintSize = HINT * density * scale
        val hintTop = HINT_TOP * density * scale
        val hintEnd = HINT_END * density * scale
        var size = nominal * density * scale
        if (label != null && label.advance > 0f) {
            size = min(size, (width - 2 * SIDE * density * scale) / label.advance)
            if (hint != null) {
                val clear = CLEAR * density * scale
                // Beside the hint: the centred letter's right edge stops short of the hint's left edge.
                val beside = (width - 2 * (hintEnd + hint.advance * hintSize + clear)) / label.advance
                // Under it: the centred line box puts the capitals' top at height/2 + ((ascent - descent)/2 - cap) * size.
                val hintBottom = hintTop + hint.ascent * hintSize + clear
                val lift = label.cap - (label.ascent - label.descent) / 2
                val under = if (lift > 0f) (height / 2 - hintBottom) / lift else Float.MAX_VALUE
                size = min(size, max(beside, under))
            }
        }
        return KeyLabelSizes(max(size, 0f), hintSize, hintTop, hintEnd)
    }
}
