package ly.anoma.hex.words

import ly.anoma.hex.speech.WordReplacement
import java.util.Locale

/** A learned dictation fix as the export sees it; [inUse] is applied and not switched off in Settings. */
internal data class LearnedFix(val heard: String, val meant: String, val inUse: Boolean)

internal class WordsWritten(val bytes: ByteArray, val terms: Int, val replacements: Int, val notExported: List<WordsSkip>)

/** Writes a phone's Hex words file (docs/specs/hex-words.md, "Writing"): Custom vocabulary's terms, the Word
 * replacements and, with [includeLearned], the learned fixes in use marked `learned: true`. Text is written
 * cleaned, and every entry a reader would skip is left out by the reader's own rules, in its order, and counted.
 * UTF-8 without a byte order mark, non-ASCII as itself rather than `\u` escapes. */
internal object WordsWriter {
    const val APP = "Hex for Android"
    const val FILE_NAME = "hex-words.hexwords"
    const val MIME_TYPE = "application/vnd.anomaly.hex-words+json"

    fun phone(appVersion: String, terms: List<String>, replacements: List<WordReplacement>, learned: List<LearnedFix>,
              includeLearned: Boolean): WordsWritten {
        val notExported = ArrayList<WordsSkip>()
        val vocabulary = ArrayList<String>()
        val termKeys = HashSet<String>()
        for ((index, raw) in terms.withIndex()) {
            val path = "vocabulary[$index]"
            val term = WordsText.line(raw)
            val problem = if (vocabulary.size >= WordsReader.MAX_TERMS) "overLimit"
                else WordsReader.lineProblem(term, WordsReader.MAX_TERM_LENGTH, Int.MAX_VALUE)
                    ?: if (WordsText.key(term) in termKeys) "duplicate" else null
            if (problem != null) { notExported += WordsSkip(path, problem); continue }
            termKeys += WordsText.key(term)
            vocabulary += term
        }
        val sources = ArrayList<Triple<String, WordReplacement, Boolean>>()
        replacements.forEachIndexed { index, it -> sources += Triple("replacements[$index]", it, false) }
        if (includeLearned) learned.forEachIndexed { index, fix ->
            if (fix.inUse) sources += Triple("learnedFixes[$index]", WordReplacement(fix.heard, fix.meant), true)
        }
        val written = ArrayList<WordsReplacement>()
        val heardKeys = HashSet<String>()
        for ((path, entry, isLearned) in sources) {
            val heard = WordsText.line(entry.heard)
            val write = WordsText.output(entry.write)
            val problem = if (written.size >= WordsReader.MAX_REPLACEMENTS) "overLimit"
                else WordsReader.replacementProblem(heard, write)
                    ?: if (WordsText.key(heard) in heardKeys) "duplicate" else null
            if (problem != null) { notExported += WordsSkip(path, problem); continue }
            heardKeys += WordsText.key(heard)
            written += WordsReplacement(heard, write, isLearned)
        }
        val version = WordsText.line(appVersion).takeIf {
            it.isNotEmpty() && !WordsText.lineHidden(it) && WordsText.length(it) <= WordsReader.MAX_APP_VERSION_LENGTH
        }
        val json = StringBuilder()
        json.append("{\n  \"format\": \"").append(WordsReader.FORMAT).append("\",\n  \"version\": 1,\n")
        json.append("  \"madeBy\": {\"app\": ").append(quote(APP))
        if (version != null) json.append(", \"version\": ").append(quote(version))
        json.append("},\n  \"vocabulary\": [")
        vocabulary.forEachIndexed { index, term ->
            json.append(if (index == 0) "\n" else ",\n").append("    {\"term\": ").append(quote(term)).append('}')
        }
        json.append(if (vocabulary.isEmpty()) "],\n" else "\n  ],\n").append("  \"replacements\": [")
        written.forEachIndexed { index, it ->
            json.append(if (index == 0) "\n" else ",\n").append("    {\"heard\": ").append(quote(it.heard))
                .append(", \"write\": ").append(quote(it.write))
            if (it.learned) json.append(", \"learned\": true")
            json.append('}')
        }
        json.append(if (written.isEmpty()) "]\n}\n" else "\n  ]\n}\n")
        return WordsWritten(json.toString().toByteArray(Charsets.UTF_8), vocabulary.size, written.size, notExported)
    }

    private fun quote(text: String): String {
        val out = StringBuilder(text.length + 2).append('"')
        for (c in text) when {
            c == '"' -> out.append("\\\"")
            c == '\\' -> out.append("\\\\")
            c == '\n' -> out.append("\\n")
            c == '\t' -> out.append("\\t")
            c < ' ' -> out.append("\\u").append("%04x".format(Locale.ROOT, c.code))
            else -> out.append(c)
        }
        return out.append('"').toString()
    }
}
