package ly.anoma.hex.keyboard

import kotlin.math.roundToInt

/** Android 11's inline autofill suggestions (Labs › Keyboard › Password manager suggestions):
 * whether to ask for them and how the strip shows what comes back. Android's autofill service
 * builds the chips and fills the field when one is tapped; Hex only asks for them and lays them
 * out, so it never reads a login, a code or a card. Kept pure so the decisions can be tested
 * without a view or the framework. */
internal object InlineAutofill {
    /** The most suggestions asked for: one scrolling line of small chips. */
    const val MAX = 6
    /** A chip's height and width range in dp: the strip's words are 30 dp tall and at least 56 dp
     * wide to tap, and a login's title and subtitle need no more than 240. */
    const val CHIP = 30f
    const val MIN_WIDTH = 56f
    const val MAX_WIDTH = 240f

    /** Whether to ask the system at all: with the lab off Hex asks for none, so Android keeps its
     * own autofill dropdown and the keyboard is exactly as it was. */
    fun requests(lab: Boolean) = lab

    /** The suggestions to show, in Android's order but with the pinned ones last: a pinned
     * suggestion can outlive its field, so it stays out of the way of the ones that answer it. */
    fun ordered(pinned: List<Boolean>): List<Int> =
        pinned.indices.filter { !pinned[it] } + pinned.indices.filter { pinned[it] }

    /** A chip's height in px: [CHIP] dp, never taller than the [strip] it sits in. */
    fun chipHeight(strip: Int, density: Float): Int = minOf((CHIP * density).roundToInt(), strip)

    /** A chip's minimum and maximum width in px. */
    fun chipWidths(density: Float): IntRange =
        (MIN_WIDTH * density).roundToInt()..(MAX_WIDTH * density).roundToInt()
}
