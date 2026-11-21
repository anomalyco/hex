package ly.anoma.hex.keyboard

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.view.View
import ly.anoma.hex.R
import kotlin.math.abs
import kotlin.math.sqrt

/** The recording bar's dot waveform: the latest levels, scrolling right to left. */
internal class DotScopeView(context: Context) : View(context) {
    private val levels = FloatArray(66)
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val ink = context.getColor(R.color.ink)
    private var tick = 0
    fun clear() { tick = 0; levels.fill(0f); invalidate() }
    fun push(rms: Float) {
        levels.copyInto(levels, 0, 1, levels.size)
        levels[levels.lastIndex] = (sqrt(rms.coerceAtLeast(0f)) * 3.2f).coerceIn(0f, 1f)
        tick++; invalidate()
    }
    override fun onDraw(canvas: Canvas) {
        val step = resources.displayMetrics.density * 5f
        val columns = (width / step).toInt().coerceAtLeast(1)
        val radius = resources.displayMetrics.density * .95f
        paint.color = ink
        for (x in 0 until columns) {
            val level = levels[(x * levels.size / columns).coerceAtMost(levels.lastIndex)]
            val wave = level * kotlin.math.sin((x + tick) * .55f) * 2.65f
            for (y in -3..3) {
                val edge = (1f - abs(y - wave)).coerceIn(0f, 1f)
                paint.alpha = (26 + 220 * edge).toInt()
                canvas.drawCircle((x + .5f) * width / columns, height / 2f + y * step * .72f, radius, paint)
            }
        }
    }
}
