package ly.anoma.hex.typing

import android.content.Context
import java.security.MessageDigest
import ly.anoma.hex.keyboard.HeldDeleteWords
import ly.anoma.hex.keyboard.KeyClick
import ly.anoma.hex.keyboard.KeyboardHeight
import ly.anoma.hex.keyboard.KeyColor
import ly.anoma.hex.speech.Spoken

internal class TypingPreferences(context: Context) {
    private val prefs = context.getSharedPreferences("typing", Context.MODE_PRIVATE)
    var layout: Spoken?
        get() = Spoken.matching(prefs.getString("layout", null))
        set(value) { prefs.edit().putString("layout", value?.layoutCode).apply() }
    /** Settings › Keyboard › Auto-capitalisation: shift comes on where the field asks for a capital. On by default. */
    var autoCapitalize: Boolean
        get() = prefs.getBoolean("autoCapitalize", true)
        set(value) { prefs.edit().putBoolean("autoCapitalize", value).apply() }
    var autocorrect: Boolean
        get() = prefs.getBoolean("autocorrect", true)
        set(value) { prefs.edit().putBoolean("autocorrect", value).apply() }
    var suggestSwearWords: Boolean
        get() = prefs.getBoolean(SUGGEST_SWEAR_WORDS, false)
        set(value) { prefs.edit().putBoolean(SUGGEST_SWEAR_WORDS, value).apply() }
    var spatial: Boolean
        get() = prefs.getBoolean("spatial", true)
        set(value) { prefs.edit().putBoolean("spatial", value).apply() }
    /** Keyboard › Swipe typing: slide through a word's letters to type it. On by default, as on iPhone and Gboard. */
    var swipe: Boolean
        get() = prefs.getBoolean("swipe", true)
        set(value) { prefs.edit().putBoolean("swipe", value).apply() }
    var popups: Boolean
        get() = prefs.getBoolean("popups", true)
        set(value) { prefs.edit().putBoolean("popups", value).apply() }
    var haptics: Boolean
        get() = prefs.getBoolean("haptics", true)
        set(value) { prefs.edit().putBoolean("haptics", value).apply() }
    /** Labs › Keyboard › Symbols on letter keys: a held second or third row key offers its symbol first. */
    var letterSymbols: Boolean
        get() = prefs.getBoolean("letterSymbols", false)
        set(value) { prefs.edit().putBoolean("letterSymbols", value).apply() }
    /** Labs › Keyboard › Number row: the digits above the letters. Off by default. */
    var numberRow: Boolean
        get() = prefs.getBoolean(NUMBER_ROW, false)
        set(value) { prefs.edit().putBoolean(NUMBER_ROW, value).apply() }
    /** Labs › Keyboard › Local currency: the phone's own currency on the ?123 page and d's held symbol
     * instead of $. Off by default. */
    var localCurrency: Boolean
        get() = prefs.getBoolean(LOCAL_CURRENCY, false)
        set(value) { prefs.edit().putBoolean(LOCAL_CURRENCY, value).apply() }
    /** Labs › Keyboard › Emoji suggestions: the word being typed's emoji in the strip, off by default. */
    var emojiSuggestions: Boolean
        get() = prefs.getBoolean(EMOJI_SUGGESTIONS, false)
        set(value) { prefs.edit().putBoolean(EMOJI_SUGGESTIONS, value).apply() }
    /** Labs › Keyboard › Keyboard tools: emoji, paste, settings and undo in the strip, off by default. */
    var keyboardTools: Boolean
        get() = prefs.getBoolean(KEYBOARD_TOOLS, false)
        set(value) { prefs.edit().putBoolean(KEYBOARD_TOOLS, value).apply() }
    /** Labs › Keyboard › Smart quotes: typographic ’ ‘ “ ” for the quote keys, suggestions and
     * dictation, off by default. */
    var smartQuotes: Boolean
        get() = prefs.getBoolean(SMART_QUOTES, false)
        set(value) { prefs.edit().putBoolean(SMART_QUOTES, value).apply() }
    /** Labs › Keyboard › Delete swiped words letter by letter: delete right after a swipe removes one letter
     * instead of taking the word back, as iPhone Hex's experiment (#2323). Off by default. */
    var swipeDeleteLetters: Boolean
        get() = prefs.getBoolean(SWIPE_DELETE_LETTERS, false)
        set(value) { prefs.edit().putBoolean(SWIPE_DELETE_LETTERS, value).apply() }
    /** Labs › Keyboard › Delete words when held: a held Delete removes whole words, slowly at first and faster the
     * longer it is held ([ly.anoma.hex.keyboard.HeldDeleteWords]), as SwiftKey's does (#3007). Off by default. */
    var deleteWords: Boolean
        get() = prefs.getBoolean(DELETE_WORDS, false)
        set(value) { prefs.edit().putBoolean(DELETE_WORDS, value).apply() }
    /** Labs › Keyboard › Delete words when held › Initial delay: the gap after the first word, in ms (#3032). Kept
     * while the lab is off. */
    var deleteWordsDelay: Int
        get() = HeldDeleteWords.clampDelay(prefs.getInt(DELETE_WORDS_DELAY, HeldDeleteWords.FIRST_GAP))
        set(value) { prefs.edit().putInt(DELETE_WORDS_DELAY, HeldDeleteWords.clampDelay(value)).apply() }
    /** Labs › Keyboard › Delete words when held › Acceleration: how much shorter each next gap is, in % (#3032). Kept
     * while the lab is off. */
    var deleteWordsSpeedup: Int
        get() = HeldDeleteWords.clampSpeedup(prefs.getInt(DELETE_WORDS_SPEEDUP, HeldDeleteWords.SPEEDUP))
        set(value) { prefs.edit().putInt(DELETE_WORDS_SPEEDUP, HeldDeleteWords.clampSpeedup(value)).apply() }
    /** Labs › Keyboard › SwiftKey words: the words imported from another keyboard's export ([ImportedWords])
     * join your own words in suggestions. Off by default; the imported list stays on the phone while off. */
    var importedWords: Boolean
        get() = prefs.getBoolean(USE_IMPORTED_WORDS, false)
        set(value) { prefs.edit().putBoolean(USE_IMPORTED_WORDS, value).apply() }
    /** Labs › Keyboard › Punctuation after autocorrect: a clause mark typed right after an autocorrection or a text
     * replacement on space takes the space it typed, as one does after a pick (#2813). Off by default. */
    var marksTakeFixSpace: Boolean
        get() = prefs.getBoolean(MARKS_TAKE_FIX_SPACE, false)
        set(value) { prefs.edit().putBoolean(MARKS_TAKE_FIX_SPACE, value).apply() }
    /** Labs › Keyboard › Space after punctuation: outside URL fields, a clause mark that takes a pick's space (or, with
     * Punctuation after autocorrect, a fix's) puts it back after itself ("here", ",", gives "here, "), and a space typed
     * straight after is swallowed as one after a pick is (#3000). Off by default. */
    var marksKeepSpace: Boolean
        get() = prefs.getBoolean(MARKS_KEEP_SPACE, false)
        set(value) { prefs.edit().putBoolean(MARKS_KEEP_SPACE, value).apply() }
    /** Labs › Keyboard › Click when a hold opens: with Haptics on, a held key plays a key press's click the moment its
     * symbols or accents open (#3006). Off by default. */
    var holdClick: Boolean
        get() = prefs.getBoolean(HOLD_CLICK, false)
        set(value) { prefs.edit().putBoolean(HOLD_CLICK, value).apply() }
    /** Labs › Keyboard › Closing quotes take the space: a " or ” typed while a quote is open in the paragraph takes
     * the space before it, however it got there (#3001). Off by default. */
    var closingQuoteTakesSpace: Boolean
        get() = prefs.getBoolean(CLOSING_QUOTE_TAKES_SPACE, false)
        set(value) { prefs.edit().putBoolean(CLOSING_QUOTE_TAKES_SPACE, value).apply() }
    /** Labs › Keyboard › SwiftKey layout: Microsoft SwiftKey's held letter symbols, bottom row (emoji before the
     * comma), one-row held period with flicks, a held comma that dictates, and its 123 and {&= pages (#3021). Off by
     * default. */
    var swiftKeyLayout: Boolean
        get() = prefs.getBoolean(SWIFTKEY_LAYOUT, false)
        set(value) { prefs.edit().putBoolean(SWIFTKEY_LAYOUT, value).apply() }
    /** Labs › Keyboard › Learn new words: a spelling no dictionary knows that you keep as typed (its chip tapped over a
     * correction, or the correction undone) joins Android's personal dictionary ([LearnedWords], #3020). Off by default. */
    var learnWords: Boolean
        get() = prefs.getBoolean(LEARN_WORDS, false)
        set(value) { prefs.edit().putBoolean(LEARN_WORDS, value).apply() }
    /** Labs › Keyboard › Adaptive touch: the decoder reads taps with your own key centres once it has learned them
     * ([AdaptiveTouch], kept by [AdaptiveTouchStore]). Off by default. */
    var adaptiveTouch: Boolean
        get() = prefs.getBoolean(ADAPTIVE_TOUCH, false)
        set(value) { prefs.edit().putBoolean(ADAPTIVE_TOUCH, value).apply() }
    /** Labs › Keyboard › Clipboard history: the last texts copied, kept on this phone and offered in the Hex menu
     * ([ly.anoma.hex.keyboard.ClipboardHistory]). Off by default; off, the clipboard is never read for it. */
    var clipboardHistory: Boolean
        get() = prefs.getBoolean(CLIPBOARD_HISTORY, false)
        set(value) { prefs.edit().putBoolean(CLIPBOARD_HISTORY, value).apply() }
    /** Labs › Keyboard › Password manager suggestions: Android's inline autofill suggestions in the
     * strip, off by default. Hex shows the service's own chips and never reads them. */
    var passwordManagerSuggestions: Boolean
        get() = prefs.getBoolean(PASSWORD_MANAGER_SUGGESTIONS, false)
        set(value) { prefs.edit().putBoolean(PASSWORD_MANAGER_SUGGESTIONS, value).apply() }
    /** Labs › Keyboard › Hex key on the left: the strip's Hex key, ✕ and the recording bar at its left end instead
     * of its right, for the other hand (#2800). Off by default. */
    var hexKeyLeft: Boolean
        get() = prefs.getBoolean(HEX_KEY_LEFT, false)
        set(value) { prefs.edit().putBoolean(HEX_KEY_LEFT, value).apply() }
    /** Labs › Keyboard › Volume in the Hex menu: a Volume row that opens Android's own volume panel (#2868).
     * Off by default. */
    var volumeInMenu: Boolean
        get() = prefs.getBoolean(VOLUME_IN_MENU, false)
        set(value) { prefs.edit().putBoolean(VOLUME_IN_MENU, value).apply() }
    /** Labs › Keyboard › Key click volume: the key click at [keyClickPercent] instead of the phone's own level (#3005).
     * Off by default. */
    var keyClickVolume: Boolean
        get() = prefs.getBoolean(KEY_CLICK_VOLUME, false)
        set(value) { prefs.edit().putBoolean(KEY_CLICK_VOLUME, value).apply() }
    /** Labs › Keyboard › Key click volume's slider, [KeyClick.MIN]–[KeyClick.MAX]%; kept while the lab is off. */
    var keyClickPercent: Int
        get() = KeyClick.clamp(prefs.getInt(KEY_CLICK_PERCENT, KeyClick.DEFAULT))
        set(value) { prefs.edit().putInt(KEY_CLICK_PERCENT, KeyClick.clamp(value)).apply() }
    /** The percent the keys click at, or null for the phone's own level (the lab off). */
    val clickPercent: Int? get() = if (keyClickVolume) keyClickPercent else null
    /** Labs › Keyboard › Voice-only mode: a "voice" subtype other keyboards switch to for dictation
     * alone, off by default. */
    var voiceOnly: Boolean
        get() = prefs.getBoolean(VOICE_ONLY, false)
        set(value) { prefs.edit().putBoolean(VOICE_ONLY, value).apply() }
    /** Labs › Keyboard › Voice bar from the Hex menu: the Hex menu's Voice only collapses the keys into a dictation
     * bar in the same field, and its Show keyboard brings them back (#2869). Off by default. */
    var voiceBarFromMenu: Boolean
        get() = prefs.getBoolean(VOICE_BAR_FROM_MENU, false)
        set(value) { prefs.edit().putBoolean(VOICE_BAR_FROM_MENU, value).apply() }
    /** Settings › Keyboard › Height: Normal until another step is chosen. */
    var height: KeyboardHeight
        get() = KeyboardHeight.of(prefs.getString(HEIGHT, null))
        set(value) { prefs.edit().putString(HEIGHT, value.key).apply() }
    /** Labs › Keyboard › Key colors: the keys in [keyColor]'s colours, off by default. */
    var keyColors: Boolean
        get() = prefs.getBoolean(KEY_COLORS, false)
        set(value) { prefs.edit().putBoolean(KEY_COLORS, value).apply() }
    /** Labs › Keyboard › Key colors › Color: Automatic until a theme is chosen; kept while the lab is off. */
    var keyColor: KeyColor
        get() = KeyColor.of(prefs.getString(KEY_COLOR, null))
        set(value) { prefs.edit().putString(KEY_COLOR, value.key).apply() }
    /** The colours the keys are drawn in: [keyColor] while the lab is on, else Automatic. */
    val drawnKeyColor get() = if (keyColors) keyColor else KeyColor.Automatic
    /** Labs › Keyboard › Test keyboard on Home: the practice field moves from Keyboard to Home (#2801). Off by default. */
    var testKeyboardOnHome: Boolean
        get() = prefs.getBoolean(TEST_KEYBOARD_ON_HOME, false)
        set(value) { prefs.edit().putBoolean(TEST_KEYBOARD_ON_HOME, value).apply() }

    // Remember only explicit rejection of this exact pair in this language; never save typed text.
    private fun pair(language: String, original: String, replacement: String): String {
        // A smart apostrophe counts as the straight one it replaces, so rejecting a fix twice still
        // blocks it when the keyboard inserts the fix's curly spelling.
        val bytes = (language + "\u0000" + original.lowercase().replace('\u2019', '\'') + "\u0000" +
            replacement.lowercase().replace('\u2019', '\'')).toByteArray()
        return android.util.Base64.encodeToString(MessageDigest.getInstance("SHA-256").digest(bytes), android.util.Base64.NO_WRAP)
    }
    fun blocked(language: String, original: String, replacement: String) =
        prefs.getInt(pair(language, original, replacement), 0) >= 2
    fun undo(language: String, original: String, replacement: String) {
        val key = pair(language, original, replacement)
        prefs.edit().putInt(key, (prefs.getInt(key, 0) + 1).coerceAtMost(2)).apply()
    }
    companion object {
        const val SUGGEST_SWEAR_WORDS = "suggestSwearWords"
        const val NUMBER_ROW = "numberRow"
        const val LOCAL_CURRENCY = "localCurrency"
        const val EMOJI_SUGGESTIONS = "emojiSuggestions"
        const val KEYBOARD_TOOLS = "keyboardTools"
        const val SMART_QUOTES = "smartQuotes"
        const val SWIPE_DELETE_LETTERS = "swipeDeleteLetters"
        const val DELETE_WORDS = "deleteWords"
        const val DELETE_WORDS_DELAY = "deleteWordsDelay"
        const val DELETE_WORDS_SPEEDUP = "deleteWordsSpeedup"
        const val MARKS_TAKE_FIX_SPACE = "marksTakeFixSpace"
        const val MARKS_KEEP_SPACE = "marksKeepSpace"
        const val HOLD_CLICK = "holdClick"
        const val CLOSING_QUOTE_TAKES_SPACE = "closingQuoteTakesSpace"
        const val SWIFTKEY_LAYOUT = "swiftKeyLayout"
        const val LEARN_WORDS = "learnWords"
        const val ADAPTIVE_TOUCH = "adaptiveTouch"
        const val USE_IMPORTED_WORDS = "importedWords"
        /** A counter [ImportedWords] bumps whenever the imported list changes, so the keyboard reads it again. */
        const val IMPORTED_WORDS = "importedWordsRevision"
        /** How many imported words are kept, saved with them. */
        const val IMPORTED_WORDS_COUNT = "importedWordsCount"
        const val PASSWORD_MANAGER_SUGGESTIONS = "passwordManagerSuggestions"
        const val CLIPBOARD_HISTORY = "clipboardHistory"
        const val VOICE_ONLY = "voiceOnly"
        const val VOICE_BAR_FROM_MENU = "voiceBarFromMenu"
        const val HEIGHT = "keyboardHeight"
        const val HEX_KEY_LEFT = "hexKeyLeft"
        const val VOLUME_IN_MENU = "volumeInMenu"
        const val KEY_CLICK_VOLUME = "keyClickVolume"
        const val KEY_CLICK_PERCENT = "keyClickPercent"
        const val KEY_COLORS = "keyColors"
        const val KEY_COLOR = "keyColor"
        const val TEST_KEYBOARD_ON_HOME = "testKeyboardOnHome"
    }
}
