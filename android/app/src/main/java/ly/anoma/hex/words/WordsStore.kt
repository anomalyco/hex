package ly.anoma.hex.words

import android.content.Context
import ly.anoma.hex.speech.DictationFixes
import ly.anoma.hex.speech.SpeechPreferences
import ly.anoma.hex.speech.WordReplacement

/** What Undo puts back: both lists as they were before Add, and whether Custom vocabulary was on. Never the file. */
internal data class WordsUndo(val lists: WordsLists, val customVocabulary: Boolean) {
    /** A first line "on" or "off", then "t" and a term, or "r", heard and output, a line each, tab-separated and
     * escaped as the learned fixes are. */
    fun encode() = buildString {
        append(if (customVocabulary) "on" else "off")
        for (term in lists.terms) append("\nt\t").append(DictationFixes.escape(term))
        for (entry in lists.replacements)
            append("\nr\t").append(DictationFixes.escape(entry.heard)).append('\t').append(DictationFixes.escape(entry.write))
    }

    companion object {
        fun decode(text: String): WordsUndo? {
            val lines = text.split('\n')
            val on = when (lines.first()) { "on" -> true; "off" -> false; else -> return null }
            val terms = ArrayList<String>()
            val replacements = ArrayList<WordReplacement>()
            for (line in lines.drop(1)) {
                val fields = line.split('\t')
                when {
                    fields.size == 2 && fields[0] == "t" -> terms += DictationFixes.unescape(fields[1])
                    fields.size == 3 && fields[0] == "r" ->
                        replacements += WordReplacement(DictationFixes.unescape(fields[1]), DictationFixes.unescape(fields[2]))
                    else -> return null
                }
            }
            return WordsUndo(WordsLists(terms, replacements), on)
        }
    }
}

/** The two lists an import changes, in the `speech` preferences, and the Undo copy kept beside them until either
 * list is edited or another file is imported. Only these keys and the Custom vocabulary switch the sheet turned
 * on are ever written. */
internal class WordsStore(context: Context) {
    private val speech = SpeechPreferences(context)
    private val prefs = context.getSharedPreferences("speech", Context.MODE_PRIVATE)

    fun lists() = WordsLists(speech.vocabularyTerms, speech.wordReplacements)

    val customVocabulary get() = speech.customVocabulary

    /** Saves [merged], keeping [before] for Undo. [turnOn] is the sheet's own Custom vocabulary switch. */
    fun add(before: WordsLists, merged: WordsMerged, turnOn: Boolean) {
        prefs.edit().putString(UNDO, WordsUndo(before, speech.customVocabulary).encode()).putInt(IMPORTS, imports + 1).apply()
        speech.vocabularyTerms = merged.after.terms
        speech.wordReplacements = merged.after.replacements
        if (turnOn) speech.customVocabulary = true
    }

    val canUndo get() = prefs.contains(UNDO)

    /** Puts both lists and the switch back exactly as they were before the last Add. */
    fun undo(): Boolean {
        val undo = prefs.getString(UNDO, null)?.let(WordsUndo::decode)
        prefs.edit().remove(UNDO).putInt(IMPORTS, imports + 1).apply()
        if (undo == null) return false
        speech.vocabularyTerms = undo.lists.terms
        speech.wordReplacements = undo.lists.replacements
        speech.customVocabulary = undo.customVocabulary
        return true
    }

    /** The person edited a list by hand: the last import can no longer be undone. */
    fun edited() { if (canUndo) prefs.edit().remove(UNDO).apply() }

    /** Counts imports and undos, so Settings reloads an editor's text after one. */
    val imports get() = prefs.getInt(IMPORTS, 0)

    private companion object {
        const val UNDO = "wordsUndo"
        const val IMPORTS = "wordsImports"
    }
}
