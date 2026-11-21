package ly.anoma.hex.speech

import java.text.Normalizer
import java.util.Locale

/** Punctuation said by name ("comma", "question mark", "open quote") written as the mark, with
 * string rules and no model, in English, Spanish, French, German, Italian and Portuguese; other
 * languages pass through. Mirrors iOS SpokenPunctuation, guards included: a name after a
 * determiner, preposition, number or noun-making word, before a word that continues the noun, or
 * inside a compound stays a word, and everyday-noun names also need command-like placement. A mark
 * the model already wrote beside a name is replaced, never doubled. French spacing comes later. */
internal object SpokenPunctuation {
    fun format(text: String, language: String?): String {
        val table = table(language) ?: return text
        if (text.isEmpty()) return text
        return Writer(tokenize(text), table).write()
    }

    private val diacritics = Regex("\\p{Mn}+")
    /** Lowercased, accents folded, curly apostrophes straight: "D’Interrogation" reads "d'interrogation". */
    private fun fold(s: String) =
        diacritics.replace(Normalizer.normalize(s.lowercase(Locale.ROOT), Normalizer.Form.NFD), "").replace('’', '\'')

    private class Token(val text: String, val isWord: Boolean) {
        val folded = if (isWord) fold(text) else text
    }

    private fun wordChar(cp: Int) = Character.isAlphabetic(cp) || numberChar(cp) || cp == '\''.code || cp == '’'.code
    private fun numberChar(cp: Int) = Character.getType(cp).let {
        it == Character.DECIMAL_DIGIT_NUMBER.toInt() || it == Character.LETTER_NUMBER.toInt() || it == Character.OTHER_NUMBER.toInt()
    }
    private fun extends(cp: Int) = Character.getType(cp).let {
        it == Character.NON_SPACING_MARK.toInt() || it == Character.COMBINING_SPACING_MARK.toInt() || it == Character.ENCLOSING_MARK.toInt()
    }

    /** Alternating words (letters, digits, apostrophes) and the separators between them. A
     * combining mark stays with the character before it, as in a Swift Character. */
    private fun tokenize(text: String): List<Token> {
        val tokens = mutableListOf<Token>()
        val current = StringBuilder()
        var word: Boolean? = null
        var i = 0
        while (i < text.length) {
            val cp = text.codePointAt(i)
            val isWord = word?.takeIf { extends(cp) } ?: wordChar(cp)
            if (word == isWord) current.appendCodePoint(cp)
            else {
                word?.let { tokens += Token(current.toString(), it) }
                current.setLength(0); current.appendCodePoint(cp); word = isWord
            }
            i += Character.charCount(cp)
        }
        word?.let { tokens += Token(current.toString(), it) }
        return tokens
    }

    /** Marks a speech model puts around a spoken name: the name's mark replaces them. */
    private const val pauseMarks = ",.;:!?…"
    private const val openers = "([“«„¿¡\"‘"
    private const val closers = ")]”»“\"’"
    /** Endings after "apostrophe": "John apostrophe S" is "John's". */
    private val contractions = setOf("s", "t", "d", "m", "ll", "re", "ve")
    private fun newline(c: Char) = c in "\n\r\u000B\u000C\u0085\u2028\u2029"

    private class Writer(val tokens: List<Token>, val table: Table) {
        val out = StringBuilder()
        var capitalizeNext = false
        var lowercaseNext = false

        fun write(): String {
            var index = 0
            while (index < tokens.size) {
                val found = if (tokens[index].isWord) command(index) else null
                if (found == null) { emit(tokens[index]); index++; continue }
                val (command, end) = found
                // A name kept as said stays whole: "a dos puntos" never loses its "puntos".
                index = if (allows(command, index, end)) apply(command, end) else { for (i in index until end) emit(tokens[i]); end }
            }
            return out.toString()
        }

        private fun emit(token: Token) {
            if (!token.isWord) { out.append(token.text); return }
            out.append(when {
                lowercaseNext && token.folded in contractions -> token.text.lowercase(Locale.ROOT)
                capitalizeNext && token.text.first().isLowerCase() && token.text.drop(1).none(Char::isUpperCase) ->
                    token.text.replaceFirstChar { it.uppercase() }
                else -> token.text
            })
            capitalizeNext = false; lowercaseNext = false
        }

        /** The command whose words start at index, longest first, and where its words end. */
        private fun command(index: Int): kotlin.Pair<Command, Int>? {
            for (c in table.commands) phraseEnd(c.words, index)?.let { return c to it }
            return null
        }

        /** Between the words only spaces, a hyphen ("point-virgule") or one comma a model heard
         * as a pause ("close, Paren"). */
        private fun phraseEnd(words: List<String>, index: Int): Int? {
            var position = index
            for ((number, word) in words.withIndex()) {
                if (number > 0) {
                    if (position >= tokens.size || tokens[position].isWord) return null
                    val s = tokens[position].text
                    if (s.length > 3 || s.any { it !in " -," } || s.count { it == ',' } > 1) return null
                    position++
                }
                if (position >= tokens.size || !tokens[position].isWord || tokens[position].folded != word) return null
                position++
            }
            return position
        }

        private fun count(c: Char) = out.count { it == c }

        private fun isOpen(pair: Bracket) = when (pair) {
            Bracket.Paren -> count('(') > count(')')
            Bracket.Quote -> count(table.open) > count(table.close) || count('"') % 2 == 1
        }

        /** Whether a closing quote name follows end, with words between. */
        private fun closingQuote(end: Int, toggles: Boolean): Boolean {
            var index = end
            var between = false
            while (index < tokens.size) {
                if ('\n' in tokens[index].text) return false
                val found = if (tokens[index].isWord) command(index) else null
                if (found != null) {
                    val mark = found.first.mark
                    if (mark is Close && mark.pair == Bracket.Quote) return between
                    if (mark is ToggleQuote) return toggles && between
                    index = found.second
                    between = true
                    continue
                }
                between = between || tokens[index].isWord
                index++
            }
            return false
        }

        private fun allows(command: Command, index: Int, end: Int): Boolean {
            val before = if (index >= 1) tokens[index - 1].text else ""
            val after = if (end < tokens.size) tokens[end].text else ""
            val previous = if (index >= 2 && '\n' !in before) tokens[index - 2] else null
            val next = if (end + 1 < tokens.size && '\n' !in after) tokens[end + 1] else null
            val nextDirect = next != null && after.all { it == ' ' || it == '\t' }
            val mark = command.mark
            val family = command.family

            // A compound: "comma-separated", "pre-period", "and/or".
            if (after.firstOrNull().let { it == '-' || it == '/' }) return false
            if (family != Family.Hyphen && before.lastOrNull().let { it == '-' || it == '/' }) return false

            // A name: "I'll text Dash later", "George Smiley".
            if (table.capitalizesSentences && tokens[index].text.first().isUpperCase() && previous != null) {
                if (family == Family.Dash && before.none { it in ".!?" }) return false
                if (family == Family.Smiley && previous.text.first().isUpperCase()) return false
            }

            var opens = false
            var closes = false
            when (mark) {
                is Open, is Prefix -> opens = true
                is ToggleQuote -> {
                    opens = !isOpen(Bracket.Quote)
                    closes = !opens
                    if (opens && (next == null || !closingQuote(end, mark.toggles))) return false
                }
                is Close -> { if (!isOpen(mark.pair)) return false; closes = true }
                else -> {}
            }

            if (!opens) {
                // Attached to something said before it on the line; an open quote or bracket
                // closes after any word ("hello, their close quote").
                if (previous == null) return false
                if (!closes && nounPhrase(previous, index, family)) return false
            } else if (mark is Prefix && previous != null && nounPhrase(previous, index, family)) return false

            if (mark is Open) {
                // "The open quote was missing."
                if (previous != null && previous.folded in table.blockers) return false
                if (nextDirect && next!!.folded in table.followers) return false
                return next != null
            }
            if (mark is ToggleQuote && opens) return true

            // A decimal or a time: "two comma five", "3 colon 30".
            if (family in numeric && previous != null && nextDirect && number(previous) && number(next!!)) return false

            if (nextDirect) {
                val word = next!!.folded
                if (word in table.followers || table.familyFollowers[family]?.contains(word) == true ||
                    table.followerPrefixes.any { word.startsWith(it) }) return false
            }

            when (mark) {
                is Join -> {
                    val joinable = if (family == Family.Hyphen) " \t-" else " \t"
                    if (before.any { it !in joinable } || !nextDirect) return false
                }
                is Prefix -> if (!nextDirect) return false
                else -> {}
            }

            if (command.needsFollowing && next == null) return false
            if (command.strict) {
                val atLineEnd = next == null
                val pauseAfter = after.firstOrNull { it != ' ' && it != '\t' }?.let { it in pauseMarks || it in closers } ?: false
                val startsClause = nextDirect && next!!.let { n ->
                    n.folded in table.starters || table.starterPrefixes.any { n.folded.startsWith(it) } ||
                        (table.capitalizesSentences && n.text.first().isUpperCase())
                }
                val anotherCommand = next != null && command(end + 1) != null
                if (!(atLineEnd || pauseAfter || startsClause || anotherCommand)) return false
            }
            return true
        }

        private fun number(token: Token) = token.folded.all { numberChar(it.code) } || token.folded in table.numbers

        /** Whether the word before a name makes the name a noun: "a period", "the comma",
         * "trial period", "Sarah's colon", "ein wichtiger Punkt". */
        private fun nounPhrase(previous: Token, index: Int, family: Family): Boolean {
            val word = previous.folded
            if (word in table.blockers || table.familyBlockers[family]?.contains(word) == true) return true
            // An English or German sentence may end on a preposition or a particle ("Who is it
            // for?", "Ruf mich an."), a noun phrase not.
            if (word in table.prepositions && !(table.prepositionsEndSentences && family in sentenceEnds)) return true
            if (table.possessives && word.endsWith("'s") && word !in table.starters) return true
            // German nouns are capitalized: a lowercase word between an article and the name is
            // an adjective ("ein wichtiger Punkt"), not the end of a clause ("Ich komme morgen").
            return table.adjectivesBeforeNouns && previous.text.first().isLowerCase() && index >= 4 &&
                tokens[index - 3].text.all { it == ' ' } && tokens[index - 4].folded in table.blockers
        }

        /** Writes the command's mark and returns where reading goes on. */
        private fun apply(command: Command, end: Int): Int {
            val after = if (end < tokens.size) tokens[end].text else ""
            val resume = if (end < tokens.size) end + 1 else end
            val wordAfter = end + 1 < tokens.size
            fun rest(set: String) = after.dropWhile { it in set }
            fun spaced(rest: String) = if (rest.isEmpty() && wordAfter) " " else rest
            when (val mark = command.mark) {
                is Attach -> {
                    trimEnd(" \t" + pauseMarks)
                    out.append(mark.mark).append(spaced(rest(" \t" + pauseMarks)))
                    capitalizeNext = mark.mark in ".?!"
                }
                is LineEnd -> {
                    trimEnd(" \t" + pauseMarks)
                    out.append(mark.mark).append(mark.breaks).append(rest(" \t\n" + pauseMarks))
                    capitalizeNext = true
                }
                is Open -> { openWith(mark.mark); out.append(rest(" \t" + pauseMarks)) }
                is Close -> { closeWith(mark.pair); out.append(spaced(after)) }
                is ToggleQuote ->
                    if (isOpen(Bracket.Quote)) { closeWith(Bracket.Quote); out.append(spaced(after)) }
                    else { openWith(table.open.toString()); out.append(rest(" \t" + pauseMarks)) }
                is Join -> {
                    trimEnd(if (command.family == Family.Hyphen) " \t-" else " \t")
                    out.append(mark.mark).append(rest(" \t"))
                    lowercaseNext = command.family == Family.Apostrophe
                }
                is Prefix -> { openWith(mark.mark); out.append(rest(" \t")) }
                is Spaced -> { trimEnd(" \t,;:"); out.append(' ').append(mark.mark).append(spaced(rest(" \t,;:"))) }
                is Emoticon -> { trimEnd(" \t,"); out.append(' ').append(mark.face).append(spaced(rest(" \t,."))) }
            }
            return resume
        }

        private fun trimEnd(set: String) {
            while (out.isNotEmpty() && out.last() in set) out.setLength(out.length - 1)
        }

        /** An opening mark with a space before it, none after. */
        private fun openWith(mark: String) {
            trimEnd(" \t")
            val last = out.lastOrNull()
            if (last != null && !newline(last) && last !in openers) out.append(' ')
            out.append(mark)
        }

        /** A closing mark against the word before it; a straight quote closes a straight one. */
        private fun closeWith(pair: Bracket) {
            val straight = pair == Bracket.Quote && count('"') % 2 == 1
            trimEnd(" \t,;:")
            out.append(if (pair == Bracket.Paren) ')' else if (straight) '"' else table.close)
        }
    }

    private enum class Bracket { Paren, Quote }

    private sealed interface Mark
    /** Against the word before: , . ? ! : ; … */
    private class Attach(val mark: String) : Mark
    /** A mark, then line breaks: "punto y aparte". */
    private class LineEnd(val mark: String, val breaks: String) : Mark
    /** Before the next word: ( ¿ ¡ “ */
    private class Open(val mark: String) : Mark
    private class Close(val pair: Bracket) : Mark
    /** A bare "quote": opens, or closes the quote that is open. With toggles, two make a pair
     * ("Anführungszeichen … Anführungszeichen"); English needs "end quote". */
    private class ToggleQuote(val toggles: Boolean) : Mark
    /** Between two words, no spaces: - ' @ */
    private class Join(val mark: String) : Mark
    /** On the next word: # $ */
    private class Prefix(val mark: String) : Mark
    /** With a space each side: – */
    private class Spaced(val mark: String) : Mark
    private class Emoticon(val face: String) : Mark

    private enum class Family {
        Comma, Period, Question, Exclamation, Colon, Semicolon, Ellipsis, Dash, Hyphen,
        Apostrophe, At, Hashtag, Dollar, Quote, Paren, Smiley
    }
    private val numeric = setOf(Family.Comma, Family.Period, Family.Colon)
    private val sentenceEnds = setOf(Family.Question, Family.Exclamation, Family.Period)

    /** Strict: needs the line's end, a mark after it, or a clause start. Following: needs words after it. */
    private enum class Placement { Lenient, Strict, Following, StrictFollowing }

    private class Command(phrase: String, val mark: Mark, val family: Family, placement: Placement) {
        val words = fold(phrase).split(' ', '-').filter(String::isNotEmpty)
        val strict = placement == Placement.Strict || placement == Placement.StrictFollowing
        val needsFollowing = placement == Placement.Following || placement == Placement.StrictFollowing
    }
    private fun c(phrase: String, mark: Mark, family: Family, placement: Placement = Placement.Lenient) =
        Command(phrase, mark, family, placement)

    private fun words(s: String) = s.split(' ').filter(String::isNotEmpty).map(::fold)

    private class Table(
        commands: List<Command>,
        blockers: String,
        numbers: String,
        prepositions: String,
        val prepositionsEndSentences: Boolean = false,
        familyBlockers: Map<Family, String> = emptyMap(),
        followers: String,
        familyFollowers: Map<Family, String> = emptyMap(),
        followerPrefixes: String = "",
        starters: String,
        starterPrefixes: String = "",
        val open: Char,
        val close: Char,
        val capitalizesSentences: Boolean = true,
        val adjectivesBeforeNouns: Boolean = false,
        val possessives: Boolean = false,
    ) {
        val commands = commands.sortedByDescending { it.words.size }
        val blockers = words(blockers).toSet()
        val numbers = words(numbers).toSet()
        val prepositions = words(prepositions).toSet()
        val familyBlockers = familyBlockers.mapValues { words(it.value).toSet() }
        val followers = words(followers).toSet()
        val familyFollowers = familyFollowers.mapValues { words(it.value).toSet() }
        val followerPrefixes = words(followerPrefixes)
        val starters = words(starters).toSet()
        val starterPrefixes = words(starterPrefixes)
    }

    private fun table(language: String?): Table? {
        val code = language?.let { Locale.forLanguageTag(it.replace('_', '-')).language }?.takeIf(String::isNotEmpty) ?: "en"
        return when (code) {
            "en" -> english
            "es" -> spanish
            "fr" -> french
            "de" -> german
            "it" -> italian
            "pt" -> portuguese
            else -> null
        }
    }

    private val english by lazy {
        Table(
            commands = listOf(
                c("question mark", Attach("?"), Family.Question, Placement.Strict),
                c("exclamation point", Attach("!"), Family.Exclamation, Placement.Strict),
                c("exclamation mark", Attach("!"), Family.Exclamation, Placement.Strict),
                c("full stop", Attach("."), Family.Period),
                c("period", Attach("."), Family.Period, Placement.Strict),
                c("comma", Attach(","), Family.Comma),
                c("semicolon", Attach(";"), Family.Semicolon),
                c("semi colon", Attach(";"), Family.Semicolon),
                c("colon", Attach(":"), Family.Colon, Placement.Following),
                c("ellipsis", Attach("…"), Family.Ellipsis),
                c("dot dot dot", Attach("…"), Family.Ellipsis),
                c("em dash", Join("—"), Family.Dash, Placement.Strict),
                c("dash", Spaced("–"), Family.Dash),
                c("hyphen", Join("-"), Family.Hyphen),
                c("apostrophe", Join("'"), Family.Apostrophe),
                // "at sign" belongs to Clean up's spoken symbols, which run first.
                c("hashtag", Prefix("#"), Family.Hashtag),
                c("dollar sign", Prefix("$"), Family.Dollar),
                c("open quote", Open("“"), Family.Quote),
                c("open quotes", Open("“"), Family.Quote),
                c("begin quote", Open("“"), Family.Quote),
                c("close quote", Close(Bracket.Quote), Family.Quote),
                c("close quotes", Close(Bracket.Quote), Family.Quote),
                c("end quote", Close(Bracket.Quote), Family.Quote),
                c("end quotes", Close(Bracket.Quote), Family.Quote),
                c("unquote", Close(Bracket.Quote), Family.Quote),
                c("quote", ToggleQuote(false), Family.Quote),
                c("open paren", Open("("), Family.Paren),
                c("open parenthesis", Open("("), Family.Paren),
                c("open parentheses", Open("("), Family.Paren),
                c("close paren", Close(Bracket.Paren), Family.Paren),
                c("close parenthesis", Close(Bracket.Paren), Family.Paren),
                c("close parentheses", Close(Bracket.Paren), Family.Paren),
                c("smiley face", Emoticon(":-)"), Family.Smiley, Placement.Strict),
                c("smiley", Emoticon(":-)"), Family.Smiley, Placement.Strict),
                c("frowny face", Emoticon(":-("), Family.Smiley, Placement.Strict),
                c("frowny", Emoticon(":-("), Family.Smiley, Placement.Strict),
                c("winky face", Emoticon(";-)"), Family.Smiley, Placement.Strict),
                c("winky", Emoticon(";-)"), Family.Smiley, Placement.Strict),
            ),
            blockers = "a an the this that these those my your his her its our their no every each any some another " +
                "what which whose one of big huge giant little small large tiny red black blue green white yellow gold " +
                "golden silver bold bright extra missing stray misplaced single double triple final last first second " +
                "third next same whole entire upside inverted mad quick em en more fewer less",
            numbers = "zero one two three four five six seven eight nine ten",
            prepositions = "in on at to for with by from about into than like without per",
            prepositionsEndSentences = true,
            familyBlockers = mapOf(
                Family.Comma to "oxford serial decimal",
                Family.Period to "trial grace time free waiting cooling notice billing school class study rest " +
                    "transition transitional warranty introductory honeymoon probation probationary historical victorian " +
                    "jurassic cretaceous triassic renaissance colonial medieval early late given certain limited specific " +
                    "extended brief reporting accounting fiscal lunch recovery adjustment settling initial current previous " +
                    "short long missed heavy light fourth fifth sixth seventh eighth dry rainy wet holiday summer winter " +
                    "spring fall lockdown quiet busy difficult hard tough rough happy sad",
                Family.Colon to "sigmoid transverse descending ascending inflamed spastic irritable healthy",
                Family.Dash to "desperate frantic yard metre meter hundred fifty mile i you we they he she i'll we'll " +
                    "you'll they'll i'd will must gotta gonna can could should would might let's and",
                Family.Hashtag to "i you we they he she will can can't don't do didn't would should could must never " +
                    "always just popular trending viral new own official branded",
            ),
            followers = "of is was are were isn't wasn't aren't weren't has hasn't had",
            familyFollowers = mapOf(
                Family.Comma to "after before between in inside instead splice splices separated delimited key button " +
                    "placement rule rules usage sign symbol",
                Family.Period to "after before in during when where for piece pieces drama dramas costume costumes " +
                    "cramps cramp pain pains tracker app poverty products product underwear blood end ends ended starts " +
                    "started begins began lasts lasted correct accurate film films features furniture room rooms key " +
                    "button sign symbol to",
                Family.Question to "tattoo tattoos key keys button sign symbol symbols icon emoji shape shaped necklace " +
                    "over hanging hangs hung on in after at",
                Family.Exclamation to "tattoo tattoos key keys button sign symbol symbols icon emoji shape on in after at",
                Family.Colon to "cancer cancers surgery health cleanse cleansing polyps polyp screening screenings exam " +
                    "problems problem issues wall lining tumor tumour disease inflammation function symbol sign key " +
                    "character in after between and or",
                Family.Semicolon to "in after between key sign symbol usage",
                Family.Ellipsis to "in after key sign symbol usage",
                Family.Dash to "for to cam cams board button light lights through across into out off away over around " +
                    "home back past down up in on mat",
                Family.Hyphen to "in after between key sign symbol usage",
                Family.Apostrophe to "in after key sign symbol usage",
                Family.At to "key symbol",
                Family.Hashtag to "on in for and or to",
                Family.Dollar to "on in for and or key symbol",
                Family.Smiley to "emoji emojis icon sticker stickers faces ball balls",
            ),
            starters = "i i'm i'll i've i'd you you're you'll we we're we'll he he's she she's they they're it it's " +
                "that's there there's here here's this so and but or then because also plus thanks thank please see " +
                "let let's talk call hope love cheers ok okay yes yeah no hi hey hello sorry maybe anyway what what's " +
                "why how when where who if sure best regards just can could would will do did don't didn't not great " +
                "cool talk speak bye goodbye kind warm",
            open = '“', close = '”',
            possessives = true,
        )
    }

    private val spanish by lazy {
        Table(
            commands = listOf(
                c("punto y coma", Attach(";"), Family.Semicolon),
                c("punto y aparte", LineEnd(".", "\n\n"), Family.Period),
                c("punto y seguido", Attach("."), Family.Period),
                c("punto final", Attach("."), Family.Period),
                c("punto", Attach("."), Family.Period, Placement.Strict),
                c("coma", Attach(","), Family.Comma),
                c("dos puntos", Attach(":"), Family.Colon, Placement.StrictFollowing),
                c("abrir signo de interrogación", Open("¿"), Family.Question),
                c("abre signo de interrogación", Open("¿"), Family.Question),
                c("signo de interrogación de apertura", Open("¿"), Family.Question),
                c("abrir interrogación", Open("¿"), Family.Question),
                c("cerrar signo de interrogación", Attach("?"), Family.Question),
                c("cierra signo de interrogación", Attach("?"), Family.Question),
                c("signo de interrogación de cierre", Attach("?"), Family.Question),
                c("cerrar interrogación", Attach("?"), Family.Question),
                c("signo de interrogación", Attach("?"), Family.Question, Placement.Strict),
                c("abrir signo de exclamación", Open("¡"), Family.Exclamation),
                c("abre signo de exclamación", Open("¡"), Family.Exclamation),
                c("signo de exclamación de apertura", Open("¡"), Family.Exclamation),
                c("abrir exclamación", Open("¡"), Family.Exclamation),
                c("cerrar signo de exclamación", Attach("!"), Family.Exclamation),
                c("cierra signo de exclamación", Attach("!"), Family.Exclamation),
                c("signo de exclamación de cierre", Attach("!"), Family.Exclamation),
                c("cerrar exclamación", Attach("!"), Family.Exclamation),
                c("signo de exclamación", Attach("!"), Family.Exclamation, Placement.Strict),
                c("signo de admiración", Attach("!"), Family.Exclamation, Placement.Strict),
                c("abrir comillas", Open("“"), Family.Quote),
                c("abre comillas", Open("“"), Family.Quote),
                c("comillas de apertura", Open("“"), Family.Quote),
                c("comillas iniciales", Open("“"), Family.Quote),
                c("cerrar comillas", Close(Bracket.Quote), Family.Quote),
                c("cierra comillas", Close(Bracket.Quote), Family.Quote),
                c("comillas de cierre", Close(Bracket.Quote), Family.Quote),
                c("comillas finales", Close(Bracket.Quote), Family.Quote),
                c("comillas", ToggleQuote(true), Family.Quote),
                c("abrir paréntesis", Open("("), Family.Paren),
                c("abre paréntesis", Open("("), Family.Paren),
                c("paréntesis de apertura", Open("("), Family.Paren),
                c("cerrar paréntesis", Close(Bracket.Paren), Family.Paren),
                c("cierra paréntesis", Close(Bracket.Paren), Family.Paren),
                c("paréntesis de cierre", Close(Bracket.Paren), Family.Paren),
                c("puntos suspensivos", Attach("…"), Family.Ellipsis),
                c("guion", Join("-"), Family.Hyphen),
                c("arroba", Join("@"), Family.At),
                c("apóstrofo", Join("'"), Family.Apostrophe),
                c("hashtag", Prefix("#"), Family.Hashtag),
            ),
            blockers = "el la los las un una unos unas este esta ese esa aquel aquella mi mis tu tus su sus nuestro " +
                "nuestra cada otro otra ningún ninguna algún alguna del al de buen gran mismo misma primer último " +
                "tercer cuyo cuya",
            numbers = "cero uno dos tres cuatro cinco seis siete ocho nueve diez",
            prepositions = "a en con por para sin entre sobre hasta desde ante tras",
            followers = "de del es era fue son está estaba para en que",
            familyFollowers = mapOf(
                Family.Period to "medio muerto fuerte débil clave crítico cardinal cardinales culminante neutro álgido exacto",
                Family.Comma to "inducido etílico profundo diabético",
                Family.Colon to "más menos importantes clave principales",
            ),
            starters = "yo tú ella nosotros nosotras ellos ellas usted ustedes y pero o entonces porque también gracias " +
                "por nos te me se que qué como cómo cuando donde hola sí si no vale bueno oye saludos espero ya pues " +
                "luego después tengo estoy voy vamos llego quiero besos abrazos hasta adiós chao mira claro",
            open = '“', close = '”',
        )
    }

    private val french by lazy {
        Table(
            commands = listOf(
                c("point virgule", Attach(";"), Family.Semicolon),
                c("point d'interrogation", Attach("?"), Family.Question, Placement.Strict),
                c("point d'exclamation", Attach("!"), Family.Exclamation, Placement.Strict),
                c("points de suspension", Attach("…"), Family.Ellipsis),
                c("point final", Attach("."), Family.Period),
                c("point à la ligne", LineEnd(".", "\n"), Family.Period),
                c("point", Attach("."), Family.Period, Placement.Strict),
                c("virgule", Attach(","), Family.Comma),
                c("deux points", Attach(":"), Family.Colon, Placement.StrictFollowing),
                c("ouvrir les guillemets", Open("«"), Family.Quote),
                c("ouvrez les guillemets", Open("«"), Family.Quote),
                c("ouvrir guillemets", Open("«"), Family.Quote),
                c("ouvrez guillemets", Open("«"), Family.Quote),
                c("guillemets ouvrants", Open("«"), Family.Quote),
                c("guillemet ouvrant", Open("«"), Family.Quote),
                c("fermer les guillemets", Close(Bracket.Quote), Family.Quote),
                c("fermez les guillemets", Close(Bracket.Quote), Family.Quote),
                c("fermer guillemets", Close(Bracket.Quote), Family.Quote),
                c("fermez guillemets", Close(Bracket.Quote), Family.Quote),
                c("guillemets fermants", Close(Bracket.Quote), Family.Quote),
                c("guillemet fermant", Close(Bracket.Quote), Family.Quote),
                c("fin des guillemets", Close(Bracket.Quote), Family.Quote),
                c("guillemets", ToggleQuote(true), Family.Quote),
                c("ouvrir la parenthèse", Open("("), Family.Paren),
                c("ouvrez la parenthèse", Open("("), Family.Paren),
                c("ouvrir parenthèse", Open("("), Family.Paren),
                c("ouvrez parenthèse", Open("("), Family.Paren),
                c("parenthèse ouvrante", Open("("), Family.Paren),
                c("fermer la parenthèse", Close(Bracket.Paren), Family.Paren),
                c("fermez la parenthèse", Close(Bracket.Paren), Family.Paren),
                c("fermer parenthèse", Close(Bracket.Paren), Family.Paren),
                c("fermez parenthèse", Close(Bracket.Paren), Family.Paren),
                c("parenthèse fermante", Close(Bracket.Paren), Family.Paren),
                c("tiret", Spaced("–"), Family.Dash, Placement.Strict),
                c("trait d'union", Join("-"), Family.Hyphen),
                c("arobase", Join("@"), Family.At),
                c("apostrophe", Join("'"), Family.Apostrophe),
                c("hashtag", Prefix("#"), Family.Hashtag),
            ),
            blockers = "le la les l' un une des du de d' ce cet cette ces mon ma mes ton ta tes son sa ses notre nos " +
                "votre vos leur leurs aucun aucune chaque quel quelle quelque au aux bon mauvais dernier premier seul " +
                "petit grand gros même d'un d'une qu'un qu'une l'un",
            numbers = "zéro un deux trois quatre cinq six sept huit neuf dix",
            prepositions = "à en sur par pour avec sans dans entre vers chez",
            followers = "de du des est était sont important importante essentiel principal central faible fort commun " +
                "précis crucial culminant clé mort noir chaud flottante décimale",
            followerPrefixes = "d'",
            starters = "je tu il elle on nous vous ils elles ce ça et mais ou donc alors puis parce merci bonjour " +
                "bonsoir salut coucou oui non à bisous bises si quand comment pourquoi où qui que voilà ok cordialement " +
                "bonne bonnes d'accord désolé désolée",
            starterPrefixes = "j' c' qu' n' s' m' t'",
            open = '«', close = '»',
        )
    }

    private val german by lazy {
        Table(
            commands = listOf(
                c("komma", Attach(","), Family.Comma),
                c("punkt", Attach("."), Family.Period, Placement.Strict),
                c("fragezeichen", Attach("?"), Family.Question, Placement.Strict),
                c("ausrufezeichen", Attach("!"), Family.Exclamation, Placement.Strict),
                c("ausrufungszeichen", Attach("!"), Family.Exclamation, Placement.Strict),
                c("doppelpunkt", Attach(":"), Family.Colon, Placement.Following),
                c("semikolon", Attach(";"), Family.Semicolon),
                c("strichpunkt", Attach(";"), Family.Semicolon),
                c("auslassungspunkte", Attach("…"), Family.Ellipsis),
                c("anführungszeichen unten", Open("„"), Family.Quote),
                c("anführungszeichen auf", Open("„"), Family.Quote),
                c("anführungszeichen oben", Close(Bracket.Quote), Family.Quote),
                c("anführungszeichen zu", Close(Bracket.Quote), Family.Quote),
                c("abführungszeichen", Close(Bracket.Quote), Family.Quote),
                c("anführungszeichen", ToggleQuote(true), Family.Quote),
                c("klammer auf", Open("("), Family.Paren),
                c("öffnende klammer", Open("("), Family.Paren),
                c("klammer zu", Close(Bracket.Paren), Family.Paren),
                c("schließende klammer", Close(Bracket.Paren), Family.Paren),
                c("gedankenstrich", Spaced("–"), Family.Dash, Placement.Strict),
                c("bindestrich", Join("-"), Family.Hyphen),
                c("at zeichen", Join("@"), Family.At),
                c("klammeraffe", Join("@"), Family.At),
                c("apostroph", Join("'"), Family.Apostrophe),
                c("hashtag", Prefix("#"), Family.Hashtag),
            ),
            blockers = "der die das den dem des ein eine einen einem einer eines kein keine keinen keinem keiner mein " +
                "meine meinen meinem meiner dein deine deinen sein seine seinen seinem ihr ihre ihren ihrem unser " +
                "unsere unseren euer eure dieser diese dieses diesen diesem jeder jede jedes jeden welcher welche " +
                "welches zum zur im am um beim vom ins ans springende",
            numbers = "null eins zwei drei vier fünf sechs sieben acht neun zehn",
            prepositions = "mit ohne für auf an in nach vor bei von zu aus über unter gegen durch als",
            prepositionsEndSentences = true,
            followers = "ist war sind waren hat hatte für der des von auf eins zwei drei vier fünf sechs sieben acht " +
                "neun zehn elf zwölf uhr",
            starters = "ich du er sie es wir ihr und aber oder dann also denn weil danke bitte hallo hi hey ja nein bis " +
                "viele liebe lieber gruß grüße wie was wann wo warum wer wenn dass ok okay mal sorry tschüss schöne " +
                "schönen mach machs",
            open = '„', close = '“',
            capitalizesSentences = false,
            adjectivesBeforeNouns = true,
        )
    }

    private val italian by lazy {
        Table(
            commands = listOf(
                c("punto e virgola", Attach(";"), Family.Semicolon),
                c("punto interrogativo", Attach("?"), Family.Question, Placement.Strict),
                c("punto di domanda", Attach("?"), Family.Question, Placement.Strict),
                c("punto esclamativo", Attach("!"), Family.Exclamation, Placement.Strict),
                c("punto fermo", Attach("."), Family.Period),
                c("punto a capo", LineEnd(".", "\n"), Family.Period),
                c("punto", Attach("."), Family.Period, Placement.Strict),
                c("virgola", Attach(","), Family.Comma),
                c("due punti", Attach(":"), Family.Colon, Placement.StrictFollowing),
                c("puntini di sospensione", Attach("…"), Family.Ellipsis),
                c("tre puntini", Attach("…"), Family.Ellipsis),
                c("apri virgolette", Open("“"), Family.Quote),
                c("aperte virgolette", Open("“"), Family.Quote),
                c("aperta virgolette", Open("“"), Family.Quote),
                c("virgolette aperte", Open("“"), Family.Quote),
                c("chiudi virgolette", Close(Bracket.Quote), Family.Quote),
                c("chiuse virgolette", Close(Bracket.Quote), Family.Quote),
                c("chiusa virgolette", Close(Bracket.Quote), Family.Quote),
                c("virgolette chiuse", Close(Bracket.Quote), Family.Quote),
                c("virgolette", ToggleQuote(true), Family.Quote),
                c("apri parentesi", Open("("), Family.Paren),
                c("aperta parentesi", Open("("), Family.Paren),
                c("parentesi aperta", Open("("), Family.Paren),
                c("chiudi parentesi", Close(Bracket.Paren), Family.Paren),
                c("chiusa parentesi", Close(Bracket.Paren), Family.Paren),
                c("parentesi chiusa", Close(Bracket.Paren), Family.Paren),
                c("lineetta", Spaced("–"), Family.Dash, Placement.Strict),
                c("trattino", Join("-"), Family.Hyphen),
                c("chiocciola", Join("@"), Family.At),
                c("apostrofo", Join("'"), Family.Apostrophe),
                c("hashtag", Prefix("#"), Family.Hashtag),
                c("cancelletto", Prefix("#"), Family.Hashtag),
            ),
            blockers = "il lo la i gli le l' un uno una un' questo questa quel quello quella quei quegli quelle mio mia " +
                "miei mie tuo tua suo sua nostro nostra vostro vostra ogni nessun nessuno nessuna del della dello dei " +
                "degli delle al alla allo ai agli alle nel nella nello nei sul sulla di buon bel gran grande ultimo " +
                "primo stesso",
            numbers = "zero uno due tre quattro cinque sei sette otto nove dieci",
            prepositions = "a da in con su per tra fra",
            followers = "di del della dello dei era sono importante fondamentale chiave centrale debole forte cruciale " +
                "cardine morto vendita cui in",
            followerPrefixes = "d'",
            starters = "io tu lui lei noi voi loro e ma o poi quindi perché allora grazie ciao sì si no che come quando " +
                "dove chi ci ti mi vi ok va a buongiorno buonasera salve ecco dai baci ho hai ha abbiamo vado arrivo " +
                "vengo sono",
            starterPrefixes = "c' l'",
            open = '“', close = '”',
        )
    }

    private val portuguese by lazy {
        Table(
            commands = listOf(
                c("ponto e vírgula", Attach(";"), Family.Semicolon),
                c("ponto de interrogação", Attach("?"), Family.Question, Placement.Strict),
                c("ponto de exclamação", Attach("!"), Family.Exclamation, Placement.Strict),
                c("ponto final", Attach("."), Family.Period),
                c("ponto parágrafo", LineEnd(".", "\n\n"), Family.Period),
                c("ponto", Attach("."), Family.Period, Placement.Strict),
                c("vírgula", Attach(","), Family.Comma),
                c("dois pontos", Attach(":"), Family.Colon, Placement.StrictFollowing),
                c("reticências", Attach("…"), Family.Ellipsis),
                c("abre aspas", Open("“"), Family.Quote),
                c("abrir aspas", Open("“"), Family.Quote),
                c("fecha aspas", Close(Bracket.Quote), Family.Quote),
                c("fechar aspas", Close(Bracket.Quote), Family.Quote),
                c("aspas", ToggleQuote(true), Family.Quote),
                c("abre parênteses", Open("("), Family.Paren),
                c("abre parêntese", Open("("), Family.Paren),
                c("abrir parênteses", Open("("), Family.Paren),
                c("abrir parêntese", Open("("), Family.Paren),
                c("fecha parênteses", Close(Bracket.Paren), Family.Paren),
                c("fecha parêntese", Close(Bracket.Paren), Family.Paren),
                c("fechar parênteses", Close(Bracket.Paren), Family.Paren),
                c("fechar parêntese", Close(Bracket.Paren), Family.Paren),
                c("travessão", Spaced("–"), Family.Dash, Placement.Strict),
                c("hífen", Join("-"), Family.Hyphen),
                c("arroba", Join("@"), Family.At),
                c("apóstrofo", Join("'"), Family.Apostrophe),
                c("hashtag", Prefix("#"), Family.Hashtag),
            ),
            blockers = "o a os as um uma uns umas este esta estes estas esse essa esses essas aquele aquela meu minha " +
                "meus minhas teu tua seu sua seus suas nosso nossa cada nenhum nenhuma algum alguma outro outra do da " +
                "dos das no na nas ao aos pelo pela pelos pelas de bom grande mesmo último primeiro dois duas",
            numbers = "zero um uma dois duas três quatro cinco seis sete oito nove dez",
            prepositions = "em com por para sem entre sobre",
            followers = "de do da dos das era são foi principal importante chave central fraco forte crucial em morto " +
                "alto baixo",
            starters = "eu tu você ele ela nós eles elas vocês e mas ou então porque também obrigado obrigada olá oi " +
                "sim não que como quando onde quem até te me se beijos beijo abraço abraços tudo estou vou vamos chego " +
                "tenho",
            open = '“', close = '”',
        )
    }
}
