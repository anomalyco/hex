package ly.anoma.hex.typing

/** What a space or closing mark does to the text before the cursor: [original], which that text
 * ends with, becomes [replacement]; the space or mark goes in after it. [original] may hold the
 * word before too ("well se" ... "well see" -> "we'll see"). */
internal data class Correction(val original: String, val replacement: String)

/** One slot of the suggestion strip. [Kind.Typed] keeps what was typed as typed; [Kind.Fix] is what
 * space will put in its place (shown highlighted); [Kind.Word] a completion or another reading;
 * [Kind.Next] a word that may follow, offered after a space; [Kind.Emoji] the word's own emoji
 * (Labs › Keyboard › Emoji suggestions), kept in the last slot. */
internal data class Suggestion(val text: String, val kind: Kind) {
    enum class Kind { Typed, Fix, Word, Next, Emoji }

    companion object {
        /** The strip shows three slots. */
        const val SLOTS = 3
        /** [slots] with [emoji] appended, dropping the least likely word to make room. A null
         * [emoji] leaves [slots] exactly as they were. */
        fun withEmoji(slots: List<Suggestion>, emoji: String?): List<Suggestion> =
            if (emoji == null) slots else slots.take(SLOTS - 1) + Suggestion(emoji, Kind.Emoji)

        /** The strip's [SLOTS] slots in display order. With a [Kind.Typed] slot they show as given,
         * except that the emoji is padded into the last slot (so `[Typed]` plus an emoji leaves the
         * middle empty rather than putting it there). Without one, the likeliest word takes the
         * middle and the next the left, as Gboard and iPhone place them; the emoji keeps the last. */
        fun place(slots: List<Suggestion>): List<Suggestion?> {
            val shown = arrayOfNulls<Suggestion>(SLOTS)
            val emoji = slots.firstOrNull { it.kind == Kind.Emoji }
            val words = slots.filter { it.kind != Kind.Emoji }
            if (slots.any { it.kind == Kind.Typed }) {
                words.take(if (emoji == null) SLOTS else SLOTS - 1).forEachIndexed { i, slot -> shown[i] = slot }
            } else {
                shown[1] = words.getOrNull(0); shown[0] = words.getOrNull(1)
                shown[2] = words.getOrNull(2)
            }
            if (emoji != null) shown[2] = emoji
            return shown.toList()
        }
    }
}

/** The current word, and the one a space just finished while the next is typed. Both the keyboard
 * and replay feed the real tap at each letter. */
internal class TypingWord(val decoder: SpatialDecoder, val spatial: Boolean) {
    private val model = decoder.model
    var token = ""
        private set
    private var previous = -1
    private var literalOnly = false
    private val taps = ArrayList<SpatialDecoder.Tap>()
    // The same taps read on the drawn keys. Labs › Keyboard › Adaptive touch reads [taps] on your own centres once it
    // has them ([personalized]); these decide which of its corrections space may make, and every join.
    private val drawn = ArrayList<SpatialDecoder.Tap>()
    private var personalized = false
    private var decision: SpatialDecoder.Decision? = null
    // The text before the word, the word before it (lowercase, null after punctuation) and
    // whether the word starts a sentence.
    private var before = ""
    private var previousText: String? = null
    private var sentenceStart = true
    /** The word a space finished, as it reads and as typed, with its taps and its space tap, while the
     * next word may still revise or join it; [anchor] is the text before the cursor right after its space. */
    private class Finished(val text: String, val typed: String, val taps: List<SpatialDecoder.Tap>?,
                           val space: SpatialDecoder.Tap?, val reach: Double, val anchor: String)
    private var pending: Finished? = null
    private var finished: Finished? = null
    var suggestSwearWords = false
        set(value) {
            if (value != field) { field = value; decision = null }
        }
    /** Settings › Keyboard › Auto-capitalisation (iOS AutocorrectEngine.addsCapitals). Off, nothing adds a
     * capital you didn't type: "i" stays "i", "monday" "monday", German nouns as typed, the strip and its
     * predictions lowercase; your own words and shortcuts keep theirs. */
    var addsCapitals = true
    /** English's word list standing in for a dictation language with no layout or typing model of its own
     * (Nederlands, Polski, Русский): the keys' geometry and your personal dictionary still work,
     * but the model's completions, next words, autocorrection, tap correction and emoji keywords are English's
     * and stay off. */
    var standIn = false
    /** Labs › Keyboard › Adaptive touch while it is on: taps are read on your own key centres once it has them. */
    var adaptive: AdaptiveTouch? = null
    /** Starts a word after `before`. The word a space just finished stays revisable only when
     * `before` is still the text it left; an empty `before` keeps it for the next call. */
    fun reset(before: String = "") {
        token = ""; taps.clear(); drawn.clear(); personalized = false; decision = null; literalOnly = false
        this.before = before
        previousText = previousWord(before).takeIf { it.isNotEmpty() }
        previous = model.context(previousText.orEmpty())
        sentenceStart = startsSentence(before)
        finished = null
        if (before.isEmpty()) return
        finished = pending?.takeIf { sameText(before, it.anchor) }
        pending = null
    }
    fun restored(text: String, before: String) { reset(before); explicit(text) } // No taps: space keeps the undone spelling.
    /** Up to three words for the strip between words (iOS #2125): at a sentence's start three common
     * first words, after a comma or other mid-sentence mark words that start a clause, after a word
     * the likeliest next words from the word pairs with the language's commonest words filling the
     * rest; nothing after a handle, path, number or a second space, or before a space is typed.
     * `before` is the text before the cursor; a bare word counts as one just finished. Capitalised
     * as shift would be, by default at a sentence's start. */
    fun predictions(before: String, capitalize: Boolean? = null): List<String> {
        if (standIn) return emptyList()
        val text = if (before.lastOrNull()?.let { it.isLetter() || it == '\'' || it == '\u2019' } == true) "$before " else before
        val words = when {
            startsSentence(text) -> sentenceStarters[model.language]
            !text.last().isWhitespace() -> return emptyList()
            text.dropLast(1).lastOrNull()?.let { it in midSentenceMarks } == true -> clauseStarters[model.language]
            else -> {
                val finished = currentToken(text.dropLast(1)) ?: return emptyList()
                val previous = lookup(finished.lowercase())
                val index = model.context(previous)
                val next = if (index >= 0) model.nextWords(index, suggestSwearWords, fill = false) else emptyList()
                (next + commonWords[model.language].orEmpty()).filter {
                    it.lowercase() != previous && !model.neverSuggest.blocks(it.lowercase(), suggestSwearWords, index)
                }
            }
        }.orEmpty()
        val upper = addsCapitals && (capitalize ?: startsSentence(text))
        val out = ArrayList<String>(3)
        for (w in words) {
            val shown = if (upper) w.replaceFirstChar { it.uppercase() } else if (addsCapitals) displayed(w) else w.lowercase()
            if (out.none { it.equals(shown, ignoreCase = true) }) out.add(shown)
            if (out.size == 3) break
        }
        return out
    }

    // A model word as the strip shows it: English "i" and its contractions with their capital, and
    // a name the books nearly always capitalise ("monday" -> "Monday").
    private fun displayed(word: String): String {
        if (model.language == "en" && (word == "i" || word.startsWith("i'"))) return "I" + word.drop(1)
        return capitalisedName(word) ?: word
    }

    // An Italian or French elision typed without its apostrophe, joined back up ("laltro" ->
    // "l'altro", "dellanno" -> "dell'anno", "lheure" -> "l'heure"), or null. The longest elision the
    // language has whose remainder is a reasonably common word of its own of two letters or more
    // ("dellanno" gives "dell'anno", never "d'ellanno"); a spelling only the books have as typed is
    // read as an elision only when the word after it is much likelier than it. A word of the model's
    // own word list, and your own words, are no elision ("lago", "della"). The straight apostrophe is
    // the strip's spelling, as suggestOnly's contractions are: shown() and smartWord (KeyboardEngine)
    // make it ’ when Labs › Keyboard › Smart quotes is on. A suggestion only, like those: space never
    // applies it.
    private fun elided(word: String): String? {
        val elisions = ELISIONS[model.language] ?: return null
        if (model.index(word) >= 0 || (decoder.personal?.index(word) ?: -1) >= 0) return null
        // What the books charge for the spelling as typed, in nats, or null when they have no such word.
        val typed = model.validCost(word)
        for (cut in word.length - 2 downTo 1) {
            val prefix = word.substring(0, cut)
            if (prefix !in elisions) continue
            val rest = word.substring(cut)
            if (!elidesInto(model.language, rest)) continue
            val index = model.index(rest)
            if (index < 0 || model.frequencyCost(index) > ELISION_REST_COST) continue
            // "laltro" and "lanno" are the books' run-together spellings too, so a known spelling alone
            // can't rule the elision out: the word after it must be much likelier than it is
            // ("allodio" -> "all'odio" and "noh" -> "n'oh" are not that much likelier).
            if (typed != null && typed - model.unigramCost(index) < ELISION_MARGIN) continue
            // A listed slur or swear stays out of an elision too (your own words are yours to offer).
            if (model.neverSuggest.blocks(rest, suggestSwearWords, -1) && decoder.personal?.owns(rest) != true) continue
            return prefix + "'" + rest
        }
        return null
    }
    // Whether an elision runs into [rest]: a vowel, accents and ligatures included ("un'altra",
    // "l'uomo", "l'heure"), and an h. Italian elides the h of the avere forms alone (l'ho, l'hai,
    // l'ha, l'hanno), never another ("l'hotel" is no elision); French elides h generally ("l'homme").
    private fun elidesInto(language: String, rest: String): Boolean {
        val c = rest[0].lowercaseChar()
        if (c in "aeiouàèéìíîòóùúâêëïôûüÿœæ") return true
        return c == 'h' && (language != "it" || rest in ITALIAN_AVERE)
    }

    /** The strip while a word is typed (iOS AutocorrectEngine.suggestions): what was typed
     * ([Suggestion.Kind.Typed]), then what space will put in ([Suggestion.Kind.Fix]) or the likeliest
     * candidate, then the runner-up. An unfinished word that space leaves alone (no word yet, "th")
     * gets three candidates instead. Candidates: a suggest-only English contraction ("ill" ->
     * "I'll") or an Italian or French elision typed without its apostrophe ("laltro" -> "l'altro"),
     * the likeliest completions of a 1–3 letter word after the word before, then, for a
     * word of three letters or more, the decoder's other readings of the taps before completions by
     * frequency (after two letters the word is usually unfinished: completions first). Never what was
     * typed again. [autocorrect] off: no fix and no readings, completions only, unless [nearWords]: then what
     * space would put in and the readings are offered too, as plain words space never puts in (Keyboard ›
     * Autocorrect off, as Gboard keeps its suggestions with auto-correction off). */
    @JvmOverloads
    fun strip(autocorrect: Boolean = true, nearWords: Boolean = autocorrect): List<Suggestion> {
        if (token.length !in 1..MAX_LENGTH) return emptyList()
        val typed = Suggestion(token, Suggestion.Kind.Typed)
        // Your shortcut: what you typed and its phrase, which space puts in whatever autocorrect does.
        decoder.personal?.shortcut(lookup(token))?.let { return listOf(typed, Suggestion(it, Suggestion.Kind.Fix)) }
        val fix = if (autocorrect && !standIn) replacement() else null
        val near = if (!autocorrect && nearWords && !standIn) replacement() else null
        val pool = ArrayList<String>(4)
        val lower = lookup(token.lowercase())
        val showTyped = standIn || fix != null || near != null || blocked(token) || knows(lower)
        val size = if (showTyped) 2 else 3
        val lowered = !addsCapitals && token.none { it.isUpperCase() }
        fun add(word: String) {
            val s = if (lowered && decoder.personal?.owns(word) != true) word.lowercase() else word
            if (pool.size < size && !s.equals(token, ignoreCase = true) && pool.none { it.equals(s, ignoreCase = true) } &&
                !blocked(s)) pool.add(s)
        }
        fix?.let { pool.add(it) }
        near?.let(::add)
        if (nearWords && fix == null && !standIn && model.language == "en") suggestOnly[lower]?.let { add(matchCase(token, it)) }
        // An elision typed without its apostrophe ("laltro", "dellanno"): Italian and French write the
        // article against the word, so the strip offers the two joined up, as English's contractions are.
        // A whole-word decoder fix already reads the token ("diro" -> "dirò"), so the elision would only
        // be a second guess at it. A fix that splits the token must be the elision's own two words
        // ("l ho" for "l'ho", "lho"); another reading of the letters ("non c'è" for "nonce", "la e" for
        // "lae") is no elision at all.
        val elision = if (nearWords && !standIn && '\'' !in lower) elided(lower) else null
        if (elision != null && (fix == null || fix.equals(elision.replace('\'', ' '), ignoreCase = true)))
            add(matchCase(token, elision))
        // Your own word, completed once two letters match, before the dictionary's: a word you added is
        // likelier what you mean than a reading's runner-up ("zor" offers "Zorbex" beside the fix "or").
        if (token.length >= 2) decoder.personal?.completions(lookup(token))?.firstOrNull()?.let { add(yours(it)) }
        val short = if (!standIn && token.length <= 3) model.completionsAfter(lower, previous, suggestSwearWords).map { shown(model.text(it)) } else emptyList()
        if (fix == null) short.firstOrNull { s -> pool.none { it.equals(s, ignoreCase = true) } }?.let(::add)
        val readings = nearWords && !standIn && spatial && token.length >= 2 && !protected()
        fun reading(text: String) = if (decoder.personal?.owns(text) == true) yours(text) else shown(text)
        if (readings && token.length >= 3) for (r in decision().readings) add(reading(r.text))
        for (s in short) add(s)
        // Your own words before the rest of the dictionary's (iOS lists UILexicon terms with them).
        decoder.personal?.completions(lookup(token))?.forEach { add(yours(it)) }
        if (!standIn) for (w in model.completions(lower, suggestSwearWords, previous)) add(shown(model.text(w)))
        if (readings && token.length < 3) for (r in decision().readings) add(reading(r.text))
        if (pool.isEmpty()) return listOf(typed)
        val first = if (fix != null) Suggestion(fix, Suggestion.Kind.Fix) else Suggestion(pool[0], Suggestion.Kind.Word)
        val words = listOf(first) + pool.drop(1).map { Suggestion(it, Suggestion.Kind.Word) }
        return if (showTyped) listOf(typed) + words else words
    }

    // A candidate cased as the word was typed, with the capital the strip shows for names.
    private fun shown(word: String) = matchCase(token, if (token.firstOrNull()?.isUpperCase() == true) word else displayed(word))
    fun letter(text: String, x: Double, y: Double, grid: KeyGrid) {
        token += text; decision = null
        if (spatial && !literalOnly && taps.size < MAX_LENGTH) {
            val tap = decoder.score(grid, x, y)
            val personal = adaptive?.personal(grid)
            taps.add(if (personal != null) decoder.score(personal, x, y) else tap)
            drawn.add(tap)
            if (personal != null) personalized = true
        }
    }
    fun explicit(text: String) {
        token += text; taps.clear(); drawn.clear(); decision = null; literalOnly = true
    }
    fun pop() {
        if (token.isEmpty()) return
        if (taps.size == token.length) { taps.removeAt(taps.lastIndex); drawn.removeAt(drawn.lastIndex) }
        token = token.dropLast(1); decision = null
    }
    private fun decision() = decision ?: guarded(decoder.decide(lookup(token.lowercase()), taps, previous, suggestSwearWords))
        .also { decision = it }
    // Adaptive touch's centres rank the readings, but space corrects a word only when the drawn centres would correct it
    // too, and puts in their reading unless yours also clears their margin: never a correction the keyboard wouldn't make
    // without them. Stricter than iPhone's AutocorrectEngine.decoderBaselineKeys, whose margin check alone let a reading
    // the drawn centres never consider change a word they leave ("sun" to "su", "Mary" to "Matt" in the gate).
    private fun guarded(personal: SpatialDecoder.Decision): SpatialDecoder.Decision {
        val replace = personal.replace
        if (!personalized || replace == null || adaptive?.guards == false) return personal
        val typed = lookup(token.lowercase())
        val own = decoder.decide(typed, drawn, previous, suggestSwearWords).replace
        val kept = when {
            own == null -> null
            own.equals(replace, ignoreCase = true) -> replace
            decoder.replaces(replace, typed, drawn, previous, suggestSwearWords) == true -> replace
            else -> own
        }
        return if (kept == replace) personal else SpatialDecoder.Decision(kept, personal.readings)
    }
    // Your own words are always yours to offer, as iOS offers UILexicon terms.
    private fun blocked(spelling: String) = model.neverSuggest.blocks(lookup(spelling.lowercase()), suggestSwearWords, previous) &&
        decoder.personal?.owns(spelling) != true
    /** The key a touch near a key's edge presses (iOS FullKeyboardEngine.key(at:), #2091): the key it
     * landed on ([hit]), or a neighbouring letter whose touch area is within [REACH] of it and which
     * is enough likelier to come next in the word, by the word list ("th" then a touch on the e/d
     * border types e). [near] gives each neighbouring letter's distance from the touch to its touch
     * area, in dp (iOS's points), only those under [REACH]; [keys] is every letter on the keyboard,
     * so a letter without a key of its own counts as its base letter's (é as e). Only while
     * autocorrect is on in a word field, never with caps lock; the keyboard decides that. */
    fun likelierKey(before: String, hit: Char, near: Map<Char, Double>, keys: Collection<Char>): Char {
        if (standIn || near.isEmpty()) return hit
        val token = currentToken(before)
        if (token == null && before.lastOrNull()?.let { it.isLetterOrDigit() || it == '\'' || it == '\u2019' } == true) return hit
        val continuations = model.continuations(lookup((token ?: "").lowercase())) ?: return hit
        val keyed = keys.mapTo(HashSet()) { model.code(it.lowercaseChar()) }
        val mass = DoubleArray(continuations.size)
        for (code in 1 until continuations.size) if (continuations[code] > 0)
            mass[if (code in keyed) code else model.bases[code]] += continuations[code]
        val total = mass.sum()
        // Right after an elided article or pronoun ("l", "qu"), the apostrophe key is the likeliest: the
        // books split the commonest elisions off ("l'", "d'"), so the word list can't say so itself.
        val elided = token != null && lookup(token.lowercase()) in ELISIONS[model.language].orEmpty() &&
            before.dropLast(token.length).lastOrNull().let { boundary(it) }
        if (elided && model.apostrophe != 0) mass[model.apostrophe] = maxOf(mass[model.apostrophe], total)
        fun odds(c: Char) = model.code(c.lowercaseChar()).takeIf { it != 0 }?.let { kotlin.math.ln(mass[it] + total * 1e-4) }
        var best = hit
        var bestScore = odds(hit) ?: return hit
        for ((c, distance) in near) {
            if (distance >= REACH) continue
            val score = (odds(c) ?: continue) - ODDS_PER_DP * distance
            if (score > bestScore) { best = c; bestScore = score }
        }
        return if (hit.isUpperCase()) best.uppercaseChar() else best
    }

    /** Whether a letter typed after `before` starts a word the keyboard should read: after a space or
     * an opening mark (`boundary`), and in French and Italian also right after an elided article
     * or pronoun ("l'", "qu'", "dell'"), so "l'hpmme" is read as "homme" after "l'". */
    fun startsWord(before: String): Boolean {
        if (boundary(before.lastOrNull())) return true
        val elisions = ELISIONS[model.language] ?: return false
        if (before.lastOrNull() != '\'' && before.lastOrNull() != '\u2019') return false
        val prefix = before.dropLast(1).takeLastWhile { it.isLetter() }.lowercase()
        return prefix in elisions && before.dropLast(1 + prefix.length).lastOrNull().let { boundary(it) }
    }

    /** Your text replacement whose shortcut ends `before` (the text before the cursor), from the personal
     * dictionary: on space, a closing mark or return, put it in before anything else, autocorrect on
     * or off (iOS #2152, #2173). Null when no shortcut ends the text. */
    fun textReplacement(before: String): Correction? = decoder.personal?.replacement(before)
    // A capitalised word of yours that is also an ordinary lowercase word ("Will", "Bill") stays
    // lowercase when typed so, as iOS's withoutStrayCapital.
    private fun ordinary(word: String) = word.lowercase().let { model.index(it) >= 0 && (model.capitals?.percent(it) ?: -1) < 95 }
    private fun yours(word: String): String =
        if (token.firstOrNull()?.isUpperCase() != true && word.drop(1).none { it.isUpperCase() } && ordinary(word)) word.lowercase()
        else matchCase(token, word)
    fun literalChip(suggestions: List<String>) = token.takeIf { it.isNotEmpty() && (suggestions.isEmpty() || blocked(it)) }
    private fun protected() = token.drop(1).any { it.isUpperCase() } ||
        (model.language == "en" && token.lowercase() in shorthand)
    private fun cased(s: String) = if (token.firstOrNull()?.isUpperCase() == true) s.replaceFirstChar { it.uppercase() } else s
    /** What space puts in place of the word alone, or null. */
    fun replacement(): String? {
        val fix = withCapitals() ?: return null
        if (addsCapitals || token.any { it.isUpperCase() } || decoder.personal?.owns(fix) == true) return fix
        return fix.lowercase().takeIf { it != token }
    }
    private fun withCapitals(): String? {
        // A stand-in changes nothing on space or a mark: English's fixes and your own spellings alike wait
        // for a layout or a language with a model. Your shortcuts go in through textReplacement before it.
        if (standIn || token.isEmpty()) return null
        val personal = decoder.personal
        personal?.shortcut(lookup(token))?.let { return it }
        // Your own word: never corrected into another, and spelled your way ("github" -> "GitHub").
        personal?.index(lookup(token))?.takeIf { it >= 0 }?.let { return yours(personal.text(it)).takeIf { s -> s != token } }
        if (literalOnly || blocked(token)) return null
        val lower = lookup(token.lowercase())
        val allCaps = token.length >= 2 && token.all { it.isUpperCase() }
        if (token.drop(1).any { it.isUpperCase() } && !allCaps) return null
        if (model.language == "en" && lower in shorthand) return null
        // A word the same letters spell with an apostrophe or an accent, or one that sounds alike,
        // chosen by the word before when it says so clearly ("yes id" -> "yes I'd", "the id" stays).
        if (!allCaps && '\'' !in token && '\u2019' !in token) ContextWords.choice(model, token, previousText, null, sentenceStart)
            ?.takeUnless(::blocked)?.let { return it }
        if (model.language == "en") {
            englishFixes[lower]?.takeUnless(::blocked)?.let { return if (allCaps) {
                if (token.length >= 3) it.uppercase() else null
            } else cased(it) }
        }
        if (allCaps || !spatial) return null
        // A name, day or place typed in lowercase takes its capital ("mary" -> "Mary"), and so does a
        // German noun ("haus" -> "Haus"), as the capitals list beside the model says.
        val fix = decision().replace?.let { if (decoder.personal?.owns(it) == true) yours(it) else cased(it) } ?: return capitalised(token)
        // A missed space after English "i" ("iwant") gives "I want", as "i" alone does.
        if (model.language == "en" && ' ' in fix) return fix.split(' ').joinToString(" ") { if (it == "i") "I" else it }
        // The German and English models are lowercase: a German noun or a name they correct to takes its capital
        // ("hsus" -> "Haus", "mray" -> "Mary").
        return if ((model.language == "de" || model.language == "en") && ' ' !in fix) capitalised(fix) ?: fix else fix
    }

    /** A swiped reading ([SwipeReader]'s: the model's lowercase spelling, a contraction such as "I'm", or your own
     * word) as it goes in mid-sentence after [previous] (lowercase, or null), iPhone Hex's swipeCased: "i" and its
     * contractions, names the books nearly always capitalise and German nouns take their capital (as the tap decoder
     * gives them), your own words keep your spelling unless that is an ordinary word with a stray capital ("Will").
     * With Auto-capitalisation off, no capital you didn't type, your own spellings aside. */
    fun swiped(reading: String, previous: String?): String {
        val own = decoder.personal?.owns(reading) == true
        if (!addsCapitals) return if (own) reading else reading.lowercase()
        if (own) return if (reading.drop(1).none { it.isUpperCase() } && ordinary(reading)) reading.lowercase() else reading
        val lower = reading.lowercase()
        if (model.language == "en" && (lower == "i" || lower.startsWith("i'"))) return "I" + lower.drop(1)
        if (lower.length < 2) return lower
        val share = model.capitals?.percent(lower) ?: return lower
        val capital = share >= 95 || (model.language == "de" && share >= 50 && previous in germanDeterminers)
        val first = lower.first().uppercase()
        return if (capital && first.length == 1) first + lower.drop(1) else lower
    }

    private fun capitalisedName(word: String): String? {
        if (word.length < 2 || word != word.lowercase() || (model.capitals?.percent(word) ?: -1) < 95) return null
        val first = word.first().uppercase()
        return if (first.length == 1) first + word.drop(1) else null
    }

    // `word` with its capital when it is all lowercase and the books nearly always write it with one
    // (95%), or, in German, half the time after an article or determiner ("das leben"); else null.
    private fun capitalised(word: String): String? {
        if (word.length < 2 || word != word.lowercase()) return null
        val share = model.capitals?.percent(word) ?: return null
        if (share < 95 && !(model.language == "de" && share >= 50 && previousText in germanDeterminers)) return null
        val first = word.first().uppercase()
        return if (first.length == 1) first + word.drop(1) else null
    }

    /** What a space (or a closing mark, [ending]) does as it finishes the word: the word's own fix,
     * the word before revised now that this one follows ("well see" -> "we'll see"), or both words
     * joined when the space between them was tapped for a letter ("deli ate" -> "delicate"). Null
     * leaves the text as typed. [space] is where the space bar was tapped, in [grid]'s coordinates.
     * [commit] spells a replacement as it will actually go in (Labs › Keyboard › Smart quotes turns
     * its apostrophes into ’), so the word kept for the next word's revision is the one inserted.
     * Call it once per space or mark, then `reset` with the text after it. */
    fun finish(ending: Char = ' ', space: KeyPoint? = null, grid: KeyGrid? = null,
               commit: (String) -> String = { it }): Correction? {
        val word = finished
        finished = null; pending = null
        if (standIn || token.isEmpty() || (ending == '.' && token.lowercase() in abbreviations)) return null
        var result: Correction? = null
        // Joins read the taps on the drawn keys alone: Adaptive touch never joins two words its own way.
        if (word?.taps != null && word.space != null && spatial && !literalOnly && drawn.size == token.length &&
            listOf(word.typed, token).none { it.lowercase() in shorthand || decoder.personal?.owns(it) == true }) {
            val prev = previousWord(before.dropLast(word.text.length + 1)).takeIf { it.isNotEmpty() }
            decoder.merge(word.typed.lowercase(), word.taps, word.space, word.reach, token.lowercase(), drawn,
                model.context(prev.orEmpty()), knows(word.typed), knows(token), suggestSwearWords)?.let { merged ->
                result = Correction(word.text + " " + token, matchCase(word.typed, merged))
            }
        }
        if (result == null) {
            val fix = replacement()
            val now = fix ?: token
            // A word typed or corrected with an apostrophe is left as it is, whether that apostrophe
            // is straight or the smart ’ (Labs › Keyboard › Smart quotes), and so is a Portuguese word
            // typed with an accent held for it.
            val revised = if (word != null && '\'' !in lookup(word.typed) &&
                !ContextWords.keepsTypedAccent(model.language, word.typed)) revision(word.text, now) else null
            if (fix != null || revised != null)
                result = Correction((if (revised != null) word!!.text + " " else "") + token, (revised?.let { "$it " } ?: "") + now)
        }
        result = result?.let { Correction(it.original, commit(it.replacement)) }
        if (ending == ' ') remember(result, space, grid)
        return result
    }

    // The word before, revised now that `next` follows it, or null: words the context decides
    // (`ContextWords`); a word typed with its own apostrophe never gets here.
    private fun revision(text: String, next: String): String? {
        if (!ContextWords.decides(model.language) || text.isEmpty() || (model.language == "en" && text.length < 2)) return null
        // ContextWords knows the straight spellings, so a word inserted with a smart apostrophe
        // ("it’s") is looked up by the straight one ("it's").
        val key = lookup(text)
        // Typed in capitals, left alone; one capital letter is a sentence's start ("E andato" -> "È andato").
        if (text.length >= 2 && text == text.uppercase() && text != text.lowercase() && !key.startsWith("I'")) return null
        val rest = before.dropLast(text.length + 1)
        val prev = previousWord(rest).takeIf { it.isNotEmpty() }
        val choice = ContextWords.choice(model, key, prev, next.substringBefore(' ').lowercase(), startsSentence(rest)) ?: return null
        if (model.neverSuggest.blocks(choice.lowercase(), suggestSwearWords, model.context(prev.orEmpty()))) return null
        return if (addsCapitals || text.first().isUpperCase()) choice else choice.lowercase().takeIf { it != key }
    }

    private fun knows(spelling: String) = lookup(spelling.lowercase()).let {
        model.index(it) >= 0 || model.known(it) || it in shorthand || (decoder.personal?.index(it) ?: -1) >= 0
    }

    // Keeps the word this space finished for the next word's revision or join. After a split or a
    // join it reads other letters than were typed: kept as it reads, without taps.
    private fun remember(result: Correction?, space: KeyPoint?, grid: KeyGrid?) {
        val full = before + token
        val after = (if (result == null) full else full.dropLast(result.original.length) + result.replacement) + " "
        val text = after.dropLast(1).takeLastWhile { it.isLetter() || it == '\'' || it == '\u2019' }
        if (text.isEmpty()) return
        val sameLetters = text.count { it != '\'' && it != '\u2019' } == token.length
        val wordTaps = if (sameLetters && spatial && !literalOnly && drawn.size == token.length) ArrayList(drawn) else null
        val spaceTap = if (wordTaps != null && space != null && grid != null) decoder.score(grid, space.x, space.y) else null
        val reach = if (spaceTap != null) decoder.rowCost(grid!!, space!!.x, space.y) else Double.POSITIVE_INFINITY
        pending = Finished(text, if (sameLetters) token else text, wordTaps, spaceTap, reach, after)
    }
    fun suggestions(): List<String> {
        if (standIn || token.length !in 1..MAX_LENGTH) return emptyList()
        val options = LinkedHashMap<String, Int>()
        for (w in model.completions(lookup(token.lowercase()), suggestSwearWords, previous)) options[model.text(w)] = model.frequencyCost(w)
        if (spatial && options.size < 3 && token.length >= 2 && !protected()) for (r in decision().readings)
            options.putIfAbsent(r.text, r.frequency)
        return options.entries.sortedWith(compareBy<Map.Entry<String, Int>> { it.value }.thenBy { it.key })
            .take(3).map { cased(it.key) }
    }
    companion object {
        const val MAX_LENGTH = 32
        /** How far past its own touch area a likelier letter may reach, in dp, and the nats of odds
         * (ln of how many times likelier) it needs for each dp it claims (iOS letterReach, letterOddsPerPoint). */
        const val REACH = 5.0
        const val ODDS_PER_DP = .8
        // Words the strip offers in place of these, but space never puts in: each is a word too.
        val suggestOnly = mapOf("ill" to "I'll", "id" to "I'd", "were" to "we're", "wed" to "we'd",
            "well" to "we'll", "its" to "it's", "lets" to "let's", "hell" to "he'll", "shell" to "she'll")
        // Hex's own short lists, as iOS: a sentence's common first words, a clause's after a comma, and
        // the commonest words for next-word slots the model can't fill.
        private val sentenceStarters = mapOf("en" to listOf("I", "the", "I'm"), "es" to listOf("yo", "el", "la"),
            "de" to listOf("ich", "das", "die"), "nb" to listOf("jeg", "det", "vi"), "fr" to listOf("je", "le", "la"),
            "it" to listOf("io", "il", "la"), "pt" to listOf("eu", "o", "a"))
        private val clauseStarters = mapOf("en" to listOf("I", "but", "and"), "es" to listOf("que", "pero", "y"),
            "de" to listOf("und", "aber", "ich"), "nb" to listOf("og", "men", "jeg"), "fr" to listOf("et", "mais", "je"),
            "it" to listOf("e", "ma", "che"), "pt" to listOf("e", "mas", "que"))
        private val commonWords = mapOf("en" to listOf("the", "to", "and", "a", "I", "of"),
            "es" to listOf("de", "que", "y", "la", "el", "en"), "de" to listOf("und", "die", "der", "ich", "ist", "das"),
            "nb" to listOf("og", "i", "det", "er", "på", "som"), "fr" to listOf("de", "la", "et", "le", "je", "à"),
            "it" to listOf("di", "e", "che", "la", "il", "a"), "pt" to listOf("de", "a", "o", "que", "e", "em"))
        private val midSentenceMarks = ",;:)\"'\u201D\u2019\u2014"
        /** The word right before the cursor, or null inside a handle, path, address or number. */
        fun currentToken(before: CharSequence): String? {
            var start = before.length
            while (start > 0) {
                val c = before[start - 1]
                if (c.isLetter() || ((c == '\'' || c == '\u2019') && start < before.length && start >= 2 &&
                        before[start - 2].isLetter())) start-- else break
            }
            if (start == before.length) return null
            if (start > 0 && before[start - 1].let { it in "@/.#_-:" || it.isDigit() }) return null
            return before.substring(start).takeIf { it.first().isLetter() }
        }
        /** Marks that close a word and correct it as space does. */
        fun endsWord(c: Char) = c in ",.!?;:)\"\u201D"
        // A period after these makes an abbreviation ("approx.", "etc."), never a typo.
        private val abbreviations = ("approx appt apt assn asst ave blvd cf co corp dept dr eg est etc fig govt ie inc jr lb " +
            "lbs ltd misc mr mrs ms mt no nr oz pp prof pt rd sr st vol vs wk yr").split(' ').toSet()
        // Whether the next word starts a sentence: the start, a new line, or . ! ? (and closing quotes
        // or brackets) and a space.
        fun startsSentence(before: String): Boolean {
            val last = before.lastOrNull() ?: return true
            if (last == '\n') return true
            if (!last.isWhitespace()) return false
            var rest = before.trimEnd { it.isWhitespace() && it != '\n' }
            val written = rest.lastOrNull() ?: return true
            if (written == '\n') return true
            rest = rest.trimEnd { it in "\"')]\u201D\u2019" }
            return rest.lastOrNull()?.let { it in ".!?" } ?: false
        }
        // After these German writes the noun with a capital ("das Haus", "im Haus", "mein Haus"), so
        // a word the books write both ways is a noun here (iOS GermanCapitals.determiners).
        private val germanDeterminers = ("der die das den dem des ein eine einen einem einer eines kein keine keinen keinem " +
            "keiner keines mein meine meinen meinem meiner meines dein deine deinen deinem deiner deines sein seine " +
            "seinen seinem seiner seines ihre ihren ihrem ihrer ihres unser unsere unseren unserem unserer unseres euer " +
            "eure euren eurem eurer eures dies dieser diese dieses diesem diesen jeder jede jedes jedem jeden im in am " +
            "zum zur vom beim ins ans").split(' ').toSet()
        // Hosts give a window of the text: compare its last 64 characters once both are that long.
        private fun sameText(a: String, b: String) =
            if (a.length >= 64 && b.length >= 64) a.takeLast(64) == b.takeLast(64) else a == b
        // `target` cased as `typed` was: all capitals, or a capital first letter.
        private fun matchCase(typed: String, target: String) = when {
            typed.length >= 2 && typed.all { it.isUpperCase() } -> target.uppercase()
            typed.firstOrNull()?.isUpperCase() == true -> target.replaceFirstChar { it.uppercase() }
            else -> target
        }
        private val shorthand = "ok okay ty thx pls plz tmrw tbh idk imo imho lol lmao omg brb btw afaik fyi nvm rn irl jk np yw smh ikr ngl fr gg dm pr prs ci ui ux ios macos api apis url urls json sql ssh cli gpu cpu ram sdk hex wisp seer rune kit gonna wanna gotta kinda sorta yeah yep nope nah hey yo".split(' ').toSet()
        /** [text] with a smart apostrophe as the straight one the model and the personal dictionary store, so
         * a word typed with ’ completes and corrects as the same word typed with '. */
        private fun lookup(text: String) = text.replace('\u2019', '\'')
        // The word before the cursor, lowercase; an elided one without its apostrophe ("l'" as "l").
        fun previousWord(before: String) = before.trimEnd().takeLastWhile { it.isLetter() || it == '\'' || it == '\u2019' }
            .lowercase().replace('\u2019', '\'').trimEnd('\'')
        // The remainder of an elision must be one of the model's reasonably common words: its unigram
        // cost in eighths of a nat at most this. "uva" (93) and "insalata" (98) fit; the junk report's
        // "om" (99), "ae" (100), "ol" (104) and the rest (114 to 120) do not.
        private const val ELISION_REST_COST = 98
        // A run-together spelling the books list ("laltro", "lanno") is read as an elision only when the
        // word after it is much likelier: at least this many nats, about 1,450 times, since a books cost
        // already includes a 0.92-nat backoff. It sits between the lowest wanted case ("lanno", 8.55
        // nats) and the highest unwanted one ("noh", 8.05).
        private const val ELISION_MARGIN = 8.2
        // The only Italian words whose h an elision takes.
        private val ITALIAN_AVERE = setOf("ho", "hai", "ha", "hanno")
        // The articles and pronouns French and Italian elide before a vowel; a word starts after them.
        private val ELISIONS = mapOf(
            "fr" to setOf("c", "d", "j", "l", "m", "n", "s", "t", "qu", "jusqu", "lorsqu", "puisqu", "quoiqu", "presqu"),
            "it" to setOf("c", "d", "l", "m", "n", "s", "t", "v", "un", "all", "dall", "dell", "nell", "sull", "coll",
                "quest", "quell", "bell", "sant", "tutt", "nessun", "buon"))
        fun boundary(c: Char?) = c == null || c.isWhitespace() || c in "([{“‘\",;!?"
    }
}
