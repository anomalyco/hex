package ly.anoma.hex.keyboard

import android.content.Context
import android.graphics.Color
import android.view.inputmethod.EditorInfo
import ly.anoma.hex.R

/** Labs › Keyboard › Key colors: iPhone Hex's key themes (KeyboardTheme.swift), in the same colours so the two apps
 * match. A theme colours the keys only: every keycap, the special keys included, a pressed key, the pop-up and the
 * held key's options, an action field's enter key and the labels. The keyboard's background, the strip, the emoji
 * panel's grid and the Hex menu keep the system's light or dark. Automatic is the keys as they always were. */
internal enum class KeyColor(val key: String, val label: Int, val keys: KeyColorSet?) {
    Automatic("automatic", R.string.settings_automatic, null),
    /** Black keys with white labels, on whichever background the phone shows. */
    Black("black", R.string.key_color_black, KeyColorSet(keyFill = 0x000000, keyPressedFill = 0x2C2C2E,
        functionHeldFill = 0x48484A, bubbleFill = 0x1C1C1E, accentSelectFill = 0x3A3A3C, actionFill = 0x007AFF,
        actionPressedFill = 0x0062CC)),
    /** White keys with black labels: the light keys, kept in dark mode too. */
    White("white", R.string.key_color_white, KeyColorSet(darkLabels = true, keyFill = 0xFFFFFF, keyPressedFill = 0xD8DCE4,
        functionHeldFill = 0xC5C5C9, bubbleFill = 0xFFFFFF, accentSelectFill = 0xE3E3E6, actionFill = 0x007AFF,
        actionPressedFill = 0x0062CC)),
    // The tinted themes are OKLCH lightness 0.40 keys, each state a fixed step lighter at the theme's hue
    // (KeyboardTheme.swift has the recipe), and the theme's own action colour.
    Navy("navy", R.string.key_color_navy, KeyColorSet(keyFill = 0x294875, keyPressedFill = 0x416190,
        functionHeldFill = 0x4F70A0, bubbleFill = 0x345381, accentSelectFill = 0x4F70A0, actionFill = 0x2563EB,
        actionPressedFill = 0x1D4ED8)),
    Forest("forest", R.string.key_color_forest, KeyColorSet(keyFill = 0x215339, keyPressedFill = 0x3B6D51,
        functionHeldFill = 0x4A7B5F, bubbleFill = 0x2D5E44, accentSelectFill = 0x4A7B5F, actionFill = 0x1F7A4C,
        actionPressedFill = 0x186B41)),
    Ember("ember", R.string.key_color_ember, KeyColorSet(keyFill = 0x6E3524, keyPressedFill = 0x894E3C,
        functionHeldFill = 0x995D4A, bubbleFill = 0x7A402E, accentSelectFill = 0x995D4A, actionFill = 0xC2410C,
        actionPressedFill = 0x9A3412)),
    Burgundy("burgundy", R.string.key_color_burgundy, KeyColorSet(keyFill = 0x6F303C, keyPressedFill = 0x8B4955,
        functionHeldFill = 0x9B5763, bubbleFill = 0x7C3B47, accentSelectFill = 0x9B5763, actionFill = 0xBC4059,
        actionPressedFill = 0x9E354A));

    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key } ?: Automatic
    }
}

/** One theme's key colours (0xRRGGBB, opaque), the same in light and dark: iPhone Hex's KeyboardThemeColors, named
 * as there. Android has one pressed state for every key, so the iPhone's held steps are not used, except the special
 * key's held step as the pop-up's rim. */
internal class KeyColorSet(val darkLabels: Boolean = false, keyFill: Int, keyPressedFill: Int, functionHeldFill: Int,
                           bubbleFill: Int, accentSelectFill: Int, actionFill: Int, actionPressedFill: Int) {
    val keyFill = opaque(keyFill)
    val keyPressedFill = opaque(keyPressedFill)
    val functionHeldFill = opaque(functionHeldFill)
    val bubbleFill = opaque(bubbleFill)
    val accentSelectFill = opaque(accentSelectFill)
    val actionFill = opaque(actionFill)
    val actionPressedFill = opaque(actionPressedFill)
    /** A label or icon on a key, its pop-up or a held key's options. */
    val ink = if (darkLabels) Color.BLACK else Color.WHITE
    /** Row 1's digits, the space bar's language and a phone key's letters: white at 0.7 on tinted keys keeps 5:1 or
     * more, and on White iPhone Hex's grey. */
    val quiet = if (darkLabels) opaque(0x525763) else 0xB3FFFFFF.toInt()
    private companion object { fun opaque(rgb: Int) = rgb or 0xFF000000.toInt() }
}

/** What the keys are drawn with: a theme's colours, or Automatic's own (the resources, light or dark with the phone). */
internal class KeyPaints(context: Context, val color: KeyColor) {
    private val set = color.keys
    val key = set?.keyFill ?: context.getColor(R.color.key)
    /** Shift, delete, ?123, enter and the other special keys: one colour with the letters under a theme, as on iPhone. */
    val modifier = set?.keyFill ?: context.getColor(R.color.modifier)
    val pressed = set?.keyPressedFill ?: context.getColor(R.color.pressed)
    val ink = set?.ink ?: context.getColor(R.color.ink)
    val quiet = set?.quiet ?: context.getColor(R.color.quiet)
    val popup = set?.bubbleFill ?: context.getColor(R.color.sheet)
    val popupRim = set?.functionHeldFill ?: context.getColor(R.color.modifier)
    /** A held key's chosen option: Automatic inverts it; a theme puts it on a lighter square, as on iPhone. */
    val choice = set?.accentSelectFill ?: context.getColor(R.color.ink)
    val choiceInk = set?.ink ?: context.getColor(R.color.page)
    /** Enter in a field whose action is Send, Go, Search or Done (iPhone Hex's action keys): the theme's action
     * colour under a white symbol. Null for Automatic, whose enter key stays a special key. */
    fun action(editorAction: Int): Int? = set?.actionFill?.takeIf { isAction(editorAction) }
    val actionPressed get() = set?.actionPressedFill ?: pressed

    companion object {
        fun isAction(editorAction: Int) = editorAction == EditorInfo.IME_ACTION_SEND || editorAction == EditorInfo.IME_ACTION_GO ||
            editorAction == EditorInfo.IME_ACTION_SEARCH || editorAction == EditorInfo.IME_ACTION_DONE
    }
}
