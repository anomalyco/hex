package ly.anoma.hex.typing

/** What a space or closing mark does to the text before the cursor: [original], which that text
 * ends with, becomes [replacement]; the space or mark goes in after it. [original] may hold the
 * word before too ("well se" ... "well see" -> "we'll see"). */
internal data class Correction(val original: String, val replacement: String)

/** The current word, and the one a space just finished while the next is typed. Both the keyboard
 * and replay feed the real tap at each letter. */
internal class TypingWord(val decoder: SpatialDecoder, val spatial: Boolean) {
    private val model = decoder.model
    var token = ""
        private set
    private var previous = -1
    private var literalOnly = false
    private val taps = ArrayList<SpatialDecoder.Tap>()
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
    /** Starts a word after `before`. The word a space just finished stays revisable only when
     * `before` is still the text it left; an empty `before` keeps it for the next call. */
    fun reset(before: String = "") {
        token = ""; taps.clear(); decision = null; literalOnly = false
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
    fun predictions(before: String) = model.nextWords(model.context(previousWord(before)), suggestSwearWords)
    fun letter(text: String, x: Double, y: Double, grid: KeyGrid) {
        token += text; decision = null
        if (spatial && !literalOnly && taps.size < MAX_LENGTH) taps.add(decoder.score(grid, x, y))
    }
    fun explicit(text: String) {
        token += text; taps.clear(); decision = null; literalOnly = true
    }
    fun pop() {
        if (token.isEmpty()) return
        if (taps.size == token.length) taps.removeAt(taps.lastIndex)
        token = token.dropLast(1); decision = null
    }
    private fun decision() = decision ?: decoder.decide(token.lowercase(), taps, previous, suggestSwearWords).also { decision = it }
    private fun blocked(spelling: String) = model.neverSuggest.blocks(spelling.lowercase(), suggestSwearWords, previous)
    fun literalChip(suggestions: List<String>) = token.takeIf { it.isNotEmpty() && (suggestions.isEmpty() || blocked(it)) }
    private fun protected() = token.drop(1).any { it.isUpperCase() } ||
        (model.language == "en" && token.lowercase() in shorthand)
    private fun cased(s: String) = if (token.firstOrNull()?.isUpperCase() == true) s.replaceFirstChar { it.uppercase() } else s
    /** What space puts in place of the word alone, or null. */
    fun replacement(): String? {
        if (literalOnly || token.isEmpty() || blocked(token)) return null
        val lower = token.lowercase()
        val allCaps = token.length >= 2 && token.all { it.isUpperCase() }
        if (token.drop(1).any { it.isUpperCase() } && !allCaps) return null
        if (model.language == "en") {
            if (lower in shorthand) return null
            // A word the same letters spell with an apostrophe, or one that sounds alike, chosen by
            // the word before when it says so clearly ("yes id" -> "yes I'd", "the id" stays).
            if (!allCaps && '\'' !in token) ContextWords.choice(model, token, previousText, null, sentenceStart)
                ?.takeUnless(::blocked)?.let { return it }
            englishFixes[lower]?.takeUnless(::blocked)?.let { return if (allCaps) {
                if (token.length >= 3) it.uppercase() else null
            } else cased(it) }
        }
        if (allCaps || !spatial) return null
        val fix = decision().replace?.let(::cased) ?: return null
        // A missed space after English "i" ("iwant") gives "I want", as "i" alone does.
        return if (model.language == "en" && ' ' in fix) fix.split(' ').joinToString(" ") { if (it == "i") "I" else it } else fix
    }

    /** What a space (or a closing mark, [ending]) does as it finishes the word: the word's own fix,
     * the word before revised now that this one follows ("well see" -> "we'll see"), or both words
     * joined when the space between them was tapped for a letter ("deli ate" -> "delicate"). Null
     * leaves the text as typed. [space] is where the space bar was tapped, in [grid]'s coordinates.
     * Call it once per space or mark, then `reset` with the text after it. */
    fun finish(ending: Char = ' ', space: KeyPoint? = null, grid: KeyGrid? = null): Correction? {
        val word = finished
        finished = null; pending = null
        if (token.isEmpty() || (ending == '.' && token.lowercase() in abbreviations)) return null
        var result: Correction? = null
        if (word?.taps != null && word.space != null && spatial && !literalOnly && taps.size == token.length &&
            listOf(word.typed, token).none { it.lowercase() in shorthand }) {
            val prev = previousWord(before.dropLast(word.text.length + 1)).takeIf { it.isNotEmpty() }
            decoder.merge(word.typed.lowercase(), word.taps, word.space, word.reach, token.lowercase(), taps,
                model.context(prev.orEmpty()), knows(word.typed), knows(token), suggestSwearWords)?.let { merged ->
                result = Correction(word.text + " " + token, matchCase(word.typed, merged))
            }
        }
        if (result == null) {
            val fix = replacement()
            val now = fix ?: token
            val revised = if (word != null && '\'' !in word.typed) revision(word.text, now) else null
            if (fix != null || revised != null)
                result = Correction((if (revised != null) word!!.text + " " else "") + token, (revised?.let { "$it " } ?: "") + now)
        }
        if (ending == ' ') remember(result, space, grid)
        return result
    }

    // The word before, revised now that `next` follows it, or null. English only: words the
    // context decides (`ContextWords`); a word typed with its own apostrophe never gets here.
    private fun revision(text: String, next: String): String? {
        if (model.language != "en" || text.length < 2) return null
        if (text == text.uppercase() && text != text.lowercase() && !text.startsWith("I'")) return null
        val rest = before.dropLast(text.length + 1)
        val prev = previousWord(rest).takeIf { it.isNotEmpty() }
        val choice = ContextWords.choice(model, text, prev, next.substringBefore(' ').lowercase(), startsSentence(rest)) ?: return null
        return choice.takeUnless { model.neverSuggest.blocks(it.lowercase(), suggestSwearWords, model.context(prev.orEmpty())) }
    }

    private fun knows(spelling: String) = spelling.lowercase().let { model.index(it) >= 0 || model.validLiteral(it) || it in shorthand }

    // Keeps the word this space finished for the next word's revision or join. After a split or a
    // join it reads other letters than were typed: kept as it reads, without taps.
    private fun remember(result: Correction?, space: KeyPoint?, grid: KeyGrid?) {
        val full = before + token
        val after = (if (result == null) full else full.dropLast(result.original.length) + result.replacement) + " "
        val text = after.dropLast(1).takeLastWhile { it.isLetter() || it == '\'' }
        if (text.isEmpty()) return
        val sameLetters = text.count { it != '\'' } == token.length
        val wordTaps = if (sameLetters && spatial && !literalOnly && taps.size == token.length) ArrayList(taps) else null
        val spaceTap = if (wordTaps != null && space != null && grid != null) decoder.score(grid, space.x, space.y) else null
        val reach = if (spaceTap != null) decoder.rowCost(grid!!, space!!.x, space.y) else Double.POSITIVE_INFINITY
        pending = Finished(text, if (sameLetters) token else text, wordTaps, spaceTap, reach, after)
    }
    fun suggestions(): List<String> {
        if (token.length !in 1..MAX_LENGTH) return emptyList()
        val options = LinkedHashMap<String, Int>()
        for (w in model.completions(token.lowercase(), suggestSwearWords, previous)) options[model.text(w)] = model.frequencyCost(w)
        if (spatial && options.size < 3 && token.length >= 2 && !protected()) for (r in decision().readings)
            options.putIfAbsent(r.text, r.frequency)
        return options.entries.sortedWith(compareBy<Map.Entry<String, Int>> { it.value }.thenBy { it.key })
            .take(3).map { cased(it.key) }
    }
    companion object {
        const val MAX_LENGTH = 32
        /** Marks that close a word and correct it as space does. */
        fun endsWord(c: Char) = c in ",.!?;:)\""
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
        fun previousWord(before: String) = before.trimEnd().takeLastWhile { it.isLetter() || it == '\'' }.lowercase()
        fun boundary(c: Char?) = c == null || c.isWhitespace() || c in "([{“‘\",;!?"
    }
}
