package ly.anoma.hex.typing

import java.io.File
import java.io.FileDescriptor
import java.io.FileInputStream
import org.junit.Assert.*
import org.junit.Test

/** The Rust core's JNI entry points, on a host build of it (build.gradle.kts, buildHostKeyboardCore). */
class KeyboardCoreTest {
    private val english = File("src/main/assets/typing/en.hextm")

    /** The descriptor's number, as Android's ParcelFileDescriptor.fd gives it. */
    private fun FileInputStream.number(): Int =
        FileDescriptor::class.java.getDeclaredField("fd").apply { isAccessible = true }.getInt(fd)

    @Test
    fun theAbiVersionIsTheHeaders() {
        val header = File("../../crates/keyboard/ffi/include/hex_keyboard.h").readText()
        val version = Regex("#define HK_ABI_VERSION (\\d+)").find(header)!!.groupValues[1].toInt()
        assertEquals(version, KeyboardCore.abiVersion())
    }

    @Test
    fun opensTheEnglishModelByPath() {
        val model = KeyboardCore.modelOpen(english.path)
        assertNotEquals(0L, model)
        assertEquals(86_247L, KeyboardCore.modelWordCount(model))
        KeyboardCore.modelFree(model)
        assertEquals(0L, KeyboardCore.modelOpen("src/main/assets/typing/missing.hextm"))
        assertEquals(0L, KeyboardCore.modelOpen("src/main/assets/typing/SHA256SUMS"))
        assertEquals(0L, KeyboardCore.modelWordCount(0))
        KeyboardCore.modelFree(0)
    }

    @Test
    fun opensTheEnglishModelFromADescriptor() {
        val model = FileInputStream(english).use { KeyboardCore.modelOpenFd(it.number(), 0, english.length()) }
        assertNotEquals(0L, model)
        assertEquals(86_247L, KeyboardCore.modelWordCount(model))
        KeyboardCore.modelFree(model)
        FileInputStream(english).use {
            assertEquals(0L, KeyboardCore.modelOpenFd(it.number(), 1, english.length()))
            assertEquals(0L, KeyboardCore.modelOpenFd(it.number(), 0, english.length() + 1))
        }
        assertEquals(0L, KeyboardCore.modelOpenFd(-1, 0, english.length()))
    }

    @Test
    fun aDecoderNeedsAModel() {
        assertEquals(0L, KeyboardCore.swipeNew(0, IntArray(0), IntArray(0), emptyArray(), DoubleArray(0), BooleanArray(0)))
        assertNull(KeyboardCore.swipeRead(0, DoubleArray(0), null, IntArray(0), DoubleArray(0), DoubleArray(0), 40.0, 50.0, null, 5))
        KeyboardCore.swipeFree(0)
    }

    /** A clean swipe of "hello" on the 390 pt iPhone's English keys (pitch 38.3 × 54 pt) reads "hello" first. */
    @Test
    fun readsASwipe() {
        val model = KeyboardCore.modelOpen(english.path)
        val decoder = KeyboardCore.swipeNew(model, IntArray(0), IntArray(0), arrayOf("hexkb"), doubleArrayOf(11.0), booleanArrayOf(false))
        KeyboardCore.modelFree(model) // the decoder keeps its own reference
        assertNotEquals(0L, decoder)
        val centres = buildMap {
            for ((letters, first, y) in listOf(Triple("qwertyuiop", 22.65, 67.5), Triple("asdfghjkl", 41.8, 121.5), Triple("zxcvbnm", 80.1, 175.5)))
                for ((i, c) in letters.withIndex()) put(c, first + 38.3 * i to y)
        }
        val stops = "hello".map { centres.getValue(it) }
        val points = buildList {
            for ((a, b) in stops.zipWithNext()) for (step in 0 until 8) {
                val t = step / 8.0
                add(a.first + (b.first - a.first) * t)
                add(a.second + (b.second - a.second) * t)
            }
            add(stops.last().first)
            add(stops.last().second)
        }.toDoubleArray()
        val letters = centres.keys.map { it.code }.toIntArray()
        val xs = centres.values.map { it.first }.toDoubleArray()
        val ys = centres.values.map { it.second }.toDoubleArray()
        val reading = KeyboardCore.swipeRead(decoder, points, null, letters, xs, ys, 38.3, 54.0, null, 5)!!
        assertEquals(reading.texts.contentToString(), "hello", reading.texts.first())
        assertTrue(reading.costs.toList().zipWithNext().all { (a, b) -> a <= b })
        assertEquals(reading.texts.size, reading.costs.size)
        assertTrue(reading.texts.size in 1..5 && reading.scored >= reading.texts.size)
        val times = DoubleArray(points.size / 2) { it * 0.01 }
        assertEquals("hello", KeyboardCore.swipeRead(decoder, points, times, letters, xs, ys, 38.3, 54.0, "say", 3)!!.texts.first())
        assertNull(KeyboardCore.swipeRead(decoder, points, DoubleArray(1), letters, xs, ys, 38.3, 54.0, null, 3))
        KeyboardCore.swipeFree(decoder)
    }

    /** A dictionary of a few words, logging every question as the Swift stub does. */
    private class Words(private val valid: Set<String>, val log: MutableList<String>) : KeyboardCore.TextChecker {
        override fun isWordValid(word: String): Boolean =
            (word.length == 1 || word.lowercase() in valid).also { log += "v|$word|$it" }
        override fun guesses(word: String): Array<String> =
            valid.filter { it.length in word.length - 1..word.length + 1 && it[0] == word[0] }.sorted().toTypedArray().also { log += "g|$word" }
        override fun completions(prefix: String): Array<String> =
            valid.filter { it.startsWith(prefix) && it != prefix }.sorted().toTypedArray().also { log += "c|$prefix" }
        override fun useLanguage(code: String) {
            log += "u|$code"
        }
    }

    @Test
    fun autocorrectsThroughTheCore() {
        val log = mutableListOf<String>()
        val tsv = "*\tswear\tfuck\nfr\tswear\tmerde\n"
        val ac = KeyboardCore.autocorrectNew(tsv, Words(setOf("the", "they", "you", "hello", "help"), log))
        assertNotEquals(0L, ac)
        val available = arrayOf("en_US", "fr")
        assertTrue(KeyboardCore.autocorrectSetLanguage(ac, "en", available))
        assertEquals("u|en_US", log.first())
        val rows = listOf("qwertyuiop" to 0.0, "asdfghjkl" to 0.5, "zxcvbnm" to 1.5)
        val letters = rows.flatMap { (r, _) -> r.map { it.code } }.toIntArray()
        val columns = rows.flatMap { (r, offset) -> r.indices.map { it + offset } }.toDoubleArray()
        val rowOf = rows.flatMapIndexed { i, (r, _) -> r.map { i.toDouble() } }.toDoubleArray()
        assertTrue(KeyboardCore.autocorrectSetKeys(ac, "en", letters, columns, rowOf))
        assertTrue(KeyboardCore.autocorrectLoadLexicon(ac, arrayOf("omw"), arrayOf("On my way!")))

        assertArrayEquals(arrayOf("teh", "the"), KeyboardCore.autocorrectReplacementOnSpace(ac, "teh", 0, false))
        assertEquals("en", KeyboardCore.autocorrectLastFixLanguage(ac))
        assertArrayEquals(arrayOf("omw", "On my way!"), KeyboardCore.autocorrectReplacementOnSpace(ac, "say omw", 0, false))
        assertNull(KeyboardCore.autocorrectReplacementOnSpace(ac, "the", 0, false))
        assertNull(KeyboardCore.autocorrectReplacementOnSpace(ac, "teh", 1, false))
        assertNull(KeyboardCore.autocorrectLastFixLanguage(ac))
        val strip = KeyboardCore.autocorrectSuggestions(ac, "teh", 0, false, 0, false)!!
        assertEquals(listOf(0, 2), strip.take(2).map { it.kind })
        assertEquals("the", strip[1].replacement)
        val starters = KeyboardCore.autocorrectSuggestions(ac, "Done. ", 0, false, 0, true)!!
        assertEquals(listOf("I", "The", "I'm"), starters.map { it.replacement })
        assertFalse(KeyboardCore.autocorrectMayOffer(ac, "fuck", null))
        assertNull(KeyboardCore.autocorrectRevision(ac, "the", "the", "end", 0, false))

        assertTrue(KeyboardCore.autocorrectSetPartner(ac, "fr", available, Words(setOf("merci", "est"), mutableListOf())))
        assertNull(KeyboardCore.autocorrectReplacementOnSpace(ac, "merci", 0, false))
        KeyboardCore.autocorrectSuppressAndKeep(ac, "teh")
        assertNull(KeyboardCore.autocorrectReplacementOnSpace(ac, "teh", 0, false))
        KeyboardCore.autocorrectFree(ac)
        KeyboardCore.autocorrectFree(0)
    }

    @Test
    fun autocorrectReadsTextAndManifestsAlone() {
        assertEquals("wor", KeyboardCore.currentToken("hello wor"))
        assertNull(KeyboardCore.currentToken("@handle"))
        assertTrue(KeyboardCore.languageKnown("fr-CA"))
        assertTrue(KeyboardCore.languageKnown("ko"))
        assertFalse(KeyboardCore.languageKnown("xx"))
        assertNull(KeyboardCore.autocorrectSuggestions(0, "teh", 0, false, 0, false))
    }
}
