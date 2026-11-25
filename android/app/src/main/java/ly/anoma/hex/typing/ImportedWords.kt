package ly.anoma.hex.typing

import android.content.Context
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.util.zip.ZipInputStream
import ly.anoma.hex.words.Json
import ly.anoma.hex.words.JsonRefusal
import ly.anoma.hex.words.StrictJson

/** Labs › Keyboard › SwiftKey words (#2802): the words another keyboard learned, read from its export and
 * kept on this phone beside Android's personal dictionary. SwiftKey's data portal (View data › Export all)
 * gives a ZIP whose `services/sync_words.json` holds `{"terms": [...]}`; the ZIP, that JSON, or any plain
 * list of words imports. Only single words are kept (letters and apostrophes, 2 to [TypingWord.MAX_LENGTH]
 * letters): numbers, addresses, links and phrases are left out. */
internal object ImportedWords {
    /** The most a file (or a ZIP's word list) may be. */
    const val MAX_BYTES = 16 * 1024 * 1024
    /** The most words Hex keeps. */
    const val LIMIT = 50_000

    /** What one import did: words new to Hex, words it already had, and new words left out because the
     * list was full. */
    data class Result(val words: List<String>, val added: Int, val already: Int, val full: Int)

    /** The words in a SwiftKey export: its ZIP, its JSON, or a plain list. Null when the file holds none
     * Hex can read (an empty list, a ZIP without `sync_words.json`, malformed JSON). */
    fun parse(bytes: ByteArray): List<String>? = if (isZip(bytes)) zip(bytes) else text(bytes)

    private fun isZip(bytes: ByteArray) = bytes.size >= 2 && bytes[0] == 'P'.code.toByte() && bytes[1] == 'K'.code.toByte()

    /** A file's words read as JSON or plain text, never as a ZIP: a ZIP inside one holds no words. */
    private fun text(bytes: ByteArray): List<String>? {
        if (isZip(bytes)) return null
        val text = bytes.decodeToString().removePrefix("\uFEFF")
        val first = text.firstOrNull { !it.isWhitespace() }
        val words = if (first == '{' || first == '[') json(text) ?: return null else plain(text)
        return words.takeIf { it.isNotEmpty() }
    }

    private fun zip(bytes: ByteArray): List<String>? {
        ZipInputStream(ByteArrayInputStream(bytes)).use { zip ->
            while (true) {
                val entry = zip.nextEntry ?: return null
                if (entry.isDirectory || !entry.name.substringAfterLast('/').equals("sync_words.json", ignoreCase = true)) continue
                return text(bounded(zip) ?: return null)
            }
        }
    }

    /** All of [input], or null past [MAX_BYTES]. */
    internal fun bounded(input: InputStream): ByteArray? {
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(64 * 1024)
        while (true) {
            val count = input.read(buffer)
            if (count < 0) return out.toByteArray()
            if (out.size() + count > MAX_BYTES) return null
            out.write(buffer, 0, count)
        }
    }

    private fun json(text: String): List<String>? {
        val value = try { StrictJson.parse(text, maxDepth = 4) } catch (_: JsonRefusal) { return null }
        val terms = when (value) {
            is Json.Obj -> value.members["terms"] as? Json.Arr ?: return null
            is Json.Arr -> value
            else -> return null
        }
        return terms.items.mapNotNull { (it as? Json.Str)?.value?.trim()?.let { s -> word(s) } }
    }

    // A list's other columns (counts, dates) are not words, so whatever splits them apart doesn't matter.
    private fun plain(text: String) = text.split(Regex("[\\s,;]+")).mapNotNull { word(it) }

    /** The word as Hex keeps it (a curly apostrophe made straight), or null when it isn't a single word. */
    fun word(text: String): String? {
        val w = text.replace('\u2019', '\'')
        if (w.length !in 2..TypingWord.MAX_LENGTH || w.any { !it.isLetter() && it != '\'' }) return null
        return w.takeIf { it.count(Char::isLetter) >= 2 }
    }

    /** [incoming] added to [existing], which stays as it is: one copy of each word whatever its case, a
     * lowercase spelling preferred among the new ones (so learned sentence starts don't force capitals),
     * up to [LIMIT]. */
    fun merge(existing: List<String>, incoming: List<String>): Result {
        val have = existing.mapTo(HashSet()) { it.lowercase() }
        val chosen = LinkedHashMap<String, String>()
        for (w in incoming) {
            val lower = w.lowercase()
            if (lower in have) continue
            val before = chosen[lower]
            if (before == null || (w == lower && before != lower)) chosen[lower] = w
        }
        val already = incoming.map { it.lowercase() }.distinct().count { it in have }
        val room = (LIMIT - existing.size).coerceAtLeast(0)
        val added = chosen.values.take(room)
        return Result(existing + added, added.size, already, chosen.size - added.size)
    }

    private fun file(context: Context) = File(context.filesDir, "imported-words.txt")

    /** The imported words, one per line; none when nothing was imported or the file can't be read. */
    fun read(context: Context): List<String> = try {
        file(context).takeIf { it.isFile }?.readLines()?.mapNotNull { word(it) }?.take(LIMIT).orEmpty()
    } catch (_: Exception) { emptyList() }

    /** Saves [words] and tells the keyboard, which reads them again. */
    fun save(context: Context, words: List<String>) {
        val target = file(context)
        val temp = File(context.filesDir, "imported-words.tmp")
        temp.writeText(words.joinToString("\n"))
        if (!temp.renameTo(target)) { target.delete(); temp.renameTo(target) }
        changed(context, words.size)
    }

    fun clear(context: Context) { file(context).delete(); changed(context, 0) }

    /** How many words are kept, as saved with them, so Settings never reads the list to show it. */
    fun count(context: Context) = prefs(context).getInt(TypingPreferences.IMPORTED_WORDS_COUNT, 0)

    private fun prefs(context: Context) = context.getSharedPreferences("typing", Context.MODE_PRIVATE)

    private fun changed(context: Context, count: Int) {
        val prefs = prefs(context)
        prefs.edit().putInt(TypingPreferences.IMPORTED_WORDS_COUNT, count)
            .putInt(TypingPreferences.IMPORTED_WORDS, prefs.getInt(TypingPreferences.IMPORTED_WORDS, 0) + 1).apply()
    }
}
