package ly.anoma.hex.speech

import ly.anoma.hex.words.WordsReader
import ly.anoma.hex.words.WordsText

/** One of Custom vocabulary's Word replacements: words heard, and what Hex writes instead. */
internal data class WordReplacement(val heard: String, val write: String)

/** Custom vocabulary › Word replacements: a list the person edits, kept in the `speech` preferences and applied
 * to dictations before learned fixes, by [DictationFixes.apply]'s whole-word, case-following rules with up to
 * [maxWords] heard words. Entries follow the Hex words file's limits, so every one can be exported. */
internal object WordReplacements {
    const val maxCount = WordsReader.MAX_REPLACEMENTS
    const val maxWords = WordsReader.MAX_WORDS

    enum class Problem { NoLetters, Hidden, TooLong, TooManyWords, Blank, Duplicate, Full }

    /** The entry as it is kept: the heard words cleaned to one line, the output composed with LF line ends. */
    fun cleaned(heard: String, write: String) = WordReplacement(WordsText.line(heard), WordsText.output(write))

    /** Why [entry] (already [cleaned]) can't join [list], or null. [replacing] is the index it would take the
     * place of, when it is an edit. Empty or blank outputs, which delete words, aren't allowed yet. */
    fun problem(entry: WordReplacement, list: List<WordReplacement>, replacing: Int = -1): Problem? {
        when (WordsReader.replacementProblem(entry.heard, entry.write)) {
            "hiddenCharacters" -> return Problem.Hidden
            "noLetters" -> return Problem.NoLetters
            "tooLong" -> return Problem.TooLong
            "tooManyWords" -> return Problem.TooManyWords
        }
        if (WordsText.blank(entry.write)) return Problem.Blank
        val key = WordsText.key(entry.heard)
        if (list.withIndex().any { (i, it) -> i != replacing && WordsText.key(it.heard) == key }) return Problem.Duplicate
        if (replacing < 0 && list.size >= maxCount) return Problem.Full
        return null
    }

    /** Keeps only entries that still pass [problem], in order, so a damaged preference never reaches a dictation. */
    fun valid(entries: List<WordReplacement>): List<WordReplacement> {
        val kept = ArrayList<WordReplacement>()
        for (entry in entries) {
            val clean = cleaned(entry.heard, entry.write)
            if (problem(clean, kept) == null) kept += clean
        }
        return kept
    }

    /** One entry a line, heard and output tab-separated and escaped as [DictationFixes.encode] escapes. */
    fun encode(entries: List<WordReplacement>) =
        entries.joinToString("\n") { DictationFixes.escape(it.heard) + "\t" + DictationFixes.escape(it.write) }

    fun decode(text: String): List<WordReplacement> = valid(text.split('\n').mapNotNull { line ->
        val fields = line.split('\t')
        if (fields.size != 2) null else WordReplacement(DictationFixes.unescape(fields[0]), DictationFixes.unescape(fields[1]))
    })

    /** Writes each entry's output in place of its heard words, whole words only, following the original's case
     * unless the output has capitals of its own. */
    fun apply(entries: List<WordReplacement>, text: String): String {
        if (entries.isEmpty() || text.isEmpty()) return text
        val table = LinkedHashMap<List<String>, String>()
        for (entry in entries) {
            val keys = DictationText.words(entry.heard).map { DictationText.key(it.text) }
            if (keys.isEmpty() || keys.size > maxWords || WordsText.blank(entry.write) || table.containsKey(keys)) continue
            table[keys] = entry.write
        }
        return DictationFixes.replace(table, text)
    }

    /** Learned fixes for heard words a Word replacement already has: the person's own entry wins. */
    fun without(fixes: List<DictationFix>, entries: List<WordReplacement>): List<DictationFix> {
        if (entries.isEmpty()) return fixes
        val heard = entries.mapTo(HashSet()) { DictationText.phraseKey(it.heard) }
        return fixes.filter { DictationText.phraseKey(it.heard) !in heard }
    }
}
