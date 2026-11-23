package ly.anoma.hex.typing

import java.text.Normalizer

/**
 * The swipe rules fixture (`crates/keyboard/core/tests/fixtures/swipe-rules.jsonl`, written by iOS's
 * `SwipeRulesFixtureTests` and read by the core's `swipe_rules.rs`), through [KeyboardCore]: what a swipe
 * types (`typed`), the readings' order with rare names (`rank`), the space rule (`space`) and the slid tap
 * (`slid`) must each be the fixture's. Its `name` and `ends` cases test parts of the core no entry point
 * exposes on their own, and are counted as skipped. Each profile's dictionary is its list of words,
 * matched by canonical equivalence as Swift's `Set<String>` does.
 */
internal object SwipeRules {
    class Result(val cases: Map<String, Int>, val skipped: Map<String, Int>, val differences: List<String>) {
        override fun toString() = "cases $cases, skipped $skipped, differences ${differences.size}" +
            differences.take(10).joinToString("") { "\n  $it" }
    }

    private class Profile(val words: Long, val valid: Set<String>) {
        val checker = WordChecker { nfc(it) in valid }
    }

    /** [model]: the core's handle on a language's model, for the decoders `rank` cases read with. */
    fun check(lines: Sequence<String>, model: (language: String) -> Long): Result {
        val profiles = HashMap<Long, Profile>()
        val models = HashMap<String, Long>()
        val decoders = HashMap<String, Long>()
        val cases = HashMap<String, Int>(); val skipped = HashMap<String, Int>()
        val differences = ArrayList<String>()
        try {
            for (line in lines) {
                if (line.isBlank()) continue
                @Suppress("UNCHECKED_CAST") val o = SwipeParity.Json(line).value() as Map<String, Any?>
                val kind = o["kind"] as String
                val got: Any? = when (kind) {
                    "header" -> { check(o["generator"] == 1L) { "a generator this test reads" }; continue }
                    "profile" -> {
                        val replacements = (o["replacements"] as List<*>).map { it as List<*> }
                        val words = KeyboardCore.swipeWordsNew(o["language"] as String, o["hasDictionary"] as Boolean,
                            replacements.map { it[0] as String }.toTypedArray(), replacements.map { it[1] as String }.toTypedArray(),
                            strings(o["own"]), strings(o["vocabulary"]))
                        check(words != 0L) { "profile ${o["id"]}: no words" }
                        profiles[o["id"] as Long] = Profile(words, strings(o["valid"]).map(::nfc).toSet())
                        continue
                    }
                    "typed" -> {
                        val profile = profiles.getValue(o["profile"] as Long)
                        @Suppress("UNCHECKED_CAST") val s = o["state"] as Map<String, Any?>
                        KeyboardCore.swipeTyped(profile.words, strings(o["readings"]), s["before"] as String?, (s["shift"] as Long).toInt(),
                            s["shiftHeld"] as Boolean, (s["autoCapitalization"] as Long).toInt(), s["autoCapitalizes"] as Boolean,
                            s["autocorrection"] as Boolean, profile.checker)
                            ?.let { mapOf("space" to it.space, "word" to it.word, "alternatives" to it.alternatives.toList()) }
                    }
                    "rank" -> {
                        val profile = profiles.getValue(o["profile"] as Long)
                        val language = o["language"] as String
                        val decoder = decoders.getOrPut(language) {
                            val handle = models.getOrPut(language) { model(language) }
                            KeyboardCore.swipeNew(handle, IntArray(0), IntArray(0), emptyArray(), DoubleArray(0), BooleanArray(0))
                        }
                        val candidates = (o["candidates"] as List<*>).map { it as List<*> }
                        KeyboardCore.swipeRank(decoder, profile.words, candidates.map { it[0] as String }.toTypedArray(),
                            candidates.map { java.lang.Double.longBitsToDouble(java.lang.Long.parseUnsignedLong(it[1] as String, 16)) }
                                .toDoubleArray(), profile.checker)?.map { it.toLong() }
                    }
                    "space" -> KeyboardCore.swipeNeedsSpace(o["before"] as String?)
                    "slid" -> {
                        val points = (o["points"] as List<*>).flatMap { (it as String).split(',').map(String::toDouble) }.toDoubleArray()
                        KeyboardCore.swipeIsSlidTap(points, (o["pitch"] as String).toDouble(), o["key"] as String?, o["first"] as String?)
                    }
                    else -> { skipped.merge(kind, 1, Int::plus); continue }
                }
                cases.merge(kind, 1, Int::plus)
                if (got != o["out"]) differences.add("${line.take(200)}: read $got")
            }
        } finally {
            profiles.values.forEach { KeyboardCore.swipeWordsFree(it.words) }
            decoders.values.forEach(KeyboardCore::swipeFree)
            models.values.forEach(KeyboardCore::modelFree)
        }
        return Result(cases, skipped, differences)
    }

    private fun strings(v: Any?) = (v as List<*>).map { it as String }.toTypedArray()
    private fun nfc(word: String) = Normalizer.normalize(word, Normalizer.Form.NFC)
}
