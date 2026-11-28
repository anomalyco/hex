package ly.anoma.hex.keyboard

import android.content.Context
import android.view.View
import android.view.ViewGroup
import kotlin.math.roundToInt

/** One row of keys on a grid of cells. Each key's keycap spans its cells less half the gap on each
 * side; its touch target runs to the middle of the gaps around it, and the first and last keys'
 * reach the row's ends, so every point of the row types a key. A key's padding is its keycap's
 * place inside its target. Keys are read left to right by where they start, and a gone key takes no
 * room, so [place] can move a key in place without taking it out of the row (a finger already on
 * another key keeps its press). */
internal class KeyRowLayout(context: Context, private val cells: Float, private val side: Float,
                            private val gap: Float, private val insetTop: Int, private val insetBottom: Int) : ViewGroup(context) {
    private class Span(val start: Float, val width: Float) : LayoutParams(MATCH_PARENT, MATCH_PARENT)

    fun add(view: View, start: Float, width: Float) = addView(view, Span(start, width))

    /** Moves [view], already in the row, to [start] and [width] cells. */
    fun place(view: View, start: Float, width: Float) {
        val span = view.layoutParams as Span
        if (span.start != start || span.width != width) view.layoutParams = Span(start, width)
    }

    private fun span(v: View) = v.layoutParams as Span
    private fun shown(): List<View> {
        val out = ArrayList<View>(childCount)
        for (i in 0 until childCount) getChildAt(i).takeIf { it.visibility != GONE }?.let(out::add)
        out.sortBy { span(it).start }
        return out
    }
    private fun capLeft(v: View, cell: Float): Int = (side + span(v).start * cell + gap / 2).roundToInt()
    private fun capRight(v: View, cell: Float): Int = span(v).let { (side + (it.start + it.width) * cell - gap / 2).roundToInt() }
    private fun hitLeft(keys: List<View>, i: Int, cell: Float) = if (i == 0) 0 else (capRight(keys[i - 1], cell) + capLeft(keys[i], cell)) / 2
    private fun hitRight(keys: List<View>, i: Int, cell: Float, width: Int) =
        if (i == keys.lastIndex) width else (capRight(keys[i], cell) + capLeft(keys[i + 1], cell)) / 2
    private fun cell(width: Int) = (width - 2 * side) / cells

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val width = MeasureSpec.getSize(widthMeasureSpec)
        val height = MeasureSpec.getSize(heightMeasureSpec)
        setMeasuredDimension(width, height)
        val cell = cell(width)
        val keys = shown()
        for ((i, child) in keys.withIndex()) {
            val left = hitLeft(keys, i, cell); val right = hitRight(keys, i, cell, width)
            child.setPadding(capLeft(child, cell) - left, insetTop, right - capRight(child, cell), insetBottom)
            child.measure(MeasureSpec.makeMeasureSpec(right - left, MeasureSpec.EXACTLY), MeasureSpec.makeMeasureSpec(height, MeasureSpec.EXACTLY))
        }
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        val cell = cell(r - l)
        val keys = shown()
        for ((i, child) in keys.withIndex()) child.layout(hitLeft(keys, i, cell), 0, hitRight(keys, i, cell, r - l), b - t)
    }
}
