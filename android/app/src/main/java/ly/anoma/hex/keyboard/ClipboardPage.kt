package ly.anoma.hex.keyboard

import android.content.Context
import android.graphics.Rect
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable
import android.text.TextUtils
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.view.ViewCompat
import ly.anoma.hex.R
import ly.anoma.hex.useInter
import kotlin.math.roundToInt

/** The Hex menu's Clipboard page (Labs › Keyboard › Clipboard history): a line saying what a tap and a hold do, with
 * Clear set apart at its end, then each kept text on a key-coloured tile, pinned first. A tap types the text and
 * closes the menu; a hold keeps the text in view and adds Pin, Delete and Cancel under it. Clear asks first and says
 * that pinned texts stay. The tiles take the system's key colour, as the emoji panel does, whatever the key colours. */
internal class ClipboardPage(private val context: Context, private val body: LinearLayout,
                             private val paste: (String) -> Unit, private val redraw: () -> Unit) {
    /** The text held for Pin and Delete, and whether Clear is asking. Both reset when the page opens. */
    var holding: String? = null
    var clearing = false
    private val density = context.resources.displayMetrics.density
    private fun dp(n: Float) = (density * n).roundToInt()
    private fun s(id: Int) = context.getString(id)
    private fun color(id: Int) = context.getColor(id)

    fun fill() {
        val items = ClipboardHistory.items(context)
        if (items.isEmpty()) {
            body.addView(text(s(R.string.keyboard_clipboard_empty), 14f, 450, R.color.quiet).apply {
                gravity = Gravity.CENTER; setPadding(dp(16f), dp(28f), dp(16f), dp(28f))
            }, LinearLayout.LayoutParams(-1, -2))
            return
        }
        header(items.count { !it.pinned })
        items.forEach(::tile)
        body.addView(View(context), LinearLayout.LayoutParams(-1, dp(KeyboardMetrics.GAP)))
    }

    /** The line above the tiles: how to use them, and Clear at its end; while Clear asks, its question instead. */
    private fun header(unpinned: Int) {
        val line = LinearLayout(context).apply {
            gravity = Gravity.CENTER_VERTICAL; minimumHeight = dp(40f); setPadding(dp(16f), 0, dp(4f), 0)
        }
        if (clearing && unpinned > 0) {
            line.addView(text(context.resources.getQuantityString(R.plurals.keyboard_clipboard_clear_confirm, unpinned, unpinned),
                13f, 500, R.color.ink).apply { maxLines = 2 }, LinearLayout.LayoutParams(0, -2, 1f))
            line.addView(action(R.string.keyboard_clipboard_cancel) { clearing = false; redraw() })
            line.addView(action(R.string.keyboard_clipboard_clear) {
                ClipboardHistory.clearUnpinned(context); clearing = false; redraw()
            })
        } else {
            line.addView(text(s(R.string.keyboard_clipboard_hint), 13f, 450, R.color.quiet).apply { maxLines = 2 },
                LinearLayout.LayoutParams(0, -2, 1f))
            if (unpinned > 0) line.addView(action(R.string.keyboard_clipboard_clear) { clearing = true; holding = null; redraw() })
        }
        body.addView(line, LinearLayout.LayoutParams(-1, -2))
    }

    private fun tile(item: CopiedText) {
        val held = item.text == holding
        val tile = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            background = keyShape(held)
        }
        val face = LinearLayout(context).apply {
            gravity = Gravity.CENTER_VERTICAL; setPadding(dp(12f), dp(10f), dp(12f), dp(10f)); minimumHeight = dp(44f)
            isFocusable = true; contentDescription = item.text +
                if (item.pinned) ", " + s(R.string.keyboard_clipboard_pinned) else ""
            setOnClickListener { holding = null; clearing = false; paste(item.text) }
            setOnLongClickListener { holding = if (held) null else item.text; clearing = false; redraw(); true }
            // TalkBack's actions in place of the hold.
            ViewCompat.addAccessibilityAction(this, s(if (item.pinned) R.string.keyboard_clipboard_unpin else R.string.keyboard_clipboard_pin)) { _, _ ->
                ClipboardHistory.pin(context, item.text, !item.pinned); redraw(); true
            }
            ViewCompat.addAccessibilityAction(this, s(R.string.keyboard_clipboard_delete)) { _, _ ->
                ClipboardHistory.delete(context, item.text); redraw(); true
            }
        }
        face.addView(text(ClipboardRules.preview(item.text), 15f, 450, R.color.ink).apply {
            maxLines = if (held) 2 else 3; ellipsize = TextUtils.TruncateAt.END
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }, LinearLayout.LayoutParams(0, -2, 1f))
        if (item.pinned) face.addView(text(s(R.string.keyboard_clipboard_pinned), 12f, 500, R.color.quiet).apply {
            setPadding(dp(10f), 0, 0, 0); importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        })
        tile.addView(face, LinearLayout.LayoutParams(-1, -2))
        if (held) {
            // The text stays in view above what is done to it.
            val actions = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL }
            fun add(label: Int, act: () -> Unit) = actions.addView(action(label) { act(); holding = null; redraw() },
                LinearLayout.LayoutParams(0, dp(44f), 1f))
            add(if (item.pinned) R.string.keyboard_clipboard_unpin else R.string.keyboard_clipboard_pin) {
                ClipboardHistory.pin(context, item.text, !item.pinned)
            }
            add(R.string.keyboard_clipboard_delete) { ClipboardHistory.delete(context, item.text) }
            add(R.string.keyboard_clipboard_cancel) { }
            tile.addView(View(context).apply { setBackgroundColor(color(R.color.rule)) },
                LinearLayout.LayoutParams(-1, maxOf(1, dp(0.5f))).apply { marginStart = dp(12f); marginEnd = dp(12f) })
            tile.addView(actions, LinearLayout.LayoutParams(-1, -2))
            // A tile held near the bottom scrolls up so Pin, Delete and Cancel are in view.
            tile.post { tile.requestRectangleOnScreen(Rect(0, 0, tile.width, tile.height)) }
        }
        body.addView(tile, LinearLayout.LayoutParams(-1, -2).apply {
            val side = dp(KeyboardMetrics.SIDE + KeyboardMetrics.GAP / 2)
            marginStart = side; marginEnd = side; topMargin = dp(KeyboardMetrics.GAP)
        })
    }

    /** A key's own look: its colour and corner, darker while pressed; a held tile keeps the pressed colour. */
    private fun keyShape(held: Boolean): StateListDrawable {
        fun shape(c: Int) = GradientDrawable().apply { cornerRadius = KeyboardMetrics.CORNER * density; setColor(color(c)) }
        return StateListDrawable().apply {
            addState(intArrayOf(android.R.attr.state_pressed), shape(R.color.pressed))
            addState(intArrayOf(), shape(if (held) R.color.modifier else R.color.key))
        }
    }

    private fun action(label: Int, act: () -> Unit) = text(s(label), 15f, 500, R.color.ink).apply {
        gravity = Gravity.CENTER; minWidth = dp(48f); minHeight = dp(44f); setPadding(dp(12f), 0, dp(12f), 0)
        isFocusable = true; setOnClickListener { act() }
    }

    private fun text(value: String, size: Float, weight: Int, color: Int) = TextView(context).apply {
        text = value; setTextSize(TypedValue.COMPLEX_UNIT_DIP, size); useInter(weight); includeFontPadding = false
        setTextColor(color(color))
    }
}
