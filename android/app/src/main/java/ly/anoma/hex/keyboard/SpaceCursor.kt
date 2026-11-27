package ly.anoma.hex.keyboard

import android.view.KeyEvent
import java.text.BreakIterator
import kotlin.math.abs
import kotlin.math.sign

/** The space bar's touch, as Gboard's (black box, Pixel 8 emulator, API 36): a slide moves the cursor
 * along the line at once, one character after [start] of travel and one more each [step]; a press
 * that lifts within [QUICK_MS] types a space however far it drifted (Gboard: 150 ms always, 200 ms
 * for short drifts), and so does any press that never
 * moved the cursor. Only Hex's long press differs: held still, it opens the dictation-language menu, or with
 * Labs › Keyboard › Hold space to move the cursor it becomes a trackpad ([hold]), as iPhone Hex's space bar. */
internal class SpaceGesture(private val start: Float, private val step: Float, private val stillSlop: Float) {
    companion object { const val QUICK_MS = 180L }
    var sliding = false
        private set
    /** Held still past the hold delay with Labs › Keyboard › Hold space to move the cursor on ([hold]). */
    var held = false
        private set
    private var originX = 0f
    private var lastX = 0f
    private var originY = 0f
    private var downAt = 0L
    private var moved = 0f

    fun down(x: Float, y: Float, time: Long) { sliding = false; held = false; originX = x; lastX = x; originY = y; downAt = time; moved = 0f }

    /** The finger moved: how many characters the cursor should be from where it was, or null until it slides. */
    fun move(x: Float, y: Float, time: Long): Int? {
        val dx = x - originX
        lastX = x
        if (held) return dx.sign.toInt() * (abs(dx) / step).toInt()
        moved = maxOf(moved, abs(dx), abs(y - originY))
        if (!sliding && abs(dx) >= start && time - downAt >= QUICK_MS) sliding = true
        return if (sliding) dx.sign.toInt() * characters(abs(dx)) else null
    }

    fun characters(distance: Float) = if (distance < start) 0 else ((distance - start) / step).toInt() + 1

    /** The press was held still past the hold delay: the bar is a trackpad from where the finger is now, as iPhone
     * Hex's (one character each [step], no [start] to travel first), and lifting types nothing, moved or not. */
    fun hold() { held = true; sliding = true; originX = lastX }

    /** Still enough for a long press to mean the menu. */
    val still get() = moved < stillSlop

    /** The finger lifted: true when the press types its space. */
    fun up() = !sliding
}

/** Moves the cursor whole characters (grapheme clusters, so 👍🏽 is one) from where a slide began: left
 * from the selection's start, right from its end. Without a known position, or in a password field
 * whose text it shouldn't read, it steps with arrow keys instead. */
internal class CursorSlide(private val target: KeyboardTextTarget, private val start: Int, private val end: Int, readable: Boolean) {
    private val window = 2000
    private val left: IntArray
    private val right: IntArray
    private val arrows = !readable || start < 0 || end < 0
    private var at = 0

    init {
        left = if (arrows) IntArray(0) else boundaries(target.before(window).orEmpty(), backwards = true)
        right = if (arrows) IntArray(0) else boundaries(target.after(window).orEmpty(), backwards = false)
    }

    fun moveTo(characters: Int) {
        if (arrows) {
            while (at > characters) { target.sendKey(KeyEvent.KEYCODE_DPAD_LEFT); at-- }
            while (at < characters) { target.sendKey(KeyEvent.KEYCODE_DPAD_RIGHT); at++ }
            return
        }
        val next = characters.coerceIn(-left.size, right.size)
        if (next == at) return
        at = next
        val position = when {
            next < 0 -> start - left[-next - 1]
            next > 0 -> end + right[next - 1]
            else -> if (start == end) start else return
        }
        target.setSelection(position, position)
    }

    private companion object {
        /** UTF-16 distances from the cursor to each character boundary, nearest first. */
        fun boundaries(text: String, backwards: Boolean): IntArray {
            if (text.isEmpty()) return IntArray(0)
            val it = BreakIterator.getCharacterInstance().apply { setText(text) }
            val out = ArrayList<Int>()
            if (backwards) {
                var p = it.last()
                while (true) { p = it.previous(); if (p == BreakIterator.DONE) break; out.add(text.length - p) }
            } else {
                var p = it.first()
                while (true) { p = it.next(); if (p == BreakIterator.DONE) break; out.add(p) }
            }
            return out.toIntArray()
        }
    }
}
