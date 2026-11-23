package ly.anoma.hex.keyboard

import android.animation.ValueAnimator
import android.graphics.Canvas
import android.graphics.Paint
import android.os.SystemClock
import android.view.View
import java.util.concurrent.Executor
import java.util.concurrent.Executors
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.SwipeReader
import kotlin.math.hypot

/** Keyboard › Swipe typing's text rules, as Gboard places a glided word (black box, Pixel 8 emulator, API 36) and
 * iPhone Hex where Gboard says nothing. */
internal object SwipeText {
    /** Whether a swiped word needs a space before it: unless the text before the cursor is empty or ends in a
     * space, a line break or opening punctuation (a bracket, an opening quote, ¿ or ¡, or a mark that starts a word
     * such as @ # /). A straight quote opens when nothing or a space comes before it. */
    fun needsSpace(before: String?): Boolean {
        val last = before?.lastOrNull() ?: return false
        if (last.isWhitespace() || last == '\uFFFC' || last in OPENING) return false
        if (last == '"' || last == '\'') {
            val previous = before.getOrNull(before.length - 2)
            return !(previous == null || previous.isWhitespace() || previous in OPENING)
        }
        return true
    }
    private const val OPENING = "([{<«‹“‘„¿¡@#/"
}

/** Words Delete took back right after a swipe typed them, at one place in the text: the next swipe there types its
 * next reading instead (Gboard: "there" deleted, the same path swiped again types "they're"). One place: the text
 * before it is the same, trailing spaces aside. Any other edit forgets them. */
internal class SwipeRetries {
    private var place: String? = null
    private val words = ArrayList<String>()

    /** [readings] with the words taken back where [before] ends moved after the others. */
    fun reordered(readings: List<String>, before: String?): List<String> {
        if (words.isEmpty() || place(before) != place) return readings
        return readings.filter { it.lowercase() !in words } + readings.filter { it.lowercase() in words }
    }

    /** [reordered], for a swipe going in now; one somewhere else forgets the words. */
    fun typing(readings: List<String>, before: String?): List<String> {
        if (place(before) != place) forget()
        return reordered(readings, before)
    }

    /** Delete took back [word], which had [before] before it. */
    fun tookBack(word: String, before: String?) {
        val at = place(before)
        if (at != place) { words.clear(); place = at }
        words.add(word.lowercase())
    }

    fun forget() { place = null; words.clear() }

    private fun place(before: String?) = before.orEmpty().trimEnd(' ')
}

/** The light trail behind a swiping finger, drawn over the keys by [KeyPlaneView]: the last [TAIL_MS] of its path,
 * shortening behind the finger (iPhone Hex's), in the keys' ink; it fades out when the finger lifts, animated as the
 * strip's recording bar is, only while Android's animations are on. */
internal class SwipeTrail(private val view: View, color: Int, width: Float) {
    private companion object {
        const val TAIL_MS = 450L
        const val FADE_MS = 200L
        const val MAX = 512
    }
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND; strokeWidth = width; this.color = color
    }
    private val alpha = android.graphics.Color.alpha(color)
    private val xs = FloatArray(MAX); private val ys = FloatArray(MAX); private val at = LongArray(MAX)
    private val path = android.graphics.Path()
    private var count = 0
    private var live = false
    private var fade = 1f
    private var fading: ValueAnimator? = null

    val showing get() = count >= 2

    fun begin(points: List<FloatArray>) {
        fading?.cancel(); fading = null; fade = 1f
        count = 0; live = true
        val now = SystemClock.uptimeMillis()
        for (p in points) add(p[0], p[1], now)
    }

    fun add(x: Float, y: Float, time: Long = SystemClock.uptimeMillis()) {
        if (count == MAX) { System.arraycopy(xs, 1, xs, 0, MAX - 1); System.arraycopy(ys, 1, ys, 0, MAX - 1); System.arraycopy(at, 1, at, 0, MAX - 1); count-- }
        xs[count] = x; ys[count] = y; at[count] = time; count++
        view.postInvalidateOnAnimation()
    }

    /** The finger lifted: the trail fades out. */
    fun fade() {
        live = false
        if (!showing || !ValueAnimator.areAnimatorsEnabled()) { clear(); return }
        fading = ValueAnimator.ofFloat(1f, 0f).apply {
            duration = FADE_MS
            addUpdateListener { fade = it.animatedValue as Float; view.invalidate() }
            addListener(object : android.animation.AnimatorListenerAdapter() {
                override fun onAnimationEnd(animation: android.animation.Animator) { if (fading === animation) clear() }
            })
            start()
        }
    }

    fun clear() {
        fading?.cancel(); fading = null
        live = false; count = 0; fade = 1f
        view.invalidate()
    }

    fun draw(canvas: Canvas) {
        if (count < 2) return
        val now = SystemClock.uptimeMillis()
        // Points older than the tail drop off behind the finger, down to its last stretch.
        var first = 0
        while (first < count - 2 && now - at[first] > TAIL_MS) first++
        path.rewind()
        path.moveTo(xs[first], ys[first])
        for (i in first + 1 until count) path.lineTo(xs[i], ys[i])
        paint.alpha = (alpha * fade).toInt().coerceIn(0, 255)
        canvas.drawPath(path, paint)
        if (live) view.postInvalidateOnAnimation()
    }
}

/** One letter touch that may become a swipe, and the swipe it became: its points in the keys' coordinates (the
 * KeyGrid's) and their times. */
internal class SwipeStroke(val pointer: Int) {
    var xs = DoubleArray(128); var ys = DoubleArray(128); var ts = DoubleArray(128)
    var count = 0
    fun add(x: Double, y: Double, seconds: Double) {
        if (count == xs.size) { xs = xs.copyOf(count * 2); ys = ys.copyOf(count * 2); ts = ts.copyOf(count * 2) }
        xs[count] = x; ys[count] = y; ts[count] = seconds; count++
    }
    fun travelled() = if (count < 2) 0.0 else hypot(xs[count - 1] - xs[0], ys[count - 1] - ys[0])
    /** Whether every point stayed within [reach] of the first. */
    fun within(reach: Double): Boolean { for (i in 0 until count) if (hypot(xs[i] - xs[0], ys[i] - ys[0]) > reach) return false; return true }
}

/** Reads a swipe's path so far off the main thread, at most every [INTERVAL_MS], for the strip while the finger
 * moves (iPhone: 70 ms). Results for an older swipe are dropped. */
internal class SwipePreview(private val post: (Runnable) -> Unit) {
    companion object { const val INTERVAL_MS = 70L }
    private val worker: Executor by lazy { Executors.newSingleThreadExecutor { Thread(it, "Hex swipe").apply { isDaemon = true } } }
    private var inFlight = false
    private var last = 0L
    var generation = 0
        private set

    fun next() { generation++; last = 0L }

    fun schedule(stroke: SwipeStroke, reader: SwipeReader, grid: KeyGrid, previous: String?, show: (List<String>) -> Unit) {
        val now = SystemClock.uptimeMillis()
        if (inFlight || stroke.count < 3 || now - last < INTERVAL_MS) return
        inFlight = true; last = now
        val n = stroke.count
        val xs = stroke.xs.copyOf(n); val ys = stroke.ys.copyOf(n); val ts = stroke.ts.copyOf(n)
        val generation = generation
        worker.execute {
            val words = reader.ranked(reader.decode(xs, ys, ts, n, grid, previous, 3).candidates)
            post(Runnable { inFlight = false; if (generation == this.generation) show(words) })
        }
    }
}
