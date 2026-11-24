package ly.anoma.hex.speech

import android.content.Context
import java.util.Locale

/** A learned dictation fix: words the speech model keeps getting wrong, what the person meant, how often they
 * fixed it that way, and how often they kept it or fixed it back. iPhone Hex's TypingJournalDictationFix. */
internal data class DictationFix(val heard: String, val meant: String, val count: Int, val contradictions: Int,
                                 val lastAt: Long = 0) {
    val id get() = DictationFixes.id(heard, meant)
}

/** Labs › Learn from my fixes: the rules that decide when a fixed word pair is applied, and the text rules that
 * apply it. Pure except [encode]/[decode], which keep the list in one escaped line per fix. */
internal object DictationFixes {
    const val maxRecords = 1000
    private const val minimumCount = 2
    private val joiners = setOf('-', '_', '/', '\\', '@', '#', '&', '+', '=', '~', '*', '.')

    fun id(heard: String, meant: String) = DictationText.phraseKey(heard) + " → " + DictationText.phraseKey(meant)

    /** Folds one dictation into the records: a count for each plausible fix, then a contradiction for every fix
     * whose heard words were kept, changed to something else, or fixed the other way round. */
    fun merge(records: List<DictationFix>, edit: DictationEdit, at: Long): List<DictationFix> {
        val out = records.toMutableList()
        for (pair in edit.pairs) {
            val heardKey = DictationText.phraseKey(pair.heard)
            val meantKey = DictationText.phraseKey(pair.meant)
            if (heardKey.isEmpty() || meantKey.isEmpty() || pair.meant.isBlank()) continue
            if (pair.meant == pair.heard) continue
            // A case change is not a fix, unless the capital is part of the spelling.
            if (heardKey == meantKey && pair.meant.none(Char::isUpperCase)) continue
            // A folded long phrase (a spoken number) is no fix; the classifier already leaves these out.
            if (DictationText.words(heardKey).size > DictationEdits.maxPairWords ||
                DictationText.words(meantKey).size > DictationEdits.maxPairWords) continue
            // The meant side matches regardless of case, so "claude" and "Claude" stay one record.
            val index = out.indexOfFirst { DictationText.phraseKey(it.heard) == heardKey && DictationText.phraseKey(it.meant) == meantKey }
            if (index >= 0) {
                val old = out[index]
                // The latest spelling wins, as it is the one the person last typed.
                out[index] = DictationFix(old.heard, pair.meant.trim(), old.count + 1, old.contradictions, at)
            } else {
                out += DictationFix(heardKey, pair.meant.trim(), 1, 0, at)
            }
        }
        for (index in out.indices) {
            val fix = out[index]
            if (contradicts(edit, fix)) out[index] = DictationFix(fix.heard, fix.meant, fix.count, fix.contradictions + 1, fix.lastAt)
        }
        if (out.size > maxRecords) {
            val drop = out.indices.sortedWith(compareBy({ out[it].count }, { out[it].lastAt }))
                .take(out.size - maxRecords).sortedDescending()
            for (index in drop) out.removeAt(index)
        }
        return out
    }

    /** Fixes seen at least twice, never contradicted, and the only meaning for those heard words. */
    fun learned(records: List<DictationFix>): List<DictationFix> {
        class Group(val heard: String, var meant: String, var count: Int = 0, var contradictions: Int = 0,
                    var lastAt: Long = 0)
        val groups = LinkedHashMap<String, Group>()
        for (record in records) {
            val heardKey = DictationText.phraseKey(record.heard)
            val meantKey = DictationText.phraseKey(record.meant)
            if (heardKey.isEmpty() || meantKey.isEmpty()) continue
            if (DictationText.words(heardKey).size > DictationEdits.maxPairWords ||
                DictationText.words(meantKey).size > DictationEdits.maxPairWords) continue
            val group = groups.getOrPut(heardKey + " → " + meantKey) { Group(heardKey, record.meant) }
            group.count += record.count
            group.contradictions += record.contradictions
            group.lastAt = maxOf(group.lastAt, record.lastAt)
            if (record.meant.any(Char::isUpperCase)) group.meant = record.meant
        }
        val meanings = HashMap<String, Int>()
        for (group in groups.values) meanings[group.heard] = (meanings[group.heard] ?: 0) + 1
        return groups.values.mapNotNull { group ->
            if (group.count < minimumCount || group.contradictions != 0 || meanings[group.heard] != 1) null
            else DictationFix(group.heard, group.meant, group.count, 0, group.lastAt)
        }
    }

    /** The fixes Hex applies, with the ones switched off in Settings left out. */
    fun active(fixes: List<DictationFix>, disabled: Set<String>) = fixes.filter { it.id !in disabled }

    /** Learned spellings with capitals ("Claude", "OpenCode"), which formatting must not undo. */
    fun spellings(fixes: List<DictationFix>) = fixes.map { it.meant.trim() }.filter { it.any(Char::isUpperCase) }

    /** Replaces each heard word or phrase, whole words only and never inside a longer token. The meant spelling
     * is kept when it has capitals; otherwise it follows the original's case. */
    fun apply(fixes: List<DictationFix>, text: String): String {
        if (fixes.isEmpty() || text.isEmpty()) return text
        val table = HashMap<List<String>, String>()
        var longest = 1
        for (fix in fixes) {
            val keys = DictationText.words(fix.heard).map { DictationText.key(it.text) }
            val meant = fix.meant.trim()
            if (keys.isEmpty() || keys.size > DictationEdits.maxPairWords ||
                DictationText.words(meant).size > DictationEdits.maxPairWords || meant.isEmpty() || table.containsKey(keys)) continue
            table[keys] = meant
            longest = maxOf(longest, keys.size)
        }
        if (table.isEmpty()) return text
        val words = DictationText.words(text)
        val out = StringBuilder(text.length)
        var copied = 0
        var index = 0
        while (index < words.size) {
            var matched = 0
            for (count in minOf(longest, words.size - index) downTo 1) {
                val window = words.subList(index, index + count)
                if (!joinedBySpaces(window, text)) continue
                val keys = window.map { DictationText.key(it.text) }
                val meant = table[keys] ?: continue
                val start = window.first().start
                val end = window.last().end
                if (!bounded(start, end, text)) continue
                out.append(text, copied, start)
                out.append(matchCase(meant, text.substring(start, end)))
                copied = end
                matched = count
                break
            }
            index += maxOf(1, matched)
        }
        return out.append(text, copied, text.length).toString()
    }

    /** The custom vocabulary first, then the meant words of learned fixes, deduplicated and within the
     * vocabulary's 100-term limit. Either list alone still gives hints. */
    fun hints(vocabulary: List<String>, fixes: List<DictationFix>): List<String> {
        val seen = HashSet<String>()
        val hints = ArrayList<String>()
        for (term in vocabulary + fixes.map { it.meant }) {
            val trimmed = term.trim()
            val key = VocabularySpelling.normalizedKey(trimmed)
            if (key.length < 2 || !seen.add(key)) continue
            hints += trimmed
            if (hints.size == VocabularySpelling.maxTermCount) break
        }
        return hints
    }

    /** One fix a line, tab-separated: heard, meant, count, contradictions, lastAt. Backslashes, tabs and newlines
     * in a word are escaped, so the list survives a SharedPreferences string. */
    fun encode(records: List<DictationFix>) = records.joinToString("\n") {
        listOf(it.heard, it.meant, it.count.toString(), it.contradictions.toString(), it.lastAt.toString())
            .joinToString("\t") { value -> escape(value) }
    }

    fun decode(text: String): List<DictationFix> = text.split('\n').mapNotNull { line ->
        if (line.isEmpty()) return@mapNotNull null
        val fields = line.split('\t')
        if (fields.size != 5) return@mapNotNull null
        val heard = unescape(fields[0])
        val meant = unescape(fields[1])
        val count = fields[2].toIntOrNull() ?: return@mapNotNull null
        val contradictions = fields[3].toIntOrNull() ?: return@mapNotNull null
        val at = fields[4].toLongOrNull() ?: return@mapNotNull null
        if (heard.isBlank() || meant.isBlank() || count < 1 || contradictions < 0) null
        else DictationFix(heard, meant, count, contradictions, at)
    }

    private fun contradicts(edit: DictationEdit, fix: DictationFix): Boolean {
        val heardKey = DictationText.phraseKey(fix.heard)
        val meantKey = DictationText.phraseKey(fix.meant)
        val heardRun = heardKey.split(' ')
        if (heardKey != meantKey && edit.keptHeard.any { containsRun(it, heardRun) }) return true
        for (pair in edit.pairs) {
            val pairHeard = DictationText.phraseKey(pair.heard)
            val pairInserted = DictationText.phraseKey(pair.inserted)
            val pairMeant = DictationText.phraseKey(pair.meant)
            val extended = DictationText.letters(pair.meant).length > DictationText.letters(fix.meant).length &&
                DictationText.letters(pair.meant).startsWith(DictationText.letters(fix.meant))
            val changedFix = pairHeard == heardKey && pairInserted == meantKey && pair.meant != fix.meant && !extended
            val reversed = pairMeant == heardKey && pairMeant != meantKey &&
                (pairInserted == meantKey || pairHeard == meantKey)
            if (changedFix || reversed) return true
        }
        return false
    }

    private fun containsRun(run: List<String>, phrase: List<String>): Boolean {
        if (phrase.isEmpty() || phrase.size > run.size) return false
        for (start in 0..(run.size - phrase.size)) if (run.subList(start, start + phrase.size) == phrase) return true
        return false
    }

    private fun joinedBySpaces(window: List<DictationWord>, text: String) = window.indices.drop(1).all { index ->
        val gap = text.substring(window[index - 1].end, window[index].start)
        gap.isNotEmpty() && gap.all { it == ' ' || it == '\t' }
    }

    /** Not part of a longer token: no joiner right before, and none right after except a sentence-ending
     * period. */
    private fun bounded(start: Int, end: Int, text: String): Boolean {
        if (start > 0 && (text[start - 1] in joiners || DictationText.isMark(text[start - 1]))) return false
        if (end >= text.length) return true
        val next = text[end]
        // A combining mark after the match makes it part of a longer word ("cafe" in "cafe" + U+0301).
        if (next !in joiners && !DictationText.isMark(next)) return true
        if (next != '.') return false
        if (end + 1 >= text.length) return true
        val following = text[end + 1]
        return following.isWhitespace() || following == '.' || following in "\"”’')]"
    }

    private fun matchCase(meant: String, like: String): String {
        val letters = like.filter(Char::isLetter)
        if (letters.length >= 2 && letters == letters.uppercase(Locale.ROOT) && letters != letters.lowercase(Locale.ROOT))
            return meant.uppercase(Locale.ROOT)
        if (meant.any(Char::isUpperCase)) return meant
        val first = like.firstOrNull(Char::isLetter)
        return if (first?.isUpperCase() == true) meant.replaceFirstChar { it.uppercaseChar() } else meant
    }

    private fun escape(value: String) = value.replace("\\", "\\\\").replace("\n", "\\n")
        .replace("\t", "\\t").replace("\r", "\\r")

    private fun unescape(value: String): String {
        val out = StringBuilder(value.length)
        var i = 0
        while (i < value.length) {
            val c = value[i]
            if (c != '\\' || i + 1 >= value.length) { out.append(c); i++; continue }
            when (value[i + 1]) {
                'n' -> out.append('\n')
                't' -> out.append('\t')
                'r' -> out.append('\r')
                '\\' -> out.append('\\')
                else -> out.append(value[i + 1])
            }
            i += 2
        }
        return out.toString()
    }
}

/** The fixes on this phone: one SharedPreferences string, plus the ids turned off in Settings. */
internal class DictationFixStore(context: Context) {
    private val prefs = context.getSharedPreferences("speech", Context.MODE_PRIVATE)
    private val records get() = DictationFixes.decode(prefs.getString(FIXES, null).orEmpty())

    fun learned() = DictationFixes.learned(records)

    /** Whether this phone keeps any fix at all, learned or still below the threshold. */
    fun hasRecords() = records.isNotEmpty()

    fun disabled(): Set<String> = prefs.getString(DISABLED, null).orEmpty().split('\n')
        .filter(String::isNotEmpty).toSet()

    /** The fixes Hex applies, with the ones turned off in Settings left out. */
    fun active() = DictationFixes.active(learned(), disabled())

    fun record(edit: DictationEdit) {
        val merged = DictationFixes.merge(records, edit, System.currentTimeMillis())
        prefs.edit().putString(FIXES, DictationFixes.encode(merged)).apply()
    }

    fun disable(id: String, off: Boolean) {
        val ids = disabled().toMutableSet()
        if (off) ids += id else ids -= id
        val text = ids.joinToString("\n")
        val editor = prefs.edit()
        if (text.isEmpty()) editor.remove(DISABLED) else editor.putString(DISABLED, text)
        editor.apply()
    }

    fun clear() { prefs.edit().remove(FIXES).remove(DISABLED).apply() }

    private companion object {
        const val FIXES = "dictationFixes"
        const val DISABLED = "dictationFixesOff"
    }
}
