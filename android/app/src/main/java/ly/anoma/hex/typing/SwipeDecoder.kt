package ly.anoma.hex.typing

import java.text.Normalizer
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sqrt

/** Reads a finger's path across the letter keys as a word (Keyboard › Swipe typing). A port of iPhone
 * Hex's `SwipeDecoder`, a shape-matching decoder after SHARK2 (Kristensson and Zhai, 2004), with the same
 * parameters, so the two platforms read a path alike:
 * 1. The path is resampled to evenly spaced points in key pitches (the grid's [KeyGrid.pitchX] across and
 *    [KeyGrid.pitchY] down), so a slip of half a key counts the same either way.
 * 2. Words come from the typing model's sorted word list: the words starting on a key near where the finger
 *    went down are one range of it per letter, read in place from the mapped file. Of those, only words
 *    ending on a key near where it lifted, with every key near the path and an ideal path (the polyline
 *    through their keys' centres) about as long, are scored.
 * 3. Each is scored on where its ideal path runs against the gesture (a quick point-by-point pass, then
 *    dynamic time warping for the best 80), on its shape alone, and on how near the finger went down and
 *    lifted to its first and last keys;
 * 4. the best 80 also on how their keys account for where the finger turned and slowed (from the touches'
 *    times): a straight stroke runs over the letters between its ends, so the path alone can't tell "tired"
 *    from "tied"; a pause on r can;
 * 5. and weighed with the model's frequency of the word after the word before.
 * A letter without a key reads through the keys a person swipes for it: ß through s, French œ and æ
 * through o and e, a and e (iPhone drops those words). Nothing is built ahead or kept between swipes, and a
 * read touches only the mapped model and its own arrays, so it may run off the main thread. */
internal class SwipeDecoder(val model: TypingModel, val parameters: Parameters = Parameters()) {
    /** iPhone's, tuned on synthetic swipes of the autocorrect benchmark's held-out dev sentences (English
     * and Spanish; experiment 2026-11-20-hex-swipe-typing), never its test sentences. */
    data class Parameters(
        /** Points the gesture and each word's ideal path are resampled to. */
        val samples: Int = 32,
        /** Keys this many pitches from where the finger went down (lifted) may start (end) the word. */
        val startRadius: Double = 1.6,
        val endRadius: Double = 2.1,
        val maxStartKeys: Int = 4,
        val maxEndKeys: Int = 8,
        /** Every key of a word passes at most this far from the path (pitches). */
        val nearRadius: Double = 1.5,
        /** How much shorter or longer than the gesture a word's ideal path may be. */
        val minLengthRatio: Double = 0.4,
        val maxLengthRatio: Double = 1.8,
        /** How far from its first and last keys a finger goes down and lifts: most within the first
         * spread, the rest ([stray]) within the wider second. */
        val startSigma: Double = 0.3,
        val endSigma: Double = 0.3,
        val startStraySigma: Double = 0.8,
        val endStraySigma: Double = 0.9,
        val stray: Double = 0.25,
        /** Nats per mean squared pitch between the paths, and between their shapes (each in a unit box). */
        val locationWeight: Double = 30.0,
        val shapeWeight: Double = 60.0,
        /** Weight of the word model's cost, -ln P(word | word before). */
        val lmWeight: Double = 1.0,
        /** Words of the quick pass the aligned pass scores again. */
        val shortlist: Int = 80,
        /** Past this many words, only the likeliest by how near their keys lie to the path get the quick pass. */
        val prefilter: Int = 1500,
        /** How far (a share of the samples) the aligned pass may run ahead or behind. */
        val band: Double = 0.1,
        /** The shortest path read as a swipe (pitches). */
        val minimumLength: Double = 0.6,
        /** A contraction whose first word never follows the word before in the model's pairs costs this much more. */
        val contractionMissingPair: Double = 3.0,
        /** Turns and pauses: the path is read every [evidenceStep] pitches. */
        val evidenceStep: Double = 0.1,
        val turnWindow: Double = 0.2,
        val turnFrom: Double = 35.0,
        val turnFull: Double = 120.0,
        val dwellWindow: Double = 0.1,
        val dwellFrom: Double = 1.3,
        val dwellFull: Double = 4.0,
        val evidenceFloor: Double = 0.1,
        val evidenceSpacing: Double = 0.8,
        val evidenceGuard: Double = 0.7,
        val evidenceSigma: Double = 0.35,
        val evidenceReach: Double = 1.4,
        /** Nats for a full turn or pause no key of the word accounts for, a key the word goes straight
         * through without a pause (only with times), and a key it turns at without a turn or pause. */
        val unexplainedCost: Double = 6.0,
        val straightKeyCost: Double = 2.5,
        val cornerKeyCost: Double = 2.0,
        val straightAngle: Double = 20.0,
    ) {
        /** -ln of a point landing [squared] pitches² from a key's centre under a tight and a stray spread; 0 at the centre. */
        fun cost(squared: Double, sigma: Double, straying: Double): Double {
            val near = (1 - stray) / (sigma * sigma)
            val far = stray / (straying * straying)
            val density = near * exp(-squared / (2 * sigma * sigma)) + far * exp(-squared / (2 * straying * straying))
            return ln((near + far) / max(density, java.lang.Double.MIN_NORMAL))
        }
    }

    /** A reading: the word as the model spells it (lowercase, with its accents), or an extra word as given. */
    data class Candidate(val text: String, val cost: Double)
    data class Reading(val candidates: List<Candidate>, val scored: Int)

    /** Words read beside the model's, each with its own cost (-ln P): English contractions, which Google
     * Books barely counts ("do n't"), and your own words. */
    class ExtraWords(model: TypingModel, entries: List<Word>) {
        /** A word as you'd see it ("I'm"), its cost, and whether it is a contraction its first word's pairs weigh. */
        data class Word(val text: String, val cost: Double, val contraction: Boolean = false)
        val codes: List<IntArray>
        val texts: List<String>
        val costs: DoubleArray
        /** The model's index of the same word, for its pairs; -1 when it lacks it. */
        val modelIndex: IntArray
        /** A contraction's first word in the model ("i" for "I'm"), whose pairs weigh it after the word before; -1 otherwise. */
        val heads: IntArray
        init {
            val seen = HashSet<String>()
            val c = ArrayList<IntArray>(); val t = ArrayList<String>(); val k = ArrayList<Double>()
            val m = ArrayList<Int>(); val h = ArrayList<Int>()
            for ((word, cost, contraction) in entries) {
                val lower = word.lowercase()
                if (lower.length < 2 || !seen.add(lower)) continue
                val encoded = IntArray(lower.length) { model.code(lower[it]) }
                if (encoded.any { it == 0 }) continue
                c.add(encoded); t.add(word); k.add(cost); m.add(model.index(lower))
                h.add(if (contraction) head(lower)?.let(model::index) ?: -1 else -1)
            }
            codes = c; texts = t; costs = k.toDoubleArray(); modelIndex = m.toIntArray(); heads = h.toIntArray()
        }
        val size get() = texts.size
        companion object {
            /** A contraction's first word: "do" for "don't", "i" for "i'm". */
            fun head(contraction: String): String? {
                if (contraction.endsWith("n't")) return contraction.dropLast(3)
                val apostrophe = contraction.indexOf('\'')
                return if (apostrophe > 0) contraction.substring(0, apostrophe) else null
            }
        }
    }

    /** Swear words may be read (Keyboard › Suggest swear words); slurs never are. */
    @Volatile var swears = false
    @Volatile var extra: ExtraWords? = null
        set(value) {
            field = value
            extraIndices = value?.modelIndex?.filter { it >= 0 }?.toHashSet() ?: emptySet()
        }
    /** The model's words that [extra] also has, read only as extra words. */
    @Volatile private var extraIndices: Set<Int> = emptySet()

    /** The likeliest words for a swipe along the points ([xs], [ys], the first [count], in [grid]'s
     * coordinates), best first; empty when it can't be read as one. [times] (seconds, each point's) show
     * where the finger paused; without them only its turns count. [previous] is the word before, lowercase. */
    fun read(xs: DoubleArray, ys: DoubleArray, times: DoubleArray?, count: Int, grid: KeyGrid, previous: String?, limit: Int = 4): Reading {
        val p = parameters
        val none = Reading(emptyList(), 0)
        if (grid.pitchX <= 0 || grid.pitchY <= 0 || count < 2 || limit <= 0 || p.samples < 4) return none
        val keys = Keys(grid)
        if (keys.size < 2 || keys.size > 64) return none
        val gesture = Gesture.of(xs, ys, times, count, grid.pitchX, grid.pitchY, p) ?: return none
        if (gesture.length < p.minimumLength) return none
        val startX = gesture.rawX[0]; val startY = gesture.rawY[0]
        val endX = gesture.rawX[gesture.rawCount - 1]; val endY = gesture.rawY[gesture.rawCount - 1]

        // 1. The keys the word may start and end on, and how near the path passes each key.
        val keyCount = keys.size
        val near = DoubleArray(keyCount) { distanceToPolyline(keys.x[it], keys.y[it], gesture.rawX, gesture.rawY, gesture.rawCount) }
        val startCost = DoubleArray(keyCount) { Double.POSITIVE_INFINITY }
        val endCost = DoubleArray(keyCount) { Double.POSITIVE_INFINITY }
        val starts = nearest(keys, startX, startY, p.startRadius, p.maxStartKeys)
        for ((k, squared) in starts) startCost[k] = p.cost(squared, p.startSigma, p.startStraySigma)
        var endMask = 0L
        for ((k, squared) in nearest(keys, endX, endY, p.endRadius, p.maxEndKeys)) {
            endCost[k] = p.cost(squared, p.endSigma, p.endStraySigma)
            endMask = endMask or (1L shl k)
        }
        if (starts.isEmpty() || endMask == 0L) return none

        // 2. Words that start on a start key, end on an end key, keep near the path and are about as long:
        // their keys (repeats run together, apostrophes left out). An id is a model word's index, or
        // -1 - n for extra word n.
        val words = IntList(); val offsets = IntList().also { it.add(0) }; val sequence = IntList()
        val lengths = DoubleList(); val nearness = DoubleList()
        val scratch = IntArray(64); val codes = IntArray(64)
        val gestureLength = gesture.length
        fun consider(word: IntArray, size: Int, id: Int) {
            // The last key first: most words end elsewhere.
            var j = size - 1
            while (j > 0 && keys.skips(word[j])) j--
            if (j <= 0) return
            val endKey = keys.last(word[j])
            if (endKey < 0 || endMask and (1L shl endKey) == 0L) return
            var n = 0; var last = -1; var length = 0.0; var squares = 0.0
            for (i in 0 until size) {
                val code = word[i]
                if (keys.skips(code)) continue
                val first = keys.first(code)
                if (first < 0) return
                for (pass in 0..1) {
                    val k = if (pass == 0) first else keys.second(code)
                    if (k < 0 || k == last) continue
                    if (n >= scratch.size || near[k] > p.nearRadius) return
                    if (last >= 0) length += hypot(keys.x[last] - keys.x[k], keys.y[last] - keys.y[k])
                    squares += near[k] * near[k]
                    scratch[n++] = k; last = k
                }
            }
            if (n < 2 || !startCost[scratch[0]].isFinite()) return
            val ratio = length / gestureLength
            if (ratio < p.minLengthRatio || ratio > p.maxLengthRatio) return
            for (i in 0 until n) sequence.add(scratch[i])
            words.add(id); lengths.add(length); offsets.add(sequence.size); nearness.add(squares / n)
        }
        val swears = swears
        val extras = extra
        val skipped = extraIndices
        for ((s, _) in starts) for (code in keys.codesOfKey[s]) {
            val lo = model.bound(0, model.words, 0, code)
            val hi = model.bound(lo, model.words, 0, code + 1)
            for (i in lo until hi) {
                if (model.neverSuggest.blocks(i, swears) || i in skipped) continue
                consider(codes, model.codes(i, codes), i)
            }
        }
        if (extras != null) for (n in 0 until extras.size) { val c = extras.codes[n]; if (c.size <= 64) consider(c, c.size, -1 - n) }
        if (words.size == 0) return none

        // 3. A quick point-by-point pass over them all, then the aligned pass for the best.
        val samples = p.samples
        val templateX = DoubleArray(samples); val templateY = DoubleArray(samples)
        val vx = DoubleArray(64); val vy = DoubleArray(64)
        val rowA = DoubleArray(samples); val rowB = DoubleArray(samples)
        val evidenceA = DoubleArray(gesture.evidenceCount + 1); val evidenceB = DoubleArray(gesture.evidenceCount + 1)
        val band = max(1, (p.band * samples).toInt())
        fun spatial(c: Int, aligned: Boolean): Double {
            val from = offsets[c]; val to = offsets[c + 1]; val m = to - from
            for (v in from until to) { val k = sequence[v]; vx[v - from] = keys.x[k]; vy[v - from] = keys.y[k] }
            resample(vx, vy, m, lengths[c], templateX, templateY)
            val location = if (aligned) warpedDistance(gesture.pointsX, gesture.pointsY, templateX, templateY, band, rowA, rowB)
                else meanSquaredDistance(gesture.pointsX, gesture.pointsY, templateX, templateY)
            val shape = shapeDistance(gesture.shapeX, gesture.shapeY, templateX, templateY)
            val evidence = if (aligned) evidenceCost(vx, vy, m, gesture, p, evidenceA, evidenceB) else 0.0
            return startCost[sequence[from]] + endCost[sequence[to - 1]] + p.locationWeight * location + p.shapeWeight * shape + evidence
        }
        var pool = IntArray(words.size) { it }
        if (pool.size > p.prefilter) {
            val rough = DoubleArray(pool.size) { c ->
                startCost[sequence[offsets[c]]] + endCost[sequence[offsets[c + 1] - 1]] + p.locationWeight * nearness[c] +
                    p.lmWeight * lmCost(words[c], -1, extras)
            }
            pool = pool.sortedWith(compareBy { rough[it] }).take(p.prefilter).toIntArray()
        }
        val quick = DoubleArray(pool.size) { spatial(pool[it], aligned = false) + p.lmWeight * lmCost(words[pool[it]], -1, extras) }
        val order = pool.indices.sortedWith(compareBy { quick[it] })

        val previousWord = previous?.let { model.index(it.lowercase()) } ?: -1
        val scored = ArrayList<Pair<Int, Double>>()
        for (q in order.take(max(limit, p.shortlist))) {
            val c = pool[q]
            val word = words[c]
            if (word >= 0 && model.neverSuggest.blocksFollowing(word, swears, previousWord)) continue
            scored.add(word to spatial(c, aligned = true) + p.lmWeight * lmCost(word, previousWord, extras))
        }
        scored.sortBy { it.second }
        val candidates = scored.take(limit).map { (word, cost) -> Candidate(if (word >= 0) model.text(word) else extras!!.texts[-1 - word], cost) }
        return Reading(candidates, words.size)
    }

    /** -ln P(word | previous) with the unigram backoff, as the tap decoder's; an extra word's own cost when lower. */
    private fun lmCost(id: Int, previous: Int, extras: ExtraWords?): Double {
        if (id >= 0) return model.languageCost(previous, id)
        val n = -1 - id
        var own = 0.92 + extras!!.costs[n]
        // A contraction is as much likelier after the word before as its first word is.
        val head = extras.heads[n]
        if (head >= 0 && previous >= 0) {
            val alone = 0.92 + model.unigramCost(head)
            val pair = model.pairCost(previous, head)
            own += (if (pair >= 0) pair else alone + parameters.contractionMissingPair) - alone
        }
        val index = extras.modelIndex[n]
        return if (index >= 0) min(own, model.languageCost(previous, index)) else own
    }

    /** The grid's letter keys in pitches, and which keys each of the model's letters types through: its own,
     * else its unaccented letter's (é on e), else the keys a person swipes for it (ß on s, œ on o and e); the
     * apostrophe has none and is skipped. */
    private inner class Keys(grid: KeyGrid) {
        val x: DoubleArray; val y: DoubleArray
        private val firstKey = IntArray(model.alphabet + 1) { MISSING }
        private val secondKey = IntArray(model.alphabet + 1) { -1 }
        val codesOfKey: Array<IntArray>
        init {
            val letters = grid.centres.keys.filter { it != '\'' }.sorted()
            x = DoubleArray(letters.size) { grid.centres.getValue(letters[it]).x / grid.pitchX }
            y = DoubleArray(letters.size) { grid.centres.getValue(letters[it]).y / grid.pitchY }
            val index = HashMap<Char, Int>().apply { letters.forEachIndexed { i, c -> put(c, i) } }
            val byKey = Array(letters.size) { ArrayList<Int>() }
            for (code in 1..model.alphabet) {
                if (code == model.apostrophe) { firstKey[code] = SKIP; continue }
                val c = model.character(code)
                val own = index[c] ?: index[model.character(model.bases[code])]
                val keysOf = if (own != null) listOf(own) else spelled(c).map { index[it] ?: -1 }
                if (keysOf.isEmpty() || keysOf.any { it < 0 }) continue
                firstKey[code] = keysOf[0]
                if (keysOf.size > 1) secondKey[code] = keysOf[1]
                byKey[keysOf[0]].add(code)
            }
            codesOfKey = Array(letters.size) { byKey[it].toIntArray() }
        }
        val size get() = x.size
        fun skips(code: Int) = firstKey[code] == SKIP
        fun first(code: Int) = firstKey[code]
        fun second(code: Int) = secondKey[code]
        fun last(code: Int) = if (secondKey[code] >= 0) secondKey[code] else firstKey[code]
    }

    /** The finger's path in pitches without repeated points, resampled, its shape, and where it turned or paused. */
    private class Gesture(val rawX: DoubleArray, val rawY: DoubleArray, val rawCount: Int, val length: Double,
                          val pointsX: DoubleArray, val pointsY: DoubleArray, val shapeX: DoubleArray, val shapeY: DoubleArray,
                          val evidenceX: DoubleArray, val evidenceY: DoubleArray, val strength: DoubleArray, val timed: Boolean) {
        val evidenceCount get() = strength.size
        companion object {
            fun of(xs: DoubleArray, ys: DoubleArray, times: DoubleArray?, count: Int, pitchX: Double, pitchY: Double, p: Parameters): Gesture? {
                val t = times?.takeIf { it.size >= count }
                val rawX = DoubleArray(count); val rawY = DoubleArray(count); val stamps = DoubleArray(count)
                var n = 0; var length = 0.0
                for (i in 0 until count) {
                    val vx = xs[i] / pitchX; val vy = ys[i] / pitchY
                    if (n > 0) {
                        val step = hypot(vx - rawX[n - 1], vy - rawY[n - 1])
                        if (step <= 1e-4) continue
                        length += step
                    }
                    rawX[n] = vx; rawY[n] = vy
                    // A still finger sends no points: the time it stayed goes to the next one's step.
                    if (t != null) stamps[n] = if (n == 0) t[i] else max(t[i], stamps[n - 1])
                    n++
                }
                if (n < 2 || length <= 0) return null
                val px = DoubleArray(p.samples); val py = DoubleArray(p.samples)
                resample(rawX, rawY, n, length, px, py)
                val (sx, sy) = normalized(px, py)
                val timed = t != null && stamps[n - 1] - stamps[0] > 0.02
                val evidence = evidence(rawX, rawY, n, if (timed) stamps else null, length, p)
                return Gesture(rawX, rawY, n, length, px, py, sx, sy, evidence[0], evidence[1], evidence[2], timed)
            }
        }
    }

    companion object {
        /** A common English contraction's cost (-ln P), about a word ranked 1,500th: Google Books splits them. */
        const val CONTRACTION_COST = 7.5
        /** One of your own words, as the tap decoder scores it. */
        const val OWN_WORD_COST = 11.0
        private val commonestContractions = setOf("i'm", "don't", "it's", "i'll", "can't", "that's", "didn't", "you're", "i've", "doesn't")
        private val commonContractions = setOf("isn't", "there's", "let's", "won't", "what's", "he's", "she's", "wasn't", "i'd", "we're",
            "they're", "haven't", "couldn't", "wouldn't", "aren't", "you'll", "we'll", "shouldn't", "you've")
        /** An English contraction's cost by how often a chat uses it: the commonest like a word ranked about 300th,
         * common ones at [CONTRACTION_COST], the rest ("he'll", "ma'am") about 10,000th. */
        fun costOfContraction(word: String): Double = when (word.lowercase()) {
            in commonestContractions -> CONTRACTION_COST - 1.5
            in commonContractions -> CONTRACTION_COST
            else -> CONTRACTION_COST + 2
        }

        private const val SKIP = -1
        private const val MISSING = -2

        /** The letters a person swipes through for one without a key (iPhone has none for them). */
        private fun spelled(c: Char): List<Char> = when (c) {
            'ß' -> listOf('s')
            'œ' -> listOf('o', 'e')
            'æ' -> listOf('a', 'e')
            else -> Normalizer.normalize(c.toString(), Normalizer.Form.NFD).firstOrNull()?.takeIf { it != c }?.let(::listOf) ?: emptyList()
        }

        private fun hypot(dx: Double, dy: Double) = sqrt(dx * dx + dy * dy)

        /** The keys within [radius] of the point, nearest first, at most [limit], with their squared
         * distances; the nearest two however far. */
        private fun nearest(keys: Keys, px: Double, py: Double, radius: Double, limit: Int): List<Pair<Int, Double>> {
            val sorted = (0 until keys.size).map { it to (keys.x[it] - px).let { d -> d * d } + (keys.y[it] - py).let { d -> d * d } }
                .sortedBy { it.second }
            val found = sorted.take(limit).filter { it.second <= radius * radius }
            return if (found.size < 2) sorted.take(min(2, limit)) else found
        }

        fun distanceToPolyline(px: Double, py: Double, lx: DoubleArray, ly: DoubleArray, count: Int): Double {
            var best = Double.POSITIVE_INFINITY
            for (i in 1 until count) {
                val ax = lx[i - 1]; val ay = ly[i - 1]
                val dx = lx[i] - ax; val dy = ly[i] - ay
                val span = dx * dx + dy * dy
                var t = if (span > 0) ((px - ax) * dx + (py - ay) * dy) / span else 0.0
                t = t.coerceIn(0.0, 1.0)
                val ex = px - (ax + t * dx); val ey = py - (ay + t * dy)
                best = min(best, ex * ex + ey * ey)
            }
            return sqrt(best)
        }

        /** The first [count] points of a line [length] long, evenly spaced along it into the outputs, first and last included. */
        fun resample(lx: DoubleArray, ly: DoubleArray, count: Int, length: Double, outX: DoubleArray, outY: DoubleArray) {
            val samples = outX.size
            outX[0] = lx[0]; outY[0] = ly[0]
            outX[samples - 1] = lx[count - 1]; outY[samples - 1] = ly[count - 1]
            if (samples <= 2) return
            val step = length / (samples - 1)
            var segment = 0
            var walked = 0.0
            var span = hypot(lx[min(1, count - 1)] - lx[0], ly[min(1, count - 1)] - ly[0])
            for (i in 1 until samples - 1) {
                val target = step * i
                while (walked + span < target && segment < count - 2) {
                    walked += span
                    segment++
                    span = hypot(lx[segment + 1] - lx[segment], ly[segment + 1] - ly[segment])
                }
                val b = min(segment + 1, count - 1)
                val t = if (span > 0) ((target - walked) / span).coerceIn(0.0, 1.0) else 0.0
                outX[i] = lx[segment] + t * (lx[b] - lx[segment]); outY[i] = ly[segment] + t * (ly[b] - ly[segment])
            }
        }

        /** Centroid, and the scale that makes the bounding box's longer side 1. */
        private fun frame(xs: DoubleArray, ys: DoubleArray): DoubleArray {
            var sx = 0.0; var sy = 0.0
            var minX = Double.POSITIVE_INFINITY; var maxX = Double.NEGATIVE_INFINITY
            var minY = Double.POSITIVE_INFINITY; var maxY = Double.NEGATIVE_INFINITY
            for (i in xs.indices) {
                sx += xs[i]; sy += ys[i]
                minX = min(minX, xs[i]); maxX = max(maxX, xs[i]); minY = min(minY, ys[i]); maxY = max(maxY, ys[i])
            }
            val side = max(maxX - minX, maxY - minY)
            return doubleArrayOf(sx / xs.size, sy / xs.size, if (side > 1e-6) 1 / side else 1.0)
        }

        private fun normalized(xs: DoubleArray, ys: DoubleArray): Pair<DoubleArray, DoubleArray> {
            val (cx, cy, scale) = frame(xs, ys).let { Triple(it[0], it[1], it[2]) }
            return DoubleArray(xs.size) { (xs[it] - cx) * scale } to DoubleArray(ys.size) { (ys[it] - cy) * scale }
        }

        /** Mean squared distance between the gesture's normalized shape and the template's. */
        private fun shapeDistance(sx: DoubleArray, sy: DoubleArray, tx: DoubleArray, ty: DoubleArray): Double {
            val f = frame(tx, ty)
            var total = 0.0
            for (i in sx.indices) {
                val dx = sx[i] - (tx[i] - f[0]) * f[2]; val dy = sy[i] - (ty[i] - f[1]) * f[2]
                total += dx * dx + dy * dy
            }
            return total / sx.size
        }

        private fun meanSquaredDistance(ax: DoubleArray, ay: DoubleArray, bx: DoubleArray, by: DoubleArray): Double {
            var total = 0.0
            for (i in ax.indices) { val dx = ax[i] - bx[i]; val dy = ay[i] - by[i]; total += dx * dx + dy * dy }
            return total / ax.size
        }

        /** Dynamic time warping of two equally long point lists within a band, symmetric steps (a diagonal step
         * counts twice), as a mean squared distance. */
        private fun warpedDistance(ax: DoubleArray, ay: DoubleArray, bx: DoubleArray, by: DoubleArray, band: Int,
                                   rowA: DoubleArray, rowB: DoubleArray): Double {
            val n = ax.size
            var prev = rowA; var cur = rowB
            prev.fill(Double.POSITIVE_INFINITY)
            for (i in 0 until n) {
                cur.fill(Double.POSITIVE_INFINITY)
                val lo = max(0, i - band); val hi = min(n - 1, i + band)
                for (j in lo..hi) {
                    val dx = ax[i] - bx[j]; val dy = ay[i] - by[j]
                    val c = dx * dx + dy * dy
                    var best = if (i == 0 && j == 0) 2 * c else Double.POSITIVE_INFINITY
                    if (i > 0 && j > 0) best = min(best, prev[j - 1] + 2 * c)
                    if (i > 0) best = min(best, prev[j] + c)
                    if (j > 0) best = min(best, cur[j - 1] + c)
                    cur[j] = best
                }
                val swap = prev; prev = cur; cur = swap
            }
            return prev[n - 1] / (2 * n)
        }

        /** The path's turns and pauses in order along it, as [x, y, strength] arrays: the path is read every
         * [Parameters.evidenceStep] pitches; a point scores by how sharply the path turns there and how long the
         * finger took over it against the rest of the swipe; the strongest at least [Parameters.evidenceSpacing]
         * apart, away from the ends, count. */
        private fun evidence(rawX: DoubleArray, rawY: DoubleArray, rawCount: Int, times: DoubleArray?, length: Double, p: Parameters): Array<DoubleArray> {
            val none = arrayOf(DoubleArray(0), DoubleArray(0), DoubleArray(0))
            val count = (length / p.evidenceStep).toInt() + 1
            if (count < 5) return none
            val px = DoubleArray(count); val py = DoubleArray(count)
            resample(rawX, rawY, rawCount, length, px, py)
            val step = length / (count - 1)
            val score = DoubleArray(count)
            val k = max(1, (p.turnWindow / step).roundToInt())
            if (count > 2 * k) for (i in k until count - k) {
                val ax = px[i] - px[i - k]; val ay = py[i] - py[i - k]
                val bx = px[i + k] - px[i]; val by = py[i + k] - py[i]
                val angle = atan2(abs(ax * by - ay * bx), ax * bx + ay * by) * 180 / Math.PI
                score[i] = ((angle - p.turnFrom) / (p.turnFull - p.turnFrom)).coerceIn(0.0, 1.0)
            }
            val guard = ceil(p.evidenceGuard / step).toInt()
            if (count <= 2 * guard + 1) return none
            if (times != null) {
                // The time at each point read, from the raw points' times along the path.
                val at = DoubleArray(count)
                var segment = 0
                var walked = 0.0
                var span = hypot(rawX[1] - rawX[0], rawY[1] - rawY[0])
                for (i in 0 until count) {
                    val target = step * i
                    while (walked + span < target && segment < rawCount - 2) {
                        walked += span
                        segment++
                        span = hypot(rawX[segment + 1] - rawX[segment], rawY[segment + 1] - rawY[segment])
                    }
                    val t = if (span > 0) ((target - walked) / span).coerceIn(0.0, 1.0) else 0.0
                    at[i] = times[segment] + t * (times[min(segment + 1, rawCount - 1)] - times[segment])
                }
                val w = max(1, (p.dwellWindow / step).roundToInt())
                val perPitch = DoubleArray(count) {
                    val lo = max(0, it - w); val hi = min(count - 1, it + w)
                    (at[hi] - at[lo]) / (step * (hi - lo))
                }
                val middle = perPitch.copyOfRange(guard, count - guard).also { it.sort() }
                val median = middle[middle.size / 2]
                if (median > 0) {
                    val from = ln(p.dwellFrom); val full = ln(p.dwellFull)
                    for (i in guard until count - guard) if (perPitch[i] > 0) {
                        val dwell = ((ln(perPitch[i] / median) - from) / (full - from)).coerceIn(0.0, 1.0)
                        score[i] = max(score[i], dwell)
                    }
                }
            }
            // The strongest first, each keeping its neighbours within the spacing out.
            val spacing = max(1, (p.evidenceSpacing / step).roundToInt())
            val order = (guard until count - guard).filter { score[it] >= p.evidenceFloor }
                .sortedWith(compareByDescending<Int> { score[it] }.thenBy { it })
            val taken = ArrayList<Int>()
            for (i in order) if (taken.all { abs(it - i) >= spacing }) taken.add(i)
            taken.sort()
            return arrayOf(DoubleArray(taken.size) { px[taken[it]] }, DoubleArray(taken.size) { py[taken[it]] },
                DoubleArray(taken.size) { score[taken[it]] })
        }

        /** Nats for how well a word's keys (the first [m] of [kx], [ky], in pitches) account for the path's
         * turns and pauses, in order: an edit distance where a key accounts for the ones near it, a turn or
         * pause no key accounts for costs by its strength, and a key with none costs [Parameters.cornerKeyCost]
         * where the word turns, [Parameters.straightKeyCost] where it goes straight on (when the path has
         * times). The first and last keys cost nothing without one. */
        private fun evidenceCost(kx: DoubleArray, ky: DoubleArray, m: Int, g: Gesture, p: Parameters,
                                 rowA: DoubleArray, rowB: DoubleArray): Double {
            val n = g.evidenceCount
            if (m < 2) return 0.0
            val reach = p.evidenceReach * p.evidenceReach
            val spread = 2 * p.evidenceSigma * p.evidenceSigma
            val straight = cos(p.straightAngle * Math.PI / 180)
            fun missing(i: Int): Double {
                if (i <= 0 || i >= m - 1) return 0.0
                val ax = kx[i] - kx[i - 1]; val ay = ky[i] - ky[i - 1]
                val bx = kx[i + 1] - kx[i]; val by = ky[i + 1] - ky[i]
                val la = hypot(ax, ay); val lb = hypot(bx, by)
                if (la <= 1e-6 || lb <= 1e-6) return 0.0
                val goesStraight = (ax * bx + ay * by) / (la * lb) >= straight
                return if (goesStraight) (if (g.timed) p.straightKeyCost else 0.0) else p.cornerKeyCost
            }
            fun accounts(i: Int, j: Int): Double {
                val dx = kx[i] - g.evidenceX[j]; val dy = ky[i] - g.evidenceY[j]
                val d = dx * dx + dy * dy
                return if (d <= reach) d / spread else Double.POSITIVE_INFINITY
            }
            fun unexplained(j: Int) = p.unexplainedCost * g.strength[j]
            // Row i: keys 0..i placed; column j: the first j turns and pauses read.
            var prev = rowA; var cur = rowB
            prev[0] = 0.0
            for (j in 0 until n) prev[j + 1] = prev[j] + min(unexplained(j), accounts(0, j))
            for (i in 1 until m) {
                val skip = missing(i)
                cur[0] = prev[0] + skip
                for (j in 0 until n) {
                    val near = accounts(i, j)
                    var best = prev[j + 1] + skip
                    best = min(best, prev[j] + near)
                    best = min(best, cur[j] + min(unexplained(j), near))
                    cur[j + 1] = best
                }
                val swap = prev; prev = cur; cur = swap
            }
            return prev[n]
        }
    }

    private class IntList {
        private var data = IntArray(256)
        var size = 0; private set
        fun add(v: Int) { if (size == data.size) data = data.copyOf(size * 2); data[size++] = v }
        operator fun get(i: Int) = data[i]
    }
    private class DoubleList {
        private var data = DoubleArray(256)
        var size = 0; private set
        fun add(v: Double) { if (size == data.size) data = data.copyOf(size * 2); data[size++] = v }
        operator fun get(i: Int) = data[i]
    }
}

/** What a swipe reads as for one language, as the keyboard offers it: [SwipeDecoder]'s readings of the
 * model, English's contractions and your own words, ranked with a rare name costing more. [prepare] on the
 * main thread before a swipe; [decode] and [ranked] may then run on any thread. */
internal class SwipeReader(private val spatial: SpatialDecoder) {
    val model get() = spatial.model
    private val decoder = SwipeDecoder(spatial.model)
    private var builtFrom: PersonalWords? = null
    private var built = false

    /** Swear words as Suggest swear words says, and the extra words again when your own words changed. */
    fun prepare(swears: Boolean) {
        decoder.swears = swears
        val personal = spatial.personal
        if (built && personal === builtFrom) return
        built = true; builtFrom = personal
        val words = (if (model.language == "en") contractions.map { SwipeDecoder.ExtraWords.Word(it, SwipeDecoder.costOfContraction(it), true) }
            else emptyList()) + personal?.words.orEmpty().map { SwipeDecoder.ExtraWords.Word(it, SwipeDecoder.OWN_WORD_COST) }
        decoder.extra = if (words.isEmpty()) null else SwipeDecoder.ExtraWords(model, words)
    }

    fun decode(xs: DoubleArray, ys: DoubleArray, times: DoubleArray?, count: Int, grid: KeyGrid, previous: String?, limit: Int) =
        decoder.read(xs, ys, times, count, grid, previous, limit)

    /** The readings best first once a rare name or abbreviation costs [NAME_COST] more (iPhone's `SwipeTyping.ranked`):
     * the model's lowercase list holds names ("kent", "ut") a loose swipe of a common word could read as, and they
     * would go in capitalised mid-sentence. Common ones ("john", "york") and your own words cost nothing more, and nor
     * does any in English, whose model holds SCOWL's names at their own counts (benchmark/typing/README.md › Swipe typing). */
    fun ranked(candidates: List<SwipeDecoder.Candidate>): List<String> {
        fun rareName(text: String): Boolean {
            // German's capitals list marks its nouns, not names, and English's names are words at their own counts.
            if (model.language == "de" || model.language == "en") return false
            if (text != text.lowercase() || text.length < 2 || (model.capitals?.percent(text) ?: -1) < 95) return false
            if (spatial.personal?.owns(text) == true) return false
            val i = model.index(text)
            return i < 0 || model.unigramCost(i) > COMMON_NAME_COST
        }
        return candidates.withIndex().sortedWith(compareBy<IndexedValue<SwipeDecoder.Candidate>> {
            it.value.cost + if (rareName(it.value.text)) NAME_COST else 0.0
        }.thenBy { it.index }).map { it.value.text }
    }

    companion object {
        const val NAME_COST = 4.0
        /** About the model's 2,500th word. */
        const val COMMON_NAME_COST = 10.0
        /** English contractions the tap decoder fixes or offers (iPhone's `AutocorrectEngine.englishContractions`). */
        val contractions: List<String> = (englishFixes.values.filter { '\'' in it } + TypingWord.suggestOnly.values).distinct().sorted()
    }
}
