package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.drawable.GradientDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.view.WindowInsets
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import ly.anoma.hex.R
import ly.anoma.hex.speech.SpeechPhase
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** Labs › Keyboard › Voice-only mode: the whole keyboard while its voice subtype is current. The well,
 * waveform and round keys are the strip's, so dictating from another keyboard's voice key looks the same:
 * the status and level while it records, ✓ to insert the text, ✕ to go back without it. */
internal class VoiceBarView(context: Context, private val finish: () -> Unit, private val cancel: () -> Unit) :
    LinearLayout(context) {
    private val density = resources.displayMetrics.density
    private fun dp(value: Int) = (value * density).roundToInt()
    private fun px(value: Float) = (value * density).roundToInt()
    private val ink = context.getColor(R.color.ink)
    private val pill = px(KeyboardMetrics.PILL)
    private val scope = DotScopeView(context)
    private val statusText = TextView(context)
    private val check = ImageView(context)
    private val spinner = ProgressBar(context, null, 0, android.R.style.Widget_Material_ProgressBar_Small)
    private val cancelKey = FrameLayout(context).apply {
        contentDescription = context.getString(R.string.voice_only_back); isFocusable = true
        addView(View(context).apply { isDuplicateParentStateEnabled = true; background = pressable(R.color.modifier) },
            FrameLayout.LayoutParams(pill, pill, Gravity.CENTER))
        addView(icon(R.drawable.ic_cancel), FrameLayout.LayoutParams(dp(19), dp(19), Gravity.CENTER))
        setOnClickListener { cancel() }
    }
    private val doneKey = FrameLayout(context).apply {
        contentDescription = context.getString(R.string.keyboard_finish_dictation); isFocusable = true
        addView(View(context).apply { isDuplicateParentStateEnabled = true; background = pressable(R.color.key) },
            FrameLayout.LayoutParams(pill, pill, Gravity.CENTER))
        check.setImageResource(R.drawable.ic_check); check.imageTintList = ColorStateList.valueOf(ink)
        addView(check, FrameLayout.LayoutParams(dp(19), dp(19), Gravity.CENTER))
        spinner.indeterminateTintList = ColorStateList.valueOf(ink)
        spinner.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        spinner.visibility = GONE
        addView(spinner, FrameLayout.LayoutParams(dp(18), dp(18), Gravity.CENTER))
        setOnClickListener { finish() }
    }

    init {
        orientation = HORIZONTAL
        layoutDirection = View.LAYOUT_DIRECTION_LTR
        gravity = Gravity.CENTER_VERTICAL
        setBackgroundColor(context.getColor(R.color.keyboard))
        setPadding(dp(12), dp(12), dp(12), dp(12))
        // The system's back and switcher buttons and gesture handle sit at the bottom of the IME window.
        setOnApplyWindowInsetsListener { _, insets ->
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            setPadding(dp(12) + bars.left, dp(12), dp(12) + bars.right, dp(12) + bars.bottom)
            insets
        }
        statusText.apply {
            useInter(500); setTextColor(context.getColor(R.color.quiet)); textSize = 14f
            maxLines = 1; ellipsize = TextUtils.TruncateAt.END; includeFontPadding = false
            gravity = Gravity.CENTER_VERTICAL; setPadding(dp(12), 0, dp(6), 0)
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }
        scope.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_YES
        scope.contentDescription = context.getString(R.string.keyboard_recording)
        val well = LinearLayout(context).apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            background = shape(R.color.sheet, pill / 2f)
            clipToOutline = true
        }
        well.addView(statusText, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        well.addView(scope, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f).apply { marginEnd = dp(8) })
        addView(cancelKey, LayoutParams(dp(40), pill))
        addView(well, LayoutParams(0, pill, 1f).apply { marginStart = dp(8); marginEnd = dp(8) })
        addView(doneKey, LayoutParams(dp(40), pill))
        status(SpeechPhase.Idle, context.getString(R.string.voice_only_hint))
    }

    /** [message] in the well, with the level while [phase] records and a spinner while it transcribes. */
    fun status(phase: SpeechPhase, message: String) {
        statusText.text = message
        statusText.contentDescription = message
        val recording = phase == SpeechPhase.Opening || phase == SpeechPhase.Listening
        scope.visibility = if (recording) VISIBLE else GONE
        if (!recording) scope.clear()
        check.visibility = if (recording) VISIBLE else GONE
        spinner.visibility = if (phase == SpeechPhase.Transcribing) VISIBLE else GONE
    }

    fun setLevel(value: Float) = scope.push(value)

    private fun icon(res: Int) = ImageView(context).apply { setImageResource(res); imageTintList = ColorStateList.valueOf(ink) }
    private fun shape(fill: Int, radius: Float) = GradientDrawable().apply { setColor(context.getColor(fill)); cornerRadius = radius }
    private fun pressable(fill: Int) = android.graphics.drawable.StateListDrawable().apply {
        addState(intArrayOf(android.R.attr.state_pressed), GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(context.getColor(R.color.pressed)) })
        addState(intArrayOf(), GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(context.getColor(fill)) })
    }
}
