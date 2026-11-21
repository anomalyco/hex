package ly.anoma.hex.speech

import java.text.BreakIterator
import java.text.Normalizer
import java.util.Locale

/** iOS's VocabularySpelling and CustomVocabularySettings.sanitize. Rewrites words a transcript
 * likely misheard to the vocabulary's spelling, matching across case, spaces, tabs, a single hyphen
 * and possessive apostrophes, and never changing anything outside a matched span. The listed
 * spelling is kept exactly, even at a sentence start. A term that spells its own boundaries
 * ("iPhone 7", "X-ray") matches them exactly, and a compound ("WeChat") also matches the plain
 * phrase ("we chat"). A word with its own apostrophe only matches a term that has one.
 * Characters are grapheme clusters, as in Swift. */
internal object VocabularySpelling {
    const val maxTermCount = 100
    const val maxTermLength = 64
    private const val maxWindow = 4
    private val quotes = setOf("'", "’", "‘", "\"", "“", "”", "`")

    private class Spelling(val text: String, val apostrophe: Boolean, val compound: Boolean)
    private class Word(val start: Int, val end: Int, val key: String, val apostrophe: Boolean,
        val possessive: String?, val stemKey: String?, val stemApostrophe: Boolean)
    private class Match(val count: Int, val spelling: String, val possessive: String?)

    /** The letters and digits that decide whether two terms are the same word, so a term is
     * judged the same way in settings, the editor and dictation. */
    fun normalizedKey(term: String) = normalize(term)

    /** Trims each term and drops blanks, terms over 64 characters, terms whose key is shorter
     * than two characters, duplicate keys, and anything past the 100-term limit. */
    fun sanitize(rawTerms: List<String>): List<String> {
        val seen = HashSet<String>()
        val result = ArrayList<String>()
        for (raw in rawTerms) {
            val term = raw.trim()
            if (term.isEmpty() || length(term) > maxTermLength) continue
            val key = normalize(term)
            if (length(key) < 2 || !seen.add(key)) continue
            result += term
            if (result.size == maxTermCount) break
        }
        return result
    }

    fun apply(terms: List<String>, text: String): String {
        val glued = HashMap<String, Spelling>()
        val phrases = HashMap<List<String>, Spelling>()
        for (term in terms) {
            val key = normalize(term)
            if (length(key) < 2 || key in glued) continue
            val spelling = Spelling(term, graphemes(term).any(::apostrophe), compound(term))
            glued[key] = spelling
            val keys = termWordKeys(term)
            if (keys.size > 1) phrases[keys] = spelling
        }
        if (glued.isEmpty() || text.isEmpty()) return text
        val words = words(text)
        if (words.isEmpty()) return text

        val out = StringBuilder(text.length)
        var copied = 0
        var i = 0
        while (i < words.size) {
            val m = match(i, words, text, glued, phrases)
            if (m == null) { i++; continue }
            val last = i + m.count - 1
            out.append(text, copied, words[i].start).append(m.spelling).append(m.possessive ?: "")
            copied = words[last].end
            i = last + 1
        }
        return out.append(text, copied, text.length).toString()
    }

    private fun match(i: Int, words: List<Word>, text: String,
                      glued: Map<String, Spelling>, phrases: Map<List<String>, Spelling>): Match? {
        for (count in minOf(maxWindow, words.size - i) downTo 1) {
            if (!joinable(i, count, words, text)) continue
            val window = words.subList(i, i + count)
            val keys = window.map { it.key }
            val apostrophe = window.any { it.apostrophe }
            val last = window.last()
            val possessive = last.possessive
            val stemKeys = last.stemKey?.let { keys.dropLast(1) + it }
            val stemApostrophe = window.dropLast(1).any { it.apostrophe } || last.stemApostrophe

            // A term that spells its own boundaries matches them exactly, even one-letter words.
            if (count > 1) {
                phrases[keys]?.takeIf { accepts(it, apostrophe, false) }?.let { return Match(count, it.text, null) }
                if (stemKeys != null && possessive != null) phrases[stemKeys]
                    ?.takeIf { accepts(it, stemApostrophe, false) }?.let { return Match(count, it.text, possessive) }
            }
            // A glued match never spans a one-letter word: ordinary speech would match by accident.
            if (count > 1 && keys.any { length(it) < 2 }) continue
            glued[keys.joinToString("")]
                ?.takeIf { accepts(it, apostrophe, count > 1) }?.let { return Match(count, it.text, null) }
            if (stemKeys != null && possessive != null) glued[stemKeys.joinToString("")]
                ?.takeIf { accepts(it, stemApostrophe, count > 1) }?.let { return Match(count, it.text, possessive) }
        }
        return null
    }

    // "she'll" never matches "Shell"; a possessive goes through its stem, whose apostrophe is
    // tracked separately. Gluing ordinary words together is only safe for a compound term.
    private fun accepts(s: Spelling, apostrophe: Boolean, needsCompound: Boolean) =
        (!apostrophe || s.apostrophe) && (!needsCompound || s.compound)

    private fun joinable(i: Int, count: Int, words: List<Word>, text: String) = (i + 1 until i + count).all { n ->
        val gap = text.substring(words[n - 1].end, words[n].start)
        gap == "-" || gap.all { it == ' ' || it == '\t' }
    }

    private fun words(text: String): List<Word> {
        val words = ArrayList<Word>()
        val it = BreakIterator.getCharacterInstance().apply { setText(text) }
        var wordStart = -1
        var start = it.first()
        var end = it.next()
        while (end != BreakIterator.DONE) {
            val g = text.substring(start, end)
            if (letter(g) || number(g) || apostrophe(g)) { if (wordStart < 0) wordStart = start }
            else if (wordStart >= 0) { addWord(text, wordStart, start, words); wordStart = -1 }
            start = end
            end = it.next()
        }
        if (wordStart >= 0) addWord(text, wordStart, text.length, words)
        return words
    }

    private fun addWord(text: String, from: Int, to: Int, words: MutableList<Word>) {
        var s = from
        var e = to
        while (s < e && text[s].toString() in quotes) s++
        while (s < e && text[e - 1].toString() in quotes) e--
        if (s >= e) return
        val word = text.substring(s, e)
        val key = normalize(word)
        if (key.isEmpty()) return
        val suffix = listOf("'s", "’s").firstOrNull(word::endsWith)
        val stem = suffix?.let { word.dropLast(it.length) }
        val stemKey = stem?.let(::normalize)?.takeIf(String::isNotEmpty)
        words += Word(s, e, key, word.any { apostrophe(it.toString()) }, suffix.takeIf { stemKey != null }, stemKey,
            stemKey != null && stem.any { apostrophe(it.toString()) })
    }

    /** Splits a term on its own spaces and hyphens, so "iPhone 7", "Plan B" and "X-ray" match
     * one-letter words exactly where the term puts them. */
    private fun termWordKeys(term: String): List<String> {
        val parts = mutableListOf(StringBuilder())
        for (g in graphemes(term)) if (space(g) || g == "-") parts += StringBuilder() else parts.last().append(g)
        return parts.map { normalize(it.toString()) }.filter(String::isNotEmpty)
    }

    /** Said as one word: a space, a hyphen, a digit, or a capital after a lowercase letter
     * ("OpenCode", "iPhone 17"). */
    private fun compound(term: String): Boolean {
        var previous: String? = null
        for (g in graphemes(term)) {
            if (space(g) || g == "-" || number(g)) return true
            if (previous != null && lowercase(previous) && uppercase(g)) return true
            previous = g
        }
        return false
    }

    // Swift compares strings by canonical equivalence, so keys are NFC.
    private fun normalize(text: String) = Normalizer.normalize(
        graphemes(text.lowercase(Locale.ROOT)).filter { letter(it) || number(it) }.joinToString(""), Normalizer.Form.NFC)

    private fun length(s: String) = graphemes(s).size

    private fun graphemes(s: String): List<String> {
        val it = BreakIterator.getCharacterInstance().apply { setText(s) }
        val out = ArrayList<String>()
        var start = it.first()
        var end = it.next()
        while (end != BreakIterator.DONE) { out += s.substring(start, end); start = end; end = it.next() }
        return out
    }

    private fun letter(g: String) = Character.isAlphabetic(g.codePointAt(0))
    private fun number(g: String) = Character.getType(g.codePointAt(0)).toByte().let {
        it == Character.DECIMAL_DIGIT_NUMBER || it == Character.LETTER_NUMBER || it == Character.OTHER_NUMBER
    }
    private fun space(g: String) = g[0].isWhitespace()
    private fun apostrophe(g: String) = g == "'" || g == "’"
    private fun lowercase(g: String) = g.lowercase(Locale.ROOT) == g && g.uppercase(Locale.ROOT) != g
    private fun uppercase(g: String) = g.uppercase(Locale.ROOT) == g && g.lowercase(Locale.ROOT) != g
}
