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
import kotlin.math.abs
import kotlin.math.roundToInt

/** The keys: three rows from the page's layout, and the bottom row with the Hex key, which stays
 * mounted across every page change so its original finger-up still ends a hold. */
internal class KeyPlaneView(context: Context, private val ime: HexKeyboard, private val host: KeyboardView,
                            private val feedback: LetterFeedback) : LinearLayout(context) {
    private fun dp(value: Int) = (value * resources.displayMetrics.density).roundToInt()
    private val caps = KeyCaps(context) { ime.feedback.haptics }
    private val ink = caps.ink
    private val modifier = caps.modifier
    private val quiet = context.getColor(R.color.quiet)
    private val keyHeight = dp(if (resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE)
        KeyboardMetrics.KEY_HEIGHT_LANDSCAPE else KeyboardMetrics.KEY_HEIGHT)
    val menuHeight get() = keyHeight * KeyboardMetrics.MENU_ROWS
    private val letters = LinkedHashMap<Char, KeyCap>()
    private var grid: KeyGrid? = null
    private var pendingAccents: Runnable? = null
    private val rows = LinearLayout(context)
    private val bottom = LinearLayout(context)
    private val modeKey: KeyCap
    private val enterKey: KeyCap
    private val enterIcon = ImageView(context)
    private val hexKey: KeyCap
    private val spaceKey: KeyCap
    private var deleteKey: KeyCap? = null
    private var held = false
    private var tracking = false
    private var deleting = false
    private var repeated = false
    private var originX = 0f
    private var originY = 0f
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    private val hold = Runnable { if (tracking) held = ime.beginSpeech() }
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
        addView(bottom, LayoutParams(LayoutParams.MATCH_PARENT, keyHeight))
        modeKey = caps.cap(bottom, KeyboardMetrics.MODE, context.getString(R.string.keyboard_numbers_label),
            context.getString(R.string.keyboard_numbers), modifier, 16f, ime.engine::mode)
        caps.icon(bottom, KeyboardMetrics.GLOBE, R.drawable.ic_globe, context.getString(R.string.keyboard_next), modifier, ime::nextIme)
        hexKey = caps.icon(bottom, KeyboardMetrics.HEX, R.drawable.hex_mark, context.getString(R.string.keyboard_hex), ink, ime::toggleSpeech,
            tint = context.getColor(R.color.page), size = 26)
        hexKey.setOnLongClickListener { host.openMenu(); true }
        hexKey.setOnTouchListener { v, e ->
            when (e.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    held = false; tracking = true; originX = e.x; originY = e.y; v.isPressed = true
                    postDelayed(hold, 350)
                }
                MotionEvent.ACTION_MOVE -> if (!held && (abs(originX - e.x) > slop || abs(originY - e.y) > slop)) {
                    removeCallbacks(hold)
                    if (tracking && originY - e.y > slop && abs(originX - e.x) < dp(36)) host.openMenu()
                    tracking = false; v.isPressed = false
                }
                MotionEvent.ACTION_UP -> {
                    removeCallbacks(hold); v.isPressed = false
                    if (held) ime.finishSpeech() else if (tracking) v.performClick()
                    held = false; tracking = false
                }
                MotionEvent.ACTION_CANCEL -> {
                    removeCallbacks(hold); if (held) ime.cancelSpeech()
                    held = false; tracking = false; v.isPressed = false
                }
            }
            true
        }
        spaceKey = caps.cap(bottom, KeyboardMetrics.SPACE, "",
            context.getString(R.string.keyboard_space), caps.key, 14f) { ime.engine.type(" ") }
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
        caps.cap(bottom, KeyboardMetrics.PERIOD, ".", context.getString(R.string.keyboard_period), caps.key) { ime.engine.type(".") }
        enterKey = caps.cap(bottom, KeyboardMetrics.ENTER, "", context.getString(R.string.keyboard_return), modifier, 17f, ime.engine::enter)
        enterIcon.setImageResource(R.drawable.ic_enter)
        enterIcon.imageTintList = ColorStateList.valueOf(ink)
        enterKey.face.addView(enterIcon, FrameLayout.LayoutParams(dp(23), dp(23), Gravity.CENTER))
    }

    fun showPage(page: KeyboardPage) {
        clearCallout(); grid = null; letters.clear()
        rows.removeAllViews()
        val language = page.language
        val symbols = page.symbols
        val lines = page.lines
        val columns = maxOf(lines[0].length, lines[1].length)
        val side = KeyboardMetrics.sideWidth(page, lines)
        for ((index, line) in lines.withIndex()) {
            val row = LinearLayout(context)
            rows.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, keyHeight))
            if (index < 2 && line.length < columns) row.addView(View(context), LayoutParams(0, 1, (columns - line.length) / 2f))
            if (index == 2) {
                if (symbols) caps.cap(row, side, context.getString(if (page.more) R.string.keyboard_page_numbers else R.string.keyboard_page_symbols),
                    context.getString(R.string.keyboard_more_symbols), modifier, 15f, ime.engine::shift)
                else caps.icon(row, side, R.drawable.ic_shift, context.getString(if (page.locked) R.string.keyboard_caps_lock else R.string.keyboard_shift),
                    if (page.shifted) ink else modifier, ime.engine::shift, if (page.shifted) context.getColor(R.color.page) else ink)
            }
            for (c in line) {
                val text = if (!symbols && page.shifted) c.uppercaseChar().toString() else c.toString()
                var touch: KeyPoint? = null
                val k = caps.cap(row, 1f, text, text) {
                    val g = if (symbols) null else geometry()
                    val p = touch ?: g?.centres?.get(c)
                    if (g != null && p != null) ime.engine.letter(text, p.x, p.y, g) else ime.engine.type(text)
                }
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
                            if (ime.feedback.popups && ime.previewAllowed) feedback.present(k, listOf(text))
                            if (accents.isNotEmpty()) {
                                pendingAccents = Runnable { if (v.isPressed) feedback.present(k, accents) }
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
            if (index < 2 && line.length < columns) row.addView(View(context), LayoutParams(0, 1, (columns - line.length) / 2f))
            if (index == 2) {
                deleteKey = caps.icon(row, side, R.drawable.ic_delete, context.getString(R.string.keyboard_delete), modifier, ime.engine::delete).also { k ->
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

    private fun geometry(): KeyGrid? {
        grid?.let { return it }
        val q = letters['q'] ?: return null; val w = letters['w'] ?: return null
        if (q.width == 0 || w.left == q.left) return null
        val centres = letters.mapValues { (_, cap) -> KeyPoint(cap.left + cap.width / 2.0, (cap.parent as View).top + cap.height / 2.0) }
        return KeyGrid(centres, centres.getValue('w').x - centres.getValue('q').x,
            centres.getValue('a').y - centres.getValue('q').y).also { grid = it }
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
        removeCallbacks(hold); removeCallbacks(repeatDelete); removeCallbacks(switchLanguage)
        spaceKey.isPressed = false
        held = false; tracking = false; deleting = false
        hexKey.isPressed = false
        deleteKey?.isPressed = false
    }
    override fun onDetachedFromWindow() { cancelGestures(); super.onDetachedFromWindow() }
}
