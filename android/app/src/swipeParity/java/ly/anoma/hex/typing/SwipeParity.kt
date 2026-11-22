package ly.anoma.hex.typing

import java.security.MessageDigest

/**
 * The swipe parity fixture (`crates/keyboard/core/tests/fixtures/swipe-parity.jsonl`, written by
 * iOS's `SwipeParityExportTests` and read by the core's `swipe_parity.rs`), read through
 * [KeyboardCore]: each case must come out with exactly the fixture's candidates, in its order,
 * with its costs bit for bit and its `scored`. Shared by the JVM test (the host library) and the
 * instrumented one (bionic's).
 */
internal object SwipeParity {
    /** A language's model: the core's handle on it, the Kotlin reader of the same file (for the
     * blocked words' indices), and the file's bytes (for the fixture's SHA-256). */
    class Model(val handle: Long, val words: TypingModel, val bytes: ByteArray)

    class Result(val cases: Int, val differences: List<String>, val costOnly: Int) {
        override fun toString() = "cases $cases, differences ${differences.size} ($costOnly only in cost bits)" +
            differences.take(10).joinToString("") { "\n  $it" }
    }

    private class Layout(val letters: IntArray, val xs: DoubleArray, val ys: DoubleArray, val pitch: Pair<Double, Double>,
                         val join: Triple<Double, Double, Double>?)

    fun check(lines: Sequence<String>, open: (language: String) -> Model): Result {
        val models = HashMap<String, Model>()
        val decoders = HashMap<Long, Long>()
        val layouts = HashMap<Long, Layout>()
        val differences = ArrayList<String>()
        var cases = 0
        var costOnly = 0
        try {
            for (line in lines) {
                if (line.isBlank()) continue
                @Suppress("UNCHECKED_CAST") val v = Json(line).value() as Map<String, Any?>
                when (v["kind"]) {
                    "header" -> {
                        check(v["generator"] == 1L) { "a generator this test reads" }
                        @Suppress("UNCHECKED_CAST")
                        for ((language, digest) in v["models"] as Map<String, String>) {
                            val model = open(language)
                            models[language] = model
                            check(sha256(model.bytes) == digest) { "the fixture was written for another $language model" }
                        }
                    }
                    "context" -> {
                        val model = models.getValue(v["language"] as String)
                        val words = ArrayList<Int>(); val pairs = ArrayList<Int>()
                        // NeverSuggest::new: a word, or a two-word expression, the model has.
                        for (entry in v["blocked"] as List<*>) {
                            val parts = (entry as String).split(' ').filter { it.isNotEmpty() }
                            when (parts.size) {
                                1 -> model.words.index(entry).takeIf { it >= 0 }?.let(words::add)
                                2 -> {
                                    val a = model.words.index(parts[0]); val b = model.words.index(parts[1])
                                    if (a >= 0 && b >= 0) { pairs.add(a); pairs.add(b) }
                                }
                            }
                        }
                        val extra = (v["extra"] as List<*>).map { it as List<*> }
                        val decoder = KeyboardCore.swipeNew(model.handle, words.toIntArray(), pairs.toIntArray(),
                            extra.map { it[0] as String }.toTypedArray(), extra.map { bits(it[1]) }.toDoubleArray(),
                            extra.map { it[2] as Boolean }.toBooleanArray())
                        check(decoder != 0L) { "context ${v["id"]}: no decoder" }
                        decoders[v["id"] as Long] = decoder
                    }
                    "keys" -> {
                        val centres = (v["centres"] as List<*>).map { it as List<*> }
                        val pitch = v["pitch"] as List<*>
                        val join = (v["join"] as List<*>?)?.let { Triple(decimal(it[0]), decimal(it[1]), decimal(it[2])) }
                        layouts[v["id"] as Long] = Layout(
                            centres.map { (it[0] as String).also { l -> check(l.codePointCount(0, l.length) == 1) }.codePointAt(0) }.toIntArray(),
                            centres.map { decimal(it[1]) }.toDoubleArray(), centres.map { decimal(it[2]) }.toDoubleArray(),
                            decimal(pitch[0]) to decimal(pitch[1]), join)
                    }
                    "case" -> {
                        val layout = layouts.getValue(v["keys"] as Long)
                        val points = joined(screen(v["path"] as String), layout.join)
                        val times = (v["ticks"] as String?)?.let { ticks ->
                            val base = decimal(v["t0"]); var tick = 0L
                            unpack(ticks).map { tick += it; base + tick * 0.001 }.toDoubleArray()
                        }
                        check(inputs(points, times) == v["inputs"]) { "case ${v["id"]}: the path and times rebuilt aren't the fixture's" }
                        // Times that don't run in step with the path are read as none, as iOS's SwipeDecoder and the
                        // core drop them (the JNI entry point refuses them instead).
                        val reading = KeyboardCore.swipeRead(decoders.getValue(v["context"] as Long), points,
                            times?.takeIf { it.size == points.size / 2 }, layout.letters,
                            layout.xs, layout.ys, layout.pitch.first, layout.pitch.second, v["previous"] as String?, (v["limit"] as Long).toInt())
                        cases++
                        val expected = (v["out"] as List<*>).map { (it as List<*>)[0] as String to it[1] as String }
                        val got = reading?.texts.orEmpty().zip(reading?.costs?.toList().orEmpty()) { t, c -> t to hex(c) }
                        val scored = (v["scored"] as Long).toInt()
                        if (got != expected || reading?.scored != scored) {
                            if (got.map { it.first } == expected.map { it.first } && reading?.scored == scored) costOnly++
                            differences.add("case ${v["id"]} (${v["type"]} ${v["word"]}): fixture $expected of $scored, core $got of ${reading?.scored}")
                        }
                    }
                    else -> error("unknown line kind ${v["kind"]}")
                }
            }
        } finally {
            decoders.values.forEach(KeyboardCore::swipeFree)
            models.values.forEach { KeyboardCore.modelFree(it.handle) }
        }
        return Result(cases, differences, costOnly)
    }

    private fun sha256(bytes: ByteArray) = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
    /** Swift's shortest round-trip decimal, read back exactly. */
    private fun decimal(v: Any?) = (v as String).toDouble()
    /** An IEEE 754 bit pattern in 16 hex digits. */
    private fun bits(v: Any?) = java.lang.Double.longBitsToDouble(java.lang.Long.parseUnsignedLong(v as String, 16))
    private fun hex(value: Double) = "%016x".format(java.lang.Double.doubleToRawLongBits(value))

    /** The touches, from whole quarter points packed as steps, interleaved (x, y). */
    private fun screen(packed: String): DoubleArray {
        val steps = unpack(packed)
        var x = 0L; var y = 0L
        return DoubleArray(steps.size) { i -> if (i % 2 == 0) { x += steps[i]; x / 4.0 } else { y += steps[i]; y / 4.0 } }
    }

    /** `KeyboardLayout.joinedForSwipe`. */
    private fun joined(points: DoubleArray, join: Triple<Double, Double, Double>?): DoubleArray {
        val (left, right, gap) = join ?: return points
        val shift = right - left - gap
        return DoubleArray(points.size) { i ->
            val x = points[i]
            when {
                i % 2 == 1 || x <= left -> x
                x >= right -> x - shift
                // Swift's `max(1, width)`.
                else -> left + (x - left) / (if (right - left >= 1.0) right - left else 1.0) * gap
            }
        }
    }

    /** `SwipeParityExportTests.inputs`: FNV-1a of the points' and times' bits. */
    private fun inputs(points: DoubleArray, times: DoubleArray?): String {
        var hash = -0x340d631b7bdddcdbL // 0xCBF29CE484222325
        fun add(value: Double) {
            var bits = java.lang.Double.doubleToRawLongBits(value)
            repeat(8) { hash = (hash xor (bits and 0xFF)) * 0x100000001B3L; bits = bits ushr 8 }
        }
        points.forEach(::add)
        times?.forEach(::add)
        return "%016x".format(hash)
    }

    /** `JSONLines.packed`: base64 of zigzag LEB128 numbers. */
    private fun unpack(packed: String): LongArray {
        val bytes = java.util.Base64.getDecoder().decode(packed)
        val out = ArrayList<Long>()
        var z = 0L; var shift = 0
        for (b in bytes) {
            z = z or ((b.toLong() and 0x7F) shl shift); shift += 7
            if (b.toInt() and 0x80 == 0) { out.add((z ushr 1) xor -(z and 1)); z = 0; shift = 0 }
        }
        return out.toLongArray()
    }

    /** Just enough JSON for the fixture: objects, arrays, strings, integers, booleans and null. */
    private class Json(private val s: String) {
        private var at = 0
        fun value(): Any? {
            while (s[at] == ' ') at++
            return when (s[at]) {
                '{' -> { at++; buildMap { while (s[at] != '}') { val k = value() as String; at++; put(k, value()); if (s[at] == ',') at++ }; at++ } }
                '[' -> { at++; buildList { while (s[at] != ']') { add(value()); if (s[at] == ',') at++ }; at++ } }
                '"' -> string()
                't' -> { at += 4; true }
                'f' -> { at += 5; false }
                'n' -> { at += 4; null }
                else -> { val start = at; while (at < s.length && (s[at] == '-' || s[at].isDigit())) at++; s.substring(start, at).toLong() }
            }
        }
        private fun string(): String = buildString {
            at++
            while (s[at] != '"') {
                if (s[at] == '\\') {
                    at++
                    when (val c = s[at]) {
                        'u' -> { append(s.substring(at + 1, at + 5).toInt(16).toChar()); at += 4 }
                        'n' -> append('\n'); 't' -> append('\t'); 'r' -> append('\r'); 'b' -> append('\b'); 'f' -> append('\u000c')
                        else -> append(c)
                    }
                } else append(s[at])
                at++
            }
            at++
        }
    }
}
