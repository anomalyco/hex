package ly.anoma.hex.typing

import android.content.Context
import java.security.MessageDigest
import ly.anoma.hex.keyboard.KeyboardHeight
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
    /** Labs › Keyboard › Password manager suggestions: Android's inline autofill suggestions in the
     * strip, off by default. Hex shows the service's own chips and never reads them. */
    var passwordManagerSuggestions: Boolean
        get() = prefs.getBoolean(PASSWORD_MANAGER_SUGGESTIONS, false)
        set(value) { prefs.edit().putBoolean(PASSWORD_MANAGER_SUGGESTIONS, value).apply() }
    /** Labs › Keyboard › Voice-only mode: a "voice" subtype other keyboards switch to for dictation
     * alone, off by default. */
    var voiceOnly: Boolean
        get() = prefs.getBoolean(VOICE_ONLY, false)
        set(value) { prefs.edit().putBoolean(VOICE_ONLY, value).apply() }
    /** Settings › Keyboard › Height: Normal until another step is chosen. */
    var height: KeyboardHeight
        get() = KeyboardHeight.of(prefs.getString(HEIGHT, null))
        set(value) { prefs.edit().putString(HEIGHT, value.key).apply() }

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
        const val PASSWORD_MANAGER_SUGGESTIONS = "passwordManagerSuggestions"
        const val VOICE_ONLY = "voiceOnly"
        const val HEIGHT = "keyboardHeight"
    }
}
