package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import ly.anoma.hex.R
import ly.anoma.hex.speech.SpeechPhase
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** The strip above the keys: a hint or three suggestions and the Hex menu button, and while
 * dictating the dot waveform with cancel and finish. */
internal class StripView(context: Context, private val ime: HexKeyboard, private val host: KeyboardView) : LinearLayout(context) {
    private fun dp(value: Int) = (value * resources.displayMetrics.density).roundToInt()
    private fun color(id: Int) = context.getColor(id)
    private val ink = color(R.color.ink)
    private val quiet = color(R.color.quiet)
    private val key = color(R.color.key)
    private val modifier = color(R.color.modifier)
    private val suggestionCells = LinearLayout(context)
    private var offered = emptyList<String>()
    private var idle = true
    private val statusText = TextView(context)
    private val scope = DotScopeView(context)
    private val menuKey = TextView(context)
    private val cancelKey: FrameLayout
    private val doneKey: FrameLayout
    private val check: ImageView
    private val spinner = ProgressBar(context, null, 0, android.R.style.Widget_Material_ProgressBar_Small)
    var hint = context.getString(R.string.keyboard_hint_hold)
        private set
    /** The Hex menu replaces the keys. */
    var choosing = false
    val recording get() = scope.visibility == VISIBLE

    init {
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(10), 0, dp(8), 0)
        statusText.apply {
            useInter(500); setTextColor(quiet); textSize = 14f; maxLines = 1; ellipsize = TextUtils.TruncateAt.END
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }
        addView(statusText, LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f))
        for (i in 0..2) {
            if (i > 0) suggestionCells.addView(View(context).apply { setBackgroundColor(modifier) }, LayoutParams(dp(1), dp(18)))
            suggestionCells.addView(TextView(context).apply {
                useInter(500); gravity = Gravity.CENTER; setTextColor(ink); textSize = 15f
                maxLines = 1; ellipsize = TextUtils.TruncateAt.END; isFocusable = true
                setPadding(dp(3), 0, dp(3), 0)
                setOnClickListener { offered.getOrNull(i)?.let(ime.engine::pickSuggestion) }
            }, LayoutParams(0, -1, 1f))
        }
        suggestionCells.gravity = Gravity.CENTER_VERTICAL
        addView(suggestionCells, LayoutParams(0, -1, 1f))
        menuKey.apply {
            useInter(550); gravity = Gravity.CENTER; textSize = 14f; setTextColor(ink)
            setOnClickListener { if (choosing) host.closeMenu() else host.openMenu() }
            setPadding(dp(8), 0, dp(8), 0); isFocusable = true
        }
        addView(menuKey, LayoutParams(LayoutParams.WRAP_CONTENT, -1))
        scope.background = shape(color(R.color.sheet), 18)
        scope.clipToOutline = true
        addView(scope, LayoutParams(0, dp(32), 1f).apply { marginEnd = dp(4) })
        cancelKey = panelButton(context.getString(R.string.keyboard_cancel_dictation), modifier, R.drawable.ic_cancel, ime::cancelSpeech)
        doneKey = panelButton(context.getString(R.string.keyboard_finish_dictation), key, R.drawable.ic_check, ime::finishSpeech)
        check = doneKey.getChildAt(1) as ImageView
        spinner.indeterminateTintList = ColorStateList.valueOf(ink)
        spinner.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        doneKey.addView(spinner, FrameLayout.LayoutParams(dp(18), dp(18), Gravity.CENTER))
        statusText.setOnClickListener { if (choosing) host.backInMenu() }
    }

    fun suggestions(words: List<String>, literal: String? = null) {
        offered = if (literal == null) words else listOf(literal) + words.take(2)
        for (i in 0..2) {
            val cell = suggestionCells.getChildAt(i * 2) as TextView
            val s = offered.getOrNull(i).orEmpty()
            val keep = i == 0 && literal != null
            cell.text = if (keep) context.getString(R.string.keyboard_literal, s) else s
            cell.contentDescription = when {
                s.isEmpty() -> null
                keep -> context.getString(R.string.keyboard_keep, s)
                else -> context.getString(R.string.keyboard_suggest, s)
            }
            cell.isEnabled = s.isNotEmpty()
        }
        update()
    }
    fun update() {
        val show = idle && !choosing && offered.isNotEmpty()
        suggestionCells.visibility = if (show) VISIBLE else GONE
        statusText.visibility = if (idle && !show) VISIBLE else GONE
        menuKey.text = context.getString(when { choosing -> R.string.keyboard_menu_done; show -> R.string.keyboard_menu_more; else -> R.string.keyboard_menu_open })
        menuKey.contentDescription = context.getString(if (choosing) R.string.keyboard_menu_done else R.string.keyboard_menu)
        menuKey.minimumWidth = dp(44)
    }
    /** The menu opened: the hint line shows its page's title and goes back a page when tapped. */
    fun menuOpened() { statusText.isClickable = true }
    fun menuClosed() { statusText.isClickable = false; menuKey.text = context.getString(R.string.keyboard_menu_open); statusText.text = hint }
    fun title(text: String) { statusText.text = text }

    /** Shows [phase]'s strip. The caller has already closed the menu if dictation is running. */
    fun status(phase: SpeechPhase, hint: String, menuVisible: Boolean) {
        this.hint = hint
        idle = phase == SpeechPhase.Idle
        val processing = phase == SpeechPhase.Transcribing
        statusText.visibility = if (idle) VISIBLE else GONE
        menuKey.visibility = if (idle) VISIBLE else GONE
        if (!menuVisible) { statusText.isClickable = false; menuKey.text = context.getString(R.string.keyboard_menu_open); statusText.text = hint }
        for (v in listOf(scope, cancelKey, doneKey)) v.visibility = if (idle) GONE else VISIBLE
        spinner.visibility = if (processing) VISIBLE else GONE
        check.visibility = if (processing) GONE else VISIBLE
        doneKey.isEnabled = !processing
        doneKey.contentDescription = context.getString(if (processing) R.string.keyboard_transcribing else R.string.keyboard_finish_dictation)
        scope.contentDescription = context.getString(if (processing) R.string.keyboard_transcribing else R.string.keyboard_recording)
        if (idle || phase == SpeechPhase.Opening) scope.clear()
    }
    /** Before [status]: whether this phase is idle, so the caller knows to close the menu first. */
    fun prepare(phase: SpeechPhase, hint: String) { this.hint = hint; idle = phase == SpeechPhase.Idle }
    fun setLevel(value: Float) = scope.push(value)

    private fun shape(fill: Int, radius: Int) = GradientDrawable().apply {
        setColor(fill); cornerRadius = dp(radius).toFloat()
    }
    private fun panelButton(label: String, fill: Int, icon: Int, action: () -> Unit): FrameLayout {
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
        addView(target, LayoutParams(dp(44), dp(44)))
        return target
    }
}
