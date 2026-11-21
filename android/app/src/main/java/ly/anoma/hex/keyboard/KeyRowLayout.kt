package ly.anoma.hex.keyboard

import android.content.Context
import android.view.View
import android.view.ViewGroup
import kotlin.math.roundToInt

/** One row of keys on a grid of cells. Each key's keycap spans its cells less half the gap on each
 * side; its touch target runs to the middle of the gaps around it, and the first and last keys'
 * reach the row's ends, so every point of the row types a key. A key's padding is its keycap's
 * place inside its target. Add keys left to right. */
internal class KeyRowLayout(context: Context, private val cells: Float, private val side: Float,
                            private val gap: Float, private val insetTop: Int, private val insetBottom: Int) : ViewGroup(context) {
    private class Span(val start: Float, val width: Float) : LayoutParams(MATCH_PARENT, MATCH_PARENT)

    fun add(view: View, start: Float, width: Float) = addView(view, Span(start, width))

    private fun capLeft(i: Int, cell: Float): Int = (getChildAt(i).layoutParams as Span).let { (side + it.start * cell + gap / 2).roundToInt() }
    private fun capRight(i: Int, cell: Float): Int = (getChildAt(i).layoutParams as Span).let { (side + (it.start + it.width) * cell - gap / 2).roundToInt() }
    private fun hitLeft(i: Int, cell: Float) = if (i == 0) 0 else (capRight(i - 1, cell) + capLeft(i, cell)) / 2
    private fun hitRight(i: Int, cell: Float, width: Int) = if (i == childCount - 1) width else (capRight(i, cell) + capLeft(i + 1, cell)) / 2
    private fun cell(width: Int) = (width - 2 * side) / cells

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val width = MeasureSpec.getSize(widthMeasureSpec)
        val height = MeasureSpec.getSize(heightMeasureSpec)
        setMeasuredDimension(width, height)
        val cell = cell(width)
        for (i in 0 until childCount) {
            val child = getChildAt(i)
            val left = hitLeft(i, cell); val right = hitRight(i, cell, width)
            child.setPadding(capLeft(i, cell) - left, insetTop, right - capRight(i, cell), insetBottom)
            child.measure(MeasureSpec.makeMeasureSpec(right - left, MeasureSpec.EXACTLY), MeasureSpec.makeMeasureSpec(height, MeasureSpec.EXACTLY))
        }
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        val cell = cell(r - l)
        for (i in 0 until childCount) getChildAt(i).layout(hitLeft(i, cell), 0, hitRight(i, cell, r - l), b - t)
    }
}
