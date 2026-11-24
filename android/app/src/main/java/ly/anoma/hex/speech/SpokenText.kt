package ly.anoma.hex.speech

import java.util.Locale

/** iOS's DictationPipeline and TranscriptFormatter: string rules and Labs › Learn from my fixes' local word
 * pairs, no remote rewriting. */
internal object SpokenText {
    /** Labs › Learn from my fixes' fixes, custom vocabulary, spoken symbols and counts, cleanup, the style,
     * the vocabulary's spellings again (a style can capitalize "iPhone"), then Lowercase, the same wherever Hex
     * dictates. Fixes go first, on the raw transcript, so a learned spelling reaches every later stage unchanged.
     * Translation already happened in Whisper, so a translated dictation is English. */
    fun format(raw: String, options: SpeechOptions): String {
        val language = if (options.translate) "en" else options.spoken.code
        val fixed = DictationFixes.apply(options.fixes, raw)
        val spelled = VocabularySpelling.apply(options.vocabulary, fixed)
        if (options.style == Style.Verbatim || raw.isBlank()) return finished(style(spelled, options.style, language), options)
        val spoken = SpokenCleanup.counts(SpokenCleanup.symbols(spelled.trim(), language), language)
        val cleaned = if (options.clean) SpokenCleanup.clean(spoken, language) else spoken
        val styled = style(cleaned, options.style, language, options.endWithPeriod, options.formatLists, options.spokenPunctuation)
        // French spacing goes last, since every style takes the spaces before marks out; Verbatim keeps the model's.
        return FrenchSpacing.apply(finished(styled, options), language)
    }

    private fun finished(styled: String, options: SpeechOptions): String {
        val text = VocabularySpelling.apply(options.vocabulary, styled)
        return if (options.lowercase) text.lowercase(Locale.ROOT) else text
    }

    /** TranscriptFormatter.format. [lists] (SpokenLists) then [punctuation] (SpokenPunctuation) run
     * after the fillers, in every style but Verbatim. With [endsWithPeriod] off no line ends with a
     * full stop, though "?" and "!" stay; Verbatim keeps the model's. */
    fun style(raw: String, style: Style, language: String? = null, endsWithPeriod: Boolean = true,
              lists: Boolean = false, punctuation: Boolean = false): String {
        val styled = styled(raw, style, language, lists, punctuation)
        if (endsWithPeriod || style == Style.Verbatim) return styled
        return styled.split('\n').joinToString("\n", transform = ::droppingFinalFullStop)
    }

    private fun styled(raw: String, style: Style, language: String?, lists: Boolean, punctuation: Boolean): String {
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) return ""
        val english = code(language) == "en"
        if (style == Style.Verbatim) return collapse(trimmed)
        // Lists first, so "dash … dash …" stays a list and "comma, new line" keeps its comma.
        val stripped = stripFillers(trimmed, fillers(language))
        val listed = if (lists) SpokenLists.format(stripped, language) else stripped
        val cleaned = if (punctuation) SpokenPunctuation.format(listed, language) else listed
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

    /** The dictation as it goes in after [before] (iOS insertionText's case rule): mid-sentence, its
     * first word is lowercased unless it is "I", an acronym, has inner capitals or [isWord] doesn't
     * know its lowercase form (likely a name). German capitalizes nouns, so it keeps the case. */
    fun continuing(text: String, before: String?, language: String?,
                   isWord: (String) -> Boolean = { commonWord(it, language) }): String {
        if (startsSentence(before) || code(language) in setOf("de", "lb")) return text
        return continuingSentence(text, isWord)
    }

    fun continuingSentence(text: String, isWord: (String) -> Boolean): String {
        val first = text.firstOrNull() ?: return text
        if (!first.isUpperCase()) return text
        val word = text.takeWhile { it.isLetter() || it == '\'' || it == '’' }
        if (word == "I" || word.startsWith("I'") || word.startsWith("I’")) return text
        val letters = word.filter(Char::isLetter)
        if (letters.length >= 2 && letters == letters.uppercase(Locale.ROOT)) return text
        if (letters.drop(1).any(Char::isUpperCase)) return text
        if (!isWord(word.lowercase(Locale.ROOT))) return text
        return first.lowercase() + text.substring(1)
    }

    /** Words that are never names, so a capital on them only came from Whisper starting a segment.
     * Android has no synchronous system dictionary and the typing models are case-folded. */
    fun commonWord(word: String, language: String?) = word in (commonWords[code(language)] ?: emptySet())

    private val commonWords = mapOf(
        "en" to ("a about after again all also am an and another any are as at be because been before being but by " +
            "can could did do does doing don't done each even every few for from get gets getting go goes going gonna got " +
            "had has have having he her here hers him his how however if in into is it it's its just let let's like made " +
            "make many maybe me might more most much must my need no not now of off oh ok okay on once one only or other our " +
            "out over please so some still such than thank thanks that that's the their them then there there's these they " +
            "this those though through to too under until up us very was we well were what when where which while who why " +
            "will with would yeah yes yet you your").split(' ').toSet(),
        "es" to ("a al algo así aunque bien cada como con cuando de del desde donde el ella ellas ellos en entonces era es " +
            "esa ese eso esta este esto está están estoy fue gracias ha hay la las le les lo los me mi mientras muy más nada " +
            "ni no nos o otra otro para pero poco por porque pues que qué se si sin sobre solo son su sus también tan te " +
            "tengo tiene todo todos tu un una uno vale vamos y ya yo").split(' ').toSet(),
        "fr" to ("à au aux avec ce cela ces cette c'est comme dans de des donc du elle elles en est et était il ils je j'ai la le les leur lui mais me merci moi mon ne nous on ou où oui par pas peut plus pour quand que qui sa sans se ses si son sont sur ta te tes toi ton tout tous très tu un une vous vos votre y ça").split(' ').toSet(),
        "it" to ("a al alla anche che chi ci come con da dal del della di e è gli grazie ha hanno i il in io la le li lo lui ma mi mio mia ne nel nella no non noi per perché più poi qui se si sì sono su sua suo ti tu tutto tutti un una uno voi").split(' ').toSet(),
        "pt" to ("a à ao aos as com como da das de do dos e é ela elas ele eles em então essa esse esta este eu foi isso já mas me meu minha muito na nas não no nos o obrigado obrigada os ou para pela pelo por porque pra quando que se sem seu sua também tem tudo um uma você vocês").split(' ').toSet(),
        "pl" to ("a ale bo by być był była było co czy dla do dobrze dziękuję gdy gdzie i ich im jak jakie jaki jest jestem jeszcze już ja jej jego kiedy która które który ma mam mi mnie może my na nas nie nic no o od on ona oni po pod przez przy się są ta tak tam te teraz to tu ty tylko w we wszystko z za ze że żeby").split(' ').toSet(),
        "ru" to ("а без бы был была было в вам вас во вот все всё вы где да для до его ее её если есть еще ещё же за и из или им их к как когда кто ли мне мы на нас не нет ну о он она они от по под пожалуйста потому почему при с со спасибо так там тебе то тоже только ты у уже что чтобы это я").split(' ').toSet(),
        "nl" to ("aan als bij dan dat de die dit door een en er het hij hem hun ik in is je jij kan maar me met mijn naar niet nog nu of om ook op te toch tot u uit van voor wat we wij wel zal ze zij zijn").split(' ').toSet(),
        "no" to ("alle at av bare da de del dem den denne der det dette du eller en er et etter for fordi fra går ha han " +
            "har hun hva hvis hvor i igjen ikke inn ja jeg kan kanskje med men mer meg min nei når og også om opp på så " +
            "seg skal som til takk uten var vi vil være å").split(' ').toSet(),
    ).let { it + ("nb" to it.getValue("no")) + ("nn" to it.getValue("no")) }

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
