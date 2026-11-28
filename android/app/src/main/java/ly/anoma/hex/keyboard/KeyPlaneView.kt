package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Canvas
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
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.core.view.children
import androidx.emoji2.emojipicker.EmojiPickerView
import ly.anoma.hex.R
import ly.anoma.hex.speech.DictationLanguage
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.KeyPoint
import ly.anoma.hex.typing.KeyRect
import ly.anoma.hex.typing.TouchPlane
import ly.anoma.hex.typing.TypingWord
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** The keys: three rows from the page's layout, then ?123, comma, space, period and enter. Every
 * point of the area types a key (KeyRowLayout); the bottom row stays mounted across page changes.
 * Keys that type as they lift do so in the order the fingers went down ([Rollover]). */
internal class KeyPlaneView(context: Context, private val ime: HexKeyboard, private val host: KeyboardView,
                            private val feedback: LetterFeedback, private val size: KeySize) : LinearLayout(context) {
    private companion object {
        /** How far (key pitches) a swipe may go from where it went down and still be a tap that slid (iPhone Hex). */
        const val SLID_TAP_REACH = 1.6
        fun Int.isLight() = (Color.red(this) * 299 + Color.green(this) * 587 + Color.blue(this) * 114) / 1000 > 128
    }
    private val density = resources.displayMetrics.density
    private fun dp(value: Int) = (value * density).roundToInt()
    private fun px(value: Float) = (value * density).roundToInt()
    private val caps = KeyCaps(context, host.paints, KeyRows.labelBase(resources)) { ime.feedback.haptics }
    private val ink = caps.ink
    private val modifier = caps.modifier
    private val quiet = caps.paints.quiet
    private val landscape = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
    // Gboard's rows for this screen ([KeyRows]) at Settings › Keyboard › Height. Each key's touch target is its row's
    // full height (KeyRowLayout), so the targets, and the keycap centres the typing model and swipe read
    // (geometry()), follow the rows.
    private val rowSize = size.rows
    private val rowHeight = px(rowSize.pitch)
    private val keyHeight = px(rowSize.key)
    private val insetTop = (rowHeight - keyHeight) / 2
    private val insetBottom = rowHeight - keyHeight - insetTop
    /** One row above the bottom row, in px: its step and its keycaps', and where the keycaps sit in it. */
    private class Band(val pitch: Int, val key: Int) {
        val top = (pitch - key) / 2
        val under = pitch - key - top
    }
    private val plain = Band(rowHeight, keyHeight)
    // Labs › Keyboard › Number row: Gboard's sizes for it and for the letter rows under it ([KeyRows.withNumberRow]).
    private val numberRows = KeyRows.withNumberRow(rowSize, landscape)
    private val digitBand = Band(px(numberRows.digits.pitch), px(numberRows.digits.key))
    private val letterBand = Band(px(numberRows.letters.pitch), px(numberRows.letters.key))
    /** Whether the number row's height is above this page's keys: on the letters, and on ?123, =< and the symbol
     * pad, which have no number row (their first row is the digits) and, as Gboard's, stretch their three rows over
     * the same height, so switching pages never resizes the keyboard. A number or phone field's pad has its own. */
    private fun numberBand(page: KeyboardPage?) = page != null && page.numberRow && (page.pad == null || page.pad == Pad.Symbols)
    /** The rows above the bottom row on [page], top first. */
    private fun bands(page: KeyboardPage?): List<Band> = when {
        !numberBand(page) -> List(3) { plain }
        page!!.digits != null -> List(3) { letterBand }
        else -> {
            // ?123, =< and the symbol pad: the number row's and the three letter rows' height over three rows.
            val total = digitBand.pitch + 3 * letterBand.pitch
            List(3) { i -> val p = total / 3 + if (i < total % 3) 1 else 0; Band(p, p - (letterBand.pitch - letterBand.key)) }
        }
    }
    /** The keys above the bottom row on [page], the number row included. */
    private fun upperHeight(page: KeyboardPage?) = bands(page).sumOf { it.pitch } + if (page?.digits != null) digitBand.pitch else 0
    private val bottomExtra = px(if (landscape) KeyboardMetrics.BOTTOM_LANDSCAPE else KeyboardMetrics.BOTTOM)
    /** The space under the last row's keycaps, in px, for a row whose keycaps leave [under] below them: Gboard's
     * [bottomExtra] more, or none with Labs › Keyboard › No gap under the keys, so they end at the system's bar. */
    private fun lastUnder(under: Int) = if (size.flushBottom) 0 else under + bottomExtra
    /** The bottom row's height: its step less the keycaps' own space under them, plus [lastUnder]. */
    private val bottomHeight = rowHeight - insetBottom + lastUnder(insetBottom)
    private val reach = if (landscape) 0 else px(KeyboardMetrics.ROW1_REACH)
    private val side = KeyboardMetrics.SIDE * density
    private val gap = KeyboardMetrics.GAP * density
    /** The menu's height is the keys' height for the page, the number row included, so opening it
     * never resizes the keyboard. */
    val menuHeight get() = upperHeight(current) + bottomHeight + reach
    private val letters = LinkedHashMap<Char, KeyCap>()
    private var grid: KeyGrid? = null
    private var pendingAccents: Runnable? = null
    private val rows = LinearLayout(context)
    private val bottom = KeyRowLayout(context, 10f, side, gap, insetTop, lastUnder(insetBottom))
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
    private val emojiBottom = KeyRowLayout(context, 10f, side, gap, insetTop, lastUnder(insetBottom))
    var emojiOpen = false
        private set
    private val roll = Rollover()
    // A slide from ?123 or shift types the key it lifts on: the keys it can type, and the one under it.
    private val slides = HashMap<KeyCap, (KeyPoint) -> Unit>()
    /** TalkBack's long click on the =< page's < key, whose held marks the key offers; only that page gets it. */
    private val commaHolds = View.OnLongClickListener { host.openAccents(LetterFeedback.symbolHolds('<'), it); true }
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
    // The Labs letter-symbol setting the keys were drawn with, so a change draws them when next shown.
    private var letterSymbolsDrawn = false
    private val switchLanguage = Runnable {
        if (spaceKey.isPressed && spaceGesture.still) { spaceKey.isPressed = false; spaceKey.performLongClick() }
    }
    private val deleteGesture = DeleteGesture(KeyboardMetrics.DELETE_START * density, KeyboardMetrics.DELETE_STEP * density,
        KeyboardMetrics.DELETE_CANCEL * density, slop.toFloat())
    private var deleteSlide: DeleteSlide? = null
    // Keyboard › Swipe typing: the letter touch that may become a swipe (the latest finger down on a letter), the
    // swipe it became, and its trail over the keys.
    private var stroke: SwipeStroke? = null
    private var strokeKey: KeyCap? = null
    private var swipe: KeyboardEngine.SwipeContext? = null
    private val trail = SwipeTrail(this, (ink and 0xFFFFFF) or ((if (ink.isLight()) 0x80 else 0x66) shl 24), 6 * density)
    private val preview = SwipePreview { post(it) }
    private val repeatDelete = object : Runnable {
        override fun run() {
            if (deleting) { repeated = true; ime.engine.delete(); postDelayed(this, KeyboardMetrics.DELETE_REPEAT_EVERY) }
        }
    }

    init {
        orientation = VERTICAL
        rows.orientation = VERTICAL
        addView(rows, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        addView(bottom, LayoutParams(LayoutParams.MATCH_PARENT, bottomHeight))
        modeKey = caps.cap(context.getString(R.string.keyboard_numbers_label), context.getString(R.string.keyboard_numbers),
            modifier, 17f, ime.engine::mode)
        modeKey.textEntry = false
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
        comma = caps.cap(",", context.getString(R.string.keyboard_comma), modifier) { ime.engine.type(commaText(current)) }
        // On the =< page the key is <, which that page's rows have no room for, so it keeps the holds the <
        // key had there. Elsewhere it is the field's mark or a comma, a plain key as before: the long click
        // is attached only while that page shows (showPage).
        typesOnLift(comma, preview = { null },
            holds = { if (current?.more == true) listOf(LetterFeedback.symbolHolds('<')) else emptyList() },
            type = { option -> if (option != null) ime.engine.type(option) else comma.performClick() })
        bottomSlides[comma] = { ime.engine.type(commaText(current)) }
        // Emoji on the letters; on the symbol pages Gboard's 1234 key, which opens its number pad.
        emojiKey = caps.icon(R.drawable.ic_emoji, context.getString(R.string.keyboard_emoji), caps.key,
            { if (current?.symbols == true) ime.engine.pad(Pad.Symbols) else host.openEmoji() })
        emojiKey.face.addView(TextView(context).apply {
            text = context.getString(R.string.keyboard_pad_label); setTextSize(android.util.TypedValue.COMPLEX_UNIT_DIP, 13f); useInter(500); setTextColor(ink)
            gravity = Gravity.CENTER; includeFontPadding = false; setLineSpacing(0f, 0.9f); visibility = GONE
        }, FrameLayout.LayoutParams(-1, -1))
        emojiKey.textEntry = false
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
        period = caps.cap(".", context.getString(R.string.keyboard_period), modifier) { ime.engine.type(periodText(current)) }
        // Held on the letters page, period offers Gboard's punctuation; on ?123 an ellipsis, and on =<
        // what its > holds.
        period.setOnLongClickListener {
            host.openAccents(if (current?.symbols == false) LetterFeedback.periodHolds.flatten()
                else LetterFeedback.symbolHolds(periodText(current)[0]), it); true
        }
        period.longClickLabel = context.getString(R.string.keyboard_more_characters)
        typesOnLift(period, preview = { null },
            holds = { if (current?.symbols == false) LetterFeedback.periodHolds else listOf(LetterFeedback.symbolHolds(periodText(current)[0])) },
            default = { if (current?.symbols == false) LetterFeedback.PERIOD_DEFAULT else 0 },
            type = { option -> if (option != null) ime.engine.type(option) else period.performClick() })
        bottomSlides[period] = { ime.engine.type(periodText(current)) }
        enterKey = caps.cap("", context.getString(R.string.keyboard_return), modifier, 17f, ime.engine::enter)
        enterKey.sound = AudioManager.FX_KEYPRESS_RETURN
        enterKey.textEntry = false
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
        letters.textEntry = false
        typesOnLift(letters, preview = { null }, holds = { emptyList() }, type = { letters.performClick() })
        val emojiSpace = caps.cap("", context.getString(R.string.keyboard_space), caps.key, 14f) { ime.engine.space() }
        emojiSpace.sound = AudioManager.FX_KEYPRESS_SPACEBAR
        typesOnLift(emojiSpace, preview = { null }, holds = { emptyList() }, type = { emojiSpace.performClick() })
        emojiBottom.add(letters, 0f, KeyboardMetrics.MODE)
        emojiBottom.add(emojiSpace, KeyboardMetrics.MODE, 10f - 2 * KeyboardMetrics.MODE)
        emojiBottom.add(newDeleteKey(), 10f - KeyboardMetrics.MODE, KeyboardMetrics.MODE)
        emojiBottom.visibility = GONE
        addView(emojiBottom, LayoutParams(LayoutParams.MATCH_PARENT, bottomHeight))
    }

    private fun cased(c: Char) = if (current?.let { !it.symbols && it.shifted } == true) c.uppercaseChar().toString() else c.toString()

    /** The bottom row's comma and period keys type the page's own ([KeyboardPage.commaText]), which is the
     * field's mark and . on the letters, , and . on ?123, and =<'s < and >. */
    private fun commaText(page: KeyboardPage?) = page?.commaText ?: ","

    private fun periodText(page: KeyboardPage?) = page?.periodText ?: "."

    /** Types a page key's own character at [p] (its touch, when the page has one): a quote key asks the
     * engine for its smart form (Labs › Keyboard › Smart quotes), and every other key types as before.
     * A long press's picked alternate goes through [KeyboardEngine.type] instead, so it inserts exactly
     * the character it shows. */
    private fun typeFromKey(text: String, g: KeyGrid?, p: KeyPoint?, aimed: Boolean = true) {
        if (text.length == 1 && (text[0] == '\'' || text[0] == '"')) ime.engine.quote(text[0])
        else if (g != null && p != null) ime.engine.letter(text, p.x, p.y, g, aimed = aimed)
        else ime.engine.type(text)
    }
    // A swipe that came back as a tap is typing its letter: where it went down says nothing of your aim.
    private var slidTap = false

    fun showPage(page: KeyboardPage) {
        val shown = current
        val letterSymbols = ime.feedback.letterSymbols
        // Shift only recases the letters and changes nothing on the symbol pages: neither draws the keys again,
        // which would drop a key a second finger is already on (typing the first of a sentence turns shift off).
        // A changed Labs letter-symbol setting draws them again, so the next showing picks it up.
        if (shown != null && page.copy(shifted = shown.shifted, locked = shown.locked) == shown &&
            letterSymbols == letterSymbolsDrawn) {
            current = page; if (!page.symbols) relabel(page); return
        }
        // Another kind of field in the same box (the app changed its type): the keys, not the emoji panel over them.
        if (emojiOpen) host.closeEmoji()
        current = page; letterSymbolsDrawn = letterSymbols
        clearCallout(); endSlide(); cancelSwipe(); grid = null; letters.clear(); caseKeys.clear(); slides.clear(); shiftIcon = null
        rows.removeAllViews()
        page.pad?.let { showPad(page, it); return }
        bottom.visibility = VISIBLE
        val language = page.language
        val symbols = page.symbols
        val currency = page.currency
        val letterHolds = !symbols && letterSymbols
        val lines = page.lines
        val columns = KeyboardMetrics.columns(lines).toFloat()
        val side = KeyboardMetrics.sideWidth(page, lines)
        // Labs › Keyboard › Number row: the digits above the letters, in a row of their own.
        page.digits?.let { addDigitRow(it, columns) }
        val bands = bands(page)
        for ((index, line) in lines.withIndex()) {
            val band = bands[index]
            val top = if (index == 0 && page.digits == null) reach else 0
            val row = KeyRowLayout(context, columns, this.side, gap, band.top + top, band.under)
            rows.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, band.pitch + top))
            // Rows 1 and 2 centre their letters; row 3's share what shift and delete leave.
            var at = if (index < 2) (columns - line.length) / 2f else 0f
            val width = if (index < 2) 1f else (columns - 2 * side) / line.length
            if (index == 2) {
                // Gboard labels the page switch =< going out and ?123 coming back.
                val switch = if (symbols) caps.cap(context.getString(if (page.more) R.string.keyboard_numbers_label else R.string.keyboard_page_symbols),
                    context.getString(R.string.keyboard_more_symbols), modifier, 15f, ime.engine::shiftUp).also { it.textEntry = false }
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
                    typeFromKey(text, g, p, aimed = touch != null && !slidTap)
                }
                row.add(k, at, width); at += width
                if (!symbols) { letters[c] = k; caseKeys.add(c to k) }
                val digit = if (symbols) null else page.hint(index, position)
                // Gboard's top-row set is only a fallback for a row-1 letter with no home- or bottom-row
                // symbol of its own (AZERTY a and z keep @ and *); rows 2 and 3 always keep Gboard's,
                // wherever a layout draws their letters. A local currency takes $'s place on d.
                val symbol = if (letterHolds) LetterFeedback.letterSymbol(c, topRow = index == 0,
                    currency = currency) else null
                k.hint(digit?.toString() ?: symbol?.toString())
                fun holds() = if (symbols) LetterFeedback.symbolHolds(c, currency)
                    else LetterFeedback.holds(cased(c).single(), language.layoutCode, digit, letterHolds,
                        topRow = index == 0, currency = currency)
                if (holds().isNotEmpty()) {
                    k.setOnLongClickListener { host.openAccents(holds(), it); true }
                    k.longClickLabel = context.getString(R.string.keyboard_more_characters)
                }
                slides[k] = { p ->
                    val g = if (symbols) null else geometry()
                    typeFromKey(cased(c), g, p, aimed = false)
                }
                typesOnLift(k, preview = { cased(c) }, holds = { listOf(holds()) }, swipes = !symbols,
                    down = { e ->
                        downText = cased(c)
                        touch = KeyPoint((k.left + e.getX(e.actionIndex)).toDouble(), (row.top + e.getY(e.actionIndex)).toDouble())
                    },
                    type = { option ->
                        val text = downText ?: cased(c)
                        // A held alternate inserts exactly what it shows, even when it is the key's own
                        // character (the French apostrophe's straight form): it never takes the
                        // smart-quote path a plain press does.
                        if (option != null && (option != text || text == "'" || text == "\"")) {
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
        val commaLabel = page.commaText
        comma.label?.text = commaLabel
        comma.labelSize = if (commaLabel == "@") 21f else 26f
        comma.longClickLabel = if (page.more) context.getString(R.string.keyboard_more_characters) else null
        comma.setOnLongClickListener(if (page.more) commaHolds else null)
        comma.isLongClickable = page.more
        comma.contentDescription = when (commaLabel) {
            "@" -> context.getString(R.string.keyboard_at)
            "/" -> context.getString(R.string.keyboard_slash)
            "," -> context.getString(R.string.keyboard_comma)
            else -> commaLabel
        }
        val periodLabel = page.periodText
        period.label?.text = periodLabel
        period.contentDescription = if (periodLabel == ".") context.getString(R.string.keyboard_period) else periodLabel
        // An app's own label, else Gboard's symbol for the editor's action.
        val label = page.enter.label
        enterKey.label?.text = label.orEmpty(); enterKey.requestLayout()
        enterIcon.setImageResource(actionIcon(page.enter.action))
        enterIcon.visibility = if (label == null) View.VISIBLE else View.GONE
        actionColors(enterKey, page)
        enterKey.contentDescription = label ?: actionLabel(page.enter.action) ?: context.getString(R.string.keyboard_return)
    }

    /** The number row: 1 to 0 across the letters' cells, with the ?123 page's digit keys' feedback,
     * pop-ups and held symbols. Shift neither changes them nor is changed by them. */
    private fun addDigitRow(digits: String, columns: Float) {
        val row = KeyRowLayout(context, columns, side, gap, digitBand.top + reach, digitBand.under)
        rows.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, digitBand.pitch + reach))
        val width = columns / digits.length
        for ((position, c) in digits.withIndex()) {
            val text = c.toString()
            val k = caps.cap(text, text) { ime.engine.type(text) }
            row.add(k, position * width, width)
            fun holds() = LetterFeedback.symbolHolds(c)
            if (holds().isNotEmpty()) {
                k.setOnLongClickListener { host.openAccents(holds(), it); true }
                k.longClickLabel = context.getString(R.string.keyboard_more_characters)
            }
            typesOnLift(k, preview = { text }, holds = { listOf(holds()) },
                type = { option -> if (option != null && option != text) ime.engine.type(option) else k.performClick() })
        }
    }

    /** A pad in place of the letter rows and the bottom row (Gboard's for number, phone and date fields,
     * and the number pad of its symbol page): its keys 53.3 dp tall on the letters' rows. */
    private fun showPad(page: KeyboardPage, pad: Pad) {
        bottom.visibility = GONE
        val cells = KeyPads.cells(pad)
        // With the number row on, the symbol pad's three rows take the number row's height too ([bands]).
        val steps = bands(page).map { it.pitch }
        val step = steps[0]
        val padKey = maxOf(keyHeight, step - px(KeyboardMetrics.PAD_ROW_GAP))
        val top = (step - padKey) / 2; val under = step - padKey - top
        val lines = KeyPads.rows(pad)
        // The symbol pad's operators stand in one column beside its first three rows.
        val block = if (pad == Pad.Symbols) PadBlock(cells, reach + top, under).also {
            rows.addView(it, LayoutParams(LayoutParams.MATCH_PARENT, upperHeight(page) + reach))
        } else null
        for ((index, line) in lines.withIndex()) {
            val first = index == 0; val last = index == lines.lastIndex
            // The symbol pad ends on the letters' bottom row, as Gboard's does.
            val t = if (last && pad == Pad.Symbols) insetTop else top
            val b = if (last && pad == Pad.Symbols) insetBottom else under
            val row = KeyRowLayout(context, cells, side, gap, t + (if (first) reach else 0), if (last) lastUnder(b) else b)
            // The last row is as tall as the letters' bottom row ([bottomHeight]), so the pad, the letters and the
            // Hex menu ([menuHeight]) are one height. With No gap under the keys a pad whose keycaps leave less than
            // the letters' space under them gets keycaps that much shorter, ending at the system's bar.
            val height = (if (last) bottomHeight else steps.getOrElse(index) { step }) + (if (first) reach else 0)
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
                    fill, ime.engine::enter)).also { it.sound = AudioManager.FX_KEYPRESS_RETURN; actionColors(it, page, fill) }
            else -> caps.cap(label, padDescription(key, label), fill, when { key.word != 0 -> 17f; label.length > 1 -> 21f; else -> 26f }) { act(key.action) }
        }
        // A phone digit's letters beside it, small and quiet, as Gboard's.
        key.letters?.let { letters ->
            k.label?.text = SpannableString("$label  $letters").apply {
                setSpan(RelativeSizeSpan(0.46f), label.length, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                setSpan(ForegroundColorSpan(quiet), label.length, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                setSpan(Centred, label.length, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            }
        }
        k.textEntry = key.action is PadAction.Text || key.action == PadAction.Space
        key.hold?.let { held ->
            k.setOnLongClickListener { ime.engine.type(held); true }
            k.longClickLabel = context.getString(R.string.keyboard_type_held, held)
        }
        typesOnLift(k, preview = { null }, holds = { key.hold?.let { listOf(listOf(it)) } ?: emptyList() },
            type = { option -> if (option != null) ime.engine.type(option) else k.performClick() })
        return k
    }
    // What TalkBack calls a pad's switches; other keys are their own label.
    private fun padDescription(key: PadKey, label: String) = when (val action = key.action) {
        is PadAction.Go -> context.getString(if (action.pad == Pad.PhoneSymbols) R.string.keyboard_phone_symbols else R.string.keyboard_phone_keypad)
        PadAction.Letters -> context.getString(R.string.keyboard_letters)
        PadAction.SymbolsPage -> context.getString(R.string.keyboard_symbols)
        else -> label
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
     * thirds turned). One picker for the keys' life, hidden while closed: a new one per opening was never
     * collected (its emoji are drawn into bitmaps, about 5 MB a panel). Each opening hands it the recents
     * again, which redraws its Recently used row, and starts it at the top, as a new picker did. */
    fun openEmoji(extra: Int) {
        cancelGestures()
        val keyboard = height + extra // the keys' and the strip's height as they are
        // Turned, the screen is short: the panel may take up to two thirds of it, with smaller cells.
        val share = if (landscape) 0.67f else 0.6f
        val grow = minOf(px(KeyboardMetrics.EMOJI_GROW), (resources.displayMetrics.heightPixels * share).toInt() - keyboard).coerceAtLeast(0)
        // With the number row on, the panel takes its place too, so the keyboard keeps its height.
        val height = upperHeight(current) + reach + extra + grow
        val p = picker?.also { it.setRecentEmojiProvider(recents); it.post { it.toTop() } } ?: EmojiPickerView(ContextThemeWrapper(context, R.style.HexEmoji)).also { p ->
            p.setRecentEmojiProvider(recents)
            p.setOnEmojiPickedListener { item ->
                // Gboard's tap as an emoji is chosen; the picker's own view plays the click.
                tap(context, ime.feedback.haptics, Tap.Press)
                ime.engine.type(item.emoji)
            }
            picker = p; addView(p, 0, LayoutParams(LayoutParams.MATCH_PARENT, height))
        }
        if (pickerSized != height) {
            // Square cells about Gboard's size: nine across in portrait, more when turned. The picker sizes its rows
            // from what its grid has left after two category titles.
            val cell = if (landscape) KeyboardMetrics.EMOJI_CELL_LANDSCAPE else KeyboardMetrics.EMOJI_CELL
            val columns = maxOf(9, (resources.displayMetrics.widthPixels / (cell * density)).toInt())
            p.emojiGridColumns = columns
            p.emojiGridRows = ((height - (KeyboardMetrics.EMOJI_HEADER + 2 * KeyboardMetrics.EMOJI_TITLE) * density) /
                (resources.displayMetrics.widthPixels.toFloat() / columns)).coerceAtLeast(1f)
            p.layoutParams.height = height; p.requestLayout(); pickerSized = height
        }
        rows.visibility = GONE; bottom.visibility = GONE
        p.visibility = VISIBLE; emojiBottom.visibility = VISIBLE
        emojiOpen = true
    }
    fun closeEmoji() {
        if (!emojiOpen) return
        cancelGestures()
        picker?.visibility = GONE; emojiBottom.visibility = GONE
        rows.visibility = VISIBLE; bottom.visibility = VISIBLE
        emojiOpen = false
    }
    private val recents by lazy { RecentEmoji(context) { ime.engine.learnsTyping } }
    private var pickerSized = 0
    /** The picker back at Recently used: its tab for them (the icon's clickable item), tapped. */
    private fun EmojiPickerView.toTop() {
        val name = context.getString(R.string.emoji_category_recent)
        fun tab(v: View): View? = if (v.contentDescription == name) v else (v as? ViewGroup)?.children?.firstNotNullOfOrNull(::tab)
        var item = tab(this)
        while (item != null && !item.isClickable) item = item.parent as? View
        item?.performClick()
    }

    /** Touches for a key that types as it lifts: pressed, its pop-up, after [KeyboardMetrics.HOLD_MS] its held
     * options to slide over, and on lifting (or when another key goes down first) [type] with the option
     * chosen, or null for the key's own. */
    private fun typesOnLift(k: KeyCap, preview: () -> String?, holds: () -> List<List<String>>, default: () -> Int = { 0 },
                            swipes: Boolean = false, down: (MotionEvent) -> Unit = {}, type: (String?) -> Unit, done: () -> Unit = {}) {
        fun finish(): Boolean {
            // The key types: its touch is no swipe any more.
            if (strokeKey === k && swipe == null) forgetStroke()
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
                    // A second finger on a swiping key: the swipe's word goes in first.
                    if (e.actionMasked == MotionEvent.ACTION_POINTER_DOWN && strokeKey === k && swipe != null) { roll.up(k); finishSwipe() }
                    if (e.actionMasked == MotionEvent.ACTION_POINTER_DOWN && roll.up(k)) finish()
                    roll.down(k) { finish() }
                    if (swipes && e.actionMasked == MotionEvent.ACTION_DOWN) strokeBegan(k, e)
                    clearCallout(); v.isPressed = true
                    down(e)
                    val shown = preview()
                    if (shown != null && ime.feedback.popups && ime.previewAllowed) feedback.present(k.face, listOf(shown))
                    val options = holds()
                    if (options.firstOrNull().isNullOrEmpty().not()) {
                        pendingAccents = Runnable {
                            if (!v.isPressed) return@Runnable
                            feedback.hold(k.face, options, default())
                            // Labs › Keyboard › Click when a hold opens: the press's click, felt as the options appear.
                            if (ime.feedback.holdClick && ime.feedback.haptics) tap(context, true, Tap.Press)
                        }
                        postDelayed(pendingAccents!!, KeyboardMetrics.HOLD_MS)
                    }
                }
                MotionEvent.ACTION_MOVE -> {
                    if (swipes && strokeMoved(k, e)) return@setOnTouchListener true
                    if (feedback.choosing && v.isPressed) { val (x, y) = inHost(v, e); feedback.move(x, y) }
                    else if (e.x < -slop || e.x > v.width + slop || e.y < -slop || e.y > v.height + slop) {
                        if (v.isPressed) clearCallout()
                        v.isPressed = false
                    }
                }
                // Gboard's lighter second tick when a key types as it lifts, but not for a held option.
                MotionEvent.ACTION_UP -> when {
                    swipes && strokeKey === k && swipe != null -> { roll.up(k); strokeMoved(k, e); finishSwipe(); done() }
                    else -> { if (strokeKey === k) forgetStroke(); if (roll.up(k) && finish()) k.typedOnLift() }
                }
                MotionEvent.ACTION_CANCEL -> {
                    if (strokeKey === k) cancelSwipe()
                    if (roll.up(k)) { v.isPressed = false; clearCallout(); done() }
                }
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

    // MARK: Swipe typing

    /** A finger went down on a letter key: it may become a swipe. */
    private fun strokeBegan(k: KeyCap, e: MotionEvent) {
        if (swipe != null) finishSwipe()
        // A stroke that can't be read is followed too, only so the strip can say why once it travels as a swipe would.
        if (!(ime.engine.swipes || ime.engine.swipeUnreadable) || geometry() == null) { forgetStroke(); return }
        val row = k.parent as View
        stroke = SwipeStroke(e.getPointerId(e.actionIndex)).also { it.add(k.left + e.getX(e.actionIndex).toDouble(), row.top + e.getY(e.actionIndex).toDouble(), e.eventTime / 1000.0) }
        strokeKey = k
    }

    /** The finger moved: true when the touch is a swipe (now or already), which the key leaves to it. A touch becomes
     * one once it travels a key's pitch from where it went down, unless its accents already show. */
    private fun strokeMoved(k: KeyCap, e: MotionEvent): Boolean {
        val st = stroke ?: return false
        if (strokeKey !== k) return false
        val index = e.findPointerIndex(st.pointer).takeIf { it >= 0 } ?: return false
        val row = k.parent as View
        val dx = k.left.toDouble(); val dy = row.top.toDouble()
        val active = swipe != null
        for (h in 0 until e.historySize) {
            val x = dx + e.getHistoricalX(index, h); val y = dy + e.getHistoricalY(index, h)
            st.add(x, y, e.getHistoricalEventTime(h) / 1000.0)
            if (active) trail.add(x.toFloat(), (y + rows.top).toFloat())
        }
        val x = dx + e.getX(index); val y = dy + e.getY(index)
        if (e.actionMasked != MotionEvent.ACTION_UP || st.xs[st.count - 1] != x || st.ys[st.count - 1] != y) {
            st.add(x, y, e.eventTime / 1000.0)
            if (active) trail.add(x.toFloat(), (y + rows.top).toFloat())
        }
        val grid = geometry() ?: return active
        if (!active) {
            if (st.travelled() <= grid.pitchX || feedback.choosing) return false
            val context = ime.engine.beginSwipe() ?: run { forgetStroke(); return false }
            swipe = context
            // The key no longer types: no pop-up, no accents, and another key going down ends the swipe first.
            k.isPressed = false; clearCallout()
            roll.down(k) { finishSwipe() }
            trail.begin(List(st.count) { floatArrayOf(st.xs[it].toFloat(), (st.ys[it] + rows.top).toFloat()) })
            preview.next()
        }
        swipe?.let { s -> preview.schedule(st, s.reader, grid, s.previous) { words -> if (swipe === s) ime.engine.swipePreview(words) } }
        return true
    }

    /** The swipe's finger lifted, or another key went down: its word goes in, or a tap that slid types its letter. */
    private fun finishSwipe() {
        val st = stroke; val s = swipe; val key = strokeKey
        forgetStroke(); preview.next(); trail.fade()
        if (st == null || s == null) return
        val grid = geometry() ?: run { ime.engine.cancelSwipe(); return }
        val readings = s.reader.ranked(s.reader.decode(st.xs, st.ys, st.ts, st.count, grid, s.previous, 8).candidates)
        val letter = key?.let { slidTap(st, it, grid) }
        if (key != null && letter != null && readings.firstOrNull()?.let { startsAndEnds(it, letter) } != true) {
            // A tap that slid ("a" dragged a little too far and back): its letter goes in as a tap's would.
            ime.engine.cancelSwipe()
            slidTap = true; key.performClick(); slidTap = false
            return
        }
        ime.engine.endSwipe(readings)
    }

    /** The system took the touch away: nothing goes in. */
    private fun cancelSwipe() {
        val active = swipe != null
        forgetStroke(); preview.next()
        if (active) { trail.clear(); ime.engine.cancelSwipe() }
    }
    private fun forgetStroke() { stroke = null; strokeKey = null; swipe = null }

    /** The letter of a swipe that lifted on the key it went down on without going more than 1.6 pitches from where it
     * started (iPhone Hex's slidTapLetter), or null. */
    private fun slidTap(st: SwipeStroke, key: KeyCap, grid: KeyGrid): Char? {
        if (!st.within(SLID_TAP_REACH * grid.pitchX)) return null
        val row = key.parent as View
        val x = st.xs[st.count - 1]; val y = st.ys[st.count - 1]
        if (x < key.left || x >= key.right || y < row.top || y >= row.bottom) return null
        return letters.entries.firstOrNull { it.value === key }?.key
    }
    /** Whether [word] begins and ends on [letter]'s key ("lol" on l): a word a swipe that comes back to its key can mean. */
    private fun startsAndEnds(word: String, letter: Char): Boolean {
        fun base(c: Char) = java.text.Normalizer.normalize(c.lowercaseChar().toString(), java.text.Normalizer.Form.NFD).first()
        return word.length > 1 && base(word.first()) == letter && base(word.last()) == letter
    }

    override fun dispatchDraw(canvas: Canvas) {
        super.dispatchDraw(canvas)
        trail.draw(canvas)
    }

    private fun shiftIconFor(page: KeyboardPage) = when { page.locked -> R.drawable.ic_caps_lock; page.shifted -> R.drawable.ic_shift_on; else -> R.drawable.ic_shift }
    // As Gboard's: "Shift", "Shift enabled", "Caps lock enabled".
    private fun shiftDescription(page: KeyboardPage) = context.getString(when {
        page.locked -> R.string.keyboard_caps_lock_on; page.shifted -> R.string.keyboard_shift_on; else -> R.string.keyboard_shift
    })
    /** Shift changed and nothing else: new labels on the same keys. */
    private fun relabel(page: KeyboardPage) {
        for ((c, key) in caseKeys) { val text = cased(c); key.label?.text = text; key.contentDescription = text }
        shiftIcon?.setImageResource(shiftIconFor(page))
        (shiftIcon?.parent?.parent as? View)?.contentDescription = shiftDescription(page)
    }

    /** Labs › Key colors: in a Send, Go, Search or Done field a theme's enter key takes its action colour under a
     * white symbol or label, as iPhone Hex's; elsewhere, and always with Automatic, it is a special key. */
    private fun actionColors(enter: KeyCap, page: KeyboardPage, fill: Int = modifier) {
        if (caps.paints.color == KeyColor.Automatic) return
        val action = caps.paints.action(page.enter.action)
        enter.fill(action ?: fill, if (action != null) caps.paints.actionPressed else caps.paints.pressed)
        val tint = if (action != null) Color.WHITE else ink
        for (v in enter.face.children) when (v) {
            is ImageView -> v.imageTintList = ColorStateList.valueOf(tint)
            is TextView -> v.setTextColor(tint)
        }
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
        // The pitch from the layout's own first two keys and the row under them: on AZERTY, q, w and a sit on
        // other rows (160 x -155 px instead of 107 x 155). The apostrophe is no letter for the touch model.
        val rows = KeyboardLetters.rows((current ?: return null).language.layoutCode)
        val first = letters[rows[0][0]] ?: return null; val next = letters[rows[0][1]] ?: return null; val below = letters[rows[1][0]] ?: return null
        if (first.width == 0 || next.left == first.left) return null
        val centres = letters.filterKeys { it.isLetter() }.mapValues { (_, cap) -> capCentre(cap) }
        val pitchX = capCentre(next).x - capCentre(first).x; val pitchY = capCentre(below).y - capCentre(first).y
        return KeyGrid(centres, pitchX, pitchY, spaceRect(), plane(rows, pitchX, pitchY)).also { grid = it }
    }
    /** The letter keys as Labs › Keyboard › Adaptive touch learns them: each keycap, its touch cell and its row. A key
     * plane is the keyboard's width in 10 dp steps, its orientation and the number row, so a phone, an unfolded
     * foldable, split screen and landscape each learn their own. */
    private fun plane(lines: List<String>, pitchX: Double, pitchY: Double): TouchPlane {
        val keys = letters.entries.filter { it.key.isLetter() }
        val caps = keys.map { (_, cap) ->
            val top = (cap.parent as View).top
            KeyRect((cap.left + cap.paddingLeft).toDouble(), (top + cap.top + cap.paddingTop).toDouble(),
                (cap.right - cap.paddingRight).toDouble(), (top + cap.bottom - cap.paddingBottom).toDouble())
        }
        val cells = keys.map { (_, cap) ->
            val top = (cap.parent as View).top
            KeyRect(cap.left.toDouble(), (top + cap.top).toDouble(), cap.right.toDouble(), (top + cap.bottom).toDouble())
        }
        val signature = "${(width / density / 10).roundToInt()}|${if (landscape) "landscape" else "portrait"}|" +
            if (current?.digits != null) "digits" else ""
        return TouchPlane(signature, keys.map { it.key }.toCharArray(), caps.toTypedArray(), cells.toTypedArray(),
            keys.map { (c, _) -> lines.indexOfFirst { c in it } }.toIntArray(), pitchX, pitchY)
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
        // Translation is always from Spanish, also while Automatic listens for it among other languages.
        val label = if (next.translate) context.getString(R.string.keyboard_space_translate, DictationLanguage.Spanish.label)
            else next.spoken.label
        spaceKey.contentDescription = context.getString(R.string.keyboard_space)
        spaceKey.longClickLabel = context.getString(R.string.keyboard_space_language_action, label)
        spaceKey.label?.text = label
        spaceKey.labelSize = if (next.translate) 12f else 14f
        spaceKey.requestLayout() // A longer language name is fitted to the bar again.
    }
    fun cancelGestures() {
        cancelSwipe()
        clearCallout(); endSlide(); roll.clear()
        removeCallbacks(repeatDelete); removeCallbacks(switchLanguage)
        spaceKey.isPressed = false
        deleting = false
        deleteKey?.isPressed = false
    }
    override fun onDetachedFromWindow() { cancelGestures(); super.onDetachedFromWindow() }
}
