package ly.anoma.hex.words

/** A JSON value as [StrictJson] builds it. Objects keep the file's key order; numbers keep their text. */
internal sealed interface Json {
    data class Obj(val members: Map<String, Json>) : Json
    data class Arr(val items: List<Json>) : Json
    data class Str(val value: String) : Json
    data class Num(val text: String) : Json
    data class Bool(val value: Boolean) : Json
    data object Null : Json
}

/** Why [StrictJson] refused a text: `tooDeep`, `duplicateKey` or `notJSON`, the Hex words file's names. */
internal class JsonRefusal(val reason: String) : Exception(reason, null, false, false)

/** The Hex words file's check pass and parser in one walk (docs/specs/hex-words.md, "Reading"): exactly one
 * RFC 8259 value, refused at the first problem met. `org.json` is lenient, and neither it nor
 * `android.util.JsonReader` runs in JVM unit tests. Nesting is checked as each `{` or `[` opens, so the
 * recursion never goes deeper than [maxDepth] and 10,000 levels can't exhaust a stack. */
internal class StrictJson private constructor(private val text: String, private val maxDepth: Int) {
    private var at = 0
    private var depth = 0

    companion object {
        const val TOO_DEEP = "tooDeep"
        const val DUPLICATE_KEY = "duplicateKey"
        const val NOT_JSON = "notJSON"

        fun parse(text: String, maxDepth: Int = 8): Json {
            val parser = StrictJson(text, maxDepth)
            parser.space()
            val value = parser.value()
            parser.space()
            if (parser.at != text.length) parser.fail()
            return value
        }
    }

    private fun fail(): Nothing = throw JsonRefusal(NOT_JSON)

    private fun space() {
        while (at < text.length) {
            val c = text[at]
            if (c == ' ' || c == '\t' || c == '\n' || c == '\r') at++ else return
        }
    }

    private fun next(c: Char) = at < text.length && text[at] == c

    private fun expect(c: Char) { if (!next(c)) fail(); at++ }

    private fun value(): Json {
        if (at >= text.length) fail()
        return when (text[at]) {
            '{' -> obj()
            '[' -> arr()
            '"' -> Json.Str(string())
            't' -> literal("true", Json.Bool(true))
            'f' -> literal("false", Json.Bool(false))
            'n' -> literal("null", Json.Null)
            else -> number()
        }
    }

    private fun open() {
        if (++depth > maxDepth) throw JsonRefusal(TOO_DEEP)
        at++
    }

    private fun obj(): Json {
        open()
        val members = LinkedHashMap<String, Json>()
        space()
        if (next('}')) { at++; depth--; return Json.Obj(members) }
        while (true) {
            space()
            if (!next('"')) fail()
            val key = string()
            if (key in members) throw JsonRefusal(DUPLICATE_KEY)
            space(); expect(':'); space()
            members[key] = value()
            space()
            if (next(',')) { at++; continue }
            expect('}')
            depth--
            return Json.Obj(members)
        }
    }

    private fun arr(): Json {
        open()
        val items = ArrayList<Json>()
        space()
        if (next(']')) { at++; depth--; return Json.Arr(items) }
        while (true) {
            space()
            items += value()
            space()
            if (next(',')) { at++; continue }
            expect(']')
            depth--
            return Json.Arr(items)
        }
    }

    private fun literal(word: String, value: Json): Json {
        if (!text.startsWith(word, at)) fail()
        at += word.length
        return value
    }

    private fun digit() = at < text.length && text[at] in '0'..'9'

    private fun digits() { if (!digit()) fail(); while (digit()) at++ }

    // RFC 8259: -?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?. A leading zero ends the number, so "01" fails
    // on the "1" that follows it.
    private fun number(): Json {
        val start = at
        if (next('-')) at++
        if (next('0')) at++ else digits()
        if (next('.')) { at++; digits() }
        if (next('e') || next('E')) {
            at++
            if (next('+') || next('-')) at++
            digits()
        }
        return Json.Num(text.substring(start, at))
    }

    private fun string(): String {
        at++
        val out = StringBuilder()
        var run = at
        while (true) {
            if (at >= text.length) fail()
            val c = text[at]
            when {
                c == '"' -> { out.append(text, run, at); at++; return out.toString() }
                c == '\\' -> { out.append(text, run, at); at++; escape(out); run = at }
                c < ' ' -> fail()
                else -> at++
            }
        }
    }

    private fun escape(out: StringBuilder) {
        if (at >= text.length) fail()
        when (val c = text[at++]) {
            '"', '\\', '/' -> out.append(c)
            'b' -> out.append('\b')
            'f' -> out.append('\u000C')
            'n' -> out.append('\n')
            'r' -> out.append('\r')
            't' -> out.append('\t')
            'u' -> {
                val unit = hex4()
                when {
                    // A lone surrogate, high or low, is not JSON.
                    Character.isHighSurrogate(unit) -> {
                        if (!text.startsWith("\\u", at)) fail()
                        at += 2
                        val low = hex4()
                        if (!Character.isLowSurrogate(low)) fail()
                        out.append(unit).append(low)
                    }
                    Character.isLowSurrogate(unit) -> fail()
                    else -> out.append(unit)
                }
            }
            else -> fail()
        }
    }

    private fun hex4(): Char {
        if (at + 4 > text.length) fail()
        var value = 0
        repeat(4) {
            val digit = Character.digit(text[at++], 16)
            // Character.digit also takes full-width and other scripts' digits; JSON takes ASCII only.
            if (digit < 0 || text[at - 1] > 'f') fail()
            value = value * 16 + digit
        }
        return value.toChar()
    }
}
