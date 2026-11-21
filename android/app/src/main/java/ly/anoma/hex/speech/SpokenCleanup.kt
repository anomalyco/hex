package ly.anoma.hex.speech

import java.util.Locale

/** Deletion-only repairs. ES/DE/NO mirror SpokenCleanupRules; English uses explicit
 * markers and safe replacement anchors in lieu of iOS's learned English tagger. */
internal object SpokenCleanup {
    val sounds = mapOf("en" to "um+|uh+|er+|ah+", "es" to "e+h+m*|e+m+|m{2,}|hm+",
        "de" to "ä+h+m*|ö+h+m*|hm+", "no" to "e+h+m*|ø+h+m*|ö+h+m*|hm+")
    private const val open = "¿¡\"“«(„"
    private const val close = ",.;:!?…\"”»)"
    private fun phrases(s: String) = s.split('|').filter(String::isNotEmpty).map { it.split(' ') }.sortedByDescending { it.size }
    private class Rules(f: String, m: String, p: String, k: List<String>, nc: String) {
        val fillers = phrases(f); val markers = phrases(m); val plain = phrases(p)
        val kinds = k.map { it.split(' ').toSet() }; val noComma = nc.split(' ').toSet()
        fun kind(word: String) = if (word.any(Char::isDigit)) kinds.size else kinds.indexOfFirst { word in it }
    }
    private val rules = mapOf(
        "en" to Rules("you know|like", "no wait|wait no|i mean|or rather|scratch that|actually|sorry|no",
            "i mean|or rather|scratch that|no wait|wait no",
            listOf("one two three four five six seven eight nine ten eleven twelve", "monday tuesday wednesday thursday friday saturday sunday",
                "january february march april may june july august september october november december",
                "today tomorrow yesterday tonight", "black white red yellow green blue purple orange gray grey brown"),
            "a an the to of for with at in on from by about into onto my your his her our their its i we you he she they can could would will should shall may might must do does did is are was were be been am have has had and or but if that because than just gonna wanna want need think thinking guess get got let's i'm we're you're they're it's there's i'll we'll you'll i'd we'd i've we've really very so like isn't don't can't won't didn't this these those some any every what where when how which who also go going s"),
        "es" to Rules("este|o sea", "digo|perdón|perdon|no|no espera|no mejor|mejor dicho", "mejor dicho|no mejor",
            listOf("dos tres cuatro cinco seis siete ocho nueve diez once doce", "lunes martes miércoles jueves viernes sábado domingo",
                "enero febrero marzo abril mayo junio julio agosto septiembre setiembre octubre noviembre diciembre", "hoy mañana ayer anoche"),
            "y o pero que a de en con para por el la los las un una mi tu su porque si"),
        "de" to Rules("", "nein|nein warte|ich meine|besser gesagt|sorry|entschuldigung", "besser gesagt",
            listOf("zwei drei vier fünf sechs sieben acht neun zehn elf zwölf", "montag dienstag mittwoch donnerstag freitag samstag sonntag",
                "januar jänner februar märz april mai juni juli august september oktober november dezember", "heute morgen gestern übermorgen"),
            "und oder aber dass der die das ein eine mit zu in an auf für von ich du er sie wir"),
        "no" to Rules("", "nei|nei vent|jeg mener|rettere sagt|unnskyld|sorry", "rettere sagt",
            listOf("to tre fire fem seks sju syv åtte ni ti elleve tolv", "mandag tirsdag onsdag torsdag fredag lørdag søndag",
                "januar februar mars april mai juni juli august september oktober november desember"),
            "og eller men at jeg du han hun vi en et den det på i til med for som")
    )
    private val protect = ("not no never none nothing nobody nowhere neither nor cannot cant dont doesnt didnt " +
        "wont wouldnt shouldnt couldnt isnt arent wasnt werent hasnt havent hadnt mustnt neednt aint " +
        "nicht kein keine nunca ikke aldri").split(' ').toSet()
    private fun negation(word: String) = bare(word).replace('’', '\'').let { it in protect || it.endsWith("n't") }
    private fun capitalized(word: String) =
        if (word.first().isLowerCase() && word.drop(1).none(Char::isUpperCase)) word.replaceFirstChar { it.uppercase() } else word

    private fun bare(s: String) = s.trimStart { it in open }.trimEnd { it in close }.lowercase(Locale.ROOT)
    private fun end(s: String) = s.trimEnd { it in "\"”»)" }.lastOrNull() in listOf('.', '!', '?', '…')
    private fun pause(s: String) = s.lastOrNull() in listOf(',', ';', ':')
    private fun matches(words: List<String>, i: Int, p: List<String>) = i+p.size <= words.size &&
        p.indices.all { n -> bare(words[i+n]) == p[n] && (n == p.lastIndex || words[i+n].last() !in close || words[i+n].endsWith(',')) }

    fun clean(text: String, lang: String): String {
        val r = rules[lang] ?: return text
        val input = text.split(Regex("\\s+")).filter(String::isNotBlank)
        val words = input.toMutableList()
        val sound = Regex("(?i)(?:" + sounds.getValue(lang) + if (lang == "en") "|hmm+|erm|uhm|mm)" else ")")
        var i = 0
        while (i < words.size) {
            val word = words[i]
            val f = r.fillers.firstOrNull { matches(words, i, it) }
            val n = when {
                word.first() !in open && word.last() !in "?!" && sound.matches(bare(word)) -> 1
                f != null && i+f.size < words.size && word.first() !in open &&
                    (i == 0 || pause(words[i-1]) || end(words[i-1])) && words[i+f.size-1].endsWith(',') -> f.size
                else -> 0
            }
            if (n == 0) { i++; continue }
            val terminal = words[i+n-1].last().takeIf { it in ".?!" }
            words.subList(i, i+n).clear()
            if (i > 0) {
                val previous = words[i-1].removeSuffix("...").removeSuffix("…")
                words[i-1] = when {
                    i == words.size && !end(previous) -> previous.trimEnd(',',';',':') + (terminal?.toString() ?: "")
                    terminal != null && !end(previous) && words[i].first().isUpperCase() -> previous.trimEnd(',',';',':') + terminal
                    previous.endsWith(',') && bare(previous) in r.noComma -> previous.dropLast(1)
                    else -> previous
                }
            }
            if (i < words.size && (i == 0 || end(words[i-1]))) words[i] = capitalized(words[i])
        }

        i = 1
        while (i < words.size) {
            val marker = r.markers.firstOrNull { p ->
                if (!matches(words,i,p) || i+p.size >= words.size || words[i].first() in open) false
                else {
                    val before = words[i-1]; val last = words[i+p.lastIndex]
                    ((pause(before) || end(before)) && last.endsWith(',')) ||
                        (p in r.plain && !end(before) && !end(last) && (lang == "en" || !pause(last))) ||
                        (lang == "en" && pause(before) && !end(last) && r.kind(bare(words[i+p.size])) >= 0)
                }
            }
            if (marker == null) { i++; continue }
            val repair = i + marker.size
            val first = bare(words[repair]); val kind = r.kind(first)
            var low = i-1
            while (low > 0 && i-low < 8 && !end(words[low-1])) low--
            val candidates = (i-1 downTo low)
            val anchor = candidates.firstOrNull { bare(words[it]) == first }
                ?: candidates.firstOrNull { kind >= 0 && r.kind(bare(words[it])) == kind }
                ?: (i-1).takeIf { lang == "en" && marker == listOf("i","mean") && pause(words[it]) &&
                    words[it].first().isUpperCase() && words[repair].first().isUpperCase() }
            if (anchor == null || words.subList(anchor,i).any(::negation)) { i++; continue }
            val source = words[anchor]
            val marks = source.takeWhile { it in open }
            var target = words[repair]
            if (marks.isNotEmpty() && target.first() !in open) target = marks + target
            val letter = target.indexOfFirst(Char::isLetter)
            val from = source.drop(marks.length).firstOrNull()
            if (letter >= 0 && from != null && from.isLetter() && !target.drop(letter+1).any(Char::isUpperCase)) {
                val c = target[letter]
                target = target.replaceRange(letter,letter+1,if (from.isUpperCase()) c.uppercase() else c.lowercase())
            }
            words[repair] = target
            words.subList(anchor,repair).clear()
            i = anchor+1
        }
        return if (words == input) text else words.joinToString(" ")
    }

    // Spoken symbols and counts (iOS SpokenCleanupRules.spokenSymbols/spokenCounts): English only,
    // in every style but Verbatim, with Clean up what I say on or off.
    private class Token(val word: String, val start: Int, val end: Int)
    private fun tokens(text: String) = Regex("\\S+").findAll(text).map { Token(it.value, it.range.first, it.range.last + 1) }.toList()
    private fun core(word: String) = word.trimEnd { it in close }
    private fun splice(text: String, edits: List<Triple<Int, Int, String>>): String {
        if (edits.isEmpty()) return text
        val out = StringBuilder()
        var cursor = 0
        for ((start, end, value) in edits) { out.append(text, cursor, start).append(value); cursor = end }
        return out.append(text, cursor, text.length).toString()
    }
    private fun english(lang: String?) = (lang ?: "en").substringBefore('-').lowercase(Locale.ROOT) == "en"

    /** "at the rate" (India's name for @) before a domain, and "at sign" anywhere, become "@"; an
     * address said around it joins up in lowercase: "John at the rate Gmail dot com" -> "john@gmail.com". */
    fun symbols(text: String, lang: String?): String {
        if (!english(lang) || !text.contains("at", ignoreCase = true)) return text
        val tokens = gluedDomains(tokens(text), text)
        val edits = mutableListOf<Triple<Int, Int, String>>()
        var i = 0
        while (i < tokens.size) {
            val marker = symbolMarker(tokens, i)
            if (marker == null) { i++; continue }
            val (end, ambiguous) = marker
            val next = end + 1
            if (ambiguous && next < tokens.size && bare(tokens[next].word) == "of") { i = next; continue }
            val domain = addressDomain(tokens, next)
            if (domain != null) {
                var start = i
                val local = mutableListOf<String>()
                tokens.getOrNull(i - 1)?.let { localPart(it.word) }?.let { part ->
                    local += part; start = i - 1
                    while (start >= 2 && tokens[start - 1].word.lowercase(Locale.ROOT) == "dot") {
                        val before = localPart(tokens[start - 2].word) ?: break
                        local.add(0, before); start -= 2
                    }
                }
                val address = (local.joinToString(".") + "@" + domain.first.joinToString(".")).lowercase(Locale.ROOT)
                edits += Triple(tokens[start].start, tokens[domain.second].end, address + domain.third)
                i = domain.second + 1
            } else if (!ambiguous && bare(tokens.getOrNull(next)?.word ?: "") != "language") {
                val last = tokens[end].word
                edits += Triple(tokens[i].start, tokens[end].end, "@" + last.takeLastWhile { it in close })
                i = end + 1
            } else i = end + 1
        }
        return splice(text, edits)
    }

    // Apple's recognizer glued the domain to the marker ("at the rategmail.com"); Whisper small
    // glued "the rate" too ("john at therategmail.com", emulator microphone, Daniel voice).
    private fun gluedDomains(tokens: List<Token>, text: String): List<Token> = tokens.flatMapIndexed { i, token ->
        val lower = token.word.lowercase(Locale.ROOT)
        val before = tokens.getOrNull(i - 1)?.word?.lowercase(Locale.ROOT)
        if (before == "at" && lower.startsWith("therate") && isDomain(token.word.drop(7))) return@flatMapIndexed listOf(
            Token(token.word.take(3), token.start, token.start + 3), Token(token.word.substring(3, 7), token.start + 3, token.start + 7),
            Token(token.word.drop(7), token.start + 7, token.end))
        val prefix = when {
            before == "the" && tokens.getOrNull(i - 2)?.word?.lowercase(Locale.ROOT) == "at" && lower.startsWith("rate") -> "rate"
            before == "at" && lower.startsWith("sign") -> "sign"
            else -> null
        }
        if (prefix == null || !isDomain(token.word.drop(prefix.length))) listOf(token)
        else listOf(Token(token.word.take(prefix.length), token.start, token.start + prefix.length),
            Token(token.word.drop(prefix.length), token.start + prefix.length, token.end))
    }

    private fun isDomain(word: String): Boolean {
        val labels = core(word).split('.')
        val top = labels.last()
        return labels.size >= 2 && top.length >= 2 && top.all(Char::isLetter) && labels.all(::isLabel)
    }

    private fun symbolMarker(tokens: List<Token>, i: Int): Pair<Int, Boolean>? {
        fun word(offset: Int) = tokens.getOrNull(i + offset)?.word?.lowercase(Locale.ROOT)
        if (bare(tokens[i].word) == "at-sign") return i to false
        if (word(0) != "at") return null
        if (word(1)?.let(::bare) == "sign") return i + 1 to false
        if (word(1) == "the" && word(2)?.let(::bare) == "rate") return i + 2 to true
        return null
    }

    /** Labels joined by "dot" or "." (at least two, ending in letters), its end, and the marks after it. */
    private fun addressDomain(tokens: List<Token>, from: Int): Triple<List<String>, Int, String>? {
        val labels = mutableListOf<String>()
        var position = from
        var trailing = ""
        while (position < tokens.size) {
            val word = tokens[position].word
            val core = core(word)
            val parts = core.split('.')
            if (core.isEmpty() || !parts.all(::isLabel)) return null
            labels += parts
            trailing = word.drop(core.length)
            val following = tokens.getOrNull(position + 2)
            if (trailing.isNotEmpty() || tokens.getOrNull(position + 1)?.word?.lowercase(Locale.ROOT) != "dot" ||
                following == null || !isLabel(core(following.word).split('.').first())) break
            position += 2
        }
        val top = labels.lastOrNull() ?: return null
        if (labels.size < 2 || top.length < 2 || !top.all(Char::isLetter)) return null
        return Triple(labels, position, trailing)
    }

    private fun isLabel(label: String) = label.isNotEmpty() && label.length <= 63 && label.first() != '-' &&
        label.last() != '-' && label.all { it.code < 128 && (it.isLetterOrDigit() || it == '-') }

    private fun localPart(word: String): String? {
        if (word.isEmpty() || !word.last().isLetterOrDigit() || !word.first().isLetterOrDigit() ||
            !word.all { it.code < 128 && (it.isLetterOrDigit() || it in "._-+") }) return null
        return word
    }

    private val countWords = listOf("one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty")
        .withIndex().associate { it.value to it.index + 1 }

    /** A count of three or more numbers from one, with only commas or spaces between, in digits:
     * "Test one, two, three." -> "Test 1, 2, 3."; "Testing 123" -> "Testing 1, 2, 3". */
    fun counts(text: String, lang: String?): String {
        if (!english(lang)) return text
        val tokens = tokens(text)
        val edits = mutableListOf<Triple<Int, Int, String>>()
        var i = 0
        while (i < tokens.size) {
            val joined = joinedCount(tokens, i)
            if (joined != null) { edits += Triple(tokens[i].start, tokens[i].end, joined); i++; continue }
            val count = mutableListOf<Pair<Int, String>>()
            while (true) {
                val number = tokens.getOrNull(i + count.size)?.let { countNumber(it.word) } ?: break
                if (number.first != count.size + 1) break
                count += number
                if (number.second.isNotEmpty() && number.second != ",") break
            }
            val previous = tokens.getOrNull(i - 1)?.let { bare(it.word) } ?: ""
            val followsNumber = countNumber(previous) != null || previous == "zero" ||
                (previous.isNotEmpty() && previous.all(Char::isDigit))
            if (count.size < 3 || followsNumber) { i++; continue }
            val last = i + count.size - 1
            val written = count.mapIndexed { n, (value, trailing) -> "$value" + if (n == count.lastIndex) trailing else "," }
            edits += Triple(tokens[i].start, tokens[last].end, written.joinToString(" "))
            i = last + 1
        }
        return splice(text, edits)
    }

    private fun countNumber(word: String): Pair<Int, String>? {
        val core = core(word)
        val trailing = word.drop(core.length)
        countWords[core.lowercase(Locale.ROOT)]?.let { return it to trailing }
        if (core.isEmpty() || core.length > 2 || core.first() == '0' || !core.all { it in '0'..'9' }) return null
        return core.toInt() to trailing
    }

    private fun joinedCount(tokens: List<Token>, i: Int): String? {
        val before = tokens.getOrNull(i - 1) ?: return null
        if (bare(before.word) !in setOf("test", "testing") || end(before.word)) return null
        val word = tokens[i].word
        val core = core(word)
        val trailing = word.drop(core.length)
        if (core.length < 3 || !"123456789".startsWith(core) || (trailing.isEmpty() && i != tokens.lastIndex)) return null
        return core.toList().joinToString(", ") + trailing
    }
}
