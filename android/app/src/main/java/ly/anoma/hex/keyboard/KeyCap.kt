package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable
import android.media.AudioManager
import android.provider.Settings
import android.text.TextUtils
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.accessibility.AccessibilityNodeInfo
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
    init {
        contentDescription = description
        // The key click below replaces View's own, which would sound again as performClick runs on lifting.
        isFocusable = true; isSoundEffectsEnabled = false; setOnClickListener { click() }
        val paints = StateListDrawable()
        fun shape(c: Int) = GradientDrawable().apply { cornerRadius = KeyboardMetrics.CORNER * resources.displayMetrics.density; setColor(c) }
        paints.addState(intArrayOf(android.R.attr.state_pressed), shape(context.getColor(R.color.pressed)))
        paints.addState(intArrayOf(), shape(fill))
        face.isDuplicateParentStateEnabled = true
        face.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        face.background = paints
        addView(face, LayoutParams(-1, -1))
    }
    private fun vibrates() = vibrates(context, haptics())
    // A tap as each key goes down, as on Gboard. The click is the system's own, so it follows Touch
    // sounds as iPhone Hex's follows Keyboard clicks (AudioManager.playSoundEffect checks the setting).
    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN || event.actionMasked == MotionEvent.ACTION_POINTER_DOWN) {
            downAt = event.eventTime
            if (vibrates()) performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
            context.getSystemService(AudioManager::class.java)?.playSoundEffect(sound)
        }
        if (event.actionMasked == MotionEvent.ACTION_UP) liftAt = event.eventTime
        return super.dispatchTouchEvent(event)
    }
    /** A key that acts as it lifts lifted: Gboard's second, lighter tick, only after a press of
     * [KeyboardMetrics.RELEASE_TICK_MS] or more (Gboard: none at 190 ms, one at 200 ms). */
    fun typedOnLift() {
        if (liftAt - downAt >= KeyboardMetrics.RELEASE_TICK_MS && vibrates())
            performHapticFeedback(HapticFeedbackConstants.KEYBOARD_RELEASE)
    }
    /** Row 1's digit, small in the corner, quiet enough that the key still reads as its letter. */
    fun hint(text: String?) {
        if (text == null) { hint?.visibility = GONE; return }
        val h = hint ?: TextView(context).apply {
            textSize = 9.5f; useInter(500); includeFontPadding = false; setTextColor(context.getColor(R.color.quiet))
            val d = resources.displayMetrics.density
            face.addView(this, LayoutParams(-2, -2, Gravity.TOP or Gravity.END).apply { topMargin = (3 * d).roundToInt(); marginEnd = (4.5f * d).roundToInt() })
        }.also { hint = it }
        h.text = text; h.visibility = VISIBLE
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

/** Hex's Haptics setting and the system's Keyboard vibration both on. */
internal fun vibrates(context: Context, haptics: Boolean) =
    haptics && Settings.System.getInt(context.contentResolver, "keyboard_vibration_enabled", 1) != 0

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
