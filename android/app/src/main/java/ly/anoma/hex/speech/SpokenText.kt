package ly.anoma.hex.speech

import java.util.Locale

/** iOS's DictationPipeline and TranscriptFormatter: string rules only, no learned or remote rewriting. */
internal object SpokenText {
    /** Cleanup, the style, then Lowercase, the same wherever Hex dictates. Translation already
     * happened in Whisper, so a translated dictation is formatted as English. */
    fun format(raw: String, options: SpeechOptions): String {
        val language = if (options.translate) "en" else options.spoken.code
        val cleaned = if (options.clean && options.style != Style.Verbatim && raw.isNotBlank())
            SpokenCleanup.clean(raw.trim(), language) else raw
        val styled = style(cleaned, options.style, language, options.endWithPeriod)
        return if (options.lowercase) styled.lowercase(Locale.ROOT) else styled
    }

    /** TranscriptFormatter.format. With [endsWithPeriod] off no line ends with a full stop,
     * though "?" and "!" stay; Verbatim keeps the model's. */
    fun style(raw: String, style: Style, language: String? = null, endsWithPeriod: Boolean = true): String {
        val styled = styled(raw, style, language)
        if (endsWithPeriod || style == Style.Verbatim) return styled
        return styled.split('\n').joinToString("\n", transform = ::droppingFinalFullStop)
    }

    private fun styled(raw: String, style: Style, language: String?): String {
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) return ""
        val english = code(language) == "en"
        if (style == Style.Verbatim) return collapse(trimmed)
        val cleaned = stripFillers(trimmed, fillers(language))
        return when (style) {
            Style.Polished -> polish(cleaned, english)
            Style.Casual -> casual(cleaned, english)
            else -> notes(cleaned, english)
        }
    }

    private fun code(language: String?) = language?.substringBefore('-')?.substringBefore('_')?.lowercase(Locale.ROOT) ?: "en"

    /** Each language trims only its own hesitation sounds: English "er" is Norwegian "is". */
    fun fillers(language: String?) = when (code(language)) {
        "en" -> "um+|uh+|er+m*|ah+"
        "nb", "nn", "no", "da", "sv" -> "e+h+m*|ø+h+m*|ö+h+m*|hm+"
        "es" -> "e+h+m*|e+m+|m{2,}|hm+"
        "de" -> "ä+h+m*|ö+h+m*|hm+"
        else -> null
    }

    private fun stripFillers(text: String, fillers: String?): String {
        if (fillers == null) return collapse(text)
        return collapse(text.replace(Regex("(?iu)(?:^|(?<=[\\s,]))(?:$fillers)(?=[\\s,.!?]|$),?\\s*"), " "))
    }

    private fun collapse(text: String) =
        text.split('\n').joinToString("\n") { it.replace(Regex("[ \\t]+"), " ").trim(' ', '\t') }.trim()

    private fun fixSpacing(text: String) = text
        .replace(Regex("\\s+([,.;:!?])(?!-?[()])"), "$1")
        .replace(Regex("^[,;:]+\\s*"), "").trim()

    // Polished

    private fun polish(text: String, english: Boolean): String {
        if (text.isEmpty()) return ""
        var normalized = fixSpacing(collapse(text))
        if (english) normalized = expandingContractions(normalized)
        normalized = capitalizeSentenceStarts(normalized)
        if (english) normalized = recasingI(normalized, "i", "I")
        return normalized.split('\n').joinToString("\n", transform = ::endingSentence)
    }

    private const val sentenceClosers = ".!?…\"”)]:,;。！？」』）"

    private fun endingSentence(line: String): String {
        val last = line.lastOrNull() ?: return line
        if (last in sentenceClosers) return line
        if ('@' in line && line.none(Char::isWhitespace)) return line
        return line + if (ideographic(last)) "。" else "."
    }

    private val abbreviations = setOf("mr", "mrs", "ms", "dr", "st", "jr", "sr", "prof", "vs", "etc", "inc", "ltd", "approx", "e.g", "i.e")

    /** [line] without the full stop that ends it; an ellipsis, "etc." or "U.S." keeps it. */
    fun droppingFinalFullStop(line: String): String {
        var stop = line.length - 1
        while (stop >= 0 && line[stop] in "\"”’')]」』）") stop--
        if (stop < 0 || line[stop] !in ".。．｡") return line
        if (line[stop] == '.') {
            var start = stop
            while (start > 0 && !line[start - 1].isWhitespace()) start--
            val word = line.substring(start, stop).lowercase(Locale.ROOT)
            val initials = word.split('.')
            val initialism = initials.size >= 2 && initials.all { it.length == 1 && it[0].isLetter() }
            if (word.endsWith('.') || initialism || word in abbreviations) return line
        }
        return line.removeRange(stop, stop + 1)
    }

    private fun endsSentence(chars: CharSequence, i: Int): Boolean {
        val c = chars[i]
        if (c in "。！？") return true
        if (c !in ".!?…") return false
        return i + 1 == chars.length || chars[i + 1].isWhitespace()
    }

    private fun ideographic(c: Char) = c.code in 0x4E00..0x9FFF || c.code in 0x3400..0x4DBF ||
        c.code in 0x3040..0x30FF || c.code in 0xF900..0xFAFF

    private val contractions = mapOf("gonna" to "going to", "wanna" to "want to", "gotta" to "got to",
        "kinda" to "kind of", "lemme" to "let me")

    /** Never inside quotes, nor a capitalized word mid-sentence, which is a name or a title. */
    private fun expandingContractions(text: String): String {
        val lowered = text.lowercase(Locale.ROOT)
        if (contractions.keys.none { it in lowered }) return text
        val out = StringBuilder()
        val word = StringBuilder()
        var straight = false
        var curly = 0
        fun flush() {
            val w = word.toString(); word.clear()
            val lower = w.lowercase(Locale.ROOT)
            val expansion = contractions[lower]
            when {
                expansion == null || straight || curly > 0 -> out.append(w)
                w == lower -> out.append(expansion)
                w == lower.replaceFirstChar { it.uppercaseChar() } && startsSentenceOrItem(out.toString()) ->
                    out.append(expansion.replaceFirstChar { it.uppercaseChar() })
                else -> out.append(w)
            }
        }
        for (c in text) {
            if (c.isLetterOrDigit() || c == '\'' || c == '’') { word.append(c); continue }
            flush()
            when (c) { '"' -> straight = !straight; '“' -> curly++; '”' -> curly = maxOf(0, curly - 1) }
            out.append(c)
        }
        flush()
        return out.toString()
    }

    private fun startsSentenceOrItem(text: String): Boolean {
        if (startsSentence(text)) return true
        val marker = text.substringAfterLast('\n').trim(' ', '\t')
        return marker == "-" || marker == "•"
    }

    /** The English pronoun "I" (alone or in "I'm", "I'll", "I've", "I'd"); never "i.e." or an address. */
    private fun recasingI(text: String, from: String, to: String): String {
        if (from !in text) return text
        return text.replace(Regex("(?<![\\p{L}\\p{N}@.'’])$from(?=(?:['’](?:m|ll|ve|d))?(?![\\p{L}\\p{N}@'’]|\\.\\p{L}))"), to)
    }

    private fun capitalizeSentenceStarts(text: String): String {
        val chars = text.toCharArray()
        var next = true
        var ended = false
        for (i in chars.indices) {
            val c = chars[i]
            when {
                next && c.isLetter() -> {
                    if (!isAddress(chars, i)) chars[i] = c.uppercaseChar()
                    next = false
                }
                c == '\n' -> { next = true; ended = false }
                c in ".!?" -> { ended = true; next = false }
                c.isWhitespace() -> if (ended) { next = true; ended = false }
                c in "\"”’')]" && ended -> {}
                c !in "\"“(¿¡" -> { next = false; ended = false }
            }
        }
        return String(chars)
    }

    private fun isAddress(chars: CharArray, index: Int): Boolean {
        var end = index
        while (end < chars.size && !chars[end].isWhitespace()) {
            if (chars[end] == '@') return true
            end++
        }
        return false
    }

    // Casual

    private val interjections = setOf("yeah", "yes", "yep", "yup", "no", "nope", "nah", "ok", "okay", "oh", "hey",
        "hi", "hello", "so", "well", "sure", "alright", "cool", "thanks")

    private fun casual(text: String, english: Boolean): String {
        var message = fixSpacing(collapse(text))
        if (english) message = droppingInterjectionCommas(message)
        message = lowercasingSentenceStarts(message)
        if (english) message = recasingI(message, "I", "i")
        val sentences = message.indices.count { endsSentence(message, it) }
        if ('\n' !in message && sentences <= 3 && message.lastOrNull().let { it == '.' || it == '。' })
            message = message.dropLast(1)
        return message.trim()
    }

    private fun droppingInterjectionCommas(text: String): String {
        val words = text.split(' ').toMutableList()
        var atStart = true
        for (i in words.indices) {
            val word = words[i]
            if (atStart && word.endsWith(',') && i + 1 < words.size &&
                word.dropLast(1).lowercase(Locale.ROOT) in interjections) words[i] = word.dropLast(1)
            atStart = '\n' in word || (word.isNotEmpty() && endsSentence("$word ", word.length - 1))
        }
        return words.joinToString(" ")
    }

    private fun lowercasingSentenceStarts(text: String): String {
        val chars = text.toCharArray()
        var atStart = true
        var i = 0
        while (i < chars.size) {
            if (i == 0 || chars[i - 1] == '\n') {
                val marker = listMarkerLength(chars, i)
                if (marker != null) { i += marker; atStart = false; continue }
            }
            val c = chars[i]
            if (atStart && c.isLetter()) {
                var end = i
                while (end < chars.size && (chars[end].isLetter() || chars[end] == '\'' || chars[end] == '’')) end++
                val word = String(chars, i, end - i)
                // Lowering never changes the length of the first letter or of "OK".
                if (!keepsCapital(word) && !isAddress(chars, i)) chars[i] = chars[i].lowercaseChar()
                if (word == "OK") chars[i + 1] = 'k'
                atStart = false
                i = end
                continue
            }
            if (c == '\n' || endsSentence(text, i)) atStart = true
            else if (!c.isWhitespace() && c !in "\"“'‘(¿¡”’)]") atStart = false
            i++
        }
        return String(chars)
    }

    private fun listMarkerLength(chars: CharArray, i: Int): Int? {
        if (i + 1 < chars.size && (chars[i] == '-' || chars[i] == '•') && chars[i + 1] == ' ') return 2
        var end = i
        while (end < chars.size && end - i < 2 && chars[end] in '0'..'9') end++
        if (end == i || end + 1 >= chars.size || chars[end] != '.' || chars[end + 1] != ' ') return null
        return end + 2 - i
    }

    private fun keepsCapital(word: String): Boolean {
        if (word == "OK") return false
        val letters = word.filter(Char::isLetter)
        if (letters.length >= 2 && letters == letters.uppercase(Locale.ROOT)) return true
        return letters.drop(1).any(Char::isUpperCase)
    }

    // Notes

    private fun notes(text: String, english: Boolean): String {
        if (text.isEmpty()) return ""
        val working = text.replace(Regex("(?i)\\bnew paragraph\\b[,.]?\\s*"), "\n\n")
            .replace(Regex("(?i)\\bnew line\\b[,.]?\\s*"), "\n")
            .replace(Regex("(?i)(?:^|\\s+)(?:bullet point|next bullet|bullet)\\b[:,-]?\\s*"), "\n- ")
        var lines = working.split('\n')
        if (lines.size == 1) {
            val line = fixSpacing(collapse(lines[0]))
            val sentences = noteSentences(line)
            val list = colonList(line, english)
            if (sentences.size >= 2) lines = sentences.map { "- $it" }
            else if (list != null) lines = listOf(list.first + ":") + list.second.map { "- $it" }
        }
        return lines.joinToString("\n") { line ->
            val trimmed = collapse(line)
            if (trimmed.startsWith("- ")) {
                val note = conciseNote(trimmed.drop(2).trim(), english)
                if (note.isEmpty()) "" else "- $note"
            } else conciseNote(trimmed, english)
        }.trim()
    }

    private fun conciseNote(line: String, english: Boolean): String {
        var note = fixSpacing(line)
        if (english) note = droppingConnectives(note)
        if ((note.endsWith('.') || note.endsWith('。')) && !note.endsWith("...")) note = note.dropLast(1)
        return capitalizeFirstLetter(note.trim(' ', '\t'))
    }

    private val connectives = listOf("and then", "and also", "and", "also", "then", "plus", "so", "okay", "ok", "yeah",
        "oh", "well", "anyway", "alright", "basically", "first", "firstly", "second", "secondly", "third", "thirdly",
        "next", "finally", "lastly")
    private val bareConnectives = setOf("and then", "and also", "and", "also", "then", "plus")

    private fun droppingConnectives(line: String): String {
        var note = line
        var changed = true
        while (changed) {
            changed = false
            val lowered = note.lowercase(Locale.ROOT)
            for (connective in connectives) {
                if (!lowered.startsWith(connective)) continue
                val rest = note.drop(connective.length)
                val next = when {
                    rest.startsWith(", ") -> rest.drop(2)
                    rest.startsWith(" ") && connective in bareConnectives -> rest.drop(1)
                    else -> continue
                }
                val first = next.firstOrNull() ?: continue
                if (!first.isLetterOrDigit()) continue
                note = next
                changed = true
                break
            }
        }
        return note
    }

    private fun noteSentences(line: String): List<String> {
        val sentences = mutableListOf<String>()
        var start = 0
        for (i in line.indices) {
            if (!endsSentence(line, i)) continue
            if (line[i] == '.') {
                var wordStart = i
                while (wordStart > start && !line[wordStart - 1].isWhitespace()) wordStart--
                val word = line.substring(wordStart, i).lowercase(Locale.ROOT)
                if (word in abbreviations || (word.length == 1 && word[0].isLetter())) continue
            }
            val sentence = line.substring(start, i + 1).trim(' ', '\t')
            if (sentence.isNotEmpty()) sentences += sentence
            start = i + 1
        }
        val rest = line.substring(start).trim(' ', '\t')
        if (rest.isNotEmpty()) sentences += rest
        return sentences.ifEmpty { listOf(line) }
    }

    /** "Shopping list: eggs, milk and bread." becomes three items after its colon. English
     * splits the last "and" or "or" off too; other languages take commas only. */
    private fun colonList(line: String, english: Boolean): Pair<String, List<String>>? {
        val colon = line.indexOf(": ")
        if (colon < 0) return null
        val intro = line.substring(0, colon).trim(' ', '\t')
        var body = line.substring(colon + 2).trim(' ', '\t')
        if (intro.isEmpty() || ": " in body) return null
        if (body.endsWith('.') || body.endsWith('。')) body = body.dropLast(1)
        val items = body.split(", ").map { it.trim(' ', '\t') }.toMutableList()
        if (english) {
            var tail = items.removeAt(items.lastIndex)
            for (joiner in listOf("and ", "or ")) if (tail.lowercase(Locale.ROOT).startsWith(joiner)) tail = tail.drop(joiner.length)
            val and = tail.indexOf(" and ")
            val split = if (and >= 0) and to 5 else tail.indexOf(" or ").takeIf { it >= 0 }?.let { it to 4 }
            if (split != null) { items += tail.substring(0, split.first); items += tail.substring(split.first + split.second) }
            else items += tail
        }
        if (items.size < 3 || items.any { it.split(' ').count(String::isNotEmpty) !in 1..4 } ||
            items.any { item -> item.any { it in ".!?。！？" } }) return null
        return intro to items
    }

    private fun capitalizeFirstLetter(text: String): String {
        val i = text.indexOfFirst(Char::isLetter)
        if (i < 0 || isAddress(text.toCharArray(), i)) return text
        return text.substring(0, i) + text[i].uppercase() + text.substring(i + 1)
    }

    /** Whether text before the cursor leaves the next words at a sentence start: none (or only
     * spaces), a line break, or a sentence's mark (closing quotes or brackets after it count too). */
    fun startsSentence(before: String?): Boolean {
        if (before == null) return true
        for (c in before.reversed()) {
            if (c == '\n' || c == '\r') return true
            if (c.isWhitespace() || c in "\"”’')]»") continue
            return c in ".!?…。"
        }
        return true
    }
}
