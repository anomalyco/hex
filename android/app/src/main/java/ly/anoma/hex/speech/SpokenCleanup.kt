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
}
