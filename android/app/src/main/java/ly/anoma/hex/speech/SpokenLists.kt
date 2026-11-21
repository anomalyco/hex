package ly.anoma.hex.speech

import java.text.Normalizer
import java.util.Locale

/** Dictated lists and line breaks as lines, in English and Spanish; other languages stay as said.
 * Mirrors iOS SpokenLists: runs inside a style, after its fillers and before its punctuation.
 * - "new line", "new paragraph" ("nueva línea", "nuevo párrafo", "salto de línea") become line
 *   breaks, never at the start, after "a", "the", "una", … or before "of", "is", "de", … .
 * - "first … second …", "number one … number two …", "one, … two, …" become "1. …" lines when
 *   the markers count up from one, every marker starts a clause and no item is empty;
 *   "finally" ends a list of two or more.
 * - "bullet", "next item", "dash" ("viñeta", "guion") said twice or more become "- …" lines.
 * - Text before the list is its own line, given a colon unless it ends a sentence; the last item
 *   ends at its first sentence and what follows is its own line. */
internal object SpokenLists {
    fun format(text: String, language: String?): String {
        val table = Table.forLanguage(language) ?: return text
        return if (text.isEmpty()) text else listing(breaking(text, table), table)
    }

    private class Token(val text: String, val isWord: Boolean, val folded: String)

    private val marks = Regex("\\p{M}+")
    /** Lowercased, accents and "ñ" folded: "Viñeta" reads "vineta". */
    private fun fold(s: String) = marks.replace(Normalizer.normalize(s.lowercase(Locale.ROOT), Normalizer.Form.NFD), "")
    private fun letterOrNumber(cp: Int) = Character.isLetter(cp) || Character.getType(cp).toByte().let {
        it == Character.DECIMAL_DIGIT_NUMBER || it == Character.LETTER_NUMBER || it == Character.OTHER_NUMBER
    }
    private fun isMark(cp: Int) = Character.getType(cp).toByte().let {
        it == Character.NON_SPACING_MARK || it == Character.ENCLOSING_MARK || it == Character.COMBINING_SPACING_MARK
    }
    private fun hspace(c: Char) = c == '\t' || Character.getType(c) == Character.SPACE_SEPARATOR.toInt()

    /** Alternating words (letters, digits, apostrophes) and the separators between them. Combining
     * marks stay with the character before them, as in a Swift Character. */
    private fun tokenize(text: String): List<Token> {
        val tokens = mutableListOf<Token>()
        val current = StringBuilder()
        var currentIsWord: Boolean? = null
        fun flush() {
            val isWord = currentIsWord ?: return
            val s = current.toString()
            tokens += Token(s, isWord, if (isWord) fold(s) else s)
        }
        var i = 0
        while (i < text.length) {
            val cp = text.codePointAt(i)
            val isWord = currentIsWord?.takeIf { isMark(cp) } ?: (letterOrNumber(cp) || cp == '\''.code || cp == '’'.code)
            if (currentIsWord != isWord) { flush(); current.setLength(0); currentIsWord = isWord }
            current.appendCodePoint(cp)
            i += Character.charCount(cp)
        }
        flush()
        return tokens
    }

    private fun joined(tokens: List<Token>, from: Int, to: Int = tokens.size) =
        tokens.subList(from, to).joinToString("") { it.text }

    /** Marks that end a clause, so a marker after them starts one. */
    private const val boundaries = ".,;:!?…—–\n(¿¡"

    private fun small(digits: String) =
        if (digits.length in 1..2 && digits.all { it in '0'..'9' }) digits.toInt().takeIf { it in 1..20 } else null

    private class Words(val tokens: List<Token>, val table: Table) {
        fun isWord(i: Int) = i in tokens.indices && tokens[i].isWord
        fun spacesOnly(i: Int) = i in tokens.indices && !tokens[i].isWord && tokens[i].text.all { it == ' ' || it == '\t' }

        /** Where the words said one after another (spaces between) from "at" end. */
        fun phrase(words: List<String>, at: Int): Int? {
            var p = at
            for ((n, word) in words.withIndex()) {
                if (n > 0) { if (!spacesOnly(p)) return null; p++ }
                if (!isWord(p) || tokens[p].folded != word) return null
                p++
            }
            return p
        }
        fun nextWord(end: Int) = if (spacesOnly(end) && isWord(end + 1)) tokens[end + 1].folded else null
        fun previousWord(i: Int) = if (i >= 2 && spacesOnly(i - 1) && isWord(i - 2)) tokens[i - 2].folded else null

        /** The text's start, after a mark or a line break, or after connectors ("and then") that start one. */
        fun startsClause(i: Int): Boolean {
            var p = i
            while (p >= 2) {
                if (tokens[p - 1].text.any { it in boundaries }) return true
                if (tokens[p - 2].folded !in table.connectors) return false
                p -= 2
            }
            return true
        }
        fun separator(end: Int) = if (end in tokens.indices && !tokens[end].isWord) tokens[end].text else ""
        fun number(i: Int) = if (!isWord(i)) null else tokens[i].folded.let { table.cardinals[it] ?: small(it) }
    }

    private fun Set<String>.has(word: String?) = word != null && word in this

    private fun breaking(text: String, table: Table): String {
        val words = Words(tokenize(text), table)
        val tokens = words.tokens
        val out = StringBuilder()
        var i = 0
        var sawWord = false
        while (i < tokens.size) {
            val lineBreak = if (tokens[i].isWord && sawWord) lineBreak(i, words) else null
            if (lineBreak != null) {
                while (out.isNotEmpty() && out.last() in " \t,;") out.setLength(out.length - 1)
                out.append(lineBreak.first)
                i = lineBreak.second
                if (i < tokens.size && !tokens[i].isWord) out.append(tokens[i++].text.dropWhile { it in " \t,.;:" })
                continue
            }
            sawWord = sawWord || tokens[i].isWord
            out.append(tokens[i++].text)
        }
        return out.toString()
    }

    private fun lineBreak(i: Int, words: Words): Pair<String, Int>? {
        for ((phrase, breaks) in words.table.breaks) {
            val end = words.phrase(phrase, i) ?: continue
            if (words.table.determiners.has(words.previousWord(i))) return null
            if (words.table.breakFollowers.has(words.nextWord(end))) return null
            return breaks to end
        }
        return null
    }

    private const val ORDINAL = "ordinal"
    private const val CARDINAL = "cardinal"
    private const val BULLET = "bullet"

    /** "value" is the item's number, 0 for "finally"; "family" is also "keyword <word>". */
    private class Marker(val family: String, val value: Int, val start: Int, val end: Int)

    private fun markers(words: Words): List<Marker> {
        val found = mutableListOf<Marker>()
        var i = 0
        while (i < words.tokens.size) {
            val marker = if (words.isWord(i)) marker(i, words) else null
            if (marker != null) { found += marker; i = marker.end } else i++
        }
        return found
    }

    private fun marker(i: Int, w: Words): Marker? {
        val t = w.table
        val word = w.tokens[i].folded
        for ((phrase, needsMark) in t.finals) {
            val end = w.phrase(phrase, i) ?: continue
            if (!w.startsClause(i)) continue
            val after = w.separator(end)
            if (needsMark && !after.startsWith(",") && !after.startsWith(":")) continue
            return Marker(ORDINAL, 0, i, end)
        }
        for ((phrase, value) in t.ordinalPhrases) {
            val end = w.phrase(phrase, i) ?: continue
            if (w.startsClause(i)) return Marker(ORDINAL, value, i, end)
        }
        if (word in t.keywords && w.spacesOnly(i + 1) && w.startsClause(i)) {
            w.number(i + 2)?.let { return Marker("keyword $word", it, i, i + 3) }
        }
        for ((phrase, strict) in t.bullets) {
            val end = w.phrase(phrase, i) ?: continue
            if (!w.startsClause(i) && (strict || t.determiners.has(w.previousWord(i)))) return null
            if (t.bulletFollowers.has(w.nextWord(end))) return null
            return Marker(BULLET, 1, i, end)
        }
        val ordinal = t.ordinal(word)
        if (ordinal != null && w.startsClause(i)) {
            if (w.spacesOnly(i + 1) && w.phrase(t.ofAll, i + 2) != null) return Marker(ORDINAL, ordinal, i, i + 1 + 2 * t.ofAll.size)
            if (t.ordinalFollowers.has(w.nextWord(i + 1))) return null
            return Marker(ORDINAL, ordinal, i, i + 1)
        }
        val number = w.number(i)
        if (number != null && w.startsClause(i)) {
            // A bare number counts only with its mark and a space after: "one, eggs", "1. eggs",
            // never "1:30" or "1.5".
            val separator = w.separator(i + 1)
            val mark = separator.firstOrNull() ?: return null
            val spaced = separator.getOrNull(1)?.isWhitespace() ?: (i + 2 >= w.tokens.size)
            return if (mark in ",:.)" && spaced) Marker(CARDINAL, number, i, i + 1) else null
        }
        return null
    }

    /** Numbered runs counting up from 1, and two bullets or more. */
    private fun lists(markers: List<Marker>): List<List<Marker>> {
        val runs = mutableListOf<List<Marker>>()
        for ((position, first) in markers.withIndex()) {
            if (first.family == BULLET) {
                val run = markers.drop(position).filter { it.family == BULLET }
                if (run.size >= 2) runs += run
                continue
            }
            if (first.value != 1) continue
            val run = mutableListOf(first)
            for (marker in markers.drop(position + 1)) {
                if (marker.family != first.family) continue
                if (marker.value == run.size + 1) run += marker
                else if (marker.value == 0 && run.size >= 2) { run += marker; break }
            }
            if (run.size >= 2) runs += run
        }
        return runs
    }

    private fun listing(text: String, table: Table): String {
        val words = Words(tokenize(text), table)
        // A run with an empty item ("one, two, three, go") is no list; a later one may be.
        for (run in lists(markers(words))) format(run, words)?.let { return it }
        return text
    }

    private fun format(run: List<Marker>, w: Words): String? {
        val tokens = w.tokens
        val table = w.table
        val last = run.last()

        // The last item ends at its first sentence (or line); what follows is its own line.
        var itemsEnd = tokens.size
        var tailBreak = "\n"
        var sawWord = false
        for (i in last.end until tokens.size) {
            if (tokens[i].isWord) { sawWord = true; continue }
            if (!sawWord || i + 1 >= tokens.size) continue
            val separator = tokens[i].text
            if ('\n' in separator || endsSentence(separator, tokens[i - 1].folded)) {
                itemsEnd = i
                tailBreak = if ("\n\n" in separator) "\n\n" else "\n"
                break
            }
        }

        val items = run.mapIndexed { n, marker ->
            var raw = joined(tokens, marker.end, if (n + 1 < run.size) run[n + 1].start else itemsEnd)
            // The last item keeps its sentence's mark ("?", "!") but not what follows.
            if (n + 1 == run.size && itemsEnd < tokens.size) raw += tokens[itemsEnd].text.takeWhile { !it.isWhitespace() }
            cleanItem(raw, table).ifEmpty { return null }
        }

        val output = StringBuilder()
        val leadIn = joined(tokens, 0, run[0].start)
        val leadTrimmed = leadIn.trimEnd(' ', '\t')
        if (leadTrimmed.endsWith("\n")) output.append(leadTrimmed)
        else {
            var lead = stripConnectors(leadIn.trim(), table)
            while (lead.isNotEmpty() && lead.last() in ",;-–—") lead = lead.dropLast(1).trim(::hspace)
            if (lead.isNotEmpty()) {
                if (letterOrNumber(lead.codePointBefore(lead.length))) lead += ":"
                output.append(lead).append('\n')
            }
        }

        val bullet = run[0].family == BULLET
        items.forEachIndexed { n, item ->
            if (n > 0) output.append('\n')
            output.append(if (bullet) "- " else "${n + 1}. ").append(item)
        }

        if (itemsEnd < tokens.size) {
            val tail = joined(tokens, itemsEnd + 1).trim()
            if (tail.isNotEmpty()) output.append(tailBreak).append(listing(tail, table))
        }
        return output.toString()
    }

    private val abbreviations = "mr mrs ms dr st jr sr prof vs etc inc ltd approx sra dra ud uds".split(' ').toSet()

    /** ".", "!", "?" or "…" then a space, never after "Dr" or an initial. */
    private fun endsSentence(separator: String, word: String): Boolean {
        val mark = separator.indexOfFirst { it in ".!?…" }
        if (mark < 0 || separator.getOrNull(mark + 1)?.isWhitespace() != true) return false
        return !(separator[mark] == '.' && (word in abbreviations || (word.length == 1 && word[0].isLetter())))
    }

    /** No leading marks, no trailing comma, period, "and" or "or", and a capital first letter. */
    private fun cleanItem(raw: String, table: Table): String {
        var item = raw.dropWhile { it in " \t\n,:.;)-–—" }
        var previous = ""
        while (previous != item) {
            previous = item
            item = item.trim()
            if (!item.endsWith("...") && item.isNotEmpty() && item.last() in ",;:.") item = item.dropLast(1)
            item = stripConnectors(item, table)
        }
        return capitalized(item)
    }

    /** Without the connectors ("and", "y luego") a next item's marker leaves at the end. */
    private fun stripConnectors(text: String, table: Table): String {
        var t = text
        while (true) {
            val space = t.indexOfLast { it == ' ' || it == ',' }
            if (space < 0) break
            val last = fold(t.substring(space + 1))
            if (last !in table.connectors && last !in table.alternatives) break
            t = t.substring(0, space).trim(::hspace).trimEnd(',')
        }
        return t
    }

    /** Never on "iPhone" or an address. */
    private fun capitalized(text: String): String {
        val first = text.indexOfFirst(Char::isLetter)
        if (first < 0) return text
        val word = text.substring(first).takeWhile { !it.isWhitespace() }
        if ('@' in word || word.drop(1).any(Char::isUpperCase)) return text
        return text.substring(0, first) + text[first].toString().uppercase(Locale.ROOT) + text.substring(first + 1)
    }

    private fun words(s: String) = s.split(' ').toSet()
    private fun <T> phrases(vararg p: Pair<String, T>) = p.map { (phrase, v) -> phrase.split(' ') to v }

    private class Table(
        /** Phrases that break a line. */
        val breaks: List<Pair<List<String>, String>>,
        /** Words before a phrase that make it a noun: "a new line", "the bullet". */
        val determiners: Set<String>,
        /** Words after "new line" that make it a noun: "new line of shoes". */
        val breakFollowers: Set<String>,
        val connectors: Set<String>,
        val alternatives: Set<String>,
        val ordinals: Map<String, Int>,
        val digitOrdinalSuffixes: List<String>,
        val ordinalPhrases: List<Pair<List<String>, Int>>,
        /** Words right after an ordinal that keep it prose: "first time", "second guess". */
        val ordinalFollowers: Set<String>,
        val ofAll: List<String>,
        /** "finally", "lastly"; the flag asks for a comma after ("last, …"). */
        val finals: List<Pair<List<String>, Boolean>>,
        val cardinals: Map<String, Int>,
        val keywords: Set<String>,
        /** Longest first; the flag asks for a clause start. */
        val bullets: List<Pair<List<String>, Boolean>>,
        /** Words right after "bullet" that keep it prose: "bullet points", "bullet train". */
        val bulletFollowers: Set<String>,
    ) {
        fun ordinal(word: String) = ordinals[word]
            ?: digitOrdinalSuffixes.firstNotNullOfOrNull { if (word.endsWith(it)) small(word.dropLast(it.length)) else null }

        companion object {
            fun forLanguage(language: String?): Table? =
                when (language?.let { Locale.forLanguageTag(it.replace('_', '-')).language }?.ifEmpty { null } ?: "en") {
                    "en" -> english
                    "es" -> spanish
                    else -> null
                }

            val english = Table(
                breaks = phrases("new paragraph" to "\n\n", "new line" to "\n", "line break" to "\n"),
                determiners = words("a an the this that these those my your his her its our their one another every each " +
                    "any some no to of in on with for at by from silver magic rubber plastic stray whole entire"),
                breakFollowers = words("of items item is was are were has had will would that which for from cook cooks " +
                    "worker workers manager managers up breaks"),
                connectors = words("and then also plus"),
                alternatives = words("or"),
                ordinals = mapOf("first" to 1, "firstly" to 1, "second" to 2, "secondly" to 2, "third" to 3, "thirdly" to 3,
                    "fourth" to 4, "fourthly" to 4, "fifth" to 5, "fifthly" to 5, "sixth" to 6, "seventh" to 7, "eighth" to 8,
                    "ninth" to 9, "tenth" to 10),
                digitOrdinalSuffixes = listOf("st", "nd", "rd", "th"),
                ordinalPhrases = emptyList(),
                ordinalFollowers = words("time times place thing things one ones day days week weeks month months year " +
                    "years half class person people hand floor grade round prize guess guessed guessing nature rate aid " +
                    "base cousin cousins date name impression choice language lady world edition gear love sight glance " +
                    "mate chance try attempt look night kiss draft opinion wave quarter season episode chapter page row " +
                    "team degree letter word words course act part period hour minute thought thoughts off up to in on " +
                    "at and or is was came come served last of string fiddle semester term born child son daughter baby " +
                    "wife husband job line generation step steps officer lieutenant amendment street avenue best most " +
                    "largest biggest"),
                ofAll = listOf("of", "all"),
                finals = phrases("last but not least" to false, "finally" to false, "lastly" to false, "last" to true),
                cardinals = mapOf("one" to 1, "two" to 2, "three" to 3, "four" to 4, "five" to 5, "six" to 6, "seven" to 7,
                    "eight" to 8, "nine" to 9, "ten" to 10),
                keywords = words("number step item point"),
                bullets = phrases("bullet point" to false, "next bullet" to false, "next item" to false, "bullet" to false,
                    "dash" to true),
                bulletFollowers = words("points proof proofs train trains wound wounds hole holes journal journals casing " +
                    "casings shell shells ant ants is was hit hits went flew of in list lists"),
            )

            val spanish = Table(
                breaks = phrases("nuevo parrafo" to "\n\n", "nueva linea" to "\n", "salto de linea" to "\n"),
                determiners = words("un una el la los las este esta estos estas ese esa eso mi mis tu tus su sus nuestra " +
                    "nuestro otra otro cada de del con en para por al a"),
                breakFollowers = words("de del es fue que para y"),
                connectors = words("y e luego despues tambien"),
                alternatives = words("o u"),
                ordinals = mapOf("primero" to 1, "primera" to 1, "primeramente" to 1, "segundo" to 2, "segunda" to 2,
                    "tercero" to 3, "tercera" to 3, "cuarto" to 4, "cuarta" to 4, "quinto" to 5, "quinta" to 5, "sexto" to 6,
                    "sexta" to 6, "septimo" to 7, "septima" to 7, "octavo" to 8, "octava" to 8, "noveno" to 9, "novena" to 9,
                    "decimo" to 10, "decima" to 10),
                digitOrdinalSuffixes = listOf("º", "ª"),
                ordinalPhrases = phrases("en primer lugar" to 1, "en segundo lugar" to 2, "en tercer lugar" to 3,
                    "en cuarto lugar" to 4, "en quinto lugar" to 5),
                ordinalFollowers = words("vez veces lugar dia dias plato piso mano grado premio parte nombre ano anos mes " +
                    "semana momento paso pasos hijo hija ministro ministra actor plano mundo tiempo ciclo curso nivel " +
                    "puesto de del en que es fue y o a con amor intento capitulo temporada clase fila"),
                ofAll = listOf("de", "todo"),
                finals = phrases("en ultimo lugar" to false, "por ultimo" to false, "finalmente" to false),
                cardinals = mapOf("uno" to 1, "una" to 1, "dos" to 2, "tres" to 3, "cuatro" to 4, "cinco" to 5, "seis" to 6,
                    "siete" to 7, "ocho" to 8, "nueve" to 9, "diez" to 10),
                keywords = words("numero paso"),
                bullets = phrases("siguiente vineta" to false, "siguiente elemento" to false, "vineta" to false,
                    "guion" to true),
                bulletFollowers = words("de del es fue que"),
            )
        }
    }
}
