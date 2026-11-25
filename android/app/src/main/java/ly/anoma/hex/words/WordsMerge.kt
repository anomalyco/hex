package ly.anoma.hex.words

import ly.anoma.hex.speech.WordReplacement
import java.text.Normalizer

/** A phone's two lists as an import reads and changes them: Custom vocabulary's terms and Word replacements. */
internal data class WordsLists(val terms: List<String>, val replacements: List<WordReplacement>)

internal data class WordsClash(val path: String, val heard: String, val yours: String, val theirs: String)

internal data class WordsSummary(val addedTerms: Int = 0, val alreadyHaveTerms: Int = 0, val addedReplacements: Int = 0,
                                 val changedReplacements: Int = 0, val keptYours: Int = 0,
                                 val alreadyHaveReplacements: Int = 0) {
    val nothingNew get() = addedTerms == 0 && addedReplacements == 0 && changedReplacements == 0
}

/** What Add would do: the lists after, the counts, what this phone can't use ({path, reason}) and each clash. */
internal data class WordsMerged(val after: WordsLists, val summary: WordsSummary, val notApplied: List<WordsSkip>,
                                val clashes: List<WordsClash>)

/** docs/specs/hex-words.md's merge table, the iPhone and Android column. Pure: it never touches the learned
 * fixes, and it only ever adds, or replaces an output the person chose to replace. */
internal object WordsMerge {
    const val PHONE_TERMS = 100
    const val PHONE_REPLACEMENTS = 1000

    fun phone(before: WordsLists, incoming: WordsRead.Read, vocabulary: Boolean = true, replacements: Boolean = true,
              useTheirs: Set<String> = emptySet(), maxTerms: Int = PHONE_TERMS,
              maxReplacements: Int = PHONE_REPLACEMENTS): WordsMerged {
        var summary = WordsSummary()
        val notApplied = ArrayList<WordsSkip>()
        val clashes = ArrayList<WordsClash>()
        val terms = before.terms.toMutableList()
        val list = before.replacements.toMutableList()
        // Merging runs in file order: the vocabulary, then the file's replacements, then the spellings that become
        // replacements. On a phone the spellings belong to the Replacements part.
        if (vocabulary) for ((index, entry) in incoming.vocabulary.withIndex()) {
            val path = "vocabulary[$index]"
            val key = WordsText.termKey(entry.term)
            when {
                WordsText.length(key) < 2 -> notApplied += WordsSkip(path, "tooShortForThisDevice")
                terms.any { WordsText.termKey(it) == key } -> summary = summary.copy(alreadyHaveTerms = summary.alreadyHaveTerms + 1)
                terms.size >= maxTerms -> notApplied += WordsSkip(path, "overDeviceLimit")
                else -> { terms += entry.term; summary = summary.copy(addedTerms = summary.addedTerms + 1) }
            }
        }
        if (replacements) {
            val queue = ArrayList<Triple<String, String, String>>()
            incoming.replacements.forEachIndexed { index, it -> queue += Triple("replacements[$index]", it.heard, it.write) }
            incoming.vocabulary.forEachIndexed { index, entry ->
                entry.soundsLike.forEachIndexed { at, spelling -> queue += Triple("vocabulary[$index].soundsLike[$at]", spelling, entry.term) }
            }
            val seen = HashSet<String>()
            for ((path, heard, write) in queue) {
                val key = WordsText.key(WordsText.line(heard))
                if (WordsText.blank(write)) { notApplied += WordsSkip(path, "deletionNotSupported"); continue }
                if (!seen.add(key)) { notApplied += WordsSkip(path, "duplicate"); continue }
                val index = list.indexOfFirst { WordsText.key(WordsText.line(it.heard)) == key }
                if (index >= 0) {
                    val yours = list[index]
                    if (Normalizer.normalize(yours.write, Normalizer.Form.NFC) == write) {
                        summary = summary.copy(alreadyHaveReplacements = summary.alreadyHaveReplacements + 1)
                        continue
                    }
                    clashes += WordsClash(path, yours.heard, yours.write, write)
                    if (path in useTheirs) {
                        list[index] = WordReplacement(yours.heard, write)
                        summary = summary.copy(changedReplacements = summary.changedReplacements + 1)
                    } else summary = summary.copy(keptYours = summary.keptYours + 1)
                    continue
                }
                if (list.size >= maxReplacements) { notApplied += WordsSkip(path, "overDeviceLimit"); continue }
                list += WordReplacement(heard, write)
                summary = summary.copy(addedReplacements = summary.addedReplacements + 1)
            }
        }
        return WordsMerged(WordsLists(terms, list), summary, notApplied, clashes)
    }
}
