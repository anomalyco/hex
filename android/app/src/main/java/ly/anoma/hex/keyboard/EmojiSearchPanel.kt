package ly.anoma.hex.keyboard

import android.content.Context
import android.content.res.AssetManager
import android.content.res.ColorStateList
import android.graphics.Paint
import android.graphics.drawable.GradientDrawable
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.GridLayout
import android.widget.HorizontalScrollView
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import java.io.IOException
import java.util.concurrent.Executor
import ly.anoma.hex.R
import ly.anoma.hex.typing.EmojiSearch
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** Labs › Keyboard › Emoji search's query while its field is open: the keys type here ([KeyboardEngine.emojiQuery]),
 * never into the app's field. Return ends the search, as on iPhone Hex. Nothing typed here is kept. */
internal class EmojiQuery(private val changed: (String) -> Unit, private val done: () -> Unit) {
    var text = ""
        private set

    fun type(value: String) {
        if ('\n' in value) { done(); return }
        text = (text + value).take(MAX); changed(text)
    }

    /** Delete takes back the last character as people see it. */
    fun delete() {
        if (text.isEmpty()) return
        text = text.dropLast(DeleteSlide.lastCharacter(text)); changed(text)
    }

    fun enter() = done()

    private companion object { const val MAX = 40 }
}

/** The bundled emoji search index, read once and lazily on [background] the first time a search opens; a failed read
 * behaves as an empty index and is never retried. */
internal class EmojiSearchIndex(private val assets: AssetManager, private val background: Executor) {
    @Volatile var index: EmojiSearch? = null
        private set
    @Volatile private var requested = false

    fun load(ready: () -> Unit) {
        synchronized(this) { if (requested) return; requested = true }
        background.execute {
            index = try {
                // Only what this platform can draw, as EmojiWords: minSdk 31 stops at Emoji 13.1.
                val paint = Paint()
                assets.open(EmojiSearch.ASSET).bufferedReader(Charsets.UTF_8).use {
                    EmojiSearch.parse(it.lineSequence()) { emoji -> paint.hasGlyph(emoji) }
                }
            } catch (_: IOException) {
                EmojiSearch.parse(emptySequence())
            }
            ready()
        }
    }
}

/** What an empty search offers: your recent emoji the phone can draw, then the everyday ones, then the index's first
 * (the smileys), up to [count], none twice. */
internal fun emojiStarters(recent: List<String>, index: EmojiSearch, count: Int): List<String> {
    val out = LinkedHashSet<String>()
    for (e in recent + EmojiSearch.EVERYDAY) if (e in index) out.add(e)
    for (e in index.first(count + out.size)) { if (out.size >= count) break; out.add(e) }
    return out.take(count)
}

/** The search's boost, as iPhone Hex's: your recent emoji, the newest 1 fading to 0 down the list (the picker keeps
 * no counts), or 0.5 for an everyday one, whichever is more. */
internal fun searchBoost(recent: List<String>): (String) -> Float {
    val at = recent.withIndex().associate { (i, e) -> e to 1f - i.toFloat() / maxOf(1, recent.size) }
    val everyday = EmojiSearch.EVERYDAY.toSet()
    return { maxOf(at[it] ?: 0f, if (it in everyday) 0.5f else 0f) }
}

/** The emoji panel's search: the results in two rows above the field, where the panel's extra height is, and the field
 * in the strip's place, with the keys below it typing into it. [pick] inserts an emoji into the app's field; [close]
 * ends the search, back to the panel. */
internal class EmojiSearchView(context: Context, ink: Int, quiet: Int, fill: Int,
                               private val pick: (String) -> Unit, close: () -> Unit) : LinearLayout(context) {
    private val density = resources.displayMetrics.density
    private fun dp(value: Float) = (value * density).roundToInt()
    private val ink = ink
    private val quiet = quiet
    private val results = GridLayout(context).apply { orientation = GridLayout.VERTICAL }
    private val scroller = HorizontalScrollView(context).apply {
        isHorizontalScrollBarEnabled = false; overScrollMode = OVER_SCROLL_NEVER; addView(results)
    }
    private val empty = TextView(context).apply {
        text = context.getString(R.string.emoji_search_none); setTextSize(TypedValue.COMPLEX_UNIT_DIP, 15f); useInter(450)
        setTextColor(quiet); gravity = Gravity.CENTER; visibility = GONE
    }
    private val query = TextView(context).apply {
        setTextSize(TypedValue.COMPLEX_UNIT_DIP, 16f); useInter(450); setTextColor(ink); isSingleLine = true; includeFontPadding = false
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }
    // The hint after the caret while nothing is typed, as a field's; not the query's own hint, which would keep the
    // query as wide as the hint and the caret away from the text.
    private val hint = TextView(context).apply {
        text = context.getString(R.string.emoji_search_hint); setTextSize(TypedValue.COMPLEX_UNIT_DIP, 16f); useInter(450)
        setTextColor(quiet); isSingleLine = true; includeFontPadding = false; importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }
    private val caret = View(context).apply { setBackgroundColor(ink) }
    private val field = LinearLayout(context)
    private var resultsHeight = 0

    init {
        orientation = VERTICAL
        val area = FrameLayout(context)
        area.addView(scroller, FrameLayout.LayoutParams(-1, -1))
        area.addView(empty, FrameLayout.LayoutParams(-1, -1))
        addView(area, LayoutParams(-1, 0, 1f))
        field.apply {
            gravity = Gravity.CENTER_VERTICAL
            background = GradientDrawable().apply { cornerRadius = dp(20f).toFloat(); setColor(fill) }
            setPadding(dp(12f), 0, dp(4f), 0)
            addView(ImageView(context).apply {
                setImageResource(R.drawable.ic_action_search); imageTintList = ColorStateList.valueOf(quiet)
                importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            }, LayoutParams(dp(20f), dp(20f)))
            addView(query, LayoutParams(-2, -2).apply { leftMargin = dp(8f) })
            addView(caret, LayoutParams(dp(1.5f), dp(18f)).apply { leftMargin = dp(1f) })
            addView(hint, LayoutParams(-2, -2).apply { leftMargin = dp(1f) })
            addView(View(context), LayoutParams(0, 1, 1f))
            addView(FrameLayout(context).apply {
                contentDescription = context.getString(R.string.emoji_search_close); isFocusable = true
                setOnClickListener { close() }
                addView(ImageView(context).apply { setImageResource(R.drawable.ic_cancel); imageTintList = ColorStateList.valueOf(quiet) },
                    FrameLayout.LayoutParams(dp(18f), dp(18f), Gravity.CENTER))
            }, LayoutParams(dp(40f), dp(40f)))
        }
        // The field reads as one line to TalkBack: what is searched, or the hint.
        field.isFocusable = true
        field.contentDescription = context.getString(R.string.emoji_search_hint)
    }

    /** The results' height (the panel's extra height) and the field's band (the strip's height). */
    fun size(results: Int, strip: Int) {
        resultsHeight = results
        if (field.parent == null) addView(field, LayoutParams(-1, dp(40f)).apply {
            leftMargin = dp(8f); rightMargin = dp(8f); topMargin = (strip - dp(40f)) / 2; bottomMargin = strip - dp(40f) - (strip - dp(40f)) / 2
        }) else (field.layoutParams as LayoutParams).apply { topMargin = (strip - dp(40f)) / 2; bottomMargin = strip - dp(40f) - topMargin }
        layoutParams = (layoutParams ?: LayoutParams(-1, 0)).also { it.height = results + strip }
    }

    fun show(text: String, emoji: List<String>, name: (String) -> String?) {
        query.text = text
        hint.visibility = if (text.isEmpty()) VISIBLE else GONE
        field.contentDescription = text.ifEmpty { context.getString(R.string.emoji_search_hint) }
        val rows = if (resultsHeight >= dp(2 * KeyboardMetrics.EMOJI_CELL)) 2 else 1
        val cell = dp(KeyboardMetrics.EMOJI_CELL)
        results.removeAllViews(); results.rowCount = rows
        for (e in emoji) results.addView(TextView(context).apply {
            this.text = e; gravity = Gravity.CENTER; setTextSize(TypedValue.COMPLEX_UNIT_DIP, 28f); includeFontPadding = false
            setTextColor(ink); contentDescription = name(e) ?: e; isFocusable = true
            background = GradientDrawable().apply { cornerRadius = dp(8f).toFloat() }.let { shape ->
                android.graphics.drawable.StateListDrawable().apply {
                    addState(intArrayOf(android.R.attr.state_pressed), GradientDrawable().apply { cornerRadius = dp(8f).toFloat(); setColor(context.getColor(R.color.emoji_pressed)) })
                    addState(intArrayOf(), shape)
                }
            }
            setOnClickListener { pick(e) }
        }, GridLayout.LayoutParams().apply { width = cell; height = resultsHeight / rows })
        scroller.scrollTo(0, 0)
        empty.visibility = if (emoji.isEmpty() && text.isNotBlank()) VISIBLE else GONE
    }
}

/** The panel's top line with the lab on: a field-like bar that opens the search. */
internal class EmojiSearchBar(context: Context, quiet: Int, fill: Int, open: () -> Unit) : FrameLayout(context) {
    private val density = resources.displayMetrics.density
    private fun dp(value: Float) = (value * density).roundToInt()
    init {
        val pill = LinearLayout(context).apply {
            gravity = Gravity.CENTER_VERTICAL; setPadding(dp(12f), 0, dp(12f), 0)
            background = GradientDrawable().apply { cornerRadius = dp(18f).toFloat(); setColor(fill) }
            addView(ImageView(context).apply { setImageResource(R.drawable.ic_action_search); imageTintList = ColorStateList.valueOf(quiet) },
                LinearLayout.LayoutParams(dp(20f), dp(20f)))
            addView(TextView(context).apply {
                text = context.getString(R.string.emoji_search_hint); setTextSize(TypedValue.COMPLEX_UNIT_DIP, 15f); useInter(450)
                setTextColor(quiet); includeFontPadding = false
            }, LinearLayout.LayoutParams(-2, -2).apply { leftMargin = dp(8f) })
        }
        addView(pill, LayoutParams(-1, dp(36f), Gravity.CENTER_VERTICAL).apply { leftMargin = dp(8f); rightMargin = dp(8f) })
        contentDescription = context.getString(R.string.emoji_search_hint); isFocusable = true
        setOnClickListener { open() }
    }

    companion object { const val HEIGHT = 48f }
}
