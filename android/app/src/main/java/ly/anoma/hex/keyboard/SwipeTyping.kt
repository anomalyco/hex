package ly.anoma.hex.keyboard

import android.animation.ValueAnimator
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.accessibility.AccessibilityManager
import kotlin.math.hypot
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.KeyPoint
import ly.anoma.hex.typing.KeyboardCore

/**
 * Settings › Keyboard › Swipe typing's touches (iOS `SwipeTyping`): slide from a letter through the
 * letters of a word and lift, and the word goes in. A touch that goes down on a letter key becomes a
 * swipe once it travels more than a key's pitch (width and gap) from where it went down: its key no
 * longer types, a light trail follows the finger, and the strip shows what the path reads as so far,
 * decoded off the main thread at most every [PREVIEW_MS]. Lifting types the best reading
 * ([KeyboardEngine.endSwipe]); another finger going down ends the swipe first. English and Spanish,
 * the letters page of an ordinary text field, never under TalkBack, never once an accent menu is open.
 * Points are in the key rows' coordinates, as the tap decoder's [KeyGrid].
 */
internal class SwipeTyping(private val view: View, private val engine: KeyboardEngine, private val trail: SwipeTrail) {
    private class Stroke {
        val points = ArrayList<Double>()
        val times = ArrayList<Double>()
        fun add(x: Double, y: Double, ms: Long) { points.add(x); points.add(y); times.add(ms / 1000.0) }
        val first get() = KeyPoint(points[0], points[1])
    }
    /** The swipe under way: its key, stroke, what it reads with, and how its key types a tap. */
    private class Active(val key: Any, val stroke: Stroke, val session: KeyboardEngine.SwipeSession, val grid: KeyGrid,
                         val letter: Char, val liftedOn: (KeyPoint) -> Boolean, val tap: (KeyPoint) -> Unit)

    /** Letter touches that may still become swipes. */
    private val pending = HashMap<Any, Stroke>()
    private var active: Active? = null
    /** Bumped by every swipe's start and end, so a preview of an older one is dropped. */
    private var generation = 0
    private var previewing = false
    private var lastPreview = 0L
    private val accessibility = view.context.getSystemService(AccessibilityManager::class.java)

    fun owns(key: Any) = active?.key === key

    /** A letter key's touch went down at [point]. */
    fun began(key: Any, point: KeyPoint, ms: Long) {
        pending[key] = Stroke().apply { add(point.x, point.y, ms) }
    }

    /**
     * The touch moved ([e] in the key's own coordinates, [origin] the key's place in the rows'): true when it
     * is a swipe, now or already, and the key leaves it to this. It becomes one past [grid]'s pitch when
     * [allowed] (no accent menu open) and the engine has a reader; [start] then tells the key.
     */
    fun moved(key: Any, e: MotionEvent, origin: KeyPoint, grid: KeyGrid?, allowed: Boolean, letter: Char,
              liftedOn: (KeyPoint) -> Boolean, tap: (KeyPoint) -> Unit, start: () -> Unit): Boolean {
        active?.takeIf { it.key === key }?.let { swipe ->
            add(swipe.stroke, e, origin, trail)
            preview(swipe)
            return true
        }
        val stroke = pending[key] ?: return false
        add(stroke, e, origin, null)
        val last = stroke.points.size - 2
        if (grid == null || hypot(stroke.points[last] - stroke.points[0], stroke.points[last + 1] - stroke.points[1]) <= grid.pitchX) return false
        pending.remove(key)
        if (!allowed || accessibility?.isTouchExplorationEnabled == true) return false
        val session = engine.swipeSession() ?: return false
        engine.beginSwipe()
        val swipe = Active(key, stroke, session, grid, letter, liftedOn, tap)
        active = swipe
        generation++
        lastPreview = 0
        trail.begin(stroke.points)
        start()
        preview(swipe)
        return true
    }

    /** The swipe's finger lifted at [point]: its word goes in. */
    fun ended(key: Any, point: KeyPoint, ms: Long) {
        val swipe = active?.takeIf { it.key === key } ?: return
        swipe.stroke.add(point.x, point.y, ms)
        finish(swipe)
    }

    /** Another finger went down: the swipe under way ends first, so its word goes in before that key's letter. */
    fun finishActive() { active?.let(::finish) }

    /** A touch that never became a swipe ended. */
    fun forget(key: Any) { pending.remove(key) }

    /** The system cancelled the swipe's touch: nothing goes in. */
    fun cancelled(key: Any) {
        pending.remove(key)
        if (active?.key !== key) return
        active = null; generation++
        trail.fade()
        engine.endSwipe(emptyList())
    }

    fun cancelAll() {
        pending.clear()
        if (active == null) return
        active = null; generation++
        trail.clear()
        engine.endSwipe(emptyList())
    }

    private fun finish(swipe: Active) {
        active = null; pending.remove(swipe.key); generation++
        trail.fade()
        val stroke = swipe.stroke
        val points = stroke.points.toDoubleArray()
        val words = swipe.session.reader.read(swipe.session.handles, points, stroke.times.toDoubleArray(), swipe.grid,
            swipe.session.previous, LIMIT)
        val end = KeyPoint(points[points.size - 2], points[points.size - 1])
        val key = if (swipe.liftedOn(end)) swipe.letter.lowercase() else null
        if (KeyboardCore.swipeIsSlidTap(points, swipe.grid.pitchX, key, words.firstOrNull())) {
            // A tap that slid: its letter goes in as a tap's would, where the finger went down.
            engine.endSwipe(emptyList())
            swipe.tap(stroke.first)
            return
        }
        engine.endSwipe(words)
    }

    /** Reads the path so far off the main thread, at most every [PREVIEW_MS], and shows its best word while
     * the swipe is still the same one. */
    private fun preview(swipe: Active) {
        val now = SystemClock.uptimeMillis()
        if (previewing || swipe.stroke.times.size < 3 || now - lastPreview < PREVIEW_MS) return
        previewing = true; lastPreview = now
        val points = swipe.stroke.points.toDoubleArray(); val times = swipe.stroke.times.toDoubleArray()
        val session = swipe.session; val grid = swipe.grid; val at = generation
        engine.swipeWork.execute {
            val words = session.reader.read(session.handles, points, times, grid, session.previous, PREVIEW_LIMIT)
            view.post {
                previewing = false
                if (generation == at && active != null) engine.showSwipePreview(words)
            }
        }
    }

    private fun add(stroke: Stroke, e: MotionEvent, origin: KeyPoint, trail: SwipeTrail?) {
        // The moves Android batched into this event first, so the path keeps every sample the finger made.
        for (i in 0 until e.historySize) {
            stroke.add(origin.x + e.getHistoricalX(i), origin.y + e.getHistoricalY(i), e.getHistoricalEventTime(i))
            trail?.add(origin.x + e.getHistoricalX(i), origin.y + e.getHistoricalY(i))
        }
        stroke.add(origin.x + e.x, origin.y + e.y, e.eventTime)
        trail?.add(origin.x + e.x, origin.y + e.y)
    }

    private companion object {
        const val PREVIEW_MS = 70L
        const val LIMIT = 8
        const val PREVIEW_LIMIT = 3
    }
}

/** The light trail behind a swiping finger (iOS `SwipeTrailView`): the last [TAIL_MS] of its path, which
 * shortens behind the finger and fades when it lifts. Drawn by the key plane over its keys, in the rows'
 * coordinates. */
internal class SwipeTrail(private val view: View, color: Int, width: Float) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND
        strokeWidth = width; this.color = color
    }
    private val alpha = paint.alpha
    private val xs = ArrayList<Float>(); private val ys = ArrayList<Float>(); private val at = ArrayList<Long>()
    private var drawing = false
    private var fading: ValueAnimator? = null
    private val path = Path()

    fun begin(points: List<Double>) {
        fading?.cancel(); fading = null
        clearPoints(); paint.alpha = alpha; drawing = true
        for (i in points.indices step 2) add(points[i], points[i + 1])
    }

    fun add(x: Double, y: Double) {
        xs.add(x.toFloat()); ys.add(y.toFloat()); at.add(SystemClock.uptimeMillis())
        view.postInvalidateOnAnimation()
    }

    /** The finger lifted: the trail fades out. */
    fun fade() {
        if (!drawing) return
        drawing = false
        fading = ValueAnimator.ofInt(alpha, 0).apply {
            duration = FADE_MS
            addUpdateListener { paint.alpha = it.animatedValue as Int; view.invalidate() }
            start()
        }
    }

    fun clear() { fading?.cancel(); fading = null; drawing = false; clearPoints(); view.invalidate() }

    fun draw(canvas: Canvas) {
        if (drawing) {
            // Points older than the tail drop off behind the finger, down to its last stretch.
            val old = at.indexOfLast { it < SystemClock.uptimeMillis() - TAIL_MS }
            val drop = minOf(old, at.size - 2)
            if (drop > 0) { xs.subList(0, drop).clear(); ys.subList(0, drop).clear(); at.subList(0, drop).clear() }
            view.postInvalidateOnAnimation()
        } else if (fading?.isRunning != true) { clearPoints(); return }
        if (xs.size < 2) return
        path.rewind(); path.moveTo(xs[0], ys[0])
        for (i in 1 until xs.size) path.lineTo(xs[i], ys[i])
        canvas.drawPath(path, paint)
    }

    private fun clearPoints() { xs.clear(); ys.clear(); at.clear() }

    private companion object {
        const val TAIL_MS = 450L
        const val FADE_MS = 200L
    }
}
