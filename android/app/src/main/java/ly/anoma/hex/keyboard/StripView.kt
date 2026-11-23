package ly.anoma.hex.keyboard

import android.animation.ValueAnimator
import android.content.ClipboardManager
import android.content.Context
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Rect
import android.graphics.drawable.GradientDrawable
import android.text.TextPaint
import android.text.TextUtils
import android.util.DisplayMetrics
import android.util.TypedValue
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.animation.DecelerateInterpolator
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import ly.anoma.hex.R
import ly.anoma.hex.speech.SpeechPhase
import ly.anoma.hex.typing.Suggestion
import ly.anoma.hex.useInter
import kotlin.math.ceil
import kotlin.math.hypot
import kotlin.math.roundToInt

/** The strip above the keys, with the Hex mark at its trailing end where Gboard has its mic.
 * Idle it shows three suggestions (the likeliest, or the autocorrection in a small chip, in the
 * middle) or a hint and the Hex menu button. Dictating, the mark becomes ✓ and the strip becomes
 * the recording bar: the dot waveform's well grows out of it, with ✕ beside ✓. */
internal class StripView(context: Context, private val ime: HexKeyboard, private val host: KeyboardView) : FrameLayout(context) {
    private val density = resources.displayMetrics.density
    private fun dp(value: Int) = (value * density).roundToInt()
    private fun px(value: Float) = (value * density).roundToInt()
    private fun color(id: Int) = context.getColor(id)
    private val ink = color(R.color.ink)
    private val quiet = color(R.color.quiet)
    private val key = color(R.color.key)
    private val landscape = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
    val stripHeight = px(if (landscape) KeyboardMetrics.STRIP_LANDSCAPE else KeyboardMetrics.STRIP)
    private val centre = px(if (landscape) KeyboardMetrics.STRIP_CENTER_LANDSCAPE else KeyboardMetrics.STRIP_CENTER)
    private val side = px(KeyboardMetrics.SIDE)
    private val hexWidth = px(KeyboardMetrics.HEX_KEY)
    private val pill = px(KeyboardMetrics.PILL)
    private val cancelWidth = dp(40)

    private val idleRow = LinearLayout(context)
    private val statusText = TextView(context)
    private val slotRow = LinearLayout(context)
    private val slotViews = List(3) { FrameLayout(context) }
    private val words = List(3) { Word(context) }
    private val measurePaint = TextPaint()
    private val dividers = List(2) { View(context) }
    private var offered = arrayOfNulls<String>(3)
    private val menuKey = TextView(context)
    // The tools in the hint's place while the strip has nothing else to show (Labs › Keyboard ›
    // Keyboard tools): emoji, paste, Hex's Keyboard settings and undo of the last autocorrection.
    private val toolsRow = LinearLayout(context)
    private val toolEmoji = toolButton(R.drawable.ic_emoji, R.string.keyboard_emoji) { host.openEmoji() }
    private val toolPaste = toolButton(R.drawable.ic_paste, R.string.keyboard_tool_paste) { ime.pasteClipboard() }
    private val toolSettings = toolButton(R.drawable.ic_settings, R.string.keyboard_tool_settings) { ime.openKeyboardSettings() }
    private val toolUndo = toolButton(R.drawable.ic_undo, R.string.keyboard_tool_undo) { ime.engine.undoAutocorrection() }
    private val clipboard = context.getSystemService(ClipboardManager::class.java)
    // Undo dictation, in the slots' place after a dictation (iPhone Hex's chip).
    private val undoChip = TextView(context)
    private val undoBox = FrameLayout(context)
    private var undoing = false
    /** The hint line holds a dictation's note ("No speech heard", "Stopped at 1 minute"), not the idle hint. */
    private var noting = false
    // A dismissible line in the hint's place (Hex Preview's update): it goes for this field once typing starts.
    private val noticeRow = LinearLayout(context)
    private val noticeText = TextView(context)
    private val noticeClose = FrameLayout(context)
    private var notice: StripNotice? = null

    private val well = FrameLayout(context)
    private val scope = DotScopeView(context)
    private val cancelKey = FrameLayout(context)
    private val hexKey = FrameLayout(context)
    private val hexFace = View(context)
    private val mark = ImageView(context)
    private val check = ImageView(context)
    private val spinner = ProgressBar(context, null, 0, android.R.style.Widget_Material_ProgressBar_Small)

    private var idle = true
    private var reveal: ValueAnimator? = null
    private val holdHint = context.getString(R.string.keyboard_hint_hold)
    var hint = holdHint
        private set
    /** The Hex menu replaces the keys. */
    var choosing = false
    val recording get() = !idle

    // The Hex key: a tap dictates (✓ then finishes), a still hold of 350 ms is push to talk, and
    // a slide of 10 dp opens the menu, as on iPhone. It stays mounted while it turns into ✓, so
    // a hold's own finger-up finishes the dictation it started.
    private var held = false
    private var tracking = false
    private var originX = 0f
    private var originY = 0f
    private val slide = 10 * density
    private val hold = Runnable { if (tracking) held = ime.beginSpeech() }

    init {
        // Everything centres on the line Gboard's toolbar icons sit on, a little above the middle.
        setPadding(0, 0, 0, (stripHeight - 2 * centre).coerceAtLeast(0))
        clipChildren = false

        statusText.apply {
            useInter(500); setTextColor(quiet); textSize = 14f; maxLines = 1; ellipsize = TextUtils.TruncateAt.END
            gravity = Gravity.CENTER_VERTICAL; setPadding(dp(10), 0, dp(4), 0)
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
            setOnClickListener { if (choosing) host.backInMenu() }
        }
        idleRow.addView(statusText, LinearLayout.LayoutParams(0, -1, 1f))
        noticeText.apply {
            useInter(500); setTextColor(ink); textSize = 14f; maxLines = 1; ellipsize = TextUtils.TruncateAt.END
            gravity = Gravity.CENTER_VERTICAL; setPadding(dp(10), 0, dp(4), 0); isFocusable = true
            setOnClickListener { notice?.let { notice = null; update(); it.tap() } }
        }
        noticeClose.apply {
            isFocusable = true
            addView(icon(R.drawable.ic_cancel), LayoutParams(dp(16), dp(16), Gravity.CENTER))
            setOnClickListener { notice?.let { notice = null; update(); it.dismiss() } }
        }
        noticeRow.addView(noticeText, LinearLayout.LayoutParams(0, -1, 1f))
        noticeRow.addView(noticeClose, LinearLayout.LayoutParams(dp(40), -1))
        noticeRow.visibility = GONE
        idleRow.addView(noticeRow, LinearLayout.LayoutParams(0, -1, 1f))
        for (i in 0..2) {
            if (i > 0) slotRow.addView(dividers[i - 1].apply { setBackgroundColor(color(R.color.rule)) },
                LinearLayout.LayoutParams(maxOf(1, dp(1) / 2 + 1), dp(20)).apply { gravity = Gravity.CENTER_VERTICAL })
            words[i].apply {
                useInter(450); gravity = Gravity.CENTER; setTextColor(ink); textSize = StripText.SIZE
                maxLines = 1; ellipsize = TextUtils.TruncateAt.END; includeFontPadding = false
                setPadding(dp(10), 0, dp(10), 0); importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            }
            slotViews[i].apply {
                isFocusable = true
                setOnClickListener { offered[i]?.let(ime.engine::pickSuggestion) }
                addView(words[i], LayoutParams(LayoutParams.WRAP_CONTENT, dp(30), Gravity.CENTER))
            }
            slotRow.addView(slotViews[i], LinearLayout.LayoutParams(0, -1, 1f))
        }
        idleRow.addView(slotRow, LinearLayout.LayoutParams(0, -1, 1f))
        toolsRow.gravity = Gravity.CENTER_VERTICAL
        listOf(toolEmoji, toolPaste, toolSettings, toolUndo).forEach {
            toolsRow.addView(it, LinearLayout.LayoutParams(dp(40), -1))
        }
        toolsRow.visibility = GONE
        idleRow.addView(toolsRow, LinearLayout.LayoutParams(0, -1, 1f))
        undoBox.apply {
            undoChip.apply {
                text = context.getString(R.string.keyboard_undo_dictation); useInter(500); textSize = 14f; setTextColor(ink)
                gravity = Gravity.CENTER; includeFontPadding = false; isFocusable = true
                background = shape(key, dp(15).toFloat()); setPadding(dp(12), 0, dp(14), 0)
                compoundDrawablePadding = dp(6)
                setCompoundDrawablesRelativeWithIntrinsicBounds(context.getDrawable(R.drawable.ic_undo)?.mutate()?.apply {
                    setTint(ink); setBounds(0, 0, dp(16), dp(16))
                }, null, null, null)
                setOnClickListener { ime.engine.undoDictation() }
            }
            addView(undoChip, LayoutParams(LayoutParams.WRAP_CONTENT, dp(30), Gravity.CENTER))
            visibility = GONE
        }
        idleRow.addView(undoBox, LinearLayout.LayoutParams(0, -1, 1f))
        menuKey.apply {
            useInter(550); gravity = Gravity.CENTER; textSize = 14f; setTextColor(ink)
            setOnClickListener { if (choosing) host.closeMenu() else host.openMenu() }
            setPadding(dp(8), 0, dp(8), 0); isFocusable = true; minimumWidth = dp(40)
        }
        idleRow.addView(menuKey, LinearLayout.LayoutParams(LinearLayout.LayoutParams.WRAP_CONTENT, -1))
        addView(idleRow, LayoutParams(-1, -1).apply { marginStart = side; marginEnd = side + hexWidth })

        well.background = shape(color(R.color.sheet), pill / 2f)
        well.clipToOutline = true
        well.addView(scope, LayoutParams(-1, -1).apply { marginStart = dp(10); marginEnd = dp(10) })
        // TalkBack says "Recording" as the bar appears and "Transcribing" when it changes (a live region:
        // a pane title would stay behind as the keyboard window's name).
        scope.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_YES
        scope.accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        val wellStart = side + px(KeyboardMetrics.GAP / 2)
        addView(well, LayoutParams(-1, pill, Gravity.CENTER_VERTICAL).apply {
            marginStart = wellStart; marginEnd = side + hexWidth + cancelWidth - (cancelWidth - pill) / 2 + dp(6)
        })

        cancelKey.apply {
            contentDescription = context.getString(R.string.keyboard_cancel_dictation); isFocusable = true
            setOnClickListener { ime.cancelSpeech() }
            addView(View(context).apply { isDuplicateParentStateEnabled = true; background = pressable(color(R.color.modifier)) },
                LayoutParams(pill, pill, Gravity.CENTER))
            addView(icon(R.drawable.ic_cancel), LayoutParams(dp(19), dp(19), Gravity.CENTER))
        }
        addView(cancelKey, LayoutParams(cancelWidth, -1, Gravity.END).apply { marginEnd = side + hexWidth })

        hexKey.apply {
            isFocusable = true
            hexFace.isDuplicateParentStateEnabled = true
            addView(hexFace, LayoutParams(pill, pill, Gravity.CENTER))
            mark.setImageResource(R.drawable.hex_mark); mark.imageTintList = ColorStateList.valueOf(ink)
            addView(mark, LayoutParams(px(KeyboardMetrics.MARK), px(KeyboardMetrics.MARK), Gravity.CENTER))
            check.setImageResource(R.drawable.ic_check); check.imageTintList = ColorStateList.valueOf(ink)
            addView(check, LayoutParams(dp(19), dp(19), Gravity.CENTER))
            spinner.indeterminateTintList = ColorStateList.valueOf(ink)
            spinner.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            addView(spinner, LayoutParams(dp(18), dp(18), Gravity.CENTER))
            setOnClickListener { ime.toggleSpeech() }
            setOnTouchListener { v, e -> hexTouch(v, e) }
        }
        addView(hexKey, LayoutParams(hexWidth, -1, Gravity.END).apply { marginEnd = side })
        showIdle(true)
    }

    private fun hexTouch(v: View, e: MotionEvent): Boolean {
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                held = false; tracking = true; originX = e.x; originY = e.y; v.isPressed = true
                postDelayed(hold, 350)
            }
            MotionEvent.ACTION_MOVE -> if (!held && tracking && hypot(originX - e.x, originY - e.y) > slide) {
                removeCallbacks(hold); host.openMenu()
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
        return true
    }

    /** The typing model's slots in their display order ([Suggestion.place]): with what was typed among
     * them, typed in quotes on the left then the fix (in a chip) or the likeliest word in the middle;
     * otherwise the likeliest word in the middle and the next on the left, as Gboard and iPhone place
     * them. The emoji always keeps the last slot (Labs › Keyboard › Emoji suggestions). */
    fun suggestions(slots: List<Suggestion>) {
        // Typing or swiping started: the suggestions (a swipe's word so far) matter more than the notice, which comes
        // back in the next field.
        if (slots.any { it.kind == Suggestion.Kind.Typed } || ime.engine.swiping) notice = null
        val shown = Suggestion.place(slots)
        offered = Array(Suggestion.SLOTS) { shown[it]?.text }
        var chip = -1
        for (i in 0..2) {
            val slot = shown[i]
            val s = slot?.text.orEmpty()
            val typed = slot?.kind == Suggestion.Kind.Typed
            val fix = slot?.kind == Suggestion.Kind.Fix
            val emoji = slot?.kind == Suggestion.Kind.Emoji
            if (fix) chip = i
            this.words[i].text = if (typed) context.getString(R.string.keyboard_literal, s) else s
            this.words[i].background = if (fix) shape(key, dp(15).toFloat()) else null
            slotViews[i].contentDescription = when {
                s.isEmpty() -> null
                typed -> context.getString(R.string.keyboard_keep, s)
                fix -> context.getString(R.string.keyboard_correction, s)
                emoji -> context.getString(R.string.keyboard_suggest_emoji, s)
                else -> context.getString(R.string.keyboard_suggest, s)
            }
            slotViews[i].isEnabled = s.isNotEmpty()
        }
        // A divider only between two filled slots, and never beside the chip.
        val filled = (0..2).map { words[it].text.isNotEmpty() }
        dividers[0].visibility = if (chip == 0 || chip == 1 || !filled[0] || !filled[1]) INVISIBLE else VISIBLE
        dividers[1].visibility = if (chip == 1 || chip == 2 || !filled[1] || !filled[2]) INVISIBLE else VISIBLE
        weigh()
        update()
    }
    /** Each slot's share of the row follows its word's width, so a short word leaves the room a long
     * one needs; a word still wider than its share shrinks ([Word]) before it is ellipsized. A slot
     * never falls below a tappable width. */
    private fun weigh() {
        for (i in 0..2) {
            val word = words[i]
            val params = slotViews[i].layoutParams as LinearLayout.LayoutParams
            val weight = if (word.text.isEmpty()) 0f else maxOf(
                naturalWidth(word) + word.paddingLeft + word.paddingRight, dp(56).toFloat())
            if (params.weight == weight) continue
            params.weight = weight; slotViews[i].layoutParams = params
        }
    }
    /** [word]'s width at the strip's own size, whatever size it shrank to for its last slot. */
    private fun naturalWidth(word: TextView): Float {
        measurePaint.set(word.paint)
        measurePaint.textSize = spToPx(StripText.SIZE, resources.displayMetrics)
        return measurePaint.measureText(word.text, 0, word.text.length)
    }
    /** Undo dictation shows in the slots' place, or goes. */
    fun undo(shown: Boolean) { undoing = shown; update() }
    /** Shows [notice] (or none) in the hint's place until typing starts, a tap or its ✕. */
    fun notice(notice: StripNotice?) {
        this.notice = notice
        noticeText.text = notice?.text; noticeText.contentDescription = notice?.text
        noticeClose.contentDescription = notice?.dismissLabel
        update()
    }
    fun update() {
        // A dictation's note wins over the notice (Hex Preview's update line), which returns with the idle hint:
        // after typing, a tap or the next dictation.
        val spoken = idle && !choosing && noting
        val noted = idle && !choosing && notice != null && !spoken
        val undo = idle && !choosing && undoing && !noted
        val show = idle && !choosing && !undo && !noted && offered.any { it != null }
        val tools = KeyboardTools.tools(enabled = ime.feedback.keyboardTools,
            busy = !idle || choosing || show || undo || noted || hint != holdHint, secure = { ime.engine.secure },
            emoji = { ime.engine.emojiAvailable }, canUndo = { ime.engine.canUndoAutocorrection },
            clipboard = { clipboard.hasPrimaryClip() })
        noticeRow.visibility = if (noted) VISIBLE else GONE
        undoBox.visibility = if (undo) VISIBLE else GONE
        // A note that came with text ("Stopped at 1 minute") shares the line with Undo, which then takes only its width,
        // and the menu key shrinks to ⋯ as it does beside suggestions, so the note isn't cut short.
        val shared = undo && spoken
        (undoBox.layoutParams as LinearLayout.LayoutParams).apply {
            if ((weight == 0f) != shared) { width = if (shared) LinearLayout.LayoutParams.WRAP_CONTENT else 0; weight = if (shared) 0f else 1f; undoBox.layoutParams = this }
        }
        slotRow.visibility = if (show) VISIBLE else GONE
        toolsRow.visibility = if (tools.isNotEmpty()) VISIBLE else GONE
        toolEmoji.visibility = if (StripTool.Emoji in tools) VISIBLE else GONE
        toolPaste.visibility = if (StripTool.Paste in tools) VISIBLE else GONE
        toolUndo.visibility = if (StripTool.Undo in tools) VISIBLE else GONE
        statusText.visibility = if (idle && !show && (!undo || spoken) && !noted && tools.isEmpty()) VISIBLE else GONE
        menuKey.text = context.getString(when { choosing -> R.string.keyboard_menu_done; show || shared -> R.string.keyboard_menu_more; else -> R.string.keyboard_menu_open })
        menuKey.contentDescription = context.getString(if (choosing) R.string.keyboard_menu_done else R.string.keyboard_menu)
    }
    /** The menu opened: the hint line shows its page's title and goes back a page when tapped. */
    fun menuOpened() { statusText.isClickable = true }
    fun menuClosed() { statusText.isClickable = false; menuKey.text = context.getString(R.string.keyboard_menu_open); statusText.text = hint }
    fun title(text: String) { statusText.text = text }

    /** Shows [phase]'s strip; [note] says [hint] is a dictation's note. The caller has already closed the menu if
     * dictation is running. */
    fun status(phase: SpeechPhase, hint: String, menuVisible: Boolean, note: Boolean = false) {
        this.hint = hint
        noting = note && phase == SpeechPhase.Idle
        val wasIdle = idleRow.visibility == VISIBLE
        idle = phase == SpeechPhase.Idle
        val processing = phase == SpeechPhase.Transcribing
        if (!menuVisible) { statusText.isClickable = false; menuKey.text = context.getString(R.string.keyboard_menu_open); statusText.text = hint }
        if (idle != wasIdle) showIdle(idle)
        spinner.visibility = if (processing) VISIBLE else GONE
        check.visibility = if (!idle && !processing) VISIBLE else GONE
        hexKey.contentDescription = context.getString(when {
            idle -> R.string.keyboard_hex
            processing -> R.string.keyboard_transcribing
            else -> R.string.keyboard_finish_dictation
        })
        // Set just after the bar shows, so the live region announces it.
        val state = context.getString(if (processing) R.string.keyboard_transcribing else R.string.keyboard_recording)
        if (idle) scope.contentDescription = null else scope.postDelayed({ if (!this.idle) scope.contentDescription = state }, 200)
        if (idle || phase == SpeechPhase.Opening) scope.clear()
    }
    /** Before [status]: this phase's hint and whether it is idle, so the caller can close the menu first. */
    fun prepare(phase: SpeechPhase, hint: String) { this.hint = hint; idle = phase == SpeechPhase.Idle }
    fun setLevel(value: Float) = scope.push(value)

    /** Idle: suggestions and the Hex mark. Dictating: the mark is ✓ and the well grows leftwards out of it. */
    private fun showIdle(idle: Boolean) {
        reveal?.cancel(); reveal = null
        idleRow.visibility = if (idle) VISIBLE else GONE
        well.visibility = if (idle) GONE else VISIBLE
        cancelKey.visibility = if (idle) GONE else VISIBLE
        mark.visibility = if (idle) VISIBLE else GONE
        hexFace.background = if (idle) pressable(android.graphics.Color.TRANSPARENT) else pressable(key)
        well.clipBounds = null
        if (idle || !ValueAnimator.areAnimatorsEnabled()) return
        cancelKey.alpha = 0f
        reveal = ValueAnimator.ofFloat(0f, 1f).apply {
            duration = 240; interpolator = DecelerateInterpolator()
            addUpdateListener {
                val f = it.animatedValue as Float
                val w = well.width
                if (w > 0) well.clipBounds = Rect((w * (1 - f)).roundToInt(), 0, w, well.height)
                cancelKey.alpha = f
            }
            start()
        }
    }

    fun cancelGestures() {
        removeCallbacks(hold)
        held = false; tracking = false
        hexKey.isPressed = false
    }

    /** A slot's word: [StripText.SIZE] when it fits, smaller (down to [StripText.MIN]) before the
     * strip ellipsizes it. The slot's width arrives as the measure spec, so nothing waits for layout. */
    private class Word(context: Context) : TextView(context) {
        private val measure = TextPaint()

        override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
            if (MeasureSpec.getMode(widthMeasureSpec) != MeasureSpec.UNSPECIFIED) {
                val available = MeasureSpec.getSize(widthMeasureSpec) - compoundPaddingLeft - compoundPaddingRight
                measure.set(paint)
                val fitted = fitTextSize(available.toFloat(), StripText.SIZE, StripText.MIN) { sp ->
                    measure.textSize = spToPx(sp, resources.displayMetrics)
                    ceil(measure.measureText(text, 0, text.length))
                }
                setTextSize(TypedValue.COMPLEX_UNIT_SP, fitted)
            }
            super.onMeasure(widthMeasureSpec, heightMeasureSpec)
        }
    }

    /** One of the strip's tools: its icon on the strip's line, with the cancel key's round press. */
    private fun toolButton(res: Int, label: Int, action: () -> Unit) = FrameLayout(context).apply {
        isFocusable = true; contentDescription = context.getString(label)
        addView(View(context).apply { isDuplicateParentStateEnabled = true; background = pressable(android.graphics.Color.TRANSPARENT) },
            LayoutParams(pill, pill, Gravity.CENTER))
        addView(icon(res), LayoutParams(dp(19), dp(19), Gravity.CENTER))
        setOnClickListener { action() }
    }
    private fun icon(res: Int) = ImageView(context).apply { setImageResource(res); imageTintList = ColorStateList.valueOf(ink) }
    private fun shape(fill: Int, radius: Float) = GradientDrawable().apply { setColor(fill); cornerRadius = radius }
    private fun pressable(fill: Int) = android.graphics.drawable.StateListDrawable().apply {
        addState(intArrayOf(android.R.attr.state_pressed), GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(color(R.color.pressed)) })
        addState(intArrayOf(), GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(fill) })
    }
}

/** [size] in sp as pixels through the conversion [TextView.setTextSize] uses: Android 14+ makes it
 * non-linear above a 1.0 font scale, so a cached pixel-per-sp never matches a size set in sp. */
private fun spToPx(size: Float, metrics: DisplayMetrics): Float =
    TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, size, metrics)
