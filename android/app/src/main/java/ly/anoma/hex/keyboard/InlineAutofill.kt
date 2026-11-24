package ly.anoma.hex.keyboard

import kotlin.math.roundToInt

/** Android 11's inline autofill suggestions (Labs › Keyboard › Password manager suggestions): how
 * the strip shows what Android's autofill service sends back. The service builds the chips and fills
 * the field when one is tapped; Hex only lays them out, so it never reads a login, a code or a card.
 * Kept pure so the decisions can be tested without a view or the framework. */
internal object InlineAutofill {
    /** The most suggestions asked for: one scrolling line of small chips. */
    const val MAX = 6
    /** A chip's height and width range in dp: the strip's words are 30 dp tall and at least 56 dp
     * wide to tap, and a login's title and subtitle need no more than 240. */
    const val CHIP = 30f
    const val MIN_WIDTH = 56f
    const val MAX_WIDTH = 240f

    /** Android's suggestions split by its pinned flag: [unpinned] fill the scrolling row in
     * Android's order and [pinned] the fixed row at its edge, since pinned UI may not scroll. Each
     * list keeps Android's order. */
    data class Split(val unpinned: List<Int>, val pinned: List<Int>)

    fun split(pinned: List<Boolean>): Split = Split(
        unpinned = pinned.indices.filter { !pinned[it] },
        pinned = pinned.indices.filter { pinned[it] })

    /** A chip's height in px: [CHIP] dp, never taller than the [strip] it sits in. */
    fun chipHeight(strip: Int, density: Float): Int = minOf((CHIP * density).roundToInt(), strip)

    /** A chip's minimum and maximum width in px. */
    fun chipWidths(density: Float): IntRange =
        (MIN_WIDTH * density).roundToInt()..(MAX_WIDTH * density).roundToInt()
}
