package ly.anoma.hex.speech

import java.text.BreakIterator

/** Faster English dictation's model writes every number in words ("twenty five dollars", "six forty five PM"), where
 * today's model writes digits. This writes English numbers said in words in digits, with string rules and no model: a
 * port, rule for rule, of the iPhone's SpokenNumbers.swift, so both apps write the same. Only the faster engine's text
 * goes through it, after [SentenceEnds]. Text that already has digits keeps them as written ("2,400", "$4.99", "7:15").
 *
 * What becomes digits:
 * - decimals ("five point five" 5.5, "zero point two" 0.2, "three point one point four" 3.1.4, "one point two million"
 *   1.2 million), percentages ("twenty percent" 20%);
 * - numbers from ten up and compounds ("twenty five" 25, "three hundred and twelve" 312, "fifteen thousand" 15,000;
 *   commas from 10,000, as Apple writes "8000" and "15,000");
 * - years ("nineteen ninety nine", "twenty twenty six");
 * - money with "dollars" ("five dollars" $5, "fifteen dollars and forty cents" $15.40);
 * - two to nine right before a plural noun or a unit ("two dogs", "four options", "six foot"), and "one" only before a
 *   unit ("one mile"), as a decimal, a percentage or money;
 * - ranges ending in a plural noun or unit ("five to ten minutes"), times with "am", "pm" or "o'clock" ("six am",
 *   "seven fifteen pm" 7:15), ordinals in dates ("November twenty second" November 22nd, "the fourth of July"), three
 *   or more spoken digits ("four seven two nine" 4729, "oh four seven two" 0472 mid-sentence; ten as 555-123-4567),
 *   and "minus" or "negative" before a number (-5, -$40).
 *
 * All of a number goes into digits or none of it: "a hundred and fifty" is 150, "one point twenty one" 1.21, "one in
 * ten" 1 in 10, "five foot six" 5 foot 6, never "a hundred and 50".
 *
 * What stays as said: a number the engine capitalised as a name, mid-sentence or with a capital inside, before another
 * capitalised word ("Twenty One Pilots", "Fifty Shades", "Ten Thousand Villages"); custom vocabulary and learned words
 * (protectedTerms), the person's fix for names the engine writes in lower case ("five guys"); arithmetic ("ten minus
 * five is five"); and, for now, "one" alone ("one of them", "no one"), times and prices without a marker ("three
 * thirty", "four ninety nine", "twelve fifty"), ranges and scores without a unit ("eight to twelve", "three to one"),
 * fractions ("two thirds", "one and a half"), ordinals outside dates ("finished first", "the twenty first floor", "a
 * second"), small numbers before anything but a plural noun or unit ("at eight", "GPT five"), and any run of number
 * words that could be read two ways ("one thirty", "seven fifteen"). English only: the caller sends nothing else. */
internal object SpokenNumbers {
    /** [protectedTerms] (custom vocabulary and learned words) stay as said wherever they match, whatever their case:
     * someone who adds "Five Guys" or "Seven Eleven" keeps it. */
    fun digits(text: String, protectedTerms: List<String> = emptyList()): String {
        if (text.isEmpty()) return text
        val tokens = protect(tokenize(text), protectedTerms)
        if (tokens.none { isNumberish(it) || isOrdinal(it.word) }) return text
        val edits = mutableListOf<Change>()
        var index = 0
        while (index < tokens.size) {
            when (val outcome = match(tokens, index)) {
                is Outcome.Edit -> {
                    edits += Change(tokens[outcome.first].start, tokens[outcome.last].end, outcome.text)
                    index = outcome.last + 1
                }
                is Outcome.Skip -> index = maxOf(outcome.last, index) + 1
                Outcome.None -> index++
            }
        }
        if (edits.isEmpty()) return text
        val result = StringBuilder()
        var cursor = 0
        for (edit in edits) {
            result.append(text, cursor, edit.start).append(edit.text)
            cursor = edit.end
        }
        return result.append(text, cursor, text.length).toString()
    }

    private class Change(val start: Int, val end: Int, val text: String)

    // Tokens

    /** A word between spaces, [start] until [end] in the text: the marks before it, the word, the marks after it. */
    private class Token(val start: Int, val end: Int, val lead: String, val core: String, val trail: String,
                        val hasDigit: Boolean, val startsSentence: Boolean, val letters: Boolean) {
        /** Inside a protected term. */
        var protected = false
        val word = core.lowercase()
        /** Nothing but the word: no mark after it ends the number. */
        val open: Boolean get() = trail.isEmpty()
        val capital get() = core.isNotEmpty() && Character.isUpperCase(core.codePointAt(0))
    }

    /** Where each of [text]'s characters starts, then the text's length. A character is what Swift counts as one, a
     * grapheme cluster ("1️⃣" is one), and is classed by its first code point, as Swift's Character properties are, so
     * words split here as on the iPhone. */
    private fun characters(text: String): IntArray {
        val bounds = BreakIterator.getCharacterInstance()
        bounds.setText(text)
        val starts = mutableListOf<Int>()
        var at = bounds.first()
        while (at != BreakIterator.DONE) {
            starts += at
            at = bounds.next()
        }
        return starts.toIntArray()
    }

    /** First in the text, or after "." "!" "?" "…" ":" or a line break, a word starts a sentence. */
    private fun tokenize(text: String): List<Token> {
        val at = characters(text)
        fun first(c: Int) = text.codePointAt(at[c])
        val tokens = mutableListOf<Token>()
        var previousEnd = 0
        var c = 0
        while (c < at.size - 1) {
            if (isSpace(first(c))) { c++; continue }
            var end = c
            while (end < at.size - 1 && !isSpace(first(end))) end++
            var coreStart = c
            while (coreStart < end && !isWordCharacter(first(coreStart))) coreStart++
            var coreEnd = end
            while (coreEnd > coreStart && !isWordCharacter(first(coreEnd - 1))) coreEnd--
            val previous = tokens.lastOrNull()
            val startsSentence = previous == null || (previousEnd until c).any { isLineBreak(first(it)) } ||
                previous.trail.isNotEmpty() && text.substring(at[previousEnd - 1], at[previousEnd]) in sentenceEnds
            tokens += Token(at[c], at[end], text.substring(at[c], at[coreStart]), text.substring(at[coreStart], at[coreEnd]),
                text.substring(at[coreEnd], at[end]), (c until end).any { isNumeral(first(it)) }, startsSentence,
                (coreStart until coreEnd).all { Character.isAlphabetic(first(it)) })
            previousEnd = end
            c = end
        }
        return tokens
    }

    private val sentenceEnds = setOf(".", "!", "?", "…", ":")
    /** Unicode's White_Space. */
    private fun isSpace(c: Int) = c in 0x09..0x0D || c == 0x20 || c == 0x85 || c == 0xA0 || c == 0x1680 ||
        c in 0x2000..0x200A || c == 0x2028 || c == 0x2029 || c == 0x202F || c == 0x205F || c == 0x3000
    private fun isLineBreak(c: Int) = c in 0x0A..0x0D || c == 0x85 || c == 0x2028 || c == 0x2029
    private fun isNumeral(c: Int) = Character.getType(c).let {
        it == Character.DECIMAL_DIGIT_NUMBER.toInt() || it == Character.LETTER_NUMBER.toInt() ||
            it == Character.OTHER_NUMBER.toInt()
    }
    private fun isWordCharacter(c: Int) = Character.isAlphabetic(c) || isNumeral(c)

    /** Marks the tokens of every match of a term, compared word by word without case or marks ("seven-eleven" matches
     * "Seven Eleven"). */
    private fun protect(tokens: List<Token>, terms: List<String>): List<Token> {
        fun words(text: String): List<String> {
            val lower = text.lowercase()
            val at = characters(lower)
            val words = mutableListOf<String>()
            var start = -1
            for (c in 0 until at.size - 1) {
                val inWord = isWordCharacter(lower.codePointAt(at[c]))
                if (inWord && start < 0) start = c
                if (!inWord && start >= 0) { words += lower.substring(at[start], at[c]); start = -1 }
            }
            if (start >= 0) words += lower.substring(at[start])
            return words
        }
        val flat = tokens.flatMapIndexed { index, token -> words(token.core).map { it to index } }
        for (term in terms) {
            val wanted = words(term)
            if (wanted.isEmpty() || wanted.size > flat.size ||
                wanted.none { kind(it) != null || ordinals[it] != null || it == "oh" }) continue
            for (start in 0..flat.size - wanted.size) {
                if (wanted.indices.all { flat[start + it].first == wanted[it] })
                    for (offset in wanted.indices) tokens[flat[start + offset].second].protected = true
            }
        }
        return tokens
    }

    private sealed interface Outcome {
        /** Tokens [first] through [last] become [text]. */
        class Edit(val first: Int, val last: Int, val text: String) : Outcome
        /** Tokens up to [last] stay as said. */
        class Skip(val last: Int) : Outcome
        object None : Outcome
    }

    // Words

    private sealed class Kind(val value: Long) {
        class Digit(value: Long) : Kind(value)
        class Teen(value: Long) : Kind(value)
        class Tens(value: Long) : Kind(value)
        class TensUnit(value: Long) : Kind(value)
        object Hundred : Kind(100)
        class Big(value: Long) : Kind(value)
    }

    private val digitWords = mapOf(
        "zero" to 0L, "one" to 1L, "two" to 2L, "three" to 3L, "four" to 4L, "five" to 5L, "six" to 6L, "seven" to 7L,
        "eight" to 8L, "nine" to 9L,
    )
    private val teenWords = mapOf(
        "ten" to 10L, "eleven" to 11L, "twelve" to 12L, "thirteen" to 13L, "fourteen" to 14L, "fifteen" to 15L,
        "sixteen" to 16L, "seventeen" to 17L, "eighteen" to 18L, "nineteen" to 19L,
    )
    private val tensWords = mapOf(
        "twenty" to 20L, "thirty" to 30L, "forty" to 40L, "fifty" to 50L, "sixty" to 60L, "seventy" to 70L,
        "eighty" to 80L, "ninety" to 90L,
    )
    private val bigWords = mapOf(
        "thousand" to 1_000L, "million" to 1_000_000L, "billion" to 1_000_000_000L, "trillion" to 1_000_000_000_000L,
    )
    private val ordinals = mapOf(
        "first" to 1L, "second" to 2L, "third" to 3L, "fourth" to 4L, "fifth" to 5L, "sixth" to 6L, "seventh" to 7L,
        "eighth" to 8L, "ninth" to 9L, "tenth" to 10L, "eleventh" to 11L, "twelfth" to 12L, "thirteenth" to 13L,
        "fourteenth" to 14L, "fifteenth" to 15L, "sixteenth" to 16L, "seventeenth" to 17L, "eighteenth" to 18L,
        "nineteenth" to 19L, "twentieth" to 20L, "thirtieth" to 30L, "fortieth" to 40L, "fiftieth" to 50L,
        "sixtieth" to 60L, "seventieth" to 70L, "eightieth" to 80L, "ninetieth" to 90L, "hundredth" to 100L,
        "thousandth" to 1_000L,
    )
    private val months = setOf(
        "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
        "december",
    )
    /** Units after a number, singular or short: "six foot", "five K", "ten GB". */
    private val units = setOf(
        "k", "km", "kg", "mph", "kph", "gb", "mb", "tb", "kb", "lb", "lbs", "oz", "ml", "mg", "cm", "mm", "ft",
        "mile", "foot", "inch", "kilo", "kilogram", "gram", "pound", "ounce", "degree", "liter", "litre",
        "gallon", "meter", "metre", "kilometer", "kilometre",
    )
    /** The units "one" becomes 1 before ("one mile"); not "foot" ("one foot in the door"). */
    private val oneUnits = setOf(
        "km", "kg", "mph", "gb", "mb", "tb", "kb", "lb", "oz", "mile", "inch", "kilo", "kilogram", "gram",
        "pound", "ounce", "degree", "liter", "litre", "gallon", "meter", "metre", "kilometer", "kilometre",
    )
    private val irregularPlurals = setOf("people", "children", "men", "women", "feet", "teeth", "mice", "geese", "cattle")
    /** Words that may stand between a number and its plural noun: "two more minutes". */
    private val bridges = setOf(
        "more", "other", "extra", "new", "old", "big", "small", "little", "different", "separate",
        "additional", "whole", "full", "main", "quick", "short", "long", "free", "good", "great", "bad",
        "major", "minor", "key", "young", "large", "huge", "tiny",
    )
    /** Words ending in "s" that aren't plural nouns, or after which a number stays a word: verbs ("two equals"),
     * fractions ("two thirds"), decades ("the nineteen nineties"). */
    private val notPlural = setOf(
        "this", "his", "hers", "its", "yes", "less", "unless", "perhaps", "always", "towards", "besides",
        "whereas", "across", "plus", "minus", "thus", "was", "has", "does", "goes", "gets", "says", "seems",
        "means", "makes", "equals", "times", "ours", "yours", "theirs", "sometimes", "lets", "afterwards",
        "nowadays", "looks", "sounds", "needs", "wants", "takes", "comes", "works", "feels", "stays",
        "becomes", "remains", "keeps", "tells", "gives", "knows", "thinks",
    )
    private val fractions = setOf(
        "half", "halves", "third", "thirds", "quarter", "quarters", "fourths", "fifths", "sixths", "sevenths",
        "eighths", "ninths", "tenths",
    )
    private val decades = setOf(
        "twenties", "thirties", "forties", "fifties", "sixties", "seventies", "eighties", "nineties",
        "hundreds", "thousands", "millions", "billions", "dozen", "dozens",
    )

    /** "twenty-five" as two parts; empty parts dropped, as Swift's split does. */
    private fun hyphenParts(word: String) = word.split('-').filter { it.isNotEmpty() }

    private fun kind(word: String): Kind? {
        digitWords[word]?.let { return Kind.Digit(it) }
        teenWords[word]?.let { return Kind.Teen(it) }
        tensWords[word]?.let { return Kind.Tens(it) }
        if (word == "hundred") return Kind.Hundred
        bigWords[word]?.let { return Kind.Big(it) }
        // "twenty-five", as some models write it.
        val parts = hyphenParts(word)
        if (parts.size == 2) {
            val tens = tensWords[parts[0]]
            val unit = digitWords[parts[1]]
            if (tens != null && unit != null && unit > 0) return Kind.TensUnit(tens + unit)
        }
        return null
    }

    private fun isNumberish(token: Token) = !token.hasDigit && !token.protected && kind(token.word) != null

    private fun isOrdinal(word: String): Boolean {
        if (word in ordinals) return true
        val parts = hyphenParts(word)
        return parts.size == 2 && parts[0] in tensWords && parts[1] in ordinals
    }

    /** A single digit in a run or after "point": "oh" too. */
    private fun spokenDigit(token: Token): Long? {
        if (token.hasDigit || token.protected) return null
        return if (token.word == "oh") 0L else digitWords[token.word]
    }

    // Matching

    private fun match(t: List<Token>, i: Int): Outcome {
        val token = t[i]
        if (token.hasDigit || token.protected) return Outcome.None
        // "November twenty second", "July fourth": "May" and "March" only with a capital.
        if (token.word in months && token.open &&
            (token.word != "may" && token.word != "march" || token.capital)) {
            val day = ordinal(t, i + 1)
            if (day != null) return Outcome.Edit(i + 1, day.end, t[i + 1].lead + day.text + t[day.end].trail)
        }
        // "the fourth of July".
        if (token.word == "the" && token.open) {
            val day = ordinal(t, i + 1)
            if (day != null && t[day.end].open && day.end + 2 < t.size && t[day.end + 1].word == "of" &&
                t[day.end + 1].open && t[day.end + 2].word in months) {
                return Outcome.Edit(i + 1, day.end, t[i + 1].lead + day.text)
            }
        }
        if ((token.word == "minus" || token.word == "negative") && token.open && !followsNumber(t, i)) {
            val after = number(t, i + 1, forced = true)
            if (after is Outcome.Edit && after.first == i + 1) return Outcome.Edit(i, after.last, token.lead + "-" + after.text)
            return Outcome.None
        }
        digitRun(t, i)?.let { return it }
        return number(t, i, forced = false)
    }

    /** "a hundred" or "a thousand" that a number goes on from: "a hundred and fifty" is 150. */
    private fun leadsScale(t: List<Token>, i: Int) =
        t[i].word == "a" && t[i].open && i + 2 < t.size && t[i + 1].open && !t[i + 1].protected &&
            !t[i + 1].hasDigit && (t[i + 1].word == "hundred" || t[i + 1].word == "thousand")

    /** A number the engine capitalised mid-sentence, or with a capital on a later word even at a sentence start: with a
     * capitalised word after it, a name ("Twenty One Pilots", "Fifty Shades", "Eleven Labs", "Ten Thousand Villages").
     * "Twenty people" at a sentence start can't be told from a title, so it is a number. */
    private fun isName(t: List<Token>, i: Int, end: Int): Boolean {
        fun capitalised(token: Token) = token.capital && token.core != token.core.uppercase()
        if (!t[i].capital || end + 1 >= t.size || !t[end + 1].capital) return false
        if (t[i].startsSentence && !(i < end && (i + 1..end).any { capitalised(t[it]) })) return false
        val next = t[end + 1].word
        return next != "i" && !next.startsWith("i'") && !next.startsWith("i’")
    }

    private fun followsNumber(t: List<Token>, i: Int) =
        i > 0 && t[i - 1].open && (t[i - 1].hasDigit || isNumberish(t[i - 1]))

    /** Words written for tokens up to [end]; [ambiguous] when a number word follows. */
    private class Written(val text: String, val end: Int, val ambiguous: Boolean = false)

    /** A day said as an ordinal, 1st to 31st: "third", "twenty second", "thirty-first". */
    private fun ordinal(t: List<Token>, j: Int): Written? {
        if (j >= t.size || t[j].hasDigit || t[j].protected) return null
        val word = t[j].word
        var value: Long? = null
        var end = j
        val tens = tensWords[word]
        val unit = if (j + 1 < t.size) ordinals[t[j + 1].word] else null
        if (ordinals[word] != null) {
            value = ordinals[word]
        } else if (tens != null && t[j].open && j + 1 < t.size && !t[j + 1].hasDigit && !t[j + 1].protected &&
            unit != null && unit < 10) {
            value = tens + unit
            end = j + 1
        } else {
            val parts = hyphenParts(word)
            val tensPart = parts.getOrNull(0)?.let { tensWords[it] }
            val unitPart = parts.getOrNull(1)?.let { ordinals[it] }
            if (parts.size == 2 && tensPart != null && unitPart != null && unitPart < 10) value = tensPart + unitPart
        }
        if (value == null || value !in 1L..31L) return null
        return Written("$value${suffix(value)}", end)
    }

    private fun suffix(value: Long) = if (value % 100 in 11L..13L) "th" else when (value % 10) {
        1L -> "st"
        2L -> "nd"
        3L -> "rd"
        else -> "th"
    }

    /** Three or more digits said one by one: "four seven two nine" 4729, "nine one one" 911, and ten (with or without
     * commas between groups of 3, 3 and 4) as 555-123-4567. */
    private fun digitRun(t: List<Token>, i: Int): Outcome? {
        // "oh" leads a run only mid-sentence and in lower case ("the code is oh four seven two"), never "Oh, one more
        // thing"; a run right after such an "Oh" stays in words.
        val leadingOh = t[i].word == "oh"
        if (leadingOh && (t[i].startsSentence || t[i].core != "oh" || !t[i].open)) return null
        if (!leadingOh && i > 0 && t[i - 1].word == "oh" && t[i - 1].open) return Outcome.Skip(clusterEnd(t, i))
        val first = spokenDigit(t[i]) ?: return null
        if (followsNumber(t, i)) return null
        val digits = mutableListOf(first)
        val groups = mutableListOf<Int>()
        var group = 1
        var end = i
        while (end + 1 < t.size) {
            val next = spokenDigit(t[end + 1]) ?: break
            if (!t[end].open && t[end].trail != ",") break
            if (!t[end].open) {
                groups += group
                group = 0
            }
            digits += next
            group++
            end++
        }
        groups += group
        if (digits.size < 3) return null
        if (leadingOh && groups.size > 1) return null
        if (t[end].open && end + 1 < t.size) {
            // "seven oh five am" is a time.
            if (isMeridiem(t[end + 1])) return null
            if (isNumberish(t[end + 1]) || t[end + 1].hasDigit) return Outcome.Skip(end)
        }
        val joined = digits.joinToString("")
        if (digits.size == 10 && (groups == listOf(10) || groups == listOf(3, 3, 4))) {
            return Outcome.Edit(i, end, t[i].lead + joined.substring(0, 3) + "-" + joined.substring(3, 6) + "-" +
                joined.substring(6) + t[end].trail)
        }
        // A list said with commas ("five, six, seven") isn't one number.
        if (groups.size != 1) return null
        return Outcome.Edit(i, end, t[i].lead + joined + t[end].trail)
    }

    // Numbers

    private class Cardinal(val value: Long, val end: Int, val words: Int, val isYear: Boolean = false,
                           val hasScale: Boolean = false) {
        /** "five million": the value and word of the last scale when it ends the number. */
        var bigScale: Pair<Long, String>? = null
        var bigScales = 0
        var ambiguous = false
        /** The value when the number is one digit word ("two"). */
        var single: Long? = null
    }

    private enum class Last { None, Units, Teen, Tens, Hundred, Big, And }

    /** "nineteen ninety nine", "twenty twenty six", "nineteen oh five". */
    private fun year(t: List<Token>, i: Int): Cardinal? {
        if (!t[i].open || i + 1 >= t.size || t[i + 1].hasDigit) return null
        val century = when (t[i].word) {
            "nineteen" -> 19L
            "twenty" -> 20L
            else -> return null
        }
        val j = i + 1
        fun digit(after: Int): Long? {
            if (!t[after].open || after + 1 >= t.size || t[after + 1].hasDigit) return null
            val kind = kind(t[after + 1].word)
            return if (kind is Kind.Digit && kind.value > 0) kind.value else null
        }
        var rest: Long? = null
        var end = j
        when (val kind = kind(t[j].word)) {
            is Kind.Teen -> rest = kind.value
            is Kind.Tens -> {
                rest = kind.value
                val unit = digit(j)
                if (unit != null) {
                    rest = kind.value + unit
                    end = j + 1
                }
            }
            is Kind.TensUnit -> rest = kind.value
            else -> {
                val unit = if (t[j].word == "oh") digit(j) else null
                if (unit != null) {
                    rest = unit
                    end = j + 1
                }
            }
        }
        val value = rest ?: return null
        return Cardinal(century * 100 + value, end, end - i + 1, isYear = true)
    }

    private fun cardinal(t: List<Token>, i: Int): Cardinal? {
        if (i >= t.size || !(isNumberish(t[i]) || leadsScale(t, i))) return null
        val parsed = year(t, i) ?: run {
            val fromA = leadsScale(t, i)
            var total = 0L
            var current = if (fromA) 1L else 0L
            var last = if (fromA) Last.Units else Last.None
            var words = if (fromA) 1 else 0
            var end = i
            var j = if (fromA) i + 1 else i
            var hasScale = false
            var lastBig: Pair<Long, String>? = null
            var bigScales = 0
            var ambiguous = false
            var zero = false
            val adds = setOf(Last.None, Last.Hundred, Last.Big, Last.And)
            scan@ while (j < t.size) {
                val token = t[j]
                if (token.hasDigit) break
                if (token.word == "and" && (last == Last.Hundred || last == Last.Big) && token.open && j + 1 < t.size &&
                    isNumberish(t[j + 1]) && additive(kind(t[j + 1].word))) {
                    last = Last.And
                    j++
                    continue
                }
                when (val found = kind(token.word) ?: break@scan) {
                    is Kind.Digit -> {
                        if (found.value == 0L) {
                            if (words != 0) { ambiguous = true; break@scan }
                            zero = true
                        } else {
                            if (last !in adds && last != Last.Tens) { ambiguous = true; break@scan }
                            current += found.value
                        }
                        last = Last.Units
                    }
                    is Kind.Teen, is Kind.Tens, is Kind.TensUnit -> {
                        if (last !in adds) { ambiguous = true; break@scan }
                        current += found.value
                        last = when (found) {
                            is Kind.Tens -> Last.Tens
                            is Kind.Teen -> Last.Teen
                            else -> Last.Units
                        }
                    }
                    Kind.Hundred -> {
                        if (last != Last.Units && last != Last.Teen && last != Last.Tens || current !in 1L..99L) {
                            ambiguous = words > 0
                            break@scan
                        }
                        current *= 100
                        last = Last.Hundred
                        hasScale = true
                    }
                    is Kind.Big -> {
                        if (last != Last.Units && last != Last.Teen && last != Last.Tens && last != Last.Hundred ||
                            current <= 0 || lastBig != null && found.value >= lastBig.first) {
                            ambiguous = words > 0
                            break@scan
                        }
                        total += current * found.value
                        current = 0
                        last = Last.Big
                        hasScale = true
                        lastBig = found.value to token.word
                        bigScales++
                    }
                }
                words++
                end = j
                j++
                if (zero || !token.open) break
            }
            if (words == 0) return null
            // "a hundred" alone stays: "a hundred percent", "a hundred dollars".
            if (fromA && end <= i + 1) return null
            Cardinal(total + current, end, words, hasScale = hasScale).also {
                it.bigScale = if (last == Last.Big) lastBig else null
                it.bigScales = bigScales
                it.ambiguous = ambiguous
                val first = kind(t[i].word)
                if (words == 1 && first is Kind.Digit) it.single = first.value
            }
        }
        // Another number word right after, or before, makes it two numbers or a time: "three thirty", "four ninety
        // nine", "twenty twenty five hundred".
        if (t[parsed.end].open && parsed.end + 1 < t.size && isNumberish(t[parsed.end + 1])) parsed.ambiguous = true
        if (followsNumber(t, i) && !t[i - 1].hasDigit) parsed.ambiguous = true
        // The rest of a number we didn't take whole: "… hundred and fifty", "… point twenty one".
        if (i >= 2 && t[i - 1].open && t[i - 2].open && !t[i - 2].hasDigit && !t[i - 2].protected &&
            (t[i - 1].word == "and" && (t[i - 2].word == "hundred" || bigWords[t[i - 2].word] != null) ||
                t[i - 1].word == "point" && kind(t[i - 2].word) != null)) {
            parsed.ambiguous = true
        }
        return parsed
    }

    private fun additive(kind: Kind?) = when (kind) {
        is Kind.Digit -> kind.value > 0
        is Kind.Teen, is Kind.Tens, is Kind.TensUnit -> true
        else -> false
    }

    /** The last token of the run of number words from [i]. */
    private fun clusterEnd(t: List<Token>, i: Int): Int {
        var j = i
        while (j + 1 < t.size && t[j].open && (isNumberish(t[j + 1]) || t[j + 1].word == "point")) j++
        return j
    }

    private fun written(number: Cardinal): String {
        if (number.isYear) return number.value.toString()
        val big = number.bigScale
        if (big != null && number.bigScales == 1 && big.first >= 1_000_000 && number.value % big.first == 0L) {
            return "${number.value / big.first} ${big.second}"
        }
        return grouped(number.value)
    }

    /** Commas from 10,000 up: Apple writes "8000" and "2400" but "15,000". */
    private fun grouped(value: Long): String {
        val plain = value.toString()
        if (value < 10_000) return plain
        return plain.reversed().chunked(3).joinToString(",").reversed()
    }

    /** "point five", "point one point four" after a number, and a scale after one decimal ("one point two million"). */
    private fun decimal(t: List<Token>, number: Cardinal, i: Int): Written? {
        if (number.isYear || number.bigScale != null || !t[number.end].open || number.end + 2 >= t.size ||
            t[number.end + 1].word != "point" || !t[number.end + 1].open || t[number.end + 1].hasDigit) return null
        // "at one point", "that one point".
        if (number.value == 1L && number.words == 1 && i > 0 &&
            t[i - 1].word in listOf("at", "some", "that", "this", "any", "the", "one", "a")) return null
        var text = number.value.toString()
        var end = number.end
        var points = 0
        var j = number.end + 1
        while (j + 1 < t.size && t[end].open && t[j].word == "point" && t[j].open && !t[j].hasDigit) {
            var digits = ""
            var k = j + 1
            val pair = twoDigits(t, k)
            if (pair != null) {
                // "one point twenty one" is 1.21.
                digits = pair.text
                end = pair.end
                k = pair.end + 1
            } else {
                while (k < t.size) {
                    val digit = spokenDigit(t[k]) ?: break
                    digits += digit.toString()
                    end = k
                    k++
                    if (!t[end].open) break
                }
            }
            if (digits.isEmpty()) break
            text += ".$digits"
            points++
            j = k
        }
        if (points == 0) return null
        if (points == 1 && t[end].open && end + 1 < t.size && (bigWords[t[end + 1].word] ?: 0L) >= 1_000_000 &&
            !t[end + 1].hasDigit) {
            text += " " + t[end + 1].word
            end++
        }
        return Written(text, end, t[end].open && end + 1 < t.size && isNumberish(t[end + 1]))
    }

    /** "twenty one", "fifteen", "forty" after "point". */
    private fun twoDigits(t: List<Token>, k: Int): Written? {
        if (k >= t.size || t[k].hasDigit || t[k].protected) return null
        return when (val kind = kind(t[k].word)) {
            is Kind.Teen, is Kind.TensUnit -> Written(kind.value.toString(), k)
            is Kind.Tens -> {
                val unit = if (t[k].open && k + 1 < t.size && !t[k + 1].hasDigit && !t[k + 1].protected) {
                    kind(t[k + 1].word)
                } else null
                if (unit is Kind.Digit && unit.value > 0) Written((kind.value + unit.value).toString(), k + 1)
                else Written(kind.value.toString(), k)
            }
            else -> null
        }
    }

    private fun number(t: List<Token>, i: Int, forced: Boolean): Outcome {
        if (i >= t.size || !(isNumberish(t[i]) || leadsScale(t, i))) return Outcome.None
        clockTime(t, i)?.let { return it }
        val parsed = cardinal(t, i) ?: return Outcome.None
        if (parsed.ambiguous) return Outcome.Skip(clusterEnd(t, i))
        var text = written(parsed)
        var end = parsed.end
        var isDecimal = false
        val point = decimal(t, parsed, i)
        if (point != null) {
            if (point.ambiguous) return Outcome.Skip(clusterEnd(t, point.end))
            text = point.text
            end = point.end
            isDecimal = true
        }
        val lead = t[i].lead
        val next = end + 1
        val open = t[end].open && next < t.size
        // Digits right beside it: the model already wrote this number its way.
        if (open && t[next].hasDigit) return Outcome.Skip(end)
        if (i > 0 && t[i - 1].open && t[i - 1].hasDigit) return Outcome.Skip(end)
        if (!forced && isName(t, i, end)) return Outcome.Skip(end)
        val plain = !isDecimal && !parsed.isYear && !parsed.hasScale
        // "ten minus five is five": arithmetic stays as said, whole.
        if (open && !forced && t[next].word in listOf("minus", "plus", "times", "over", "equals") && t[next].open &&
            next + 1 < t.size && isNumberish(t[next + 1])) {
            var last = clusterEnd(t, next + 1)
            // …and its result: "ten times two is twenty".
            if (t[last].open && last + 2 < t.size && t[last + 1].word in listOf("is", "equals", "makes") &&
                t[last + 1].open && isNumberish(t[last + 2])) {
                last = clusterEnd(t, last + 2)
            }
            return Outcome.Skip(last)
        }
        // "one in ten people", "eight out of ten", "nine times out of ten": with the second number in digits, the first
        // goes too.
        if (open && !forced && plain) {
            val second = ratioSecond(t, end)
            if (second != null) {
                val after = number(t, second, forced = false)
                if (after is Outcome.Edit && after.first == second) return Outcome.Edit(i, end, lead + text)
            }
        }
        // "five foot six": inches after a height in digits.
        if (!forced && plain && parsed.value <= 11 && i >= 2 && (t[i - 1].word == "foot" || t[i - 1].word == "feet") &&
            t[i - 1].open && t[i - 2].open && (t[i - 2].hasDigit || height(t, i - 2))) {
            return Outcome.Edit(i, end, lead + text + t[end].trail)
        }
        if (open) {
            val word = t[next].word
            if (word == "percent") return Outcome.Edit(i, next, lead + text + "%" + t[next].trail)
            if (word == "per" && t[next].open && next + 1 < t.size && t[next + 1].word == "cent") {
                return Outcome.Edit(i, next + 1, lead + text + "%" + t[next + 1].trail)
            }
            if (word == "dollar" || word == "dollars") return money(t, i, text, isDecimal, next)
            // "twenty first": an ordinal, outside dates for now.
            if (isOrdinal(word)) return Outcome.Skip(next)
            if (plain && parsed.value in 1L..12L && (isMeridiem(t[next]) || word == "o'clock" || word == "o’clock")) {
                return Outcome.Edit(i, end, lead + text)
            }
            if (!forced && plain && parsed.value < 100) range(t, parsed, i, end, lead + text)?.let { return it }
            if (isCountNoun(t, next)) {
                if (forced || !plain || parsed.value >= 10) return Outcome.Edit(i, end, lead + text)
                val single = parsed.single
                if (single != null) {
                    val possessive = i > 0 && t[i - 1].word in listOf("my", "your", "his", "her", "our", "their")
                    if (single >= 2 && !(word == "cents" && possessive)) return Outcome.Edit(i, end, lead + text)
                    if (single == 1L && word in oneUnits) return Outcome.Edit(i, end, lead + text)
                }
                return Outcome.Skip(end)
            }
            if (word in fractions || word in decades) return Outcome.Skip(end)
            if (word == "and" && next + 2 < t.size && t[next + 1].word == "a" && t[next + 2].word in fractions) {
                return Outcome.Skip(end)
            }
        }
        if (forced || !plain || parsed.value >= 10) return Outcome.Edit(i, end, lead + text + t[end].trail)
        return Outcome.Skip(end)
    }

    /** Where the second number of "one in ten", "eight out of ten", "nine times out of ten" starts. */
    private fun ratioSecond(t: List<Token>, end: Int): Int? {
        fun word(k: Int) = if (k < t.size && t[k].open) t[k].word else null
        var k = end + 1
        if (word(k) == "times") k++
        if (word(k) == "in" && k == end + 1) k++
        else if (word(k) == "out" && word(k + 1) == "of") k += 2
        else return null
        return if (k < t.size && isNumberish(t[k])) k else null
    }

    /** The feet of a height said in words: "five" in "five foot six". */
    private fun height(t: List<Token>, k: Int): Boolean {
        val feet = cardinal(t, k) ?: return false
        if (feet.ambiguous || feet.end != k) return false
        val value = feet.single ?: return false
        return value >= 2
    }

    /** "five to ten minutes", "two or three days", "between five and ten minutes": the first number goes into digits
     * with the second, which its own rule writes, only when a plural noun or unit follows. Without one it may be a
     * score, a time or "nine to five", so both stay as said. "one or two" stays too. */
    private fun range(t: List<Token>, first: Cardinal, i: Int, end: Int, text: String): Outcome? {
        val link = end + 1
        val word = t[link].word
        val between = word == "and" && i > 0 && t[i - 1].word == "between"
        if (!(word in listOf("to", "through", "or", "past") || between) || !t[link].open) return null
        val second = cardinal(t, link + 1) ?: return null
        if (second.ambiguous) return null
        var secondEnd = second.end
        var secondPlain = !second.isYear && !second.hasScale && second.value < 100
        val point = decimal(t, second, link + 1)
        if (point != null) {
            secondEnd = point.end
            secondPlain = false
        }
        // "from zero to one hundred": a second number that is plainly a number takes the first.
        if (!secondPlain) {
            // "two to three hundred" and "one or two hundred" mean hundreds: both stay as said.
            val endsWithScale = t[secondEnd].word == "hundred" || bigWords[t[secondEnd].word] != null
            if (word == "past" || endsWithScale && (word == "or" || first.value >= 2)) return Outcome.Skip(secondEnd)
            return Outcome.Edit(i, end, text)
        }
        val after = if (secondEnd + 1 < t.size && t[secondEnd].open) t[secondEnd + 1].word else ""
        val percent = after == "percent" || after == "per" && secondEnd + 2 < t.size && t[secondEnd + 2].word == "cent"
        val unitAfter = t[secondEnd].open && (isCountNoun(t, secondEnd + 1) || percent)
        if (word == "past" || !unitAfter || word == "or" && first.single == 1L) return Outcome.Skip(secondEnd)
        return Outcome.Edit(i, end, text)
    }

    /** "seven fifteen pm" as 7:15, "seven oh five am" as 7:05: the marker stays as written. */
    private fun clockTime(t: List<Token>, i: Int): Outcome? {
        if (i + 2 >= t.size || !t[i].open || !t[i + 1].open || t[i + 1].hasDigit) return null
        val first = kind(t[i].word)
        val hour = when {
            first is Kind.Digit && first.value > 0 -> first.value
            first is Kind.Teen && first.value <= 12 -> first.value
            else -> return null
        }
        var minutes: Long? = null
        var end = i + 1
        val third = if (!t[i + 2].hasDigit) kind(t[i + 2].word) else null
        val unit = if (third is Kind.Digit && third.value > 0) third.value else null
        val second = kind(t[i + 1].word)
        if (second is Kind.Teen) {
            minutes = second.value
        } else if (second is Kind.Tens && second.value <= 50) {
            minutes = second.value
            if (unit != null) {
                minutes = second.value + unit
                end = i + 2
            }
        } else if (second is Kind.TensUnit && second.value < 60) {
            minutes = second.value
        } else if (t[i + 1].word == "oh" && unit != null) {
            minutes = unit
            end = i + 2
        }
        if (minutes == null || !t[end].open || end + 1 >= t.size || !isMeridiem(t[end + 1])) return null
        return Outcome.Edit(i, end, t[i].lead + "$hour:" + (if (minutes < 10) "0" else "") + minutes)
    }

    private fun isMeridiem(token: Token): Boolean {
        val plain = token.word.replace(".", "")
        return !token.hasDigit && (plain == "am" || plain == "pm")
    }

    /** "$5", "$15.40" (with "and forty cents"), "$1.2 million", "$5.50". */
    private fun money(t: List<Token>, i: Int, amount: String, isDecimal: Boolean, dollars: Int): Outcome {
        var written = amount
        var end = dollars
        val dot = written.indexOf('.')
        if (isDecimal && dot >= 0 && written.length - dot == 2) written += "0"
        if (!isDecimal && ' ' !in written && t[dollars].open) {
            var j = dollars + 1
            if (j < t.size && t[j].word == "and" && t[j].open) j++
            val cents = cardinal(t, j)
            if (cents != null && !cents.ambiguous && !cents.isYear && !cents.hasScale && cents.value in 1L..99L &&
                t[cents.end].open && cents.end + 1 < t.size && t[cents.end + 1].word.let { it == "cents" || it == "cent" }) {
                written += "." + (if (cents.value < 10) "0" else "") + cents.value
                end = cents.end + 1
            }
        }
        return Outcome.Edit(i, end, t[i].lead + "\$" + written + t[end].trail)
    }

    /** A plural noun or a unit: "dogs", "people", "more minutes", "foot", "K". */
    private fun isCountNoun(t: List<Token>, k: Int): Boolean {
        if (k >= t.size || t[k].hasDigit || t[k].core.isEmpty()) return false
        val word = t[k].word
        if (word in units) return true
        if (word in bridges && t[k].open) return isPlural(t, k + 1)
        return isPlural(t, k)
    }

    private fun isPlural(t: List<Token>, k: Int): Boolean {
        if (k >= t.size || t[k].hasDigit || t[k].core.isEmpty() || !Character.isLowerCase(t[k].core.codePointAt(0)) ||
            !t[k].letters) return false
        val word = t[k].word
        if (word in irregularPlurals) return true
        if (word.length < 3 || !word.endsWith("s") || word in notPlural || word in fractions || word in decades) return false
        return listOf("ss", "us", "is").none { word.endsWith(it) }
    }
}
