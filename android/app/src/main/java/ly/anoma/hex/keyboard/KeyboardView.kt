package ly.anoma.hex.keyboard

import android.content.Context
import android.view.View
import android.view.WindowInsets
import android.widget.LinearLayout
import ly.anoma.hex.R
import ly.anoma.hex.speech.SpeechPhase
import ly.anoma.hex.typing.Suggestion

/** The keyboard's root view: the strip, then the keys or the Hex menu in their place. */
internal class KeyboardView(context: Context, private val ime: HexKeyboard) : LinearLayout(context), KeyboardSurface {
    private val feedback = LetterFeedback(this)
    private val strip = StripView(context, ime, this)
    private val keys = KeyPlaneView(context, ime, this, feedback)
    private val menu: SpeechMenu

    init {
        orientation = VERTICAL
        layoutDirection = View.LAYOUT_DIRECTION_LTR
        // On edge-to-edge IME windows the system draws its back and switcher buttons and the
        // gesture handle over the bottom of our window: 48 dp, which Android 14 and later report
        // as the caption bar inside systemBars (navigationBars alone is the 24 dp handle). Keys stay
        // above it. Where the window already ends above the navigation bar (Android 12) it is 0.
        setOnApplyWindowInsetsListener { _, insets ->
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            setPadding(bars.left, 0, bars.right, bars.bottom)
            insets
        }
        setBackgroundColor(context.getColor(R.color.keyboard))
        addView(strip, LayoutParams(LayoutParams.MATCH_PARENT, strip.stripHeight))
        addView(keys, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        menu = SpeechMenu(context, ime, strip::title, ::closeMenu).apply { visibility = GONE }
        addView(menu, LayoutParams(-1, keys.menuHeight))
        status(SpeechPhase.Idle, context.getString(R.string.keyboard_hint_hold))
    }

    override fun showPage(page: KeyboardPage) = keys.showPage(page)
    override fun suggestions(slots: List<Suggestion>) = strip.suggestions(slots)

    /** Emoji in place of the strip and the keys, the window's height unchanged (Gboard's panel
     * replaces its toolbar too); ABC brings both back. */
    fun openEmoji() {
        if (strip.recording) return
        closeMenu(); ime.engine.clearForDictation()
        strip.visibility = GONE; keys.openEmoji(strip.stripHeight)
    }
    fun closeEmoji() {
        if (!keys.emojiOpen) return
        keys.closeEmoji(); strip.visibility = VISIBLE; strip.update()
    }
    fun openMenu() {
        if (strip.recording) return
        closeEmoji()
        keys.visibility = GONE; menu.visibility = VISIBLE
        strip.choosing = true; strip.menuOpened(); menu.home(); strip.update()
    }
    fun openLanguageMenu() { openMenu(); menu.language() }
    fun openAccents(accents: List<String>) {
        openMenu(); menu.accents(accents) { accent -> closeMenu(); ime.engine.type(accent) }
    }
    fun backInMenu() { if (menu.visibility == VISIBLE) menu.back() }
    fun closeMenu() {
        if (menu.visibility != VISIBLE) return
        menu.visibility = GONE; keys.visibility = VISIBLE
        strip.choosing = false; strip.menuClosed(); keys.refreshSpace(); ime.engine.configureTyping(keepWord = true); strip.update()
    }
    fun status(phase: SpeechPhase, hint: String) {
        val idle = phase == SpeechPhase.Idle
        strip.prepare(phase, hint)
        if (!idle) closeMenu()
        strip.status(phase, hint, menu.visibility == VISIBLE)
        // Keep the snapshotted paste language visible until this dictation has finished.
        if (idle) keys.refreshSpace()
        strip.update()
    }
    fun setLevel(value: Float) = strip.setLevel(value)
    fun cancelGestures() { strip.cancelGestures(); keys.cancelGestures() }
    /** The editor went away: the next one starts on the letters. */
    fun reset() { closeMenu(); closeEmoji() }
}
