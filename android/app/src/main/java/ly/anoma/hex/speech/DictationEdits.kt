package ly.anoma.hex.speech

import java.util.Locale

/** A word as people read it: letters and digits, with apostrophes inside it ("don't"), and where it sits in
 * its text. The dictation side of iPhone Hex's DictationEditInference, kept pure so tests need no keyboard. */
internal class DictationWord(val text: String, val start: Int, val end: Int)

internal object DictationText {
    fun words(text: String): List<DictationWord> {
        val words = ArrayList<DictationWord>()
        var wordStart = -1
        var i = 0
        while (i < text.length) {
            val c = text[i]
            val join = c == '\'' || c == '’'
            // A combining mark belongs to the letter before it ("cafe" + U+0301 is one word, not "cafe" and a mark).
            val word = c.isLetterOrDigit() || (join && wordStart >= 0 && i + 1 < text.length && text[i + 1].isLetter()) ||
                (isMark(c) && wordStart >= 0)
            if (word) { if (wordStart < 0) wordStart = i }
            else if (wordStart >= 0) { words += DictationWord(text.substring(wordStart, i), wordStart, i); wordStart = -1 }
            i++
        }
        if (wordStart >= 0) words += DictationWord(text.substring(wordStart), wordStart, text.length)
        return words
    }

    /** How two words compare: lowercased, with a curly apostrophe made straight. */
    fun key(word: String) = word.lowercase(Locale.ROOT).replace('’', '\'')

    /** A phrase's word keys joined by single spaces. */
    fun phraseKey(phrase: String) = words(phrase).joinToString(" ") { key(it.text) }

    /** Letters and digits only, lowercased, so spacing and punctuation don't count. */
    fun letters(text: String) = text.lowercase(Locale.ROOT).filter(Char::isLetterOrDigit)

    /** A combining mark that continues the word before it, never a word on its own. */
    fun isMark(c: Char) = when (Character.getType(c)) {
        Character.NON_SPACING_MARK.toInt(), Character.COMBINING_SPACING_MARK.toInt(), Character.ENCLOSING_MARK.toInt() -> true
        else -> false
    }

    fun distance(a: String, b: String): Int {
        if (a.isEmpty()) return b.length
        if (b.isEmpty()) return a.length
        var previous = IntArray(b.length + 1) { it }
        for (i in 1..a.length) {
            val row = IntArray(b.length + 1)
            row[0] = i
            for (j in 1..b.length) row[j] = minOf(previous[j] + 1, row[j - 1] + 1,
                previous[j - 1] + if (a[i - 1] == b[j - 1]) 0 else 1)
            previous = row
        }
        return previous[b.length]
    }
}

/** A run of old words replaced by a run of new words; either side may be empty. */
internal class WordHunk(val old: IntRange, val new: IntRange) {
    val oldCount get() = old.last - old.first + 1
    val newCount get() = new.last - new.first + 1
}

internal object DictationWordDiff {
    private const val maxCells = 250_000

    /** The changes from [old] to [new], by longest common subsequence of whole words. */
    fun hunks(old: List<String>, new: List<String>): List<WordHunk> {
        var prefix = 0
        while (prefix < old.size && prefix < new.size && old[prefix] == new[prefix]) prefix++
        var suffix = 0
        while (suffix < old.size - prefix && suffix < new.size - prefix &&
            old[old.size - 1 - suffix] == new[new.size - 1 - suffix]) suffix++
        val a = old.subList(prefix, old.size - suffix)
        val b = new.subList(prefix, new.size - suffix)
        if (a.isEmpty() && b.isEmpty()) return emptyList()
        if (a.isEmpty() || b.isEmpty() || a.size * b.size > maxCells)
            return listOf(WordHunk(prefix until prefix + a.size, prefix until prefix + b.size))

        val n = a.size
        val m = b.size
        val table = Array(n + 1) { IntArray(m + 1) }
        for (i in n - 1 downTo 0) for (j in m - 1 downTo 0)
            table[i][j] = if (a[i] == b[j]) table[i + 1][j + 1] + 1 else maxOf(table[i + 1][j], table[i][j + 1])

        val hunks = ArrayList<WordHunk>()
        var i = 0
        var j = 0
        var openI = -1
        var openJ = -1
        while (i < n || j < m) {
            if (i < n && j < m && a[i] == b[j]) {
                if (openI >= 0) { hunks += WordHunk(prefix + openI until prefix + i, prefix + openJ until prefix + j); openI = -1 }
                i++; j++
            } else {
                if (openI < 0) { openI = i; openJ = j }
                if (j >= m || (i < n && table[i + 1][j] >= table[i][j + 1])) i++ else j++
            }
        }
        if (openI >= 0) hunks += WordHunk(prefix + openI until prefix + n, prefix + openJ until prefix + m)
        return hunks
    }
}

internal enum class DictationOutcome { Kept, Edited, Rewritten, Undone, Deleted, Redictated }

/** One word-level change: never more than three words a side. [heard] is the raw transcript's words, or the
 * inserted words when the transcript isn't known; [inserted] went in; [meant] is what the person left. */
internal data class DictationEditPair(val heard: String, val inserted: String, val meant: String)

/** What became of a dictation. [keptHeard] is the raw words the person saw and kept, in runs, so an existing
 * fix can be contradicted; it is never stored. */
internal class DictationEdit(val outcome: DictationOutcome, val pairs: List<DictationEditPair> = emptyList(),
                              val wordCount: Int, val changedWords: Int = 0,
                              val keptHeard: List<List<String>> = emptyList())

internal object DictationEdits {
    const val maxWords = 400
    const val maxPairWords = 3

    /** iPhone Hex's DictationEditClassifier: kept, edited with plausible word pairs, rewritten, deleted or
     * undone. Null when there is nothing to judge (no words, or far too long). */
    fun classify(heard: String?, inserted: String, final: String, undone: Boolean = false): DictationEdit? {
        val ins = DictationText.words(inserted)
        if (ins.isEmpty() || ins.size > maxWords) return null
        val insertedWords = ins.map { it.text }
        val finalWords = DictationText.words(final)
        if (finalWords.isEmpty()) return DictationEdit(if (undone) DictationOutcome.Undone else DictationOutcome.Deleted,
            wordCount = ins.size, changedWords = ins.size)
        if (finalWords.size > maxWords * 2) return null
        val all = DictationWordDiff.hunks(insertedWords, finalWords.map { it.text })
        // Words typed before or after the dictation are new writing, not a fix of it (iOS trims these edges).
        val hunks = all.filterNot { it.old.isEmpty() && !it.new.isEmpty() &&
            (it.old.first == 0 || it.old.first == insertedWords.size) }
        if (hunks.isEmpty()) return DictationEdit(DictationOutcome.Kept, wordCount = ins.size)

        val changed = hunks.sumOf { maxOf(it.oldCount, it.newCount) }
        if (changed > maxOf(1, ins.size / 2))
            return DictationEdit(DictationOutcome.Rewritten, wordCount = ins.size, changedWords = changed)

        val heardWords = heard?.let { DictationText.words(it).map(::wordText) }.orEmpty().ifEmpty { insertedWords }
        val alignment = HeardAlignment(heardWords, insertedWords)
        val startsSentence = insertedWords.firstOrNull()?.firstOrNull()?.isUpperCase() == true
        val pairs = ArrayList<DictationEditPair>()
        for (hunk in hunks) {
            if (hunk.old.isEmpty() || hunk.new.isEmpty() || hunk.oldCount > maxPairWords || hunk.newCount > maxPairWords) continue
            val insertedText = insertedWords.subList(hunk.old.first, hunk.old.last + 1).joinToString(" ")
            var meantText = finalWords.subList(hunk.new.first, hunk.new.last + 1).joinToString(" ") { it.text }
            val heardText = alignment.heard(hunk.old) ?: insertedText
            // Alignment can fold a long spoken number ("one hundred and twenty five") onto one word ("125"):
            // a fix never carries more than three words a side.
            if (DictationText.words(heardText).size > maxPairWords || DictationText.words(meantText).size > maxPairWords) continue
            if (!plausible(insertedText, meantText) && !plausible(heardText, meantText)) continue
            val sentenceStart = if (hunk.new.first == 0) startsSentence
                else startsSentence(finalWords[hunk.new.first], final)
            meantText = sentenceCapital(meantText, sentenceStart)
            if (meantText == insertedText) continue
            pairs += DictationEditPair(heardText, insertedText, meantText)
        }
        return DictationEdit(DictationOutcome.Edited, pairs, ins.size, changed, alignment.keptRuns(hunks, ins.size))
    }

    /** A dictation removed and dictated again soon after: the second one's words that came out differently, when
     * most of it is the same. */
    fun redictation(previousHeard: String?, previousInserted: String, nextInserted: String): DictationEdit? {
        val old = DictationText.words(previousInserted)
        if (old.isEmpty() || old.size > maxWords) return null
        val oldWords = old.map { it.text }
        val next = DictationText.words(nextInserted)
        if (next.isEmpty() || next.size > maxWords * 2)
            return DictationEdit(DictationOutcome.Redictated, wordCount = old.size, changedWords = old.size)
        val hunks = DictationWordDiff.hunks(oldWords.map(DictationText::key), next.map { DictationText.key(it.text) })
        val changed = hunks.sumOf { maxOf(it.oldCount, it.newCount) }
        if (changed > maxOf(1, old.size / 2))
            return DictationEdit(DictationOutcome.Redictated, wordCount = old.size, changedWords = changed)

        val heardWords = previousHeard?.let { DictationText.words(it).map(::wordText) }.orEmpty().ifEmpty { oldWords }
        val alignment = HeardAlignment(heardWords, oldWords)
        val startsSentence = next.firstOrNull()?.text?.firstOrNull()?.isUpperCase() == true
        val pairs = ArrayList<DictationEditPair>()
        for (hunk in hunks) {
            if (hunk.old.isEmpty() || hunk.new.isEmpty() || hunk.oldCount > maxPairWords || hunk.newCount > maxPairWords) continue
            val insertedText = oldWords.subList(hunk.old.first, hunk.old.last + 1).joinToString(" ")
            val heardText = alignment.heard(hunk.old) ?: insertedText
            var meantText = next.subList(hunk.new.first, hunk.new.last + 1).joinToString(" ") { it.text }
            if (DictationText.words(heardText).size > maxPairWords || DictationText.words(meantText).size > maxPairWords) continue
            if (!plausible(insertedText, meantText) && !plausible(heardText, meantText)) continue
            val sentenceStart = if (hunk.new.first == 0) startsSentence
                else startsSentence(next[hunk.new.first], nextInserted)
            meantText = sentenceCapital(meantText, sentenceStart)
            if (meantText == insertedText) continue
            pairs += DictationEditPair(heardText, insertedText, meantText)
        }
        return DictationEdit(DictationOutcome.Redictated, pairs, old.size, changed)
    }

    /** Whether [meant] could be a fix of [heard] rather than different words altogether. */
    fun plausible(heard: String, meant: String): Boolean {
        val a = DictationText.letters(heard)
        val b = DictationText.letters(meant)
        if (a.isEmpty() || b.isEmpty()) return false
        if (a == b) return true
        return DictationText.distance(a, b) <= maxOf(2, maxOf(a.length, b.length) / 2)
    }

    private fun wordText(word: DictationWord) = word.text

    private fun startsSentence(word: DictationWord, text: String): Boolean {
        var i = word.start
        while (i > 0) {
            i--
            val c = text[i]
            if (c == '\n' || c == '\r') return true
            if (c.isWhitespace() || c in "\"“”'‘’([") continue
            return c in ".!?…"
        }
        return true
    }

    /** A capital that only marks a sentence start isn't part of how the person spells the word. */
    private fun sentenceCapital(meant: String, sentenceStart: Boolean): String {
        val first = meant.firstOrNull() ?: return meant
        if (!sentenceStart || !first.isUpperCase() || meant.drop(1).any(Char::isUpperCase)) return meant
        val firstWord = DictationText.words(meant).firstOrNull()?.let { DictationText.key(it.text) } ?: ""
        if (firstWord in setOf("i", "i'm", "i'll", "i've", "i'd")) return meant
        return first.lowercaseChar() + meant.drop(1)
    }
}

/** Which raw words each inserted word came from, by a case-insensitive diff. */
private class HeardAlignment(private val heard: List<String>, inserted: List<String>) {
    private val source = arrayOfNulls<IntRange>(inserted.size)
    private val same = BooleanArray(inserted.size)

    init {
        val hunks = DictationWordDiff.hunks(heard.map(DictationText::key), inserted.map(DictationText::key))
        var h = 0
        var i = 0
        for (hunk in hunks) {
            while (i < hunk.new.first && h < hunk.old.first) {
                source[i] = h until h + 1
                same[i] = true
                h++; i++
            }
            if (!hunk.old.isEmpty()) for (k in hunk.new) source[k] = hunk.old
            h = hunk.old.last + 1
            i = hunk.new.last + 1
        }
        while (i < inserted.size && h < heard.size) {
            source[i] = h until h + 1
            same[i] = true
            h++; i++
        }
    }

    fun heard(range: IntRange): String? {
        if (range.isEmpty()) return null
        var lower = Int.MAX_VALUE
        var upper = Int.MIN_VALUE
        for (index in range) {
            val from = source[index] ?: return null
            lower = minOf(lower, from.first)
            upper = maxOf(upper, from.last + 1)
        }
        return heard.subList(lower, upper).joinToString(" ")
    }

    /** Runs of raw keys for inserted words that are exactly as heard and untouched by the edit. */
    fun keptRuns(hunks: List<WordHunk>, insertedCount: Int): List<List<String>> {
        val touched = BooleanArray(insertedCount)
        for (hunk in hunks) for (index in hunk.old) if (index in 0 until insertedCount) touched[index] = true
        val runs = ArrayList<List<String>>()
        var run = ArrayList<String>()
        for (index in 0 until insertedCount) {
            val from = source[index]
            if (!touched[index] && same[index] && from != null) run += DictationText.key(heard[from.first])
            else if (run.isNotEmpty()) { runs += run; run = ArrayList() }
        }
        if (run.isNotEmpty()) runs += run
        return runs
    }
}
