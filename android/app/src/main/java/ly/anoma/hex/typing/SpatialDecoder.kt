package ly.anoma.hex.typing

import java.text.Normalizer

/** Positions are all in the same layout coordinates. The IME supplies measured key centres;
 * the replay supplies the original test layout, not a guessed phone screen size. */
internal data class KeyPoint(val x: Double, val y: Double)
/** The space bar's keycap, in the same coordinates as the key centres. */
internal data class KeyRect(val left: Double, val top: Double, val right: Double, val bottom: Double) {
    val centreY get() = (top + bottom) / 2
}
/** `space` is the space bar's keycap, or null when unknown: then a tap on the bottom letter row
 * is never read as a missed space. [plane] is the keys as Labs › Keyboard › Adaptive touch learns them, when the
 * keyboard draws them; [bottomY] stays the drawn bottom row's when Adaptive touch moves the centres. */
internal class KeyGrid @JvmOverloads constructor(val centres: Map<Char, KeyPoint>, val pitchX: Double, val pitchY: Double,
                                                 val space: KeyRect? = null, val plane: TouchPlane? = null,
                                                 val bottomY: Double = centres.values.maxOf { it.y }) {
    /** How far a tap strays sideways from its key's centre, one standard deviation. */
    val spreadX get() = pitchX * .29
    // R1's reference pitch is 39.5 × 56. Keep its vertical spread and its 6-nat split
    // prior on that layout. On shorter rows the travel to space is shorter: scale its
    // Gaussian (squared-distance) cost, down to the Android 2%-omission prior.
    private val referenceY = pitchX * (56.0 / 39.5)
    val verticalSpread = .26 * maxOf(pitchY, referenceY)
    private val rowScale = minOf(1.0, pitchY / referenceY)
    val missedSpaceCost = maxOf(3.9, 6.0 * rowScale * rowScale)
}
internal class SpatialDecoder(val model: TypingModel) {
    /** Your own words (`PersonalWords`), read beside the model's; null or empty without any. */
    @Volatile var personal: PersonalWords? = null
    /** Keyboard › Swipe typing's reader for this language, made at the first swipe. */
    val swipe by lazy { SwipeReader(this) }
    /** `spaceCost`: what reading this tap as a space the finger missed costs when it landed on
     * the bottom row's upper half, or infinity (`spaceTapCost`). */
    class Tap(val cost: DoubleArray, val choices: IntArray, val belowLetters: Boolean, val missedSpaceCost: Double,
              val spaceCost: Double = Double.POSITIVE_INFINITY)
    data class Reading(val text: String, val cost: Double, val frequency: Int)
    data class Decision(val replace: String?, val readings: List<Reading>)
    // Google Books English has richer word pairs; these are the shared iOS R1 weights.
    private val lmWeight = if (model.language == "en") 1.5 else 1.25
    // Spanish's many one- and two-letter words ("a", "es", "va") make a letter read as a space too
    // cheap a split ("vaxes" -> "va es"): there, only a tap below the letters is one.
    private val spaceTapCost = if (model.language == "es") Double.POSITIVE_INFINITY else 5.0
    fun score(grid: KeyGrid, x: Double, y: Double): Tap {
        val costs = DoubleArray(model.alphabet + 1) { Double.POSITIVE_INFINITY }
        val sy = grid.verticalSpread
        for ((c, p) in grid.centres) {
            val code = model.code(c)
            if (code == 0) continue
            val dx = (x - p.x) / grid.spreadX
            val dy = (y - p.y) / sy
            costs[code] = .5 * (dx * dx + dy * dy)
        }
        val min = costs.min()
        // An accented letter also comes from its base letter's key, even when it has a key of its
        // own (ä from a on QWERTZ, ñ from n): whichever is cheaper, as iOS reads it.
        val raw = costs.copyOf()
        for (i in 1 until costs.size) {
            val base = model.bases[i]
            if (base != i && raw[base].isFinite()) costs[i] = minOf(raw[i], raw[base] + ACCENT_COST)
        }
        for (i in 1 until costs.size) costs[i] -= min
        val choices = (1 until costs.size).filter { costs[it] <= MAX_TAP_COST }.sortedBy { costs[it] }.toIntArray()
        return Tap(costs, choices, y > grid.bottomY, grid.missedSpaceCost, spaceCost(grid, x, y))
    }

    // A tap on the bottom letter row's upper half, above the space bar, read as a space the
    // finger missed ("ofbthe"): the more likely the row than the bar, the dearer.
    private fun spaceCost(grid: KeyGrid, x: Double, y: Double): Double {
        val bar = grid.space ?: return Double.POSITIVE_INFINITY
        val slack = grid.pitchX / 2
        if (!spaceTapCost.isFinite() || bar.right <= bar.left || x < bar.left - slack || x > bar.right + slack ||
            y <= grid.bottomY - grid.pitchY / 2 || y > grid.bottomY) return Double.POSITIVE_INFINITY
        val toBar = (y - bar.centreY) / grid.verticalSpread
        val toRow = (y - grid.bottomY) / grid.verticalSpread
        return spaceTapCost + SPACE_TAP_WEIGHT * maxOf(0.0, .5 * (toBar * toBar - toRow * toRow))
    }

    /** How much likelier the space bar than the bottom letter row a space tap at (x, y) was, in nats
     * (0 when the row is likelier); infinity when the bar is unknown or the tap isn't above it. */
    fun rowCost(grid: KeyGrid, x: Double, y: Double): Double {
        val bar = grid.space ?: return Double.POSITIVE_INFINITY
        val slack = grid.pitchX / 2
        if (bar.right <= bar.left || x < bar.left - slack || x > bar.right + slack) return Double.POSITIVE_INFINITY
        val toBar = (y - bar.centreY) / grid.verticalSpread
        val toRow = (y - grid.bottomY) / grid.verticalSpread
        return maxOf(0.0, .5 * (toRow * toRow - toBar * toBar))
    }

    /** One word in place of two, where the space between them was tapped for a letter on the row
     * above ("deli ate" -> "delicate"): the likeliest reading of all the taps, the space read as a
     * letter, when it beats the two words as they read by the margin and the space tap was near
     * enough the row (`reach`, from `rowCost`). Null to keep both. Lowercase spellings. */
    fun merge(first: String, firstTaps: List<Tap>, space: Tap, reach: Double, second: String, secondTaps: List<Tap>,
              previous: Int, firstValid: Boolean, secondValid: Boolean, swears: Boolean): String? {
        if (reach > MERGE_REACH || first.isEmpty() || second.isEmpty() || firstTaps.size != first.length ||
            secondTaps.size != second.length || (first + second).any { model.code(it) == 0 || it == '\'' }) return null
        val taps = firstTaps + space + secondTaps
        // Each word as space would leave it: as typed, or its likeliest reading ("i dkdnt" is "I didn't").
        fun reading(spelling: String, from: Int, before: Int, valid: Boolean): Pair<Double, Int> {
            val typed = model.index(spelling)
            var spatial = 0.0
            for (i in spelling.indices) spatial += taps[from + i].cost[model.code(spelling[i])]
            var best = spatial + lmWeight * (if (typed >= 0) model.languageCost(before, typed) else if (valid) 15.0 else 22.0)
            var word = typed
            for (f in search(taps, from, from + spelling.length, 1, swears, before).sortedWith(spatialOrder).take(3)) {
                val cost = f.cost + lmWeight * model.languageCost(before, f.word)
                if (cost < best) { best = cost; word = f.word }
            }
            return best to word
        }
        val (firstCost, firstWord) = reading(first, 0, previous, firstValid)
        val split = firstCost + reading(second, first.length + 1, firstWord, secondValid).first
        if (!split.isFinite()) return null
        var best = -1
        var bestCost = Double.POSITIVE_INFINITY
        for (f in search(taps, 0, taps.size, 1, swears, previous).sortedWith(spatialOrder).take(3)) {
            if (model.length(f.word) <= maxOf(first.length, second.length) ||
                model.neverSuggest.blocksFollowing(f.word, swears, previous)) continue
            val cost = f.cost + lmWeight * model.languageCost(previous, f.word) + reach
            if (cost < bestCost) { best = f.word; bestCost = cost }
        }
        if (best < 0) return null
        return if (split - bestCost >= if (firstValid && secondValid) 7.0 else 2.0) model.text(best) else null
    }

    private class State(val lo: Int, val hi: Int, val tap: Int, val edits: Int, val cost: Double)
    private data class Found(val word: Int, val cost: Double)
    // Keep at most 48 promising prefixes at each spelling depth. One extra, missing or
    // transposed letter is possible; an untyped apostrophe costs nothing.
    // The words of [lexicon] (the model unless given) the taps from..end read as. Only the model's
    // words are checked against the never-suggest list: your own words are yours to offer.
    private fun search(taps: List<Tap>, from: Int, end: Int, edits: Int, swears: Boolean, previous: Int,
                       lexicon: Lexicon = model): List<Found> {
        val depthLimit = end - from + edits + 2
        val context = model.neverSuggest.restrictedContext(previous)
        val own = lexicon === model
        val layers = Array(depthLimit + 1) { ArrayList<State>() }
        layers[0].add(State(0, lexicon.size, from, 0, 0.0))
        val found = HashMap<Int, Double>()
        // Dropping a clearly aimed, distinct tap needs more evidence than dropping a repeat or a
        // tap between keys. Accents share their physical key.
        val extraTap = DoubleArray(end - from) { i ->
            val t = from + i
            val code = taps[t].choices.firstOrNull() ?: return@DoubleArray EDIT_COST
            val key = model.bases[code]
            for (n in intArrayOf(t - 1, t + 1)) if (n in from until end &&
                taps[n].choices.firstOrNull()?.let { model.bases[it] == key } == true) return@DoubleArray EDIT_COST
            val gap = taps[t].choices.firstOrNull { model.bases[it] != key }?.let { taps[t].cost[it] } ?: MAX_TAP_COST
            EDIT_COST + minOf(4.0, gap)
        }
        for (d in 0..depthLimit) {
            val layer = layers[d]
            if (layer.size > 48) { layer.sortBy { it.cost }; layer.subList(48, layer.size).clear() }
            var i = 0
            while (i < layer.size) {
                val s = layer[i++]
                if (s.tap == end && lexicon.length(s.lo) == d && (!own || !model.neverSuggest.blocks(s.lo, swears, context)) &&
                    s.cost < (found[s.lo] ?: Double.POSITIVE_INFINITY))
                    found[s.lo] = s.cost
                if (d == depthLimit) continue
                fun push(c: Int, next: Int, e: Int, cost: Double) {
                    val child = lexicon.child(s.lo, s.hi, d, c)
                    if (child != 0L) layers[d + 1].add(State((child ushr 32).toInt(), child.toInt(), next, e, cost))
                }
                val t = s.tap
                if (t < end) for (c in taps[t].choices) push(c, t + 1, s.edits, s.cost + taps[t].cost[c])
                if (d > 0 && model.apostrophe != 0) push(model.apostrophe, t, s.edits, s.cost)
                if (s.edits >= edits) continue
                for (c in 1..model.alphabet) if (c != model.apostrophe) push(c, t, s.edits + 1, s.cost + EDIT_COST)
                if (t < end) layer.add(State(s.lo, s.hi, t + 1, s.edits + 1, s.cost + extraTap[t - from]))
                if (t + 1 >= end || d + 2 > depthLimit) continue
                val first = taps[t + 1]; val second = taps[t]
                for (ai in 0 until minOf(3, first.choices.size)) {
                    val a = first.choices[ai]; val r = lexicon.child(s.lo, s.hi, d, a)
                    if (r == 0L) continue
                    for (bi in 0 until minOf(3, second.choices.size)) {
                        val b = second.choices[bi]; if (b == a) continue
                        val child = lexicon.child((r ushr 32).toInt(), r.toInt(), d + 1, b)
                        if (child != 0L) layers[d+2].add(State((child ushr 32).toInt(), child.toInt(), t+2,
                            s.edits+1, s.cost + first.cost[a] + second.cost[b] + 5))
                    }
                }
            }
        }
        return found.map { Found(it.key, it.value) }
    }

    fun decide(typed: String, taps: List<Tap>, previous: Int, swears: Boolean): Decision {
        val none = Decision(null, emptyList())
        if (typed.length !in 2..32 || taps.size != typed.length || model.neverSuggest.blocks(typed, swears, previous) ||
            typed.any { model.code(it) == 0 || it == '\'' }) return none
        val literal = model.index(typed)
        val personal = personal?.takeIf { it.size > 0 }
        val yours = personal?.index(typed) ?: -1
        val knownLiteral = literal >= 0 || model.known(typed) || yours >= 0
        // A rare word the books use, outside the model: what it costs by its count, not a known word's.
        val listed = if (literal < 0 && yours < 0) model.validCost(typed) else null
        var lm = when {
            literal >= 0 -> model.languageCost(previous, literal)
            listed != null -> listed
            knownLiteral -> 15.0
            else -> 22.0
        }
        // One of your own words scores as a moderately common one (about the 10,000th).
        if (yours >= 0) lm = minOf(lm, USER_WORD_COST)
        var cost = lmWeight * lm
        for (i in taps.indices) cost += taps[i].cost[model.code(typed[i])]
        if (!cost.isFinite()) return none
        val readings = ArrayList<Reading>()
        // iOS applies the language score only to the five best spatial readings.
        for ((w, spatial) in search(taps, 0, taps.size, 1, swears, previous).sortedWith(spatialOrder).take(5)) if (w != literal)
            readings.add(Reading(model.text(w), spatial + lmWeight * model.languageCost(previous, w), model.frequencyCost(w)))
        if (personal != null) for ((w, spatial) in search(taps, 0, taps.size, 1, swears, previous, personal).sortedWith(spatialOrder).take(3))
            if (w != yours) readings.add(Reading(personal.text(w), spatial + lmWeight * USER_WORD_COST, (USER_WORD_COST * 8).toInt()))
        if (taps.size >= 3) {
            fun alone(f: Found) = model.length(f.word) > 1 || model.frequencyCost(f.word) < 56
            fun half(a: Int, b: Int, prev: Int) = search(taps, a, b, 0, swears, prev).sortedWith(spatialOrder)
                .take(3).filter(::alone)
            // Around a tap on the bottom row read as space, each half only as the letters typed, so
            // a name's middle letter ("rima") is never a space with slips around it.
            fun exact(a: Int, b: Int): List<Found> {
                val spelling = typed.substring(a, b)
                val word = model.index(spelling)
                if (word < 0 || model.neverSuggest.blocks(word, swears)) return emptyList()
                var cost = 0.0
                for (i in a until b) cost += taps[i].cost[model.code(typed[i])]
                return listOf(Found(word, cost)).filter(::alone)
            }
            fun pair(left: List<Found>, right: List<Found>, extra: Double) {
                for (l in left) for (r in right) {
                    if (model.neverSuggest.blocksFollowing(r.word, swears, l.word)) continue
                    val lm = model.languageCost(previous, l.word) + model.languageCost(l.word, r.word)
                    readings.add(Reading(model.text(l.word) + " " + model.text(r.word), l.cost + r.cost + extra + lmWeight * lm,
                        model.frequencyCost(l.word) + model.frequencyCost(r.word)))
                }
            }
            for (k in 1 until taps.size) {
                val left = half(0, k, previous)
                if (left.isNotEmpty()) pair(left, half(k, taps.size, -1), taps[k].missedSpaceCost)
                if (k + 1 >= taps.size) continue
                if (taps[k].belowLetters) {
                    if (left.isNotEmpty()) pair(left, half(k + 1, taps.size, -1), taps[k].missedSpaceCost)
                } else if (taps[k].spaceCost.isFinite()) {
                    val exactLeft = exact(0, k)
                    if (exactLeft.isNotEmpty()) pair(exactLeft, exact(k + 1, taps.size), taps[k].spaceCost)
                }
            }
        }
        readings.sortWith(compareBy<Reading> { it.cost }.thenBy { it.text })
        val best = readings.firstOrNull()
        // A listed rare word takes a one-word fix as a spelling nobody knows does, but is split into
        // words only as a known word is.
        val margin = if (best == null || !knownLiteral || foldAccents(best.text.lowercase()) == foldAccents(typed) ||
            (listed != null && ' ' !in best.text)) 2.0 else 7.0
        var replace = if (best != null && cost - best.cost >= margin) best.text else null
        var shown = readings.asSequence().filter { it.cost < cost && !it.text.equals(typed, ignoreCase = true) }
            .distinctBy { it.text.lowercase() }.toMutableList()
        // Nothing above read a spelling that is no word at all: a missed space may still explain it,
        // with a slip in the second word or three or more words run together (iOS #2244). Never a
        // known spelling or a likelier reading that only missed the margin; not German, whose model
        // is lowercase and whose nouns take their capital in the one-word path.
        if (replace == null && !knownLiteral && model.language != "de" && typed.length >= 5) {
            missedSpaceReading(taps, typed, previous, swears, cost)?.let { fallback ->
                val likeliest = shown.firstOrNull()
                if (likeliest == null || fallback.cost <= likeliest.cost || likeliest.text.equals(fallback.text, ignoreCase = true)) {
                    replace = fallback.text
                    shown.removeAll { it.text.equals(fallback.text, ignoreCase = true) }
                    shown.add(0, fallback)
                }
            }
        }
        if (best == null && replace == null) return none
        return Decision(replace, shown.take(3))
    }

    /** Whether [decide] reading [taps] would put [reading], one word, in place of [typed]: it beats the spelling as
     * typed by the same margin. Null for a reading of two words or more, which only [decide] can tell. Labs › Keyboard ›
     * Adaptive touch asks with the taps read on the drawn keys, so a correction its own centres make is one the keyboard
     * makes without them (iOS TypingDecoder.replaces). */
    fun replaces(reading: String, typed: String, taps: List<Tap>, previous: Int, swears: Boolean): Boolean? {
        if (' ' in reading) return null
        if (typed.length !in 2..32 || taps.size != typed.length || model.neverSuggest.blocks(typed, swears, previous) ||
            typed.any { model.code(it) == 0 || it == '\'' }) return false
        // The spelling as typed, scored as decide scores it.
        val literal = model.index(typed)
        val personal = personal?.takeIf { it.size > 0 }
        val yours = personal?.index(typed) ?: -1
        val knownLiteral = literal >= 0 || model.known(typed) || yours >= 0
        val listed = if (literal < 0 && yours < 0) model.validCost(typed) else null
        var lm = when {
            literal >= 0 -> model.languageCost(previous, literal)
            listed != null -> listed
            knownLiteral -> 15.0
            else -> 22.0
        }
        if (yours >= 0) lm = minOf(lm, USER_WORD_COST)
        var cost = lmWeight * lm
        for (i in taps.indices) cost += taps[i].cost[model.code(typed[i])]
        if (!cost.isFinite()) return false
        // The reading as decide scores it: a word of the model's the never-suggest list lets through after the word
        // before, or one of yours.
        val lower = reading.lowercase()
        val word = model.index(lower)
        var readingLm = if (word >= 0 && !model.neverSuggest.blocks(word, swears, model.neverSuggest.restrictedContext(previous)))
            model.languageCost(previous, word) else Double.POSITIVE_INFINITY
        if ((personal?.index(lower) ?: -1) >= 0) readingLm = minOf(readingLm, USER_WORD_COST)
        if (!readingLm.isFinite()) return false
        val margin = if (!knownLiteral || foldAccents(lower) == foldAccents(typed) || listed != null) 2.0 else 7.0
        val codes = IntArray(lower.length) { model.code(lower[it]) }
        if (codes.any { it == 0 }) return false
        // One alignment the search also tries usually clears it already: no search then.
        aligned(codes, taps)?.let { if (cost - (it + lmWeight * readingLm) >= margin) return true }
        val spatial = search(taps, 0, taps.size, 1, swears, previous, OneWord(codes)).firstOrNull()?.cost ?: return false
        return cost - (spatial + lmWeight * readingLm) >= margin
    }

    /** Other words [typed], a word already finished, could have been meant as (Labs › Keyboard › Fix earlier words): its
     * letters read as taps on their keys' centres in [grid], with one letter missed, extra or swapped as typing allows.
     * A reading is offered only when it fits the word before ([previous], a context index) and the word after ([next], a
     * model index, or -1) better than [typed] does by at least [margin] nats, so a word that already fits offers nothing.
     * Likeliest first (keys and words together), lowercase; none for a word of your own, one with a letter the keys don't
     * have, or one the never-suggest list holds. */
    fun alternatives(typed: String, grid: KeyGrid, previous: Int, next: Int, swears: Boolean,
                     margin: Double = EARLIER_MARGIN, limit: Int = 3): List<String> {
        val lower = typed.lowercase().replace('\u2019', '\'')
        val letters = lower.filter { it != '\'' }
        if (letters.length !in 2..32 || lower.any { it != '\'' && model.code(it) == 0 } ||
            model.neverSuggest.blocks(lower, swears, previous) || personal?.owns(lower) == true) return emptyList()
        val taps = letters.map { c ->
            val p = grid.centres[c] ?: grid.centres[foldAccents(c.toString()).firstOrNull() ?: c] ?: return emptyList()
            score(grid, p.x, p.y)
        }
        // How well a word fits between its neighbours: the pair before it and the pair after it.
        fun context(word: Int) = model.languageCost(previous, word) + (if (next >= 0) model.languageCost(word, next) else 0.0)
        val literal = model.index(lower)
        val own = when {
            literal >= 0 -> context(literal)
            // A spelling the model lacks scores as decide scores one, and the word after it by its own frequency.
            else -> (if (model.known(lower)) 15.0 else 22.0) + (if (next >= 0) 0.92 + model.unigramCost(next) else 0.0)
        }
        return search(taps, 0, taps.size, 1, swears, previous).sortedWith(spatialOrder).take(EARLIER_READINGS)
            .asSequence()
            .filter { it.word != literal }
            .map { Triple(it.word, context(it.word), it.cost) }
            .filter { own - it.second >= margin }
            .sortedBy { it.third + lmWeight * it.second }
            .map { model.text(it.first) }
            .filter { it != lower }
            .distinct().take(limit).toList()
    }

    // The cost of [codes] read letter for letter from the taps, or with one letter not typed at its cheapest place: never
    // below what the search finds, so a bound to clear. Null when neither fits (another edit, a tap past MAX_TAP_COST).
    private fun aligned(codes: IntArray, taps: List<Tap>): Double? {
        val letters = ArrayList<Int>(codes.size)
        for ((depth, code) in codes.withIndex()) {
            if (code == model.apostrophe) { if (depth == 0) return null; continue }
            letters.add(code)
        }
        val n = taps.size
        fun tap(i: Int, code: Int) = taps[i].cost[code].takeIf { it <= MAX_TAP_COST }
        if (letters.size == n) {
            var total = 0.0
            for (i in 0 until n) total += tap(i, letters[i]) ?: return null
            return total
        }
        if (letters.size != n + 1) return null
        // The letter not typed goes in before tap k: taps before it read letters 0 until k, the rest one letter on.
        val prefix = DoubleArray(n + 1) { Double.POSITIVE_INFINITY }
        prefix[0] = 0.0
        for (i in 0 until n) prefix[i + 1] = prefix[i] + (tap(i, letters[i]) ?: Double.POSITIVE_INFINITY)
        val suffix = DoubleArray(n + 1) { Double.POSITIVE_INFINITY }
        suffix[n] = 0.0
        for (i in n - 1 downTo 0) suffix[i] = suffix[i + 1] + (tap(i, letters[i + 1]) ?: Double.POSITIVE_INFINITY)
        var best = Double.POSITIVE_INFINITY
        for (k in 0..n) best = minOf(best, prefix[k] + suffix[k])
        return if (best.isFinite()) best + EDIT_COST else null
    }

    // One word as a lexicon, for the search to score just it.
    private class OneWord(private val codes: IntArray) : Lexicon {
        override val size get() = 1
        override fun length(word: Int) = codes.size
        override fun child(lo: Int, hi: Int, depth: Int, code: Int): Long =
            if (lo == 0 && hi == 1 && depth < codes.size && codes[depth] == code) 1L else 0L
    }

    // What a missed space can still make of taps nothing else read (iOS missedSpaceReading): the
    // cheaper of a word and a slipped second word, or three to six words spelled as typed, taken only
    // when its words, without the spaces' cost, cost no more than the run nobody knows does.
    private fun missedSpaceReading(taps: List<Tap>, typed: String, previous: Int, swears: Boolean, literalCost: Double): Reading? {
        val space = taps[0].missedSpaceCost
        fun affordable(r: Reading?) = r?.takeIf { it.cost - space * (it.text.count { c -> c == ' ' }) <= literalCost }
        val slipped = affordable(slippedSecondWord(taps, typed, previous, swears))
        val segmented = affordable(exactSegmentation(taps, typed, previous, swears))
        return when {
            slipped != null && segmented != null -> if (segmented.cost < slipped.cost) segmented else slipped
            else -> slipped ?: segmented
        }
    }

    private fun canStandAlone(word: Int) = model.length(word) > 1 || model.frequencyCost(word) < 56

    // The first letters as a word and the rest one slip from a word after it, clearing the margin it
    // would need typed after that first word ("specialchsracter" -> "special character"). A rest that
    // is a word already is the ordinary split's; the first word is two letters or more.
    private fun slippedSecondWord(taps: List<Tap>, typed: String, previous: Int, swears: Boolean): Reading? {
        val n = typed.length
        var best: Reading? = null
        for (k in 2..n - 3) {
            val left = model.index(typed.substring(0, k))
            if (left < 0 || !canStandAlone(left) || model.neverSuggest.blocks(left, swears, previous) || model.index(typed.substring(k)) >= 0) continue
            var leftSpatial = 0.0
            var rightLiteral = 0.0
            for (i in 0 until k) leftSpatial += taps[i].cost[model.code(typed[i])]
            for (i in k until n) rightLiteral += taps[i].cost[model.code(typed[i])]
            rightLiteral += lmWeight * 22.0
            val leftCost = leftSpatial + lmWeight * model.languageCost(previous, left)
            for (f in search(taps, k, n, 1, swears, left).sortedWith(spatialOrder).take(5)) {
                if (model.neverSuggest.blocksFollowing(f.word, swears, left)) continue
                val rightCost = f.cost + lmWeight * model.languageCost(left, f.word)
                if (rightLiteral - rightCost < 2.0) continue
                val cost = leftCost + rightCost + taps[k].missedSpaceCost
                if (best == null || cost < best.cost)
                    best = Reading(model.text(left) + " " + model.text(f.word), cost, model.frequencyCost(left) + model.frequencyCost(f.word))
            }
        }
        return best
    }

    private class Path(val words: List<Int>, val cost: Double)

    // Three to six words typed with no spaces, each spelled as typed ("seeyoutomorrow" -> "see you
    // tomorrow"): the cheapest path per end, last word and word count, words found by walking the
    // model's list one letter at a time from each start.
    private fun exactSegmentation(taps: List<Tap>, typed: String, previous: Int, swears: Boolean): Reading? {
        val n = typed.length
        if (n !in 5..32) return null
        val spatial = DoubleArray(n + 1)
        for (i in 0 until n) spatial[i + 1] = spatial[i] + taps[i].cost[model.code(typed[i])]
        val states = Array(n + 1) { HashMap<Long, Path>() }
        states[0][key(-1, 0)] = Path(emptyList(), 0.0)
        var best: Path? = null
        for (start in 0 until n) {
            if (states[start].isEmpty()) continue
            val ends = ArrayList<Pair<Int, Int>>()
            var lo = 0; var hi = model.size
            for (end in start + 1..n) {
                val child = model.child(lo, hi, end - start - 1, model.code(typed[end - 1]))
                if (child == 0L) break
                lo = (child ushr 32).toInt(); hi = child.toInt()
                if (model.length(lo) == end - start && canStandAlone(lo) && !model.neverSuggest.blocks(lo, swears)) ends.add(end to lo)
            }
            if (ends.isEmpty()) continue
            for ((k, path) in states[start]) {
                val last = (k shr 8).toInt()
                val before = if (last >= 0) last else previous
                for ((end, word) in ends) {
                    if (model.neverSuggest.blocksFollowing(word, swears, before)) continue
                    val words = path.words + word
                    val cost = path.cost + (spatial[end] - spatial[start]) + lmWeight * model.languageCost(before, word) +
                        (if (path.words.isEmpty()) 0.0 else taps[start].missedSpaceCost)
                    if (end == n) {
                        if (words.size in 3..6 && (best == null || cost < best.cost)) best = Path(words, cost)
                        continue
                    }
                    if (words.size >= 6) continue
                    val next = key(word, words.size)
                    val known = states[end][next]
                    if (known != null && known.cost <= cost) continue
                    states[end][next] = Path(words, cost)
                }
            }
        }
        return best?.let { p -> Reading(p.words.joinToString(" ") { model.text(it) }, p.cost, p.words.sumOf { model.frequencyCost(it) }) }
    }

    private fun key(last: Int, count: Int) = (last.toLong() shl 8) or count.toLong()
    private companion object {
        val spatialOrder = compareBy<Found> { it.cost }.thenBy { it.word }
        const val EDIT_COST = 7.0
        const val MAX_TAP_COST = 9.0
        // An accented letter typed as its base letter's key (é from e).
        const val ACCENT_COST = 1.0
        const val SPACE_TAP_WEIGHT = .25
        // A space tap reads as a letter only within about the bar's top fifth.
        const val MERGE_REACH = 3.0
        const val USER_WORD_COST = 11.0
        // Fix earlier words: how many of the search's readings are weighed, and how much better than the word as it
        // stands one must fit the words around it to be offered. The margin is on the word pairs alone (the keys don't
        // count: the word's own letters always read best), so it is smaller than a spatial-plus-language margin such as
        // the issue's 10 nats. On the English model, 3 offers "to" for "too" in "going too the" (7.3 nats better), "from"
        // for "form" in "came form the" (8.2) and "read" for "red" in "I red the" (4.7), and keeps a word that fits beside
        // a likelier neighbour: "car" for "cat" scores 1.3 to 2.4 better in "a cat sat", "the cat is", "my cat is" and
        // "a cat and", and "from" is 1.0 worse in "in form of".
        const val EARLIER_READINGS = 8
        const val EARLIER_MARGIN = 3.0
    }
    private fun foldAccents(s: String) = Normalizer.normalize(s, Normalizer.Form.NFD).filter { it !in '\u0300'..'\u0308' || it == '\u0303' }
}
