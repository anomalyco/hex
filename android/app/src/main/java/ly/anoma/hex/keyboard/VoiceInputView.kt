package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.content.res.Configuration
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

/** The keyboard's voice-only bar (Labs › Keyboard › Voice input mode): a status line, the recording's dot
 * scope, ✕ to discard and ✓ to finish, in the strip's size and place. It has no keys: Android opened Hex in
 * its voice subtype from another keyboard, and Hex goes back there once the words are in. */
internal class VoiceInputView(context: Context, private val ime: HexKeyboard) : LinearLayout(context) {
    private val density = resources.displayMetrics.density
    private fun dp(value: Int) = (value * density).roundToInt()
    private fun px(value: Float) = (value * density).roundToInt()
    private val landscape = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
    private val bar = px(if (landscape) KeyboardMetrics.STRIP_LANDSCAPE else KeyboardMetrics.STRIP)
    private val side = px(KeyboardMetrics.SIDE)
    private val pill = px(KeyboardMetrics.PILL)
    private val ink = context.getColor(R.color.ink)
    private val quiet = context.getColor(R.color.quiet)
    private val key = context.getColor(R.color.key)

    private val statusText = TextView(context)
    private val well = FrameLayout(context)
    private val scope = DotScopeView(context)
    private val cancelKey = FrameLayout(context)
    private val finishKey = FrameLayout(context)
    private val mark = ImageView(context)
    private val check = ImageView(context)
    private val spinner = ProgressBar(context, null, 0, android.R.style.Widget_Material_ProgressBar_Small)
    private var phase = SpeechPhase.Idle

    init {
        orientation = VERTICAL
        layoutDirection = LAYOUT_DIRECTION_LTR
        setBackgroundColor(context.getColor(R.color.keyboard))
        // The system's buttons and gesture handle sit over the bottom of an edge-to-edge IME window, as
        // over the keyboard's last row, so the bar stays above them.
        setOnApplyWindowInsetsListener { _, insets ->
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            setPadding(bars.left, 0, bars.right, bars.bottom)
            insets
        }
        statusText.apply {
            useInter(500); setTextColor(quiet); textSize = 14f; maxLines = 2
            ellipsize = TextUtils.TruncateAt.END; gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(16), 0, dp(4), 0)
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }
        well.background = barShape(context.getColor(R.color.sheet), pill / 2f)
        well.clipToOutline = true
        well.addView(scope, FrameLayout.LayoutParams(-1, -1).apply { marginStart = dp(10); marginEnd = dp(10) })
        scope.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_YES
        scope.accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        cancelKey.apply {
            contentDescription = context.getString(R.string.keyboard_cancel_dictation); isFocusable = true
            setOnClickListener { ime.cancelVoice() }
            addView(View(context).apply { isDuplicateParentStateEnabled = true; background = barPressable(context, context.getColor(R.color.modifier)) },
                FrameLayout.LayoutParams(pill, pill, Gravity.CENTER))
            addView(barIcon(context, R.drawable.ic_cancel, ink), FrameLayout.LayoutParams(dp(19), dp(19), Gravity.CENTER))
        }
        finishKey.apply {
            isFocusable = true
            addView(View(context).apply { isDuplicateParentStateEnabled = true; background = barPressable(context, key) },
                FrameLayout.LayoutParams(pill, pill, Gravity.CENTER))
            mark.setImageResource(R.drawable.hex_mark); mark.imageTintList = ColorStateList.valueOf(ink)
            addView(mark, FrameLayout.LayoutParams(px(KeyboardMetrics.MARK), px(KeyboardMetrics.MARK), Gravity.CENTER))
            check.setImageResource(R.drawable.ic_check); check.imageTintList = ColorStateList.valueOf(ink)
            addView(check, FrameLayout.LayoutParams(dp(19), dp(19), Gravity.CENTER))
            spinner.indeterminateTintList = ColorStateList.valueOf(ink)
            spinner.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            addView(spinner, FrameLayout.LayoutParams(dp(18), dp(18), Gravity.CENTER))
            setOnClickListener { ime.toggleVoice() }
        }
        val row = LinearLayout(context).apply {
            orientation = HORIZONTAL; gravity = Gravity.CENTER_VERTICAL
            addView(statusText, LayoutParams(0, -1, 1.4f))
            addView(well, LayoutParams(0, pill, 1f).apply { marginEnd = dp(6) })
            addView(cancelKey, LayoutParams(dp(40), -1))
            addView(finishKey, LayoutParams(px(KeyboardMetrics.HEX_KEY), -1).apply { marginEnd = side })
        }
        addView(row, LayoutParams(-1, bar))
        status(SpeechPhase.Idle, context.getString(R.string.keyboard_hint_voice))
    }

    /** [hint] is a dictation's note or the idle line ("Tap Hex to talk", a password field's message). */
    fun status(phase: SpeechPhase, hint: String) {
        speechCue(this.phase, phase, ime.feedback.haptics)
        this.phase = phase
        val idle = phase == SpeechPhase.Idle
        val processing = phase == SpeechPhase.Transcribing
        well.visibility = if (idle) GONE else VISIBLE
        statusText.text = when (phase) {
            SpeechPhase.Opening, SpeechPhase.Listening -> context.getString(R.string.voice_input_listening)
            SpeechPhase.Transcribing -> context.getString(R.string.voice_input_transcribing)
            SpeechPhase.Idle -> hint
        }
        spinner.visibility = if (processing) VISIBLE else GONE
        check.visibility = if (!idle && !processing) VISIBLE else GONE
        mark.visibility = if (idle) VISIBLE else GONE
        finishKey.contentDescription = context.getString(when {
            idle -> R.string.keyboard_hex
            processing -> R.string.keyboard_transcribing
            else -> R.string.keyboard_finish_dictation
        })
        // Idle, ✕ is the way back to the usual keyboard (a password field or a missing microphone can't listen).
        cancelKey.contentDescription = context.getString(if (idle) R.string.voice_input_back else R.string.keyboard_cancel_dictation)
        // Set just after the bar appears, so the live region announces it.
        val state = context.getString(if (processing) R.string.keyboard_transcribing else R.string.keyboard_recording)
        if (idle) scope.contentDescription = null else scope.postDelayed({ if (this.phase != SpeechPhase.Idle) scope.contentDescription = state }, 200)
        if (idle || phase == SpeechPhase.Opening) scope.clear()
    }

    fun setLevel(value: Float) = scope.push(value)
    /** The editor went away: the next voice bar starts with an empty scope. */
    fun reset() = scope.clear()

}
