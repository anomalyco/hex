package ly.anoma.hex.typing

/** Words the words around them decide rather than the taps (iOS `AutocorrectEngine.contextChoice`,
 * #2144), scored with the model's word pairs. English: the same letters with and without an
 * apostrophe ("id" or "I'd", "its" or "it's") and words that sound alike ("your", "there", "then");
 * "were" and "we're" aren't here, the pairs can't tell them apart. French and Italian: the same
 * letters with and without an accent, both words ("a" or "à", "ou" or "où", Italian "e" or "è"),
 * which iOS leaves to its dictionary. */
internal object ContextWords {
    private val groups = mapOf(
        "en" to listOf(
            listOf("id", "I'd"), listOf("ill", "I'll"), listOf("hell", "he'll"), listOf("shell", "she'll"),
            listOf("wed", "we'd"), listOf("lets", "let's"), listOf("well", "we'll"),
            listOf("its", "it's"), listOf("your", "you're"), listOf("there", "their", "they're"), listOf("then", "than"),
        ),
        "fr" to listOf(listOf("a", "à"), listOf("ou", "où"), listOf("la", "là"), listOf("ca", "ça"), listOf("du", "dû"),
            listOf("sur", "sûr")),
        "it" to listOf(listOf("e", "è"), listOf("la", "là"), listOf("li", "lì"), listOf("da", "dà"), listOf("ne", "né"),
            listOf("se", "sé")),
    )
    private val groupOf = groups.mapValues { (_, list) -> list.flatMap { group -> group.map { it.lowercase() to group } }.toMap() }
    // The pairs barely spell contractions (the books split "I'd" into "I" and "'d"), so a
    // contraction is scored as the words it stands for.
    private val expansions = mapOf(
        "i'd" to listOf(listOf("i", "would"), listOf("i", "had")), "i'll" to listOf(listOf("i", "will")),
        "he'll" to listOf(listOf("he", "will")), "she'll" to listOf(listOf("she", "will")),
        "we'd" to listOf(listOf("we", "would"), listOf("we", "had")), "let's" to listOf(listOf("let", "us")),
        "we'll" to listOf(listOf("we", "will")), "it's" to listOf(listOf("it", "is"), listOf("it", "has")),
        "you're" to listOf(listOf("you", "are")), "they're" to listOf(listOf("they", "are")),
    )
    // Nats the better word must win by, with the word before only and once the next word is known
    // too; changing only an apostrophe needs less than changing letters.
    private const val PREVIOUS_MARGIN = 3.0
    private const val NEXT_MARGIN = 1.5
    private const val LETTER_PREVIOUS_MARGIN = 6.0
    private const val LETTER_NEXT_MARGIN = 3.0
    private const val UNKNOWN = 15.0
    // Sentences start with "I'd" and "It's" far more often than the word list alone says.
    private const val SENTENCE_START = 3.0
    // "will", "would", "had" and "us" are followed by thousands of words in the pairs: a next word
    // that never follows one is evidence against the contraction ("till hell freezes").
    private const val MISSING_FOLLOWER = 5.0
    private val richFollowers = setOf("will", "would", "had", "us")
    private fun fold(s: String) = java.text.Normalizer.normalize(s, java.text.Normalizer.Form.NFD).filter { it !in '\u0300'..'\u036f' }

    /** Whether [language] has words its context decides. */
    fun decides(language: String) = language in groups

    private fun cost(model: TypingModel, word: String, previous: String?, next: String?, startsSentence: Boolean): Double {
        val expanded = if (model.language == "en") expansions[word.lowercase()] else null
        var best = Double.POSITIVE_INFINITY
        for (words in expanded ?: listOf(listOf(word.lowercase()))) {
            var cost = model.cost(words[0], previous) ?: UNKNOWN
            for (k in 1 until words.size) cost += model.cost(words[k], words[k - 1]) ?: UNKNOWN
            if (next != null) {
                val last = words.last()
                cost += model.cost(next, last) ?: UNKNOWN
                if (last in richFollowers && !model.hasPair(last, next)) cost += MISSING_FOLLOWER
            }
            best = minOf(best, cost)
        }
        return if (expanded != null && startsSentence) best - SENTENCE_START else best
    }

    /** The word of `word`'s group the words around it make clearly likelier, cased as `word`, or
     * null to keep it. `previous` and `next` are lowercase, null when unknown. */
    fun choice(model: TypingModel, word: String, previous: String?, next: String?, startsSentence: Boolean): String? {
        val lower = word.lowercase()
        val group = groupOf[model.language]?.get(lower) ?: return null
        val own = cost(model, word, previous, next, startsSentence)
        var best: String? = null
        var bestCost = Double.POSITIVE_INFINITY
        for (other in group) {
            if (other.lowercase() == lower) continue
            val c = cost(model, other, previous, next, startsSentence)
            if (c < bestCost) { best = other; bestCost = c }
        }
        best ?: return null
        val sameLetters = fold(best.replace("'", "").lowercase()) == fold(lower.replace("'", ""))
        val margin = when {
            sameLetters -> if (next == null) PREVIOUS_MARGIN else NEXT_MARGIN
            else -> if (next == null) LETTER_PREVIOUS_MARGIN else LETTER_NEXT_MARGIN
        }
        if (own - bestCost < margin) return null
        // A capital first letter carries over (a sentence's start), except the I of "I'd", which comes off.
        val startsWithI = word.startsWith("I'") && word.drop(2).all { it.isLowerCase() }
        return if (word.first().isUpperCase() && !startsWithI) best.replaceFirstChar { it.uppercase() } else best
    }
}
