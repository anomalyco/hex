package ly.anoma.hex.words

import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.nio.charset.StandardCharsets

internal data class WordsTerm(val term: String, val soundsLike: List<String> = emptyList())
internal data class WordsReplacement(val heard: String, val write: String, val learned: Boolean = false)
/** A part of a file, or of a merge or an export, left out: where it is and one of the spec's reason names. */
internal data class WordsSkip(val path: String, val reason: String)

internal sealed interface WordsRead {
    /** Nothing of the file is kept or shown. */
    data class Refused(val reason: String) : WordsRead
    data class Read(val version: Long, val newerVersion: Boolean, val madeByApp: String?, val madeByVersion: String?,
                    val vocabulary: List<WordsTerm>, val replacements: List<WordsReplacement>,
                    val skipped: List<WordsSkip>) : WordsRead
}

/** Reads a Hex words file's bytes exactly as docs/specs/hex-words.md's "Reading, step by step" says. Pure: no
 * file, log or network. */
internal object WordsReader {
    const val MAX_BYTES = 8 * 1024 * 1024
    const val FORMAT = "hex.words"
    const val VERSION = 1L
    const val MAX_TERMS = 500
    const val MAX_TERM_LENGTH = 64
    const val MAX_SPELLINGS = 16
    const val MAX_SPELLING_LENGTH = 64
    const val MAX_WORDS = 8
    const val MAX_REPLACEMENTS = 1000
    const val MAX_HEARD_LENGTH = 128
    const val MAX_WRITE_LENGTH = 1000
    const val MAX_APP_LENGTH = 64
    const val MAX_APP_VERSION_LENGTH = 32

    private val known = setOf("format", "version", "madeBy", "vocabulary", "replacements")

    fun read(bytes: ByteArray): WordsRead {
        if (bytes.size > MAX_BYTES) return WordsRead.Refused("tooLarge")
        val bom = bytes.size >= 3 && bytes[0] == 0xEF.toByte() && bytes[1] == 0xBB.toByte() && bytes[2] == 0xBF.toByte()
        // Java's strict decoder refuses bad bytes, overlong forms and surrogates encoded in UTF-8.
        val text = try {
            StandardCharsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(bytes, if (bom) 3 else 0, bytes.size - if (bom) 3 else 0)).toString()
        } catch (_: CharacterCodingException) {
            return WordsRead.Refused("notUTF8")
        }
        if (text.all { it == ' ' || it == '\t' || it == '\n' || it == '\r' }) return WordsRead.Refused("empty")
        val root = try { StrictJson.parse(text) } catch (refusal: JsonRefusal) { return WordsRead.Refused(refusal.reason) }
        if (root !is Json.Obj) return WordsRead.Refused("notAnObject")
        val members = root.members
        if ((members["format"] as? Json.Str)?.value != FORMAT) return WordsRead.Refused("notHexWords")
        val version = version(members["version"]) ?: return WordsRead.Refused("badVersion")

        val skipped = ArrayList<WordsSkip>()
        for (key in members.keys) if (key !in known) skipped += WordsSkip(key, "unknownField")
        var app: String? = null
        var appVersion: String? = null
        members["madeBy"]?.let { madeBy ->
            if (madeBy !is Json.Obj) { skipped += WordsSkip("madeBy", "badField"); return@let }
            for ((key, value) in madeBy.members) {
                val limit = when (key) { "app" -> MAX_APP_LENGTH; "version" -> MAX_APP_VERSION_LENGTH; else -> 0 }
                if (limit == 0) { skipped += WordsSkip("madeBy.$key", "unknownField"); continue }
                val cleaned = (value as? Json.Str)?.value?.let(WordsText::line)
                if (cleaned == null || cleaned.isEmpty() || WordsText.lineHidden(cleaned) || WordsText.length(cleaned) > limit) {
                    skipped += WordsSkip("madeBy.$key", "badField"); continue
                }
                if (key == "app") app = cleaned else appVersion = cleaned
            }
        }
        val vocabulary = members["vocabulary"]?.let { vocabulary(it, skipped) }.orEmpty()
        val replacements = members["replacements"]?.let { replacements(it, skipped) }.orEmpty()
        return WordsRead.Read(version, version > VERSION, app, appVersion, vocabulary, replacements, skipped)
    }

    /** A whole number of at least 1; "1.0" is 1. Read as a double, so one too large to be finite is refused. */
    private fun version(value: Json?): Long? {
        val number = (value as? Json.Num)?.text?.toDouble() ?: return null
        if (!number.isFinite() || number != Math.floor(number) || number < 1) return null
        return number.toLong()
    }

    private fun vocabulary(part: Json, skipped: MutableList<WordsSkip>): List<WordsTerm> {
        if (part !is Json.Arr) { skipped += WordsSkip("vocabulary", "badPart"); return emptyList() }
        val terms = ArrayList<WordsTerm>()
        val keys = HashSet<String>()
        for ((index, entry) in part.items.withIndex()) {
            val path = "vocabulary[$index]"
            fun skip(reason: String) { skipped += WordsSkip(path, reason) }
            if (terms.size >= MAX_TERMS) { skip("overLimit"); continue }
            val raw = ((entry as? Json.Obj)?.members?.get("term") as? Json.Str)?.value
            if (raw == null) { skip("badEntry"); continue }
            val term = WordsText.line(raw)
            val problem = lineProblem(term, MAX_TERM_LENGTH, Int.MAX_VALUE)
            if (problem != null) { skip(problem); continue }
            val key = WordsText.key(term)
            if (!keys.add(key)) { skip("duplicate"); continue }
            val members = (entry as Json.Obj).members
            for (name in members.keys) if (name != "term" && name != "soundsLike") skipped += WordsSkip("$path.$name", "unknownField")
            terms += WordsTerm(term, members["soundsLike"]?.let { spellings(it, path, key, skipped) }.orEmpty())
        }
        return terms
    }

    private fun spellings(part: Json, path: String, termKey: String, skipped: MutableList<WordsSkip>): List<String> {
        if (part !is Json.Arr) { skipped += WordsSkip("$path.soundsLike", "badField"); return emptyList() }
        val kept = ArrayList<String>()
        val keys = HashSet<String>()
        for ((index, entry) in part.items.withIndex()) {
            val at = "$path.soundsLike[$index]"
            if (kept.size >= MAX_SPELLINGS) { skipped += WordsSkip(at, "overLimit"); continue }
            val raw = (entry as? Json.Str)?.value
            if (raw == null) { skipped += WordsSkip(at, "badEntry"); continue }
            val spelling = WordsText.line(raw)
            val key = WordsText.key(spelling)
            val problem = lineProblem(spelling, MAX_SPELLING_LENGTH, MAX_WORDS)
                ?: if (key == termKey) "sameAsTerm" else if (key in keys) "duplicate" else null
            if (problem != null) { skipped += WordsSkip(at, problem); continue }
            keys += key
            kept += spelling
        }
        return kept
    }

    private fun replacements(part: Json, skipped: MutableList<WordsSkip>): List<WordsReplacement> {
        if (part !is Json.Arr) { skipped += WordsSkip("replacements", "badPart"); return emptyList() }
        val read = ArrayList<WordsReplacement>()
        val keys = HashSet<String>()
        for ((index, entry) in part.items.withIndex()) {
            val path = "replacements[$index]"
            fun skip(reason: String) { skipped += WordsSkip(path, reason) }
            if (read.size >= MAX_REPLACEMENTS) { skip("overLimit"); continue }
            val members = (entry as? Json.Obj)?.members
            val rawHeard = (members?.get("heard") as? Json.Str)?.value
            val rawWrite = (members?.get("write") as? Json.Str)?.value
            if (members == null || rawHeard == null || rawWrite == null) { skip("badEntry"); continue }
            val heard = WordsText.line(rawHeard)
            val write = WordsText.output(rawWrite)
            val problem = replacementProblem(heard, write)
            if (problem != null) { skip(problem); continue }
            if (!keys.add(WordsText.key(heard))) { skip("duplicate"); continue }
            for (name in members.keys) if (name != "heard" && name != "write" && name != "learned") skipped += WordsSkip("$path.$name", "unknownField")
            val learned = when (val value = members["learned"]) {
                null -> false
                is Json.Bool -> value.value
                else -> { skipped += WordsSkip("$path.learned", "badField"); false }
            }
            read += WordsReplacement(heard, write, learned)
        }
        return read
    }

    /** A cleaned term or spelling's first problem: hidden characters, no letter or digit, too long, too many words. */
    fun lineProblem(line: String, maxLength: Int, maxWords: Int): String? = when {
        WordsText.lineHidden(line) -> "hiddenCharacters"
        !WordsText.hasLetterOrDigit(line) -> "noLetters"
        WordsText.length(line) > maxLength -> "tooLong"
        WordsText.words(line) > maxWords -> "tooManyWords"
        else -> null
    }

    /** A cleaned replacement's first problem, in the reader's order. */
    fun replacementProblem(heard: String, write: String): String? = when {
        WordsText.lineHidden(heard) || WordsText.outputHidden(write) -> "hiddenCharacters"
        !WordsText.hasLetterOrDigit(heard) -> "noLetters"
        WordsText.length(heard) > MAX_HEARD_LENGTH || WordsText.length(write) > MAX_WRITE_LENGTH -> "tooLong"
        WordsText.words(heard) > MAX_WORDS -> "tooManyWords"
        else -> null
    }
}
