package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.WindowInsets
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import ly.anoma.hex.R
import ly.anoma.hex.useInter
import ly.anoma.hex.speech.SpeechPhase
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlin.math.sqrt

internal class KeyPlane(context: Context, private val ime: HexKeyboard) : LinearLayout(context) {
    private fun dp(value: Int) = (value * resources.displayMetrics.density).roundToInt()
    private fun color(id: Int) = context.getColor(id)
    private val ink = color(R.color.ink)
    private val quiet = color(R.color.quiet)
    private val key = color(R.color.key)
    private val modifier = color(R.color.modifier)
    private val keyHeight = dp(if (resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE) 40 else 50)
    private val statusText = TextView(context)
    private val scope = Scope(context)
    private val menuKey = TextView(context)
    private lateinit var cancelKey: FrameLayout
    private lateinit var doneKey: FrameLayout
    private lateinit var check: ImageView
    private val spinner = ProgressBar(context, null, 0, android.R.style.Widget_Material_ProgressBar_Small)
    private lateinit var menu: SpeechMenu
    private lateinit var spaceKey: Cap
    private var hint = "Hold Hex to talk"
    private val plane = LinearLayout(context)
    private val bottom = LinearLayout(context)
    private lateinit var modeKey: Cap
    private lateinit var enterKey: Cap
    private val enterIcon = ImageView(context)
    private lateinit var hexKey: Cap
    private var deleteKey: Cap? = null
    private var held = false
    private var tracking = false
    private var deleting = false
    private var repeated = false
    private var originX = 0f
    private var originY = 0f
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    private val hold = Runnable { if (tracking) held = ime.beginSpeech() }
    private val repeatDelete = object : Runnable {
        override fun run() {
            if (deleting) { repeated = true; ime.delete(); postDelayed(this, 65) }
        }
    }

    init {
        orientation = VERTICAL
        layoutDirection = View.LAYOUT_DIRECTION_LTR
        setPadding(dp(3), 0, dp(3), dp(4))
        // On edge-to-edge IME windows the system draws its switcher and gesture
        // handle over the bottom of our window. Keep every key above that region.
        setOnApplyWindowInsetsListener { _, insets ->
            val nav = insets.getInsets(WindowInsets.Type.navigationBars())
            setPadding(dp(3) + nav.left, 0, dp(3) + nav.right, dp(4) + nav.bottom)
            insets
        }
        setBackgroundColor(color(R.color.keyboard))
        val strip = LinearLayout(context).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(10), 0, dp(8), 0)
        }
        statusText.apply {
            useInter(500); setTextColor(quiet); textSize = 14f; maxLines = 1; ellipsize = TextUtils.TruncateAt.END
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }
        strip.addView(statusText, LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f))
        menuKey.apply {
            useInter(550); gravity = Gravity.CENTER; textSize = 14f; setTextColor(ink)
            setOnClickListener { if (menu.visibility == VISIBLE) closeMenu() else openMenu() }
            setPadding(dp(8), 0, dp(8), 0); isFocusable = true
        }
        strip.addView(menuKey, LayoutParams(LayoutParams.WRAP_CONTENT, -1))
        scope.background = shape(color(R.color.sheet), 18)
        scope.clipToOutline = true
        strip.addView(scope, LayoutParams(0, dp(32), 1f).apply { marginEnd = dp(4) })
        cancelKey = panelButton(strip, "Cancel dictation", modifier, R.drawable.ic_cancel, ime::cancelSpeech)
        doneKey = panelButton(strip, "Finish dictation", key, R.drawable.ic_check, ime::finishSpeech)
        check = doneKey.getChildAt(1) as ImageView
        spinner.indeterminateTintList = ColorStateList.valueOf(ink)
        spinner.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        doneKey.addView(spinner, FrameLayout.LayoutParams(dp(18), dp(18), Gravity.CENTER))
        addView(strip, LayoutParams(LayoutParams.MATCH_PARENT, dp(44)))
        plane.orientation = VERTICAL
        addView(plane, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        addView(bottom, LayoutParams(LayoutParams.MATCH_PARENT, keyHeight))
        menu = SpeechMenu(context, ime, { statusText.text = it }, ::closeMenu).apply { visibility = GONE }
        addView(menu, LayoutParams(-1, keyHeight * 4))
        statusText.setOnClickListener { if (menu.visibility == VISIBLE) menu.back() }
        modeKey = cap(bottom, 1.45f, "?123", "Numbers and symbols", modifier, 16f, ime::mode)
        icon(bottom, 1.05f, R.drawable.ic_globe, "Next keyboard", modifier, ime::nextIme)
        hexKey = icon(bottom, 1.35f, R.drawable.hex_mark, "Hex dictation", ink, ime::toggleSpeech,
            tint = color(R.color.page), size = 26)
        hexKey.setOnLongClickListener { openMenu(); true }
        hexKey.setOnTouchListener { v, e ->
            when (e.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    held = false; tracking = true; originX = e.x; originY = e.y; v.isPressed = true
                    postDelayed(hold, 350)
                }
                MotionEvent.ACTION_MOVE -> if (!held && (abs(originX - e.x) > slop || abs(originY - e.y) > slop)) {
                    removeCallbacks(hold)
                    if (tracking && originY - e.y > slop && abs(originX - e.x) < dp(36)) openMenu()
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
        spaceKey = cap(bottom, 3.5f, "English", "Space", key, 14f) { ime.type(" ") }
        spaceKey.label?.setTextColor(quiet)
        cap(bottom, 1f, ".", "Period", key) { ime.type(".") }
        enterKey = cap(bottom, 1.65f, "", "Return", modifier, 17f, ime::enter)
        enterIcon.setImageResource(R.drawable.ic_enter)
        enterIcon.imageTintList = ColorStateList.valueOf(ink)
        enterKey.face.addView(enterIcon, FrameLayout.LayoutParams(dp(23), dp(23), Gravity.CENTER))
        status(SpeechPhase.Idle, "Hold Hex to talk")
    }

    // The Hex key stays mounted across every state change: its original finger-up ends the hold.
    fun layoutKeys(symbols: Boolean, more: Boolean, shifted: Boolean, locked: Boolean, action: String?) {
        plane.removeAllViews()
        val lines = when {
            !symbols -> listOf("qwertyuiop", "asdfghjkl", "zxcvbnm")
            more -> listOf("~`|•√π÷×¶∆", "£¢€¥^°={}", "[]<>_\"*")
            else -> listOf("1234567890", "@#$%&-+()", "!,?'/;:")
        }
        for ((index, line) in lines.withIndex()) {
            val row = LinearLayout(context)
            plane.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, keyHeight))
            if (index == 1) row.addView(View(context), LayoutParams(0, 1, .5f))
            if (index == 2) {
                if (symbols) cap(row, 1.5f, if (more) "123" else "=\\<", "More symbols", modifier, 15f, ime::shift)
                else icon(row, 1.5f, R.drawable.ic_shift, if (locked) "Caps lock" else "Shift",
                    if (shifted) ink else modifier, ime::shift, if (shifted) color(R.color.page) else ink)
            }
            for (c in line) {
                val text = if (!symbols && shifted) c.uppercaseChar().toString() else c.toString()
                cap(row, 1f, text, text) { ime.type(text) }
            }
            if (index == 1) row.addView(View(context), LayoutParams(0, 1, .5f))
            if (index == 2) {
                deleteKey = icon(row, 1.5f, R.drawable.ic_delete, "Delete", modifier, ime::delete).also { k ->
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
        modeKey.label?.text = if (symbols) "ABC" else "?123"
        modeKey.contentDescription = if (symbols) "Letters" else "Numbers and symbols"
        enterKey.label?.text = action.orEmpty()
        enterIcon.visibility = if (action == null) View.VISIBLE else View.GONE
        enterKey.contentDescription = action ?: "Return"
    }

    private fun shape(fill: Int, radius: Int) = GradientDrawable().apply {
        setColor(fill); cornerRadius = dp(radius).toFloat()
    }
    private fun panelButton(row: LinearLayout, label: String, fill: Int, icon: Int, action: () -> Unit): FrameLayout {
        val target = FrameLayout(context).apply {
            contentDescription = label; isFocusable = true; setOnClickListener { action() }
            val face = View(context).apply {
                isDuplicateParentStateEnabled = true
                background = StateListDrawable().apply {
                    addState(intArrayOf(android.R.attr.state_pressed), shape(color(R.color.pressed), 16))
                    addState(intArrayOf(), shape(fill, 16))
                }
            }
            addView(face, FrameLayout.LayoutParams(dp(32), dp(32), Gravity.CENTER))
            addView(ImageView(context).apply {
                setImageResource(icon); imageTintList = ColorStateList.valueOf(ink)
            }, FrameLayout.LayoutParams(dp(19), dp(19), Gravity.CENTER))
        }
        row.addView(target, LayoutParams(dp(44), dp(44)))
        return target
    }
    private fun openMenu() {
        if (scope.visibility == VISIBLE) return
        plane.visibility = GONE; bottom.visibility = GONE; menu.visibility = VISIBLE
        statusText.isClickable = true; menuKey.text = "Done"; menu.home()
    }
    fun closeMenu() {
        if (!::menu.isInitialized || menu.visibility != VISIBLE) return
        menu.visibility = GONE; plane.visibility = VISIBLE; bottom.visibility = VISIBLE
        statusText.isClickable = false; menuKey.text = "Hex menu  ›"; statusText.text = hint; refreshSpace()
    }
    private fun refreshSpace() {
        val next = ime.preferences.options(ime.nextEnglish)
        spaceKey.label?.text = next.spoken.label + if (next.translate) " → English" else ""
        spaceKey.label?.textSize = if (next.translate) 12f else 14f
    }
    fun status(phase: SpeechPhase, hint: String) {
        this.hint = hint
        val idle = phase == SpeechPhase.Idle
        val processing = phase == SpeechPhase.Transcribing
        if (!idle) closeMenu()
        statusText.visibility = if (idle) VISIBLE else GONE
        menuKey.visibility = if (idle) VISIBLE else GONE
        if (menu.visibility != VISIBLE) { statusText.isClickable = false; menuKey.text = "Hex menu  ›"; statusText.text = hint }
        for (v in listOf(scope, cancelKey, doneKey)) v.visibility = if (idle) GONE else VISIBLE
        spinner.visibility = if (processing) VISIBLE else GONE
        check.visibility = if (processing) GONE else VISIBLE
        doneKey.isEnabled = !processing
        doneKey.contentDescription = if (processing) "Transcribing" else "Finish dictation"
        scope.contentDescription = if (processing) "Transcribing" else "Recording"
        if (idle || phase == SpeechPhase.Opening) scope.clear()
        // Keep the snapshotted paste language visible until this dictation has finished.
        if (idle) refreshSpace()
    }
    fun setLevel(value: Float) = scope.push(value)
    fun cancelGestures() {
        removeCallbacks(hold); removeCallbacks(repeatDelete)
        held = false; tracking = false; deleting = false
        if (::hexKey.isInitialized) hexKey.isPressed = false
        deleteKey?.isPressed = false
    }
    override fun onDetachedFromWindow() { cancelGestures(); super.onDetachedFromWindow() }

    // The entire cell is a hit target, including the gap around the drawn cap.
    private inner class Cap(fill: Int, description: String, click: () -> Unit) : FrameLayout(context) {
        var label: TextView? = null
        val face = FrameLayout(context)
        init {
            contentDescription = description
            isFocusable = true; setOnClickListener { click() }
            setPadding(dp(2), dp(3), dp(2), dp(3))
            val paints = StateListDrawable()
            fun shape(c: Int) = GradientDrawable().apply { cornerRadius = dp(8).toFloat(); setColor(c) }
            paints.addState(intArrayOf(android.R.attr.state_pressed), shape(color(R.color.pressed)))
            paints.addState(intArrayOf(), shape(fill))
            face.isDuplicateParentStateEnabled = true
            face.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
            face.background = paints
            addView(face, FrameLayout.LayoutParams(-1, -1))
        }
        override fun getAccessibilityClassName() = "android.widget.Button"
    }

    private fun cap(row: LinearLayout, weight: Float, s: String, desc: String, fill: Int = key,
                    sp: Float = 21f, click: () -> Unit): Cap {
        val c = Cap(fill, desc, click)
        c.label = TextView(context).apply {
            text = s; textSize = sp; useInter(if (sp >= 20f) 450 else 500)
            gravity = Gravity.CENTER; includeFontPadding = false
            maxLines = 1; ellipsize = TextUtils.TruncateAt.END; setTextColor(ink)
            c.face.addView(this, FrameLayout.LayoutParams(-1, -1))
        }
        row.addView(c, LayoutParams(0, -1, weight))
        return c
    }
    private fun icon(row: LinearLayout, weight: Float, res: Int, desc: String, fill: Int,
                     click: () -> Unit, tint: Int = ink, size: Int = 23): Cap {
        val c = Cap(fill, desc, click)
        val image = ImageView(context).apply {
            setImageResource(res); imageTintList = ColorStateList.valueOf(tint)
        }
        c.face.addView(image, FrameLayout.LayoutParams(dp(size), dp(size), Gravity.CENTER))
        row.addView(c, LayoutParams(0, -1, weight))
        return c
    }

    private inner class Scope(context: Context) : View(context) {
        private val levels = FloatArray(66)
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
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
}
