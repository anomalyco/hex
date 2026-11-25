package ly.anoma.hex.typing

import kotlin.math.max
import kotlin.math.min

/** Labs › Keyboard › Adaptive touch (iOS AdaptiveTouch): where you land on each letter key, learned on this phone from
 * taps whose meant letter the words around them show ([AdaptiveTouchSpans]), and the personal key centres the decoder
 * reads once they fit your taps better than the drawn ones. Keycaps, touch areas, the likelier next letter and swipes
 * never move, and [TypingWord] makes no correction with them that the drawn centres wouldn't make. [model] is all it
 * keeps. One instance per process, used on the main thread. */
internal class AdaptiveTouch(model: AdaptiveTouchModel = AdaptiveTouchModel()) {
    /** Replays: [Monitor] watches how the centres fit (and falls back) without learning; [Frozen] only reads them. */
    enum class Mode { Learn, Monitor, Frozen }

    var model = model
        private set
    var mode = Mode.Learn
    /** Corrections the personal centres read must clear the drawn centres' margin too. Replays turn it off to measure
     * what that holds back; the keyboard never does. */
    var guards = true
    var clock: () -> Double = { System.nanoTime() / 1e9 }
    /** Hands the averages over to be kept, every [SAVE_EVERY] labelled taps and on [save]. */
    var onSave: ((ByteArray) -> Unit)? = null
    /** Bumped when the centres the decoder should read change: a promotion, a fallback, a reset. */
    var version = 0
        private set
    /** The key plane taps are read against now. */
    var plane: TouchPlane? = null
        private set
    private var spreadX = 1.0
    private var spreadY = 1.0
    private val spans = AdaptiveTouchSpans()
    private var unsaved = 0
    // [context][window][x, y]: each recent label's log-likelihood gain of the context's centres over the drawn ones, per
    // axis. Memory only.
    private val gains = FloatArray(AdaptiveTouchModel.CONTEXTS * WINDOW * 2)
    private val gainCount = IntArray(AdaptiveTouchModel.CONTEXTS)
    private val gainHead = IntArray(AdaptiveTouchModel.CONTEXTS)
    private var cachedFrom: KeyGrid? = null
    private var cachedVersion = -1
    private var cached: KeyGrid? = null

    // MARK: The decoder

    /** [grid] with your own centres for its letters, or null while this key plane has none in use: thin evidence, a new
     * size, or a grip they don't fit. Also keeps the plane labels are read against. */
    fun personal(grid: KeyGrid): KeyGrid? {
        val plane = grid.plane ?: return null
        sync(grid)
        if (cachedFrom === grid && cachedVersion == version) return cached
        var out: KeyGrid? = null
        val c = model.contexts.indexOfFirst { it.geometry == plane.key && it.inUse && it.hasSnapshot }
        if (c >= 0) {
            val centres = LinkedHashMap(grid.centres)
            var moved = false
            for (s in plane.letters.indices) {
                if (plane.duplicated[s]) continue
                val dx = model.offsetX(c, s); val dy = model.offsetY(c, s)
                if (dx == 0f && dy == 0f) continue
                val p = grid.centres[plane.letters[s]] ?: continue
                val cap = plane.caps[s]
                centres[plane.letters[s]] = KeyPoint(p.x + dx * (cap.right - cap.left), p.y + dy * (cap.bottom - cap.top))
                moved = true
            }
            if (moved) out = KeyGrid(centres, grid.pitchX, grid.pitchY, grid.space, plane, grid.bottomY)
        }
        cachedFrom = grid; cachedVersion = version; cached = out
        return out
    }

    /** Whether your own centres are in use on the current key plane. */
    val isActive get() = plane?.let { p -> model.contexts.any { it.geometry == p.key && it.inUse } } == true

    /** For Settings: the most labelled taps on one key plane, and whether any plane reads your own centres. */
    val taps get() = model.contexts.maxOf { it.accepted }
    val anyActive get() = model.contexts.any { it.inUse }

    // A new letters page: keep its plane, and forget taps typed on another.
    private fun sync(grid: KeyGrid) {
        val next = grid.plane
        if (next === plane) return
        if (next == null || !next.same(plane)) spans.forget()
        plane = next
        spreadX = grid.spreadX
        spreadY = grid.verticalSpread
    }

    // MARK: Sessions, saving and reset

    /** The keyboard came up, or a new field: the words in hand go. */
    fun beginSession() {
        spans.forget()
        model.session++
    }

    /** Hands unsaved averages to [onSave]. */
    fun save() {
        if (unsaved == 0) return
        unsaved = 0
        onSave?.invoke(model.serialized())
    }

    /** Everything learned goes, at once. */
    fun reset() {
        model = AdaptiveTouchModel()
        spans.forget()
        gainCount.fill(0); gainHead.fill(0)
        unsaved = 0
        changed()
    }

    // MARK: Labels

    /** A letter key typed [letter] on a tap that went down at (x, y) on [grid]'s keys; [usable] is false when the touch
     * slid onto its key, so its point says nothing about where you aim. */
    fun typed(letter: Char, x: Double, y: Double, grid: KeyGrid, usable: Boolean) {
        if (mode == Mode.Frozen || grid.plane == null) return
        sync(grid)
        val plane = plane ?: return
        spans.typed(TouchTap(x.toFloat(), y.toFloat(), letter.lowercaseChar(), usable), plane.key, clock())
    }

    /** A letter went into the word without a tap (a held accent): the word teaches nothing. */
    fun untapped() = spans.untapped()

    /** Delete is about to remove the last character of [before]. */
    fun willDelete(before: String?) = spans.willDelete(before)

    /** Undo put back [original], what was typed, in place of the autocorrection just made. */
    fun undone(original: String) = spans.undone(original)

    /** Something the words can't follow (a swipe, an emoji, the cursor moving): the words in hand go. */
    fun forget() = spans.forget()

    /** The text changed without the keys: what can't be followed any more goes. */
    fun hostChanged(before: String?) {
        if (!spans.isEmpty) spans.hostChanged(before, before?.let { TypingWord.currentToken(it) })
    }

    /** The word being typed ended (a space, a mark): [token] as the field shows it, [corrected] when autocorrect replaced
     * it, [previousRevised] when it changed the word before too. Words before it that stood teach now; this one waits
     * for the next. */
    fun finish(token: String?, before: String?, corrected: String? = null, previousRevised: Boolean = false) =
        admit(spans.finish(token, corrected, null, false, previousRevised, before, clock()))

    /** A suggestion is about to replace [token], the word being typed; [overCorrection] when it is what was typed, over
     * the fix space would make. */
    fun picked(token: String, replacement: String, overCorrection: Boolean, before: String?) =
        admit(spans.finish(token, null, replacement, overCorrection, false, before, clock()))

    private fun admit(stood: List<AdaptiveTouchSpans.Span>) {
        val plane = plane ?: return
        for (span in stood) {
            if (span.geometry != plane.key || span.outcome == AdaptiveTouchSpans.Outcome.Corrected) continue
            var weak = 0
            for ((i, tap) in span.taps.withIndex()) {
                if (i >= span.labels.size) break
                val label = span.labels[i]
                val erased = tap.erasedLetter
                if (span.retyped && erased != null && erased != tap.letter) {
                    // The erased tap meant the letter that replaced it: a neighbour, not a miss.
                    val x = tap.erasedX.toDouble(); val y = tap.erasedY.toDouble()
                    val slot = plane.slotFor(label, x, y)
                    if (slot >= 0 && plane.slotAt(x, y) != slot && plane.pitches(x, y, slot) <= NEIGHBOUR_PITCHES)
                        admit(slot, x, y, STRONG)
                }
                if (!tap.usable) continue
                val x = tap.x.toDouble(); val y = tap.y.toDouble()
                val slot = plane.slotFor(label, x, y)
                if (slot < 0) continue
                when (span.outcome) {
                    // Only what the key under the finger typed: never a letter the likelier next letter claimed from
                    // a neighbour, whose label would be the keyboard's own guess.
                    AdaptiveTouchSpans.Outcome.Typed -> {
                        if (plane.slotAt(x, y) != slot || weak >= WEAK_PER_WORD) continue
                        weak++
                        admit(slot, x, y, WEAK)
                    }
                    AdaptiveTouchSpans.Outcome.Kept -> admit(slot, x, y, STRONG)
                    AdaptiveTouchSpans.Outcome.Picked -> admit(slot, x, y, PICKED)
                    AdaptiveTouchSpans.Outcome.Corrected -> Unit
                }
            }
        }
    }

    /** One tap with evidence of the letter it meant, on the current plane: [quality] is [WEAK], [PICKED] or [STRONG].
     * Sums, a bin and the fit check; a promotion every [AdaptiveTouchModel.PROMOTE_EVERY] taps. */
    private fun admit(slot: Int, x: Double, y: Double, quality: Float) {
        val plane = plane ?: return
        if (mode == Mode.Frozen || slot !in plane.letters.indices) return
        val cap = plane.caps[slot]
        val width = cap.right - cap.left; val height = cap.bottom - cap.top
        if (width <= 0 || height <= 0) return
        val centreX = (cap.left + cap.right) / 2
        val ox = (x - centreX) / width; val oy = (y - cap.centreY) / height
        val distance = plane.pitches(x, y, slot)
        if (!ox.isFinite() || !oy.isFinite() || distance > MISS_PITCHES) return
        val shrink = min(1.0, HUBER_PITCHES / max(1e-4, distance)).toFloat()

        // How each grip's centres on this plane fit the tap, before it teaches anything.
        val sx = (spreadX / width).toFloat(); val sy = (spreadY / height).toFloat()
        val rx = ox.toFloat() * shrink; val ry = oy.toFloat() * shrink
        for (c in model.contexts.indices) {
            val context = model.contexts[c]
            if (context.geometry != plane.key || !context.hasSnapshot) continue
            val dx = model.offsetX(c, slot); val dy = model.offsetY(c, slot)
            pushGain(c, (rx * dx - .5f * dx * dx) / (sx * sx), (ry * dy - .5f * dy * dy) / (sy * sy))
        }

        if (mode == Mode.Learn) {
            val c = learningContext(plane.key)
            val context = model.contexts[c]
            if (context.sessionStamp != model.session) {
                context.sessionStamp = model.session
                context.sessions = min(0xffff, context.sessions + 1)
            }
            model.add(c, slot, ox.toFloat(), oy.toFloat(), quality * shrink)
            when {
                quality >= STRONG -> context.strong++
                quality >= PICKED -> context.picked++
                else -> context.weak++
            }
            if (context.accepted >= AdaptiveTouchModel.CANDIDATE_TAPS && context.sincePromotion >= AdaptiveTouchModel.PROMOTE_EVERY) {
                model.promote(c, plane.rows)
                if (context.inUse) changed()
            }
            unsaved++
            if (unsaved >= SAVE_EVERY) save()
        }
        decideRegime(plane.key)
    }

    // The grip of [geometry] labels go to, making its first one.
    private fun learningContext(geometry: Long): Int {
        val found = model.contexts.indexOfFirst { it.geometry == geometry && it.learning }
        if (found >= 0) {
            model.clock++
            model.contexts[found].lastUsed = model.clock
            return found
        }
        val c = model.ensureContext(geometry, 0)
        model.contexts[c].learning = true
        return c
    }

    // Falls back to the drawn centres when the grip in use fits recent taps worse than they do; lets labels teach the
    // other grip when the learning one, which had the evidence to be used, stopped fitting; takes a grip into use once it
    // has the evidence and fits. Before the evidence floor, a new grip only mixes into what is learned.
    private fun decideRegime(geometry: Long) {
        val contexts = model.contexts
        val regimes = contexts.indices.filter { contexts[it].geometry == geometry }
        var didChange = false
        for (c in regimes) if (contexts[c].inUse && gainCount[c] >= MONITOR_MINIMUM && misfits(c, DISENGAGE_AXIS, DISENGAGE_GAIN)) {
            contexts[c].inUse = false
            didChange = true
        }
        val learning = regimes.firstOrNull { contexts[it].learning }
        if (mode == Mode.Learn && learning != null && contexts[learning].isActiveEligible &&
            gainCount[learning] >= MONITOR_MINIMUM + 8 && misfits(learning, SWITCH_AXIS, SWITCH_GAIN) && recentGain(learning) < 0) {
            val regime = if (contexts[learning].regime == 0) 1 else 0
            val other = model.context(geometry, regime)
            val next = if (other >= 0) {
                // The other grip fits the newest taps: it is back. One that doesn't is a third grip's to learn afresh.
                if (contexts[other].hasSnapshot && !(gainCount[other] > 0 && recentGain(other) > 0)) {
                    model.clear(other)
                    contexts[other].geometry = geometry
                    contexts[other].regime = regime
                    resetGains(other)
                }
                other
            } else model.ensureContext(geometry, regime).also { resetGains(it) }
            contexts[learning].learning = false
            contexts[next].learning = true
        }
        val now = contexts.indices.filter { contexts[it].geometry == geometry }
        if (now.none { contexts[it].inUse }) {
            val ready = now.filter {
                if (!contexts[it].isActiveEligible || gainCount[it] < MONITOR_MINIMUM) return@filter false
                val (gx, gy) = meanGain(it)
                gx + gy > 0 && min(gx, gy) > ENGAGE_AXIS && recentGain(it) > 0
            }
            ready.maxByOrNull { meanGain(it).let { (x, y) -> x + y } }?.let { best ->
                contexts[best].inUse = true
                if (mode == Mode.Learn) for (c in now) contexts[c].learning = c == best
                didChange = true
            }
        }
        if (didChange) changed()
    }

    private fun pushGain(c: Int, x: Float, y: Float) {
        val i = (c * WINDOW + gainHead[c]) * 2
        gains[i] = x; gains[i + 1] = y
        gainHead[c] = (gainHead[c] + 1) % WINDOW
        gainCount[c] = min(WINDOW, gainCount[c] + 1)
    }

    // The grip fits worse than the drawn centres along one axis, or overall.
    private fun misfits(c: Int, axis: Float, total: Float): Boolean {
        val (x, y) = meanGain(c)
        return min(x, y) < axis || x + y < total
    }

    private fun resetGains(c: Int) { gainCount[c] = 0; gainHead[c] = 0 }

    private fun meanGain(c: Int): Pair<Float, Float> {
        val n = gainCount[c]
        if (n == 0) return 0f to 0f
        var x = 0f; var y = 0f
        for (k in 0 until n) { x += gains[(c * WINDOW + k) * 2]; y += gains[(c * WINDOW + k) * 2 + 1] }
        return x / n to y / n
    }

    // The newest quarter of the window, both axes: a second, later window that must agree.
    private fun recentGain(c: Int): Float {
        val n = min(gainCount[c], WINDOW / 4)
        if (n == 0) return 0f
        var sum = 0f
        for (k in 1..n) {
            val i = (c * WINDOW + (gainHead[c] - k + WINDOW) % WINDOW) * 2
            sum += gains[i] + gains[i + 1]
        }
        return sum / n
    }

    private fun changed() { version++ }

    companion object {
        /** The fit check: mean log-likelihood gain per tap and axis, in nats, over the last [WINDOW] labels. */
        const val WINDOW = 32
        const val MONITOR_MINIMUM = 16
        const val DISENGAGE_AXIS = -0.06f
        const val DISENGAGE_GAIN = -0.1f
        const val SWITCH_AXIS = -0.08f
        const val SWITCH_GAIN = -0.12f
        const val ENGAGE_AXIS = -0.02f
        /** Label weights: a word kept as typed, a picked suggestion, a retype or an undone fix. */
        const val WEAK = 1f
        const val PICKED = 2f
        const val STRONG = 3f
        /** Letters of one word left as typed that teach, at most. */
        const val WEAK_PER_WORD = 12
        /** Taps past this many key pitches from their key are misses, not aims; past [HUBER_PITCHES] they count less. */
        const val MISS_PITCHES = 1.5
        const val HUBER_PITCHES = 0.6
        /** How far an erased tap may be from the key of the letter that replaced it: a neighbour, diagonals included. */
        const val NEIGHBOUR_PITCHES = 1.25
        const val SAVE_EVERY = 400
    }
}
