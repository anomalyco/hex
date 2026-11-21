package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import ly.anoma.hex.R
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.KeyPoint
import ly.anoma.hex.typing.KeyRect
import kotlin.math.roundToInt

/** The keys: three rows from the page's layout, then ?123, comma, space, period and enter. Every
 * point of the area types a key (KeyRowLayout); the bottom row stays mounted across page changes. */
internal class KeyPlaneView(context: Context, private val ime: HexKeyboard, private val host: KeyboardView,
                            private val feedback: LetterFeedback) : LinearLayout(context) {
    private val density = resources.displayMetrics.density
    private fun dp(value: Int) = (value * density).roundToInt()
    private fun px(value: Float) = (value * density).roundToInt()
    private val caps = KeyCaps(context) { ime.feedback.haptics }
    private val ink = caps.ink
    private val modifier = caps.modifier
    private val quiet = context.getColor(R.color.quiet)
    private val landscape = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
    private val rowHeight = px(if (landscape) KeyboardMetrics.ROW_LANDSCAPE else KeyboardMetrics.ROW)
    private val keyHeight = px(if (landscape) KeyboardMetrics.KEY_HEIGHT_LANDSCAPE else KeyboardMetrics.KEY_HEIGHT)
    private val insetTop = (rowHeight - keyHeight) / 2
    private val insetBottom = rowHeight - keyHeight - insetTop
    private val bottomExtra = px(if (landscape) KeyboardMetrics.BOTTOM_LANDSCAPE else KeyboardMetrics.BOTTOM)
    private val reach = if (landscape) 0 else px(KeyboardMetrics.ROW1_REACH)
    private val side = KeyboardMetrics.SIDE * density
    private val gap = KeyboardMetrics.GAP * density
    val menuHeight get() = rowHeight * KeyboardMetrics.MENU_ROWS + bottomExtra + reach
    private val letters = LinkedHashMap<Char, KeyCap>()
    private var grid: KeyGrid? = null
    private var pendingAccents: Runnable? = null
    private val rows = LinearLayout(context)
    private val bottom = KeyRowLayout(context, 10f, side, gap, insetTop, insetBottom + bottomExtra)
    private val modeKey: KeyCap
    private val enterKey: KeyCap
    private val enterIcon = ImageView(context)
    private val spaceKey: KeyCap
    private var deleteKey: KeyCap? = null
    private var deleting = false
    private var repeated = false
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    private val switchLanguage = Runnable {
        if (spaceKey.isPressed) { spaceKey.isPressed = false; spaceKey.performLongClick() }
    }
    private val repeatDelete = object : Runnable {
        override fun run() {
            if (deleting) { repeated = true; ime.engine.delete(); postDelayed(this, 65) }
        }
    }

    init {
        orientation = VERTICAL
        rows.orientation = VERTICAL
        addView(rows, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        addView(bottom, LayoutParams(LayoutParams.MATCH_PARENT, rowHeight + bottomExtra))
        modeKey = caps.cap(context.getString(R.string.keyboard_numbers_label), context.getString(R.string.keyboard_numbers),
            modifier, 17f, ime.engine::mode)
        val comma = caps.cap(",", context.getString(R.string.keyboard_comma), modifier) { ime.engine.type(",") }
        spaceKey = caps.cap("", context.getString(R.string.keyboard_space), caps.key, 14f) { ime.engine.type(" ") }
        spaceKey.label?.setTextColor(quiet)
        spaceKey.setOnLongClickListener { host.openLanguageMenu(); true }
        spaceKey.setOnTouchListener { v, e ->
            when (e.actionMasked) {
                MotionEvent.ACTION_DOWN -> { v.isPressed = true; postDelayed(switchLanguage, ViewConfiguration.getLongPressTimeout().toLong()) }
                MotionEvent.ACTION_MOVE -> if (e.x < -slop || e.x > v.width + slop || e.y < -slop || e.y > v.height + slop) {
                    removeCallbacks(switchLanguage); v.isPressed = false
                }
                MotionEvent.ACTION_UP -> { removeCallbacks(switchLanguage); val tap = v.isPressed; v.isPressed = false; if (tap) v.performClick() }
                MotionEvent.ACTION_CANCEL -> { removeCallbacks(switchLanguage); v.isPressed = false }
            }
            true
        }
        val period = caps.cap(".", context.getString(R.string.keyboard_period), modifier) { ime.engine.type(".") }
        enterKey = caps.cap("", context.getString(R.string.keyboard_return), modifier, 17f, ime.engine::enter)
        enterIcon.setImageResource(R.drawable.ic_enter)
        enterIcon.imageTintList = ColorStateList.valueOf(ink)
        enterKey.face.addView(enterIcon, FrameLayout.LayoutParams(dp(24), dp(24), Gravity.CENTER))
        var at = 0f
        for ((key, width) in listOf(modeKey to KeyboardMetrics.MODE, comma to KeyboardMetrics.COMMA, spaceKey to KeyboardMetrics.SPACE,
                period to KeyboardMetrics.PERIOD, enterKey to KeyboardMetrics.ENTER)) { bottom.add(key, at, width); at += width }
    }

    fun showPage(page: KeyboardPage) {
        clearCallout(); grid = null; letters.clear()
        rows.removeAllViews()
        val language = page.language
        val symbols = page.symbols
        val lines = page.lines
        val columns = KeyboardMetrics.columns(lines).toFloat()
        val side = KeyboardMetrics.sideWidth(page, lines)
        for ((index, line) in lines.withIndex()) {
            val top = if (index == 0) reach else 0
            val row = KeyRowLayout(context, columns, this.side, gap, insetTop + top, insetBottom)
            rows.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, rowHeight + top))
            // Rows 1 and 2 centre their letters; row 3's share what shift and delete leave.
            var at = if (index < 2) (columns - line.length) / 2f else 0f
            val width = if (index < 2) 1f else (columns - 2 * side) / line.length
            if (index == 2) {
                val switch = if (symbols) caps.cap(context.getString(if (page.more) R.string.keyboard_page_numbers else R.string.keyboard_page_symbols),
                    context.getString(R.string.keyboard_more_symbols), modifier, 15f, ime.engine::shift)
                else caps.icon(when { page.locked -> R.drawable.ic_caps_lock; page.shifted -> R.drawable.ic_shift_on; else -> R.drawable.ic_shift },
                    context.getString(if (page.locked) R.string.keyboard_caps_lock else R.string.keyboard_shift), modifier, ime.engine::shift)
                row.add(switch, 0f, side); at = side
            }
            for (c in line) {
                val text = if (!symbols && page.shifted) c.uppercaseChar().toString() else c.toString()
                var touch: KeyPoint? = null
                val k = caps.cap(text, text) {
                    val g = if (symbols) null else geometry()
                    val p = touch ?: g?.centres?.get(c)
                    if (g != null && p != null) ime.engine.letter(text, p.x, p.y, g) else ime.engine.type(text)
                }
                row.add(k, at, width); at += width
                if (!symbols) letters[c] = k
                val accents = if (symbols) emptyList() else LetterFeedback.options(text.single(), language.layoutCode)
                if (accents.isNotEmpty()) k.setOnLongClickListener {
                    host.openAccents(accents); true
                }
                k.setOnTouchListener { v, e ->
                    when (e.actionMasked) {
                        MotionEvent.ACTION_DOWN -> {
                            clearCallout(); v.isPressed = true
                            touch = KeyPoint((v.left + e.x).toDouble(), (row.top + e.y).toDouble())
                            if (ime.feedback.popups && ime.previewAllowed) feedback.present(k.face, listOf(text))
                            if (accents.isNotEmpty()) {
                                pendingAccents = Runnable { if (v.isPressed) feedback.present(k.face, accents) }
                                postDelayed(pendingAccents!!, 400L)
                            }
                        }
                        MotionEvent.ACTION_MOVE -> {
                            if (feedback.choosing) feedback.move(xInHost(row, v, e.x))
                            else if (e.x < -slop || e.x > v.width + slop || e.y < -slop || e.y > v.height + slop) {
                                clearCallout(); v.isPressed = false
                            }
                        }
                        MotionEvent.ACTION_UP -> {
                            val selected = if (feedback.choosing) feedback.selected() else null
                            val pressed = v.isPressed; v.isPressed = false; clearCallout()
                            if (pressed && selected != null && selected != text) {
                                val g = geometry(); val p = touch
                                if (g != null && p != null) ime.engine.letter(selected, p.x, p.y, g, explicit = true) else ime.engine.type(selected)
                            } else if (pressed) v.performClick()
                            touch = null
                        }
                        MotionEvent.ACTION_CANCEL -> { v.isPressed = false; clearCallout(); touch = null }
                    }
                    true
                }
            }
            if (index == 2) {
                deleteKey = caps.icon(R.drawable.ic_delete, context.getString(R.string.keyboard_delete), modifier, ime.engine::delete).also { k ->
                    row.add(k, columns - side, side)
                    k.setOnTouchListener { v, e ->
                        when (e.actionMasked) {
                            MotionEvent.ACTION_DOWN -> {
                                repeated = false; deleting = true; v.isPressed = true
                                postDelayed(repeatDelete, 380)
                            }
                            MotionEvent.ACTION_UP -> {
                                removeCallbacks(repeatDelete); v.isPressed = false
                                if (deleting && !repeated) v.performClick()
                                deleting = false
                            }
                            MotionEvent.ACTION_CANCEL -> {
                                removeCallbacks(repeatDelete); deleting = false; v.isPressed = false
                            }
                        }
                        true
                    }
                }
            }
        }
        modeKey.label?.text = context.getString(if (symbols) R.string.keyboard_letters_label else R.string.keyboard_numbers_label)
        modeKey.contentDescription = context.getString(if (symbols) R.string.keyboard_letters else R.string.keyboard_numbers)
        val action = page.enter.label ?: actionLabel(page.enter.action)
        enterKey.label?.text = action.orEmpty()
        enterIcon.visibility = if (action == null) View.VISIBLE else View.GONE
        enterKey.contentDescription = action ?: context.getString(R.string.keyboard_return)
    }

    private fun actionLabel(action: Int) = when (action) {
        EditorInfo.IME_ACTION_DONE -> R.string.keyboard_action_done
        EditorInfo.IME_ACTION_GO -> R.string.keyboard_action_go
        EditorInfo.IME_ACTION_NEXT -> R.string.keyboard_action_next
        EditorInfo.IME_ACTION_SEARCH -> R.string.keyboard_action_search
        EditorInfo.IME_ACTION_SEND -> R.string.keyboard_action_send
        else -> null
    }?.let(context::getString)

    /** A touch's x in the host's coordinates, where the accent callout is drawn. */
    private fun xInHost(row: View, key: View, x: Float) = left + rows.left + row.left + key.left + x

    /** Each letter's drawn keycap centre, in the rows' coordinates, which the touches use too. */
    private fun capCentre(cap: KeyCap) = KeyPoint(cap.left + cap.paddingLeft + (cap.width - cap.paddingLeft - cap.paddingRight) / 2.0,
        (cap.parent as View).top + cap.paddingTop + (cap.height - cap.paddingTop - cap.paddingBottom) / 2.0)
    private fun spaceRect(): KeyRect? {
        if (spaceKey.width == 0) return null
        val top = bottom.top - rows.top + spaceKey.paddingTop.toDouble()
        return KeyRect(spaceKey.left + spaceKey.paddingLeft.toDouble(), top, spaceKey.right - spaceKey.paddingRight.toDouble(),
            top + spaceKey.height - spaceKey.paddingTop - spaceKey.paddingBottom)
    }
    private fun geometry(): KeyGrid? {
        grid?.let { return it }
        val q = letters['q'] ?: return null; val w = letters['w'] ?: return null
        if (q.width == 0 || w.left == q.left) return null
        val centres = letters.mapValues { (_, cap) -> capCentre(cap) }
        return KeyGrid(centres, centres.getValue('w').x - centres.getValue('q').x,
            centres.getValue('a').y - centres.getValue('q').y, spaceRect()).also { grid = it }
    }
    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        super.onLayout(changed, l, t, r, b)
        if (changed) grid = null
    }
    fun clearCallout() {
        pendingAccents?.let(::removeCallbacks); pendingAccents = null; feedback.clear()
    }
    fun refreshSpace() {
        val next = ime.preferences.options(ime.nextEnglish)
        spaceKey.contentDescription = context.getString(R.string.keyboard_space_language, next.spoken.label)
        spaceKey.label?.text = if (next.translate) context.getString(R.string.keyboard_space_translate, next.spoken.label) else next.spoken.label
        spaceKey.label?.textSize = if (next.translate) 12f else 14f
    }
    fun cancelGestures() {
        clearCallout()
        removeCallbacks(repeatDelete); removeCallbacks(switchLanguage)
        spaceKey.isPressed = false
        deleting = false
        deleteKey?.isPressed = false
    }
    override fun onDetachedFromWindow() { cancelGestures(); super.onDetachedFromWindow() }
}
