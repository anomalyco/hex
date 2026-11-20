package ly.anoma.hex.typing

/** The current word only. Both the keyboard and replay feed the real tap at each letter. */
internal class TypingWord(val decoder: SpatialDecoder, val spatial: Boolean) {
    private val model = decoder.model
    var token = ""
        private set
    private var previous = -1
    private var literalOnly = false
    private val taps = ArrayList<SpatialDecoder.Tap>()
    private var decision: SpatialDecoder.Decision? = null
    fun reset(before: String = "") {
        token = ""; taps.clear(); decision = null; literalOnly = false
        previous = model.index(previousWord(before))
    }
    fun restored(text: String, before: String) { reset(before); explicit(text) } // No taps: space keeps the undone spelling.
    fun predictions(before: String) = model.nextWords(model.index(previousWord(before)))
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
    private fun decision() = decision ?: decoder.decide(token.lowercase(), taps, previous).also { decision = it }
    private fun protected() = token.drop(1).any { it.isUpperCase() } ||
        (model.language == "en" && token.lowercase() in shorthand)
    private fun cased(s: String) = if (token.firstOrNull()?.isUpperCase() == true) s.replaceFirstChar { it.uppercase() } else s
    fun replacement(): String? {
        if (literalOnly || token.isEmpty()) return null
        val lower = token.lowercase()
        val allCaps = token.length >= 2 && token.all { it.isUpperCase() }
        if (token.drop(1).any { it.isUpperCase() } && !allCaps) return null
        if (model.language == "en") {
            if (lower in shorthand) return null
            englishFixes[lower]?.let { return if (allCaps) {
                if (token.length >= 3) it.uppercase() else null
            } else cased(it) }
        }
        return if (allCaps || !spatial) null else decision().replace?.let(::cased)
    }
    fun suggestions(): List<String> {
        if (token.length !in 1..MAX_LENGTH) return emptyList()
        val options = LinkedHashMap<String, Int>()
        for (w in model.completions(token.lowercase())) if (w >= 0) options[model.text(w)] = model.frequencyCost(w)
        if (spatial && options.size < 3 && token.length >= 2 && !protected()) for (r in decision().readings)
            options.putIfAbsent(r.text, r.frequency)
        if (options.isEmpty()) return listOf(token)
        return options.entries.sortedWith(compareBy<Map.Entry<String, Int>> { it.value }.thenBy { it.key })
            .take(3).map { cased(it.key) }
    }
    companion object {
        const val MAX_LENGTH = 32
        private val shorthand = "ok okay ty thx pls plz tmrw tbh idk imo imho lol lmao omg brb btw afaik fyi nvm rn irl jk np yw smh ikr ngl fr gg dm pr prs ci ui ux ios macos api apis url urls json sql ssh cli gpu cpu ram sdk hex wisp seer rune kit gonna wanna gotta kinda sorta yeah yep nope nah hey yo".split(' ').toSet()
        fun previousWord(before: String) = before.trimEnd().takeLastWhile { it.isLetter() || it == '\'' }.lowercase()
        fun boundary(c: Char?) = c == null || c.isWhitespace() || c in "([{“‘\",;!?"
    }
}
