package ly.anoma.hex.typing

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** The letter keys of one letters page as Labs › Keyboard › Adaptive touch reads taps on them: each key's keycap
 * ([caps]) and touch cell ([cells]), its row, and the key pitch, in the keys' own pixels. [key] names the key plane
 * from [signature] (the keyboard's width, orientation, row height and number row) and the letters on their rows, so
 * a plane of the same size keeps what was learned and any other starts from the drawn centres. Offsets are kept in
 * keycap widths and heights, so a phone's pixel density never matters. */
internal class TouchPlane(signature: String, val letters: CharArray, val caps: Array<KeyRect>, val cells: Array<KeyRect>,
                          val rows: IntArray, val pitchX: Double, val pitchY: Double) {
    val key: Long
    /** Letters on two keys: learned per key, never moved for the decoder. */
    val duplicated = BooleanArray(letters.size) { i -> letters.count { it == letters[i] } > 1 }

    init {
        require(letters.size == caps.size && caps.size == cells.size && cells.size == rows.size &&
            letters.size <= AdaptiveTouchModel.SLOTS)
        key = hash(buildString {
            append(signature).append('|')
            for (i in letters.indices) append(letters[i]).append(rows[i])
        })
    }

    /** The key whose touch cell holds (x, y), or -1. */
    fun slotAt(x: Double, y: Double): Int {
        for (i in cells.indices) {
            val c = cells[i]
            if (x >= c.left && x < c.right && y >= c.top && y < c.bottom) return i
        }
        return -1
    }

    /** The key that types [letter] nearest (x, y), or -1. */
    fun slotFor(letter: Char, x: Double, y: Double): Int {
        var best = -1
        var distance = Double.POSITIVE_INFINITY
        for (i in letters.indices) {
            if (letters[i] != letter) continue
            val c = caps[i]
            val d = Math.hypot((c.left + c.right) / 2 - x, c.centreY - y)
            if (d < distance) { best = i; distance = d }
        }
        return best
    }

    /** How far (x, y) is from [slot]'s centre, in key pitches. */
    fun pitches(x: Double, y: Double, slot: Int): Double {
        val c = caps[slot]
        return Math.hypot((x - (c.left + c.right) / 2) / pitchX, (y - c.centreY) / pitchY)
    }

    /** The same keys where they were: what was learned on one still reads the other's taps. */
    fun same(other: TouchPlane?) = other != null && other.key == key && other.caps.contentEquals(caps)

    companion object {
        /** FNV-1a, the same on every launch; never 0, which marks a free context. */
        fun hash(text: String): Long {
            var h = -0x340d631b7bdddcdbL
            for (b in text.toByteArray(Charsets.UTF_8)) {
                h = h xor (b.toLong() and 0xff)
                h *= 0x100000001b3L
            }
            return if (h == 0L) 1L else h
        }
    }
}

/** Adaptive touch's averages (iOS AdaptiveTouchModel, the same numbers): fixed-size sums per physical key in four
 * rotating bins of [BIN_TAPS] labelled taps, for each context (a key plane and a grip), and the frozen centre
 * offsets the decoder reads. No letters, words, times or order of taps: it is always [BYTES] bytes. */
internal class AdaptiveTouchModel {
    class Context {
        /** [TouchPlane.key]; 0 for a free context. */
        var geometry = 0L
        var regime = 0
        var hasSnapshot = false
        /** The decoder reads this context's offsets. */
        var inUse = false
        /** Labelled taps go to this grip of its plane. */
        var learning = false
        var bin = 0
        var binFill = 0
        var sessions = 0
        var lastUsed = 0
        var accepted = 0
        var sessionStamp = 0
        var sincePromotion = 0
        var promotions = 0
        /** Labelled taps by evidence: kept as typed, a picked suggestion, a retype or undo. */
        var weak = 0
        var picked = 0
        var strong = 0

        val isActiveEligible get() = hasSnapshot && accepted >= ACTIVE_TAPS && sessions >= ACTIVE_SESSIONS
    }

    var clock = 0
    var session = 0
    val contexts = Array(CONTEXTS) { Context() }
    /** [context][slot][bin][W, Σw², Σw·x, Σw·y], in keycaps. */
    val stats = FloatArray(CONTEXTS * SLOTS * BINS * FIELDS)
    /** [context][slot][x, y]: the frozen offsets, in keycaps. */
    val snapshot = FloatArray(CONTEXTS * SLOTS * 2)

    fun offsetX(context: Int, slot: Int) = snapshot[(context * SLOTS + slot) * 2]
    fun offsetY(context: Int, slot: Int) = snapshot[(context * SLOTS + slot) * 2 + 1]

    fun context(geometry: Long, regime: Int) = contexts.indexOfFirst { it.geometry == geometry && it.regime == regime }

    /** The context for a plane and grip, taking the least recently used one when it is new. */
    fun ensureContext(geometry: Long, regime: Int): Int {
        clock++
        val found = context(geometry, regime)
        if (found >= 0) { contexts[found].lastUsed = clock; return found }
        val free = contexts.indexOfFirst { it.geometry == 0L }.takeIf { it >= 0 }
            ?: contexts.indices.minBy { contexts[it].lastUsed }
        clear(free)
        contexts[free].geometry = geometry
        contexts[free].regime = regime
        contexts[free].lastUsed = clock
        return free
    }

    fun clear(c: Int) {
        contexts[c] = Context()
        stats.fill(0f, statIndex(c, 0, 0), statIndex(c + 1, 0, 0))
        snapshot.fill(0f, c * SLOTS * 2, (c + 1) * SLOTS * 2)
    }

    /** One labelled tap, [x] and [y] its offset from its key's centre in keycaps. A full bin rotates. */
    fun add(c: Int, slot: Int, x: Float, y: Float, w: Float) {
        val context = contexts[c]
        val i = statIndex(c, slot, context.bin)
        stats[i] += w
        stats[i + 1] += w * w
        stats[i + 2] += w * x
        stats[i + 3] += w * y
        context.accepted++
        context.sincePromotion++
        context.binFill++
        if (context.binFill >= BIN_TAPS) {
            val next = (context.bin + 1) % BINS
            for (s in 0 until SLOTS) stats.fill(0f, statIndex(c, s, next), statIndex(c, s, next) + FIELDS)
            context.bin = next
            context.binFill = 0
        }
    }

    /** Rebuilds a context's frozen offsets from its bins: each key's mean shrunk toward its row's ([KEY_PRIOR]), the
     * row's toward the keyboard's ([ROW_PRIOR]), that toward zero ([ALL_PRIOR]); within [BOUND], and at most [STEP]
     * from the last offsets. */
    fun promote(c: Int, rows: IntArray) {
        val slots = minOf(rows.size, SLOTS)
        val w = FloatArray(slots); val sx = FloatArray(slots); val sy = FloatArray(slots)
        val rw = HashMap<Int, Float>(); val rx = HashMap<Int, Float>(); val ry = HashMap<Int, Float>()
        var aw = 0f; var ax = 0f; var ay = 0f
        for (s in 0 until slots) {
            for (b in 0 until BINS) {
                val i = statIndex(c, s, b)
                w[s] += stats[i]; sx[s] += stats[i + 2]; sy[s] += stats[i + 3]
            }
            rw[rows[s]] = (rw[rows[s]] ?: 0f) + w[s]
            rx[rows[s]] = (rx[rows[s]] ?: 0f) + sx[s]
            ry[rows[s]] = (ry[rows[s]] ?: 0f) + sy[s]
            aw += w[s]; ax += sx[s]; ay += sy[s]
        }
        val allX = ax / (aw + ALL_PRIOR); val allY = ay / (aw + ALL_PRIOR)
        for (s in 0 until slots) {
            val r = rows[s]
            val rowW = rw[r] ?: 0f
            val rowX = ((rx[r] ?: 0f) + ROW_PRIOR * allX) / (rowW + ROW_PRIOR)
            val rowY = ((ry[r] ?: 0f) + ROW_PRIOR * allY) / (rowW + ROW_PRIOR)
            val keyX = (sx[s] + KEY_PRIOR * rowX) / (w[s] + KEY_PRIOR)
            val keyY = (sy[s] + KEY_PRIOR * rowY) / (w[s] + KEY_PRIOR)
            val i = (c * SLOTS + s) * 2
            val oldX = snapshot[i]; val oldY = snapshot[i + 1]
            snapshot[i] = keyX.coerceIn(-BOUND, BOUND).coerceIn(oldX - STEP, oldX + STEP)
            snapshot[i + 1] = keyY.coerceIn(-BOUND, BOUND).coerceIn(oldY - STEP, oldY + STEP)
        }
        contexts[c].hasSnapshot = true
        contexts[c].sincePromotion = 0
        contexts[c].promotions++
    }

    /** The averages as [BYTES] bytes, the same size whatever was learned. */
    fun serialized(): ByteArray {
        val out = ByteBuffer.allocate(BYTES).order(ByteOrder.LITTLE_ENDIAN)
        out.putInt(MAGIC).putInt(VERSION).putLong(0).putInt(clock).putInt(session).putInt(CONTEXTS).putInt(SLOTS)
        for (c in contexts) {
            val start = out.position()
            out.putLong(c.geometry).put(c.regime.toByte())
            out.put(((if (c.hasSnapshot) 1 else 0) or (if (c.inUse) 2 else 0) or (if (c.learning) 4 else 0)).toByte())
            out.put(c.bin.toByte()).put(0.toByte()).putShort(c.binFill.toShort()).putShort(c.sessions.toShort())
            for (v in intArrayOf(c.lastUsed, c.accepted, c.sessionStamp, c.sincePromotion, c.promotions, c.weak, c.picked, c.strong))
                out.putInt(v)
            out.position(start + CONTEXT_BYTES)
        }
        for (v in stats) out.putFloat(v)
        for (v in snapshot) out.putFloat(v)
        out.putInt(checksum(out.array(), BYTES - 4))
        return out.array()
    }

    companion object {
        const val CONTEXTS = 8
        const val SLOTS = 40
        const val BINS = 4
        private const val FIELDS = 4
        /** Labelled taps per bin: the newest 600–800 count once all four are full. */
        const val BIN_TAPS = 200
        /** Offsets form from this many labelled taps in a context, and are worked out again every [PROMOTE_EVERY] more. */
        const val CANDIDATE_TAPS = 100
        const val PROMOTE_EVERY = 25
        /** The decoder reads a context's offsets only after this many labelled taps over [ACTIVE_SESSIONS] sessions. */
        const val ACTIVE_TAPS = 400
        const val ACTIVE_SESSIONS = 3
        /** Each axis of an offset stays within this many keycaps (about a quarter key pitch), and moves at most [STEP]
         * each time. */
        const val BOUND = 0.3f
        const val STEP = 0.05f
        /** Prior strengths, in weak-label taps, pulling a key to its row, a row to the keyboard, and the keyboard to
         * the drawn centre. */
        const val KEY_PRIOR = 20f
        const val ROW_PRIOR = 50f
        const val ALL_PRIOR = 40f

        private const val MAGIC = 0x41415848 // "HXAA"
        private const val VERSION = 1
        private const val HEADER_BYTES = 32
        private const val CONTEXT_BYTES = 64
        const val BYTES = HEADER_BYTES + CONTEXTS * CONTEXT_BYTES + 4 * (CONTEXTS * SLOTS * BINS * FIELDS + CONTEXTS * SLOTS * 2) + 4

        private fun statIndex(c: Int, slot: Int, bin: Int) = ((c * SLOTS + slot) * BINS + bin) * FIELDS

        private fun checksum(bytes: ByteArray, length: Int): Int {
            var h = -0x7ee3623b
            for (i in 0 until length) h = (h xor (bytes[i].toInt() and 0xff)) * 0x01000193
            return h
        }

        /** The averages [bytes] hold, or null for anything but a whole, unchanged table of this version. */
        fun from(bytes: ByteArray?): AdaptiveTouchModel? {
            if (bytes == null || bytes.size != BYTES) return null
            val data = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (data.getInt(BYTES - 4) != checksum(bytes, BYTES - 4)) return null
            if (data.getInt() != MAGIC || data.getInt() != VERSION) return null
            data.getLong()
            val model = AdaptiveTouchModel()
            model.clock = data.getInt()
            model.session = data.getInt()
            if (data.getInt() != CONTEXTS || data.getInt() != SLOTS) return null
            for (c in model.contexts) {
                val start = data.position()
                c.geometry = data.getLong()
                c.regime = data.get().toInt() and 0xff
                val flags = data.get().toInt()
                c.hasSnapshot = flags and 1 != 0
                c.inUse = flags and 2 != 0
                c.learning = flags and 4 != 0
                c.bin = (data.get().toInt() and 0xff).coerceAtMost(BINS - 1)
                data.get()
                c.binFill = data.getShort().toInt() and 0xffff
                c.sessions = data.getShort().toInt() and 0xffff
                c.lastUsed = data.getInt(); c.accepted = data.getInt(); c.sessionStamp = data.getInt()
                c.sincePromotion = data.getInt(); c.promotions = data.getInt()
                c.weak = data.getInt(); c.picked = data.getInt(); c.strong = data.getInt()
                data.position(start + CONTEXT_BYTES)
            }
            for (i in model.stats.indices) model.stats[i] = data.getFloat()
            for (i in model.snapshot.indices) model.snapshot[i] = data.getFloat()
            if (!model.stats.all { it.isFinite() } || !model.snapshot.all { it.isFinite() && kotlin.math.abs(it) <= BOUND }) return null
            return model
        }
    }
}
