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
 * is never read as a missed space. */
internal class KeyGrid(val centres: Map<Char, KeyPoint>, val pitchX: Double, val pitchY: Double, val space: KeyRect? = null) {
    val bottomY = centres.values.maxOf { it.y }
    // R1's reference pitch is 39.5 × 56. Keep its vertical spread and its 6-nat split
    // prior on that layout. On shorter rows the travel to space is shorter: scale its
    // Gaussian (squared-distance) cost, down to the Android 2%-omission prior.
    private val referenceY = pitchX * (56.0 / 39.5)
    val verticalSpread = .26 * maxOf(pitchY, referenceY)
    private val rowScale = minOf(1.0, pitchY / referenceY)
    val missedSpaceCost = maxOf(3.9, 6.0 * rowScale * rowScale)
}
internal class SpatialDecoder(val model: TypingModel) {
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
            val dx = (x - p.x) / (grid.pitchX * .29)
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
    private fun search(taps: List<Tap>, from: Int, end: Int, edits: Int, swears: Boolean, previous: Int): List<Found> {
        val depthLimit = end - from + edits + 2
        val context = model.neverSuggest.restrictedContext(previous)
        val layers = Array(depthLimit + 1) { ArrayList<State>() }
        layers[0].add(State(0, model.words, from, 0, 0.0))
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
                if (s.tap == end && model.length(s.lo) == d && !model.neverSuggest.blocks(s.lo, swears, context) &&
                    s.cost < (found[s.lo] ?: Double.POSITIVE_INFINITY))
                    found[s.lo] = s.cost
                if (d == depthLimit) continue
                fun push(c: Int, next: Int, e: Int, cost: Double) {
                    val child = model.child(s.lo, s.hi, d, c)
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
                    val a = first.choices[ai]; val r = model.child(s.lo, s.hi, d, a)
                    if (r == 0L) continue
                    for (bi in 0 until minOf(3, second.choices.size)) {
                        val b = second.choices[bi]; if (b == a) continue
                        val child = model.child((r ushr 32).toInt(), r.toInt(), d + 1, b)
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
        val knownLiteral = literal >= 0 || model.validLiteral(typed)
        var cost = lmWeight * when {
            literal >= 0 -> model.languageCost(previous, literal)
            knownLiteral -> 15.0
            else -> 22.0
        }
        for (i in taps.indices) cost += taps[i].cost[model.code(typed[i])]
        if (!cost.isFinite()) return none
        val readings = ArrayList<Reading>()
        // iOS applies the language score only to the five best spatial readings.
        for ((w, spatial) in search(taps, 0, taps.size, 1, swears, previous).sortedWith(spatialOrder).take(5)) if (w != literal)
            readings.add(Reading(model.text(w), spatial + lmWeight * model.languageCost(previous, w), model.frequencyCost(w)))
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
        val best = readings.firstOrNull() ?: return none
        val margin = if (!knownLiteral || foldAccents(best.text) == foldAccents(typed)) 2.0 else 7.0
        return Decision(if (cost - best.cost >= margin) best.text else null, readings.asSequence().filter { it.cost < cost }.distinctBy { it.text }.take(3).toList())
    }
    private companion object {
        val spatialOrder = compareBy<Found> { it.cost }.thenBy { it.word }
        const val EDIT_COST = 7.0
        const val MAX_TAP_COST = 9.0
        // An accented letter typed as its base letter's key (é from e).
        const val ACCENT_COST = 1.0
        const val SPACE_TAP_WEIGHT = .25
        // A space tap reads as a letter only within about the bar's top fifth.
        const val MERGE_REACH = 3.0
    }
    private fun foldAccents(s: String) = Normalizer.normalize(s, Normalizer.Form.NFD).filter { it !in '\u0300'..'\u0308' || it == '\u0303' }
}
