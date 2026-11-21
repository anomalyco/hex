package ly.anoma.hex.typing

import java.text.Normalizer

/** Positions are all in the same layout coordinates. The IME supplies measured key centres;
 * the replay supplies the original test layout, not a guessed phone screen size. */
internal data class KeyPoint(val x: Double, val y: Double)
internal class KeyGrid(val centres: Map<Char, KeyPoint>, val pitchX: Double, val pitchY: Double) {
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
    class Tap(val cost: DoubleArray, val choices: IntArray, val belowLetters: Boolean, val missedSpaceCost: Double)
    data class Reading(val text: String, val cost: Double, val frequency: Int)
    data class Decision(val replace: String?, val readings: List<Reading>)
    // Google Books English has richer word pairs; these are the shared iOS R1 weights.
    private val lmWeight = if (model.language == "en") 1.5 else 1.25
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
        for (i in 1 until costs.size) if (!costs[i].isFinite() && model.bases[i] != i) costs[i] = costs[model.bases[i]] + 1
        for (i in 1 until costs.size) costs[i] -= min
        val choices = (1 until costs.size).filter { costs[it] <= 9.0 }.sortedBy { costs[it] }.toIntArray()
        return Tap(costs, choices, y > grid.bottomY, grid.missedSpaceCost)
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
                for (c in 1..model.alphabet) if (c != model.apostrophe) push(c, t, s.edits + 1, s.cost + 7)
                if (t < end) layer.add(State(s.lo, s.hi, t + 1, s.edits + 1, s.cost + 7))
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
            for (k in 1 until taps.size) {
                val left = half(0, k, previous)
                if (left.isEmpty()) continue
                val stops = if (k + 1 < taps.size && taps[k].belowLetters) k..k+1 else k..k
                for (start in stops) {
                    val right = half(start, taps.size, -1)
                    for (l in left) for (r in right) {
                        if (model.neverSuggest.blocksFollowing(r.word, swears, l.word)) continue
                        val lm = model.languageCost(previous, l.word) + model.languageCost(l.word, r.word)
                        readings.add(Reading(model.text(l.word) + " " + model.text(r.word), l.cost + r.cost + taps[k].missedSpaceCost + lmWeight * lm,
                            model.frequencyCost(l.word) + model.frequencyCost(r.word)))
                    }
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
    }
    private fun foldAccents(s: String) = Normalizer.normalize(s, Normalizer.Form.NFD).filter { it !in '\u0300'..'\u0308' || it == '\u0303' }
}
