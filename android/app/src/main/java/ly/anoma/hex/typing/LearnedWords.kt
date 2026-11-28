package ly.anoma.hex.typing

import android.content.ContentResolver
import android.content.Context
import android.provider.UserDictionary
import java.util.Locale

/** Labs › Keyboard › Learn new words (#3020): which spelling you kept as typed joins Android's personal dictionary. */
internal object LearnedWords {
    /** Whether a kept word may be learned here: the lab on, in a field Hex may learn from that reads words with a model
     * of its own (never a password, URL, `TYPE_NULL` or incognito field, nor a stand-in's keys). */
    fun allowed(lab: Boolean, secure: Boolean, exact: Boolean, keyEvents: Boolean, learns: Boolean, standIn: Boolean) =
        lab && !secure && !exact && !keyEvents && learns && !standIn

    /** The spelling to add for [typed], a word you kept the way you typed it after [before] (the text before it), or
     * null: only 2 to [TypingWord.MAX_LENGTH] letters and apostrophes (what the personal dictionary is read back as),
     * never a spelling [knows] already has (the model's words, your own and imported ones), and without a capital that
     * only the sentence's start gave it ("Tailscale" first in a sentence is kept as "tailscale"). Every lowercase letter
     * must be in the typing model's alphabet ([spells]), as the personal dictionary is read back only in it: English's
     * has no accents, and German's, Spanish's, Bokmål's and Portuguese's no apostrophe. It starts and ends with a letter
     * ("users'" is a possessive, not a word), and a spelling on the never-suggest list ([blocked], given the lowercase
     * spelling) is never learned: the personal dictionary would make Hex offer it. */
    fun candidate(typed: String, before: String, spells: (Char) -> Boolean, knows: (String) -> Boolean,
                  blocked: (String) -> Boolean = { false }): String? {
        val word = typed.replace('\u2019', '\'')
        if (word.length !in 2..TypingWord.MAX_LENGTH || word.any { !it.isLetter() && it != '\'' }) return null
        if (word.lowercase().any { !spells(it) }) return null
        if (!word.first().isLetter() || !word.last().isLetter() || blocked(word.lowercase()) || knows(word)) return null
        val onlyStart = word.first().isUpperCase() && word.drop(1).none { it.isUpperCase() } && TypingWord.startsSentence(before)
        return if (onlyStart) word.replaceFirstChar { it.lowercaseChar() } else word
    }
}

/** Labs › Keyboard › Learn new words: the spelling an undone autocorrection put back, waiting to be kept. It counts as
 * kept only once that same word ends unchanged (space or a closing mark) with the text before the cursor exactly as
 * the undo left it ([anchor]); any delete, letter or other edit [drop]s it, so holding Delete never teaches a typo. */
internal class PendingKeep {
    /** The word, its typing model's language and the text before it. */
    data class Kept(val word: String, val language: String, val before: String)
    private var kept: Kept? = null
    private var anchor = ""

    /** An autocorrection of [original] was undone, putting back [word] after [before]; [anchor] is the text before the
     * cursor right after it. Of two words a missed space joined, neither is a word the person kept. */
    fun undone(original: String, word: String, language: String, before: String, anchor: String) {
        kept = if (' ' in original) null else Kept(word, language, before)
        this.anchor = anchor
    }
    fun drop() { kept = null }
    /** The word being typed ([token], in [language]) ends with [before] the text before the cursor: the kept word if it
     * is the one the undo put back, unchanged; either way nothing waits any more. */
    fun ended(before: String, token: String?, language: String?): Kept? {
        val k = kept ?: return null
        kept = null
        return k.takeIf { before == anchor && token == it.word && language == it.language }
    }
}

/** Writes and removes Labs › Keyboard › Learn new words' entries in Android's personal dictionary, which the current
 * input method may change without a permission. The keyboard's content observer reads the change back. Any failure
 * leaves the dictionary as it was. Call off the main thread. */
internal object PersonalDictionary {
    private const val FREQUENCY = 250

    /** Adds [word] for [language]'s locale; whether Android took it. */
    fun add(context: Context, word: String, language: String): Boolean = try {
        UserDictionary.Words.addWord(context, word, FREQUENCY, null, Locale.forLanguageTag(language))
        true
    } catch (_: RuntimeException) { false }

    /** Removes the entry [add] made for [word] and [language]. */
    fun remove(resolver: ContentResolver, word: String, language: String) {
        try {
            resolver.delete(UserDictionary.Words.CONTENT_URI,
                "${UserDictionary.Words.WORD}=? AND ${UserDictionary.Words.LOCALE}=?",
                arrayOf(word, Locale.forLanguageTag(language).toString()))
        } catch (_: RuntimeException) {}
    }
}
