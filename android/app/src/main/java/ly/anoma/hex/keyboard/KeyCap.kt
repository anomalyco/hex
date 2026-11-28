package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable
import android.media.AudioManager
import android.graphics.Rect
import android.text.TextPaint
import android.util.TypedValue
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.TextView
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** One key. The whole view is its touch target; its padding places the drawn keycap (KeyRowLayout sets it). */
internal class KeyCap(context: Context, fill: Int, private val paints: KeyPaints, description: String,
                      private val haptics: () -> Boolean, private val clickPercent: () -> Int?,
                      click: () -> Unit) : FrameLayout(context) {
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    var label: TextView? = null
    /** The label's size in dp at Gboard's letter keycap; the drawn size follows this key's keycap ([KeyLabels]). */
    var labelSize = 26f
        set(value) { if (field != value) { field = value; requestLayout() } }
    /** The keycap height, in dp, this screen's keys are drawn for ([KeyRows.labelBase]). */
    var labelBase = KeyLabels.REF_HEIGHT
    val face = FrameLayout(context)
    /** The system's key click this key plays as it goes down, when the system's Touch sounds are on. */
    var sound = AudioManager.FX_KEYPRESS_STANDARD
    private var hint: TextView? = null
    /** TalkBack types a text entry key as the finger lifts from it (its Typing preference, as for Gboard's
     * keys); switches and enter take a double tap. */
    var textEntry = true
    /** What TalkBack names the key's long press, when it has one. */
    var longClickLabel: String? = null
    private var downAt = 0L
    private var liftAt = 0L
    private companion object { const val ICON_SCALE = 1.15f }
    init {
        contentDescription = description
        // The key click below replaces View's own, which would sound again as performClick runs on lifting.
        isFocusable = true; isSoundEffectsEnabled = false; setOnClickListener { click() }
        face.isDuplicateParentStateEnabled = true
        face.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        fill(fill)
        addView(face, LayoutParams(-1, -1))
    }
    /** The keycap's colour, and its colour pressed: enter takes a theme's action colour in an action field. */
    fun fill(fill: Int, pressed: Int = paints.pressed) {
        fun shape(c: Int) = GradientDrawable().apply { cornerRadius = KeyboardMetrics.CORNER * resources.displayMetrics.density; setColor(c) }
        face.background = StateListDrawable().apply {
            addState(intArrayOf(android.R.attr.state_pressed), shape(pressed))
            addState(intArrayOf(), shape(fill))
        }
    }
    // A tap as each key goes down, as on Gboard. The click is the system's own, so it follows Touch
    // sounds as iPhone Hex's follows Keyboard clicks; Labs › Keyboard › Key click volume sets its level ([KeyClick]).
    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN || event.actionMasked == MotionEvent.ACTION_POINTER_DOWN) {
            downAt = event.eventTime
            tap(context, haptics(), Tap.Press)
            KeyClick.play(context, sound, clickPercent())
        }
        if (event.actionMasked == MotionEvent.ACTION_UP) liftAt = event.eventTime
        return super.dispatchTouchEvent(event)
    }
    /** A key that acts as it lifts lifted: Gboard's second, lighter tick, only after a press of
     * [KeyboardMetrics.RELEASE_TICK_MS] or more (Gboard: none at 190 ms, one at 200 ms). */
    fun typedOnLift() {
        if (liftAt - downAt >= KeyboardMetrics.RELEASE_TICK_MS) tap(context, haptics(), Tap.Release)
    }
    /** Row 1's digit, small in the corner, quiet enough that the key still reads as its letter. */
    fun hint(text: String?) {
        if (text == null) { if (hint?.visibility == VISIBLE) { hint?.visibility = GONE; requestLayout() }; return }
        val h = hint ?: TextView(context).apply {
            useInter(500); includeFontPadding = false; setTextColor(paints.quiet)
            face.addView(this, LayoutParams(-2, -2, Gravity.TOP or Gravity.END))
        }.also { hint = it }
        if (h.text != text || h.visibility != VISIBLE) requestLayout()
        h.text = text; h.visibility = VISIBLE
    }

    private var hintImage: ImageView? = null
    /** A small icon in the corner where [hint]'s text goes, in the same quiet colour (SwiftKey layout's microphone on
     * the comma), or none for null. */
    fun hintIcon(res: Int?) {
        if (res == null) { if (hintImage?.visibility == VISIBLE) { hintImage?.visibility = GONE; requestLayout() }; return }
        val image = hintImage ?: ImageView(context).apply {
            imageTintList = ColorStateList.valueOf(paints.quiet)
            face.addView(this, LayoutParams(0, 0, Gravity.TOP or Gravity.END))
        }.also { hintImage = it }
        image.setImageResource(res)
        if (image.visibility != VISIBLE) { image.visibility = VISIBLE; requestLayout() }
    }

    // The label and hint are sized for the keycap here, before they are measured: KeyRowLayout has set the
    // padding that places the keycap inside the touch target.
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        fit(MeasureSpec.getSize(widthMeasureSpec) - paddingLeft - paddingRight, MeasureSpec.getSize(heightMeasureSpec) - paddingTop - paddingBottom)
        super.onMeasure(widthMeasureSpec, heightMeasureSpec)
    }
    private val probe = TextPaint()
    private fun shape(view: TextView, text: String): TextShape {
        probe.set(view.paint); probe.textSize = 1000f
        val metrics = probe.fontMetrics
        val bounds = Rect().also { probe.getTextBounds("H", 0, 1, it) }
        val advance = if (text.isEmpty()) 0f else maxOf(probe.measureText(text.uppercase()), probe.measureText(text.lowercase()))
        return TextShape(advance / 1000f, -metrics.ascent / 1000f, metrics.descent / 1000f, bounds.height() / 1000f)
    }
    private fun fit(width: Int, height: Int) {
        if (width <= 0 || height <= 0) return
        val label = label
        val shown = hint?.takeIf { it.visibility == VISIBLE }
        val sizes = KeyLabels.sizes(width.toFloat(), height.toFloat(), resources.displayMetrics.density, labelBase, labelSize,
            label?.let { shape(it, it.text?.toString().orEmpty()) }, shown?.let { shape(it, it.text?.toString().orEmpty()) })
        if (label != null && label.textSize != sizes.label) label.setTextSize(TypedValue.COMPLEX_UNIT_PX, sizes.label)
        shown?.let { h ->
            if (h.textSize != sizes.hint) h.setTextSize(TypedValue.COMPLEX_UNIT_PX, sizes.hint)
            val params = h.layoutParams as LayoutParams
            val top = sizes.hintTop.roundToInt(); val end = sizes.hintEnd.roundToInt()
            if (params.topMargin != top || params.marginEnd != end) { params.topMargin = top; params.marginEnd = end; h.layoutParams = params }
        }
        hintImage?.takeIf { it.visibility == VISIBLE }?.let { image ->
            // As tall as a hint's capitals would stand, a little more so its outline reads.
            val side = (sizes.hint * ICON_SCALE).roundToInt()
            val top = sizes.hintTop.roundToInt(); val end = sizes.hintEnd.roundToInt()
            val params = image.layoutParams as LayoutParams
            if (params.width != side || params.topMargin != top || params.marginEnd != end) {
                params.width = side; params.height = side; params.topMargin = top; params.marginEnd = end; image.layoutParams = params
            }
        }
    }
    // View normally posts its click after UP. Letter gestures commit on UP itself;
    // posting space or shift behind the next letter can reorder fast input.
    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!isEnabled) return false
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> isPressed = true
            MotionEvent.ACTION_MOVE -> if (event.x < -slop || event.x > width + slop ||
                event.y < -slop || event.y > height + slop) isPressed = false
            MotionEvent.ACTION_UP -> { val click = isPressed; isPressed = false; if (click) performClick() }
            MotionEvent.ACTION_CANCEL -> isPressed = false
        }
        return true
    }
    // No "Button" after every key, as Gboard's keys say nothing but their names.
    override fun getAccessibilityClassName(): CharSequence = View::class.java.name
    override fun onInitializeAccessibilityNodeInfo(info: AccessibilityNodeInfo) {
        super.onInitializeAccessibilityNodeInfo(info)
        info.isTextEntryKey = textEntry
        val label = longClickLabel
        if (label != null && isLongClickable) info.addAction(AccessibilityNodeInfo.AccessibilityAction(AccessibilityNodeInfo.ACTION_LONG_CLICK, label))
    }
}

/** Builds keys with the keyboard's colours and haptics setting. */
internal class KeyCaps(private val context: Context, val paints: KeyPaints, private val labelBase: Float,
                       private val haptics: () -> Boolean, private val clickPercent: () -> Int? = { null }) {
    private fun dp(value: Int) = (value * context.resources.displayMetrics.density).roundToInt()
    val ink = paints.ink
    val key = paints.key
    val modifier = paints.modifier

    /** Letters are Gboard's size (19.4 dp capitals) on its Pixel 8 keycap; labels such as ?123 are smaller. [size]
     * is in dp at that keycap: the drawn size follows each key's own ([KeyLabels]). */
    fun cap(s: String, desc: String, fill: Int = key, size: Float = 26f, click: () -> Unit): KeyCap {
        val c = KeyCap(context, fill, paints, desc, haptics, clickPercent, click)
        c.labelSize = size; c.labelBase = labelBase
        c.label = TextView(context).apply {
            text = s; useInter(if (size >= 20f) 400 else 500)
            gravity = Gravity.CENTER; includeFontPadding = false
            maxLines = 1; setTextColor(ink)
            c.face.addView(this, FrameLayout.LayoutParams(-1, -1))
        }
        return c
    }
    fun icon(res: Int, desc: String, fill: Int, click: () -> Unit, tint: Int = ink, size: Int = 24): KeyCap {
        val c = KeyCap(context, fill, paints, desc, haptics, clickPercent, click)
        val image = ImageView(context).apply {
            setImageResource(res); imageTintList = ColorStateList.valueOf(tint)
        }
        c.face.addView(image, FrameLayout.LayoutParams(dp(size), dp(size), Gravity.CENTER))
        return c
    }
}
