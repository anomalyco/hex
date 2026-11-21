package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.media.AudioManager
import android.text.SpannableString
import android.text.Spanned
import android.text.TextPaint
import android.text.style.ForegroundColorSpan
import android.text.style.MetricAffectingSpan
import android.text.style.RelativeSizeSpan
import android.view.ContextThemeWrapper
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.emoji2.emojipicker.EmojiPickerView
import ly.anoma.hex.R
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.KeyPoint
import ly.anoma.hex.typing.KeyRect
import ly.anoma.hex.typing.TypingWord
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** The keys: three rows from the page's layout, then ?123, comma, space, period and enter. Every
 * point of the area types a key (KeyRowLayout); the bottom row stays mounted across page changes.
 * Keys that type as they lift do so in the order the fingers went down ([Rollover]). */
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
    private val comma: KeyCap
    private val emojiKey: KeyCap
    private val period: KeyCap
    // The emoji panel: AndroidX's picker where the strip and the letter rows are, then ABC, space and delete.
    private var picker: EmojiPickerView? = null
    private val emojiBottom = KeyRowLayout(context, 10f, side, gap, insetTop, insetBottom + bottomExtra)
    var emojiOpen = false
        private set
    private var commaText = ","
    private val roll = Rollover()
    // A slide from ?123 or shift types the key it lifts on: the keys it can type, and the one under it.
    private val slides = HashMap<KeyCap, (KeyPoint) -> Unit>()
    private val bottomSlides = HashMap<KeyCap, (KeyPoint) -> Unit>()
    private var slidOnto: KeyCap? = null
    private val onScreen = IntArray(2)
    private val hostAt = IntArray(2)
    private var deleting = false
    private var repeated = false
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    private val spaceGesture = SpaceGesture(KeyboardMetrics.CURSOR_START * density, KeyboardMetrics.CURSOR_STEP * density, slop.toFloat())
    private var cursorSlide: CursorSlide? = null
    private var spacePoint: KeyPoint? = null
    // The page the keys show; a change of shift alone relabels them in place, so a held shift key
    // stays mounted and its own finger-up ends a chord.
    private var current: KeyboardPage? = null
    private val caseKeys = ArrayList<Pair<Char, KeyCap>>()
    private var shiftIcon: ImageView? = null
    private val switchLanguage = Runnable {
        if (spaceKey.isPressed && spaceGesture.still) { spaceKey.isPressed = false; spaceKey.performLongClick() }
    }
    private val deleteGesture = DeleteGesture(KeyboardMetrics.DELETE_START * density, KeyboardMetrics.DELETE_STEP * density,
        KeyboardMetrics.DELETE_CANCEL * density, slop.toFloat())
    private var deleteSlide: DeleteSlide? = null
    private val repeatDelete = object : Runnable {
        override fun run() {
            if (deleting) { repeated = true; ime.engine.delete(); postDelayed(this, KeyboardMetrics.DELETE_REPEAT_EVERY) }
        }
    }

    init {
        orientation = VERTICAL
        rows.orientation = VERTICAL
        addView(rows, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        addView(bottom, LayoutParams(LayoutParams.MATCH_PARENT, rowHeight + bottomExtra))
        modeKey = caps.cap(context.getString(R.string.keyboard_numbers_label), context.getString(R.string.keyboard_numbers),
            modifier, 17f, ime.engine::mode)
        // ?123 switches pages as it goes down, as Gboard's; a slide onto a key types it and comes back.
        modeKey.setOnTouchListener { v, e ->
            when (e.actionMasked) {
                MotionEvent.ACTION_DOWN -> { roll.down(v); v.isPressed = true; endSlide(); ime.engine.mode() }
                MotionEvent.ACTION_MOVE -> slideOver(v, e)
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    roll.up(v); v.isPressed = false
                    val onto = slidOnto; endSlide()
                    if (onto != null && e.actionMasked == MotionEvent.ACTION_UP && typeSlid(onto, e)) ime.engine.mode()
                }
            }
            true
        }
        comma = caps.cap(",", context.getString(R.string.keyboard_comma), modifier) { ime.engine.type(commaText) }
        typesOnLift(comma, preview = { null }, holds = { emptyList() }, type = { comma.performClick() })
        bottomSlides[comma] = { ime.engine.type(commaText) }
        // Emoji on the letters; on the symbol pages Gboard's 1234 key, which opens its number pad.
        emojiKey = caps.icon(R.drawable.ic_emoji, context.getString(R.string.keyboard_emoji), caps.key,
            { if (current?.symbols == true) ime.engine.pad(Pad.Symbols) else host.openEmoji() })
        emojiKey.face.addView(TextView(context).apply {
            text = context.getString(R.string.keyboard_pad_label); textSize = 13f; useInter(500); setTextColor(ink)
            gravity = Gravity.CENTER; includeFontPadding = false; setLineSpacing(0f, 0.9f); visibility = GONE
        }, FrameLayout.LayoutParams(-1, -1))
        typesOnLift(emojiKey, preview = { null }, holds = { emptyList() }, type = { emojiKey.performClick() })
        spaceKey = caps.cap("", context.getString(R.string.keyboard_space), caps.key, 14f) {
            ime.engine.space(spacePoint, if (current?.symbols == false) geometry() else null); spacePoint = null
        }
        spaceKey.sound = AudioManager.FX_KEYPRESS_SPACEBAR
        spaceKey.label?.setTextColor(quiet)
        spaceKey.setOnLongClickListener { host.openLanguageMenu(); true }
        // A press that leaves the key still types its space on lifting: a quick tap that drifts must
        // never lose it. Only a slide that moved the cursor, or the long press's menu, types none.
        spaceKey.setOnTouchListener { v, e ->
            when (e.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    // Another key going down while space is held types the space first, unless it moved the cursor.
                    roll.down(v) {
                        removeCallbacks(switchLanguage); val typing = v.isPressed && cursorSlide == null
                        v.isPressed = false; cursorSlide = null
                        if (typing) v.performClick()
                    }
                    v.isPressed = true; cursorSlide = null; spaceGesture.down(e.x, e.y, e.eventTime)
                    spacePoint = KeyPoint((v.left + e.x).toDouble(), (bottom.top - rows.top + e.y).toDouble())
                    postDelayed(switchLanguage, ViewConfiguration.getLongPressTimeout().toLong())
                }
                MotionEvent.ACTION_MOVE -> if (v.isPressed) {
                    val characters = spaceGesture.move(e.x, e.y, e.eventTime)
                    if (!spaceGesture.still) removeCallbacks(switchLanguage)
                    if (characters != null) (cursorSlide ?: ime.engine.cursorSlide().also { cursorSlide = it }).moveTo(characters)
                }
                MotionEvent.ACTION_UP -> if (roll.up(v)) {
                    removeCallbacks(switchLanguage); val pressed = v.isPressed; v.isPressed = false
                    if (pressed && spaceGesture.up()) v.performClick()
                    if (pressed) spaceKey.typedOnLift()
                    cursorSlide = null
                }
                MotionEvent.ACTION_CANCEL -> { roll.up(v); removeCallbacks(switchLanguage); v.isPressed = false; cursorSlide = null }
            }
            true
        }
        period = caps.cap(".", context.getString(R.string.keyboard_period), modifier) { ime.engine.type(".") }
        // Held on the letters page, period offers Gboard's punctuation; on the symbol pages an ellipsis.
        typesOnLift(period, preview = { null },
            holds = { if (current?.symbols == false) LetterFeedback.periodHolds else listOf(LetterFeedback.symbolHolds('.')) },
            default = { if (current?.symbols == false) LetterFeedback.PERIOD_DEFAULT else 0 },
            type = { option -> if (option != null) ime.engine.type(option) else period.performClick() })
        bottomSlides[period] = { ime.engine.type(".") }
        enterKey = caps.cap("", context.getString(R.string.keyboard_return), modifier, 17f, ime.engine::enter)
        enterKey.sound = AudioManager.FX_KEYPRESS_RETURN
        typesOnLift(enterKey, preview = { null }, holds = { emptyList() }, type = { enterKey.performClick() })
        enterIcon.setImageResource(R.drawable.ic_enter)
        enterIcon.imageTintList = ColorStateList.valueOf(ink)
        enterKey.face.addView(enterIcon, FrameLayout.LayoutParams(dp(24), dp(24), Gravity.CENTER))
        var at = 0f
        for ((key, width) in listOf(modeKey to KeyboardMetrics.MODE, comma to KeyboardMetrics.COMMA, emojiKey to KeyboardMetrics.EMOJI,
                spaceKey to KeyboardMetrics.SPACE, period to KeyboardMetrics.PERIOD, enterKey to KeyboardMetrics.ENTER)) { bottom.add(key, at, width); at += width }
        // Gboard's emoji panel ends with ABC and delete; Hex puts a space bar between them.
        val letters = caps.cap(context.getString(R.string.keyboard_letters_label), context.getString(R.string.keyboard_letters),
            modifier, 17f) { host.closeEmoji(); ime.engine.letters() }
        typesOnLift(letters, preview = { null }, holds = { emptyList() }, type = { letters.performClick() })
        val emojiSpace = caps.cap("", context.getString(R.string.keyboard_space), caps.key, 14f) { ime.engine.space() }
        emojiSpace.sound = AudioManager.FX_KEYPRESS_SPACEBAR
        typesOnLift(emojiSpace, preview = { null }, holds = { emptyList() }, type = { emojiSpace.performClick() })
        emojiBottom.add(letters, 0f, KeyboardMetrics.MODE)
        emojiBottom.add(emojiSpace, KeyboardMetrics.MODE, 10f - 2 * KeyboardMetrics.MODE)
        emojiBottom.add(newDeleteKey(), 10f - KeyboardMetrics.MODE, KeyboardMetrics.MODE)
        emojiBottom.visibility = GONE
        addView(emojiBottom, LayoutParams(LayoutParams.MATCH_PARENT, rowHeight + bottomExtra))
    }

    private fun cased(c: Char) = if (current?.let { !it.symbols && it.shifted } == true) c.uppercaseChar().toString() else c.toString()

    fun showPage(page: KeyboardPage) {
        val shown = current
        if (shown != null && !page.symbols && page.copy(shifted = shown.shifted, locked = shown.locked) == shown) {
            current = page; relabel(page); return
        }
        current = page
        clearCallout(); endSlide(); grid = null; letters.clear(); caseKeys.clear(); slides.clear(); shiftIcon = null
        rows.removeAllViews()
        page.pad?.let { showPad(page, it); return }
        bottom.visibility = VISIBLE
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
                    context.getString(R.string.keyboard_more_symbols), modifier, 15f, ime.engine::shiftUp)
                else caps.icon(shiftIconFor(page), shiftDescription(page), modifier, { ime.engine.shiftDown(); ime.engine.shiftUp() }).also { key ->
                    shiftIcon = key.face.getChildAt(0) as ImageView
                    // Shift acts as it goes down and again as it lifts: a chord types capitals, and a slide
                    // onto a letter types its capital (Gboard), then shift goes off.
                    key.setOnTouchListener { v, e ->
                        when (e.actionMasked) {
                            MotionEvent.ACTION_DOWN -> { roll.down(v); v.isPressed = true; endSlide(); ime.engine.shiftDown() }
                            MotionEvent.ACTION_MOVE -> if (v.isPressed) slideOver(v, e)
                            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                                roll.up(v)
                                val onto = slidOnto; endSlide()
                                if (onto != null && e.actionMasked == MotionEvent.ACTION_UP) typeSlid(onto, e)
                                if (v.isPressed) { v.isPressed = false; ime.engine.shiftUp() }
                            }
                        }
                        true
                    }
                }
                row.add(switch, 0f, side); at = side
            }
            for ((position, c) in line.withIndex()) {
                var touch: KeyPoint? = null
                // The letter as it was when the finger went down, so shift lifting mid-press can't change it.
                var downText: String? = null
                val k = caps.cap(cased(c), cased(c)) {
                    val g = if (symbols) null else geometry()
                    val p = touch ?: g?.centres?.get(c)
                    val text = (downText ?: cased(c)).let { if (symbols || touch == null) it else likelier(c, it, touch!!) }
                    if (g != null && p != null) ime.engine.letter(text, p.x, p.y, g) else ime.engine.type(text)
                }
                row.add(k, at, width); at += width
                if (!symbols) { letters[c] = k; caseKeys.add(c to k) }
                val digit = if (symbols) null else KeyboardLetters.digit(index, position)
                k.hint(digit?.toString())
                fun holds() = if (symbols) LetterFeedback.symbolHolds(c) else LetterFeedback.holds(cased(c).single(), language.layoutCode, digit)
                if (holds().isNotEmpty()) k.setOnLongClickListener { host.openAccents(holds()); true }
                slides[k] = { p ->
                    val g = if (symbols) null else geometry()
                    if (g != null) ime.engine.letter(cased(c), p.x, p.y, g) else ime.engine.type(cased(c))
                }
                typesOnLift(k, preview = { cased(c) }, holds = { listOf(holds()) },
                    down = { e ->
                        downText = cased(c)
                        touch = KeyPoint((k.left + e.getX(e.actionIndex)).toDouble(), (row.top + e.getY(e.actionIndex)).toDouble())
                    },
                    type = { option ->
                        val text = downText ?: cased(c)
                        if (option != null && option != text) {
                            val g = geometry(); val p = touch
                            if (g != null && p != null) ime.engine.letter(option, p.x, p.y, g, explicit = true) else ime.engine.type(option)
                        } else k.performClick()
                    },
                    done = { touch = null; downText = null })
            }
            if (index == 2) deleteKey = newDeleteKey().also { row.add(it, columns - side, side) }
        }
        (emojiKey.face.getChildAt(0)).visibility = if (symbols) GONE else VISIBLE
        (emojiKey.face.getChildAt(1)).visibility = if (symbols) VISIBLE else GONE
        emojiKey.contentDescription = context.getString(if (symbols) R.string.keyboard_pad else R.string.keyboard_emoji)
        modeKey.label?.text = context.getString(if (symbols) R.string.keyboard_letters_label else R.string.keyboard_numbers_label)
        modeKey.contentDescription = context.getString(if (symbols) R.string.keyboard_letters else R.string.keyboard_numbers)
        commaText = if (symbols) "," else page.kind.mark
        comma.label?.text = commaText
        comma.label?.textSize = if (commaText == "@") 21f else 26f
        comma.contentDescription = context.getString(when (commaText) { "@" -> R.string.keyboard_at; "/" -> R.string.keyboard_slash; else -> R.string.keyboard_comma })
        // An app's own label, else Gboard's symbol for the editor's action.
        val label = page.enter.label
        enterKey.label?.text = label.orEmpty()
        enterIcon.setImageResource(actionIcon(page.enter.action))
        enterIcon.visibility = if (label == null) View.VISIBLE else View.GONE
        enterKey.contentDescription = label ?: actionLabel(page.enter.action) ?: context.getString(R.string.keyboard_return)
    }

    /** A pad in place of the letter rows and the bottom row (Gboard's for number, phone and date fields,
     * and the number pad of its symbol page): its keys 53.3 dp tall on the letters' rows. */
    private fun showPad(page: KeyboardPage, pad: Pad) {
        bottom.visibility = GONE
        val cells = KeyPads.cells(pad)
        val padKey = maxOf(keyHeight, rowHeight - px(KeyboardMetrics.PAD_ROW_GAP))
        val top = (rowHeight - padKey) / 2; val under = rowHeight - padKey - top
        val lines = KeyPads.rows(pad)
        // The symbol pad's operators stand in one column beside its first three rows.
        val block = if (pad == Pad.Symbols) PadBlock(cells, reach + top, under).also {
            rows.addView(it, LayoutParams(LayoutParams.MATCH_PARENT, 3 * rowHeight + reach))
        } else null
        for ((index, line) in lines.withIndex()) {
            val first = index == 0; val last = index == lines.lastIndex
            // The symbol pad ends on the letters' bottom row, as Gboard's does.
            val t = if (last && pad == Pad.Symbols) insetTop else top
            val b = if (last && pad == Pad.Symbols) insetBottom else under
            val row = KeyRowLayout(context, cells, side, gap, t + (if (first) reach else 0), b + (if (last) bottomExtra else 0))
            val height = rowHeight + (if (first) reach else 0) + (if (last) bottomExtra else 0)
            (if (block != null && !last) block.lines else rows).addView(row, LayoutParams(LayoutParams.MATCH_PARENT, height))
            for (key in line) row.add(padKeyView(key, page), key.start, key.width)
        }
    }

    private fun padKeyView(key: PadKey, page: KeyboardPage): KeyCap {
        val fill = if (key.modifier) modifier else caps.key
        val label = if (key.word != 0) context.getString(key.word) else key.label
        val k = when (key.action) {
            PadAction.Delete -> return newDeleteKey()
            PadAction.Space -> caps.icon(R.drawable.ic_space, context.getString(R.string.keyboard_space), fill, { ime.engine.space() })
                .also { it.sound = AudioManager.FX_KEYPRESS_SPACEBAR }
            PadAction.Enter -> (page.enter.label?.let { caps.cap(it, it, fill, 17f, ime.engine::enter) }
                ?: caps.icon(actionIcon(page.enter.action), actionLabel(page.enter.action) ?: context.getString(R.string.keyboard_return),
                    fill, ime.engine::enter)).also { it.sound = AudioManager.FX_KEYPRESS_RETURN }
            else -> caps.cap(label, if (key.action is PadAction.Go && key.word == 0) context.getString(R.string.keyboard_phone_symbols) else label,
                fill, when { key.word != 0 -> 17f; label.length > 1 -> 21f; else -> 26f }) { act(key.action) }
        }
        // A phone digit's letters beside it, small and quiet, as Gboard's.
        key.letters?.let { letters ->
            k.label?.text = SpannableString("$label  $letters").apply {
                setSpan(RelativeSizeSpan(0.46f), label.length, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                setSpan(ForegroundColorSpan(quiet), label.length, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                setSpan(Centred, label.length, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            }
        }
        typesOnLift(k, preview = { null }, holds = { key.hold?.let { listOf(listOf(it)) } ?: emptyList() },
            type = { option -> if (option != null) ime.engine.type(option) else k.performClick() })
        return k
    }
    /** Small letters beside a big digit sit at its middle, not on its baseline. */
    private object Centred : MetricAffectingSpan() {
        private fun raise(paint: TextPaint) { paint.baselineShift -= (paint.textSize * 0.62f).roundToInt() }
        override fun updateDrawState(paint: TextPaint) = raise(paint)
        override fun updateMeasureState(paint: TextPaint) = raise(paint)
    }
    private fun act(action: PadAction) = when (action) {
        is PadAction.Text -> ime.engine.type(action.text)
        is PadAction.Go -> ime.engine.pad(action.pad)
        PadAction.Letters -> ime.engine.letters()
        PadAction.SymbolsPage -> ime.engine.symbolsPage()
        PadAction.Space -> ime.engine.space()
        PadAction.Enter -> ime.engine.enter()
        PadAction.Delete -> ime.engine.delete()
    }

    /** The symbol pad's first three rows with its operator column laid over their first cells: one
     * pill of the modifier colour that scrolls, four operators showing, as Gboard's. */
    private inner class PadBlock(private val cells: Float, private val top: Int, private val under: Int) : FrameLayout(context) {
        val lines = LinearLayout(context).apply { orientation = VERTICAL }
        private val column = ScrollView(context).apply {
            background = GradientDrawable().apply { cornerRadius = KeyboardMetrics.CORNER * density; setColor(modifier) }
            clipToOutline = true; isVerticalScrollBarEnabled = false; overScrollMode = OVER_SCROLL_NEVER
        }
        private val items = LinearLayout(context).apply { orientation = VERTICAL }
        init {
            addView(lines, LayoutParams(-1, -1)); addView(column); column.addView(items)
            for (op in KeyPads.operators) {
                val k = caps.cap(op, op, Color.TRANSPARENT, 21f) { ime.engine.type(op) }
                typesOnLift(k, preview = { null }, holds = { emptyList() }, type = { k.performClick() })
                items.addView(k)
            }
        }
        private fun cell() = (measuredWidth - 2 * side) / cells
        override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
            super.onMeasure(widthMeasureSpec, heightMeasureSpec)
            val w = ((side + 1.5f * cell() - gap / 2) - (side + gap / 2)).roundToInt()
            val h = measuredHeight - top - under
            for (i in 0 until items.childCount) items.getChildAt(i).layoutParams = LinearLayout.LayoutParams(w, h / 4)
            column.measure(MeasureSpec.makeMeasureSpec(w, MeasureSpec.EXACTLY), MeasureSpec.makeMeasureSpec(h, MeasureSpec.EXACTLY))
        }
        override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
            lines.layout(0, 0, r - l, b - t)
            val left = (side + gap / 2).roundToInt()
            column.layout(left, top, left + column.measuredWidth, top + column.measuredHeight)
        }
    }

    /** Delete: a lifted press deletes once; held still it repeats, slid left it selects words to delete. */
    private fun newDeleteKey(): KeyCap = caps.icon(R.drawable.ic_delete, context.getString(R.string.keyboard_delete), modifier, ime.engine::delete).also { k ->
        k.sound = AudioManager.FX_KEYPRESS_DELETE
        fun lift(typing: Boolean) {
            removeCallbacks(repeatDelete); val pressed = k.isPressed; k.isPressed = false
            val slide = deleteSlide
            when {
                slide != null -> if (slide.words > 0) ime.engine.deleteSelection()
                typing && deleting && !repeated && deleteGesture.tap -> k.performClick()
            }
            if (typing && pressed) k.typedOnLift()
            deleting = false; deleteSlide = null
        }
        k.setOnTouchListener { v, e ->
            when (e.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    roll.down(v) { lift(typing = true) }
                    repeated = false; deleting = true; v.isPressed = true; deleteSlide = null
                    deleteGesture.down(e.x)
                    postDelayed(repeatDelete, KeyboardMetrics.DELETE_REPEAT_DELAY)
                }
                MotionEvent.ACTION_MOVE -> if (deleting) {
                    val words = deleteGesture.move(e.x)
                    if (words > 0 && deleteSlide == null && !repeated) {
                        removeCallbacks(repeatDelete)
                        deleteSlide = ime.engine.deleteSlide()
                    }
                    if (!deleteGesture.still) removeCallbacks(repeatDelete)
                    deleteSlide?.select(words)
                }
                MotionEvent.ACTION_UP -> if (roll.up(v)) lift(typing = true)
                MotionEvent.ACTION_CANCEL -> {
                    roll.up(v); removeCallbacks(repeatDelete); deleting = false; v.isPressed = false
                    deleteSlide?.select(0); deleteSlide = null
                }
            }
            true
        }
    }

    /** The emoji panel opens in place of the letter rows and the strip above them ([extra]), and grows as
     * Gboard's does (124 dp above the keys), as long as the keyboard stays within 60% of the screen (two
     * thirds turned). A new
     * picker each time: it reads its recents and skin tones as it starts (its own preferences, on this
     * device), and only refreshes them later once its Recently used row scrolls away. */
    fun openEmoji(extra: Int) {
        cancelGestures()
        val keyboard = height + extra // the keys' and the strip's height as they are
        // Turned, the screen is short: the panel may take up to two thirds of it, with smaller cells.
        val share = if (landscape) 0.67f else 0.6f
        val grow = minOf(px(KeyboardMetrics.EMOJI_GROW), (resources.displayMetrics.heightPixels * share).toInt() - keyboard).coerceAtLeast(0)
        val height = 3 * rowHeight + reach + extra + grow
        picker?.let(::removeView)
        val p = EmojiPickerView(ContextThemeWrapper(context, R.style.HexEmoji))
        p.setOnEmojiPickedListener { item ->
            // Gboard's tap as an emoji is chosen; the picker's own view plays the click.
            if (vibrates(context, ime.feedback.haptics)) p.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
            ime.engine.type(item.emoji)
        }
        picker = p; addView(p, 0, LayoutParams(LayoutParams.MATCH_PARENT, height))
        // Square cells about Gboard's size: nine across in portrait, more when turned. The picker sizes its rows
        // from what its grid has left after two category titles.
        val cell = if (landscape) KeyboardMetrics.EMOJI_CELL_LANDSCAPE else KeyboardMetrics.EMOJI_CELL
        val columns = maxOf(9, (resources.displayMetrics.widthPixels / (cell * density)).toInt())
        p.emojiGridColumns = columns
        p.emojiGridRows = ((height - (KeyboardMetrics.EMOJI_HEADER + 2 * KeyboardMetrics.EMOJI_TITLE) * density) /
            (resources.displayMetrics.widthPixels.toFloat() / columns)).coerceAtLeast(1f)
        p.layoutParams.height = height
        rows.visibility = GONE; bottom.visibility = GONE
        p.visibility = VISIBLE; emojiBottom.visibility = VISIBLE
        emojiOpen = true
    }
    fun closeEmoji() {
        if (!emojiOpen) return
        cancelGestures()
        picker?.let(::removeView); picker = null; emojiBottom.visibility = GONE
        rows.visibility = VISIBLE; bottom.visibility = VISIBLE
        emojiOpen = false
    }

    /** Touches for a key that types as it lifts: pressed, its pop-up, after [KeyboardMetrics.HOLD_MS] its held
     * options to slide over, and on lifting (or when another key goes down first) [type] with the option
     * chosen, or null for the key's own. */
    private fun typesOnLift(k: KeyCap, preview: () -> String?, holds: () -> List<List<String>>, default: () -> Int = { 0 },
                            down: (MotionEvent) -> Unit = {}, type: (String?) -> Unit, done: () -> Unit = {}) {
        fun finish(): Boolean {
            val option = if (feedback.choosing) feedback.selected() else null
            val pressed = k.isPressed; k.isPressed = false; clearCallout()
            if (pressed) type(option)
            done()
            return pressed && option == null
        }
        k.setOnTouchListener { v, e ->
            when (e.actionMasked) {
                // A second finger on the same key is a second press: the first types now.
                MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                    if (e.actionMasked == MotionEvent.ACTION_POINTER_DOWN && roll.up(k)) finish()
                    roll.down(k) { finish() }
                    clearCallout(); v.isPressed = true
                    down(e)
                    val shown = preview()
                    if (shown != null && ime.feedback.popups && ime.previewAllowed) feedback.present(k.face, listOf(shown))
                    val options = holds()
                    if (options.firstOrNull().isNullOrEmpty().not()) {
                        pendingAccents = Runnable { if (v.isPressed) feedback.hold(k.face, options, default()) }
                        postDelayed(pendingAccents!!, KeyboardMetrics.HOLD_MS)
                    }
                }
                MotionEvent.ACTION_MOVE -> {
                    if (feedback.choosing && v.isPressed) { val (x, y) = inHost(v, e); feedback.move(x, y) }
                    else if (e.x < -slop || e.x > v.width + slop || e.y < -slop || e.y > v.height + slop) {
                        if (v.isPressed) clearCallout()
                        v.isPressed = false
                    }
                }
                // Gboard's lighter second tick when a key types as it lifts, but not for a held option.
                MotionEvent.ACTION_UP -> if (roll.up(k) && finish()) k.typedOnLift()
                MotionEvent.ACTION_CANCEL -> if (roll.up(k)) { v.isPressed = false; clearCallout(); done() }
            }
            true
        }
    }

    /** A slide from ?123 or shift moved: the key under the finger shows pressed, with its pop-up. */
    private fun slideOver(from: View, e: MotionEvent) {
        val over = keyAt(e.rawX, e.rawY)?.takeIf { it !== from }
        if (over === slidOnto) return
        slidOnto?.isPressed = false; feedback.clear()
        slidOnto = over
        if (over == null) return
        over.isPressed = true
        val text = over.label?.text?.toString()
        if (!text.isNullOrEmpty() && ime.feedback.popups && ime.previewAllowed) feedback.present(over.face, listOf(text))
    }
    private fun endSlide() { slidOnto?.isPressed = false; slidOnto = null; feedback.clear() }
    /** The slide lifted on [key]: it types, with the lift point in the rows' coordinates for the spatial model. */
    private fun typeSlid(key: KeyCap, e: MotionEvent): Boolean {
        val type = slides[key] ?: bottomSlides[key] ?: return false
        rows.getLocationOnScreen(onScreen)
        type(KeyPoint((e.rawX - onScreen[0]).toDouble(), (e.rawY - onScreen[1]).toDouble()))
        return true
    }
    private fun keyAt(x: Float, y: Float): KeyCap? {
        for (key in slides.keys + bottomSlides.keys) {
            key.getLocationOnScreen(onScreen)
            if (x >= onScreen[0] && x < onScreen[0] + key.width && y >= onScreen[1] && y < onScreen[1] + key.height) return key
        }
        return null
    }
    /** A touch in the host's coordinates, where the pop-ups are drawn. */
    private fun inHost(v: View, e: MotionEvent): Pair<Float, Float> {
        host.getLocationInWindow(hostAt); v.getLocationInWindow(onScreen)
        return (onScreen[0] - hostAt[0] + e.x) to (onScreen[1] - hostAt[1] + e.y)
    }

    private fun shiftIconFor(page: KeyboardPage) = when { page.locked -> R.drawable.ic_caps_lock; page.shifted -> R.drawable.ic_shift_on; else -> R.drawable.ic_shift }
    private fun shiftDescription(page: KeyboardPage) = context.getString(if (page.locked) R.string.keyboard_caps_lock else R.string.keyboard_shift)
    /** Shift changed and nothing else: new labels on the same keys. */
    private fun relabel(page: KeyboardPage) {
        for ((c, key) in caseKeys) { val text = cased(c); key.label?.text = text; key.contentDescription = text }
        shiftIcon?.setImageResource(shiftIconFor(page))
        (shiftIcon?.parent?.parent as? View)?.contentDescription = shiftDescription(page)
    }

    private fun actionLabel(action: Int) = when (action) {
        EditorInfo.IME_ACTION_DONE -> R.string.keyboard_action_done
        EditorInfo.IME_ACTION_GO -> R.string.keyboard_action_go
        EditorInfo.IME_ACTION_NEXT -> R.string.keyboard_action_next
        EditorInfo.IME_ACTION_PREVIOUS -> R.string.keyboard_action_previous
        EditorInfo.IME_ACTION_SEARCH -> R.string.keyboard_action_search
        EditorInfo.IME_ACTION_SEND -> R.string.keyboard_action_send
        else -> null
    }?.let(context::getString)
    // Gboard's symbols (Pixel 8 emulator): ✓ done, → go, →| next, |← previous, a magnifier, send, else return.
    private fun actionIcon(action: Int) = when (action) {
        EditorInfo.IME_ACTION_DONE -> R.drawable.ic_action_done
        EditorInfo.IME_ACTION_GO -> R.drawable.ic_action_go
        EditorInfo.IME_ACTION_NEXT -> R.drawable.ic_action_next
        EditorInfo.IME_ACTION_PREVIOUS -> R.drawable.ic_action_previous
        EditorInfo.IME_ACTION_SEARCH -> R.drawable.ic_action_search
        EditorInfo.IME_ACTION_SEND -> R.drawable.ic_action_send
        else -> R.drawable.ic_enter
    }

    /** The letter a touch types: its own key's, or a neighbour's the word list makes likelier when the
     * touch is within TypingWord.REACH dp of that neighbour's touch area (never from a key's centre). */
    private fun likelier(c: Char, text: String, touch: KeyPoint): String {
        val near = HashMap<Char, Double>()
        for ((other, cap) in letters) {
            if (other == c) continue
            val row = cap.parent as View
            val dx = maxOf(cap.left - touch.x, 0.0, touch.x - cap.right)
            val dy = maxOf(row.top - touch.y, 0.0, touch.y - row.bottom)
            val dp = Math.hypot(dx, dy) / density
            if (dp < TypingWord.REACH) near[if (text.single().isUpperCase()) other.uppercaseChar() else other] = dp
        }
        if (near.isEmpty()) return text
        return ime.engine.likelierLetter(text.single(), near, letters.keys).toString()
    }

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
        clearCallout(); endSlide(); roll.clear()
        removeCallbacks(repeatDelete); removeCallbacks(switchLanguage)
        spaceKey.isPressed = false
        deleting = false
        deleteKey?.isPressed = false
    }
    override fun onDetachedFromWindow() { cancelGestures(); super.onDetachedFromWindow() }
}
