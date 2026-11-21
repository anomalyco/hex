package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable
import android.provider.Settings
import android.text.TextUtils
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.TextView
import ly.anoma.hex.R
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** One key. The whole view is its touch target; its padding places the drawn keycap (KeyRowLayout sets it). */
internal class KeyCap(context: Context, fill: Int, description: String, private val haptics: () -> Boolean,
                      click: () -> Unit) : FrameLayout(context) {
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    var label: TextView? = null
    val face = FrameLayout(context)
    init {
        contentDescription = description
        isFocusable = true; setOnClickListener { click() }
        val paints = StateListDrawable()
        fun shape(c: Int) = GradientDrawable().apply { cornerRadius = KeyboardMetrics.CORNER * resources.displayMetrics.density; setColor(c) }
        paints.addState(intArrayOf(android.R.attr.state_pressed), shape(context.getColor(R.color.pressed)))
        paints.addState(intArrayOf(), shape(fill))
        face.isDuplicateParentStateEnabled = true
        face.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        face.background = paints
        addView(face, LayoutParams(-1, -1))
    }
    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN && haptics() &&
            Settings.System.getInt(context.contentResolver, "keyboard_vibration_enabled", 1) != 0)
            performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
        return super.dispatchTouchEvent(event)
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
    override fun getAccessibilityClassName() = "android.widget.Button"
}

/** Builds keys with the keyboard's colours and haptics setting. */
internal class KeyCaps(private val context: Context, private val haptics: () -> Boolean) {
    private fun dp(value: Int) = (value * context.resources.displayMetrics.density).roundToInt()
    val ink = context.getColor(R.color.ink)
    val key = context.getColor(R.color.key)
    val modifier = context.getColor(R.color.modifier)

    /** Letters are Gboard's size (19.4 dp capitals); labels such as ?123 are smaller. */
    fun cap(s: String, desc: String, fill: Int = key, sp: Float = 26f, click: () -> Unit): KeyCap {
        val c = KeyCap(context, fill, desc, haptics, click)
        c.label = TextView(context).apply {
            text = s; textSize = sp; useInter(if (sp >= 20f) 400 else 500)
            gravity = Gravity.CENTER; includeFontPadding = false
            maxLines = 1; ellipsize = TextUtils.TruncateAt.END; setTextColor(ink)
            c.face.addView(this, FrameLayout.LayoutParams(-1, -1))
        }
        return c
    }
    fun icon(res: Int, desc: String, fill: Int, click: () -> Unit, tint: Int = ink, size: Int = 24): KeyCap {
        val c = KeyCap(context, fill, desc, haptics, click)
        val image = ImageView(context).apply {
            setImageResource(res); imageTintList = ColorStateList.valueOf(tint)
        }
        c.face.addView(image, FrameLayout.LayoutParams(dp(size), dp(size), Gravity.CENTER))
        return c
    }
}
