package ly.anoma.hex.typing

import java.io.File
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Swipe typing's reader on the host core: the words it reads beside the model's, as iOS's SwipeTyping.syncWords
 * hands them over. Swipes are drawn straight from key to key on the 390 pt iPhone's English keys. */
class SwipeDecoderTest {
    private val file = File("src/main/assets/typing/en.hextm")
    private val model = TypingModel.file(file, "en")
    private val grid = KeyGrid(buildMap {
        for ((letters, first, y) in listOf(Triple("qwertyuiop", 22.65, 67.5), Triple("asdfghjkl", 41.8, 121.5), Triple("zxcvbnm", 80.1, 175.5)))
            for ((i, c) in letters.withIndex()) put(c, KeyPoint(first + 38.3 * i, y))
    }, 38.3, 54.0)

    private fun swipe(decoder: SwipeDecoder, handles: SwipeDecoder.Handles, keys: String): List<String> {
        val stops = keys.map { grid.centres.getValue(it) }
        val points = buildList {
            for ((a, b) in stops.zipWithNext()) for (step in 0 until 8) {
                val t = step / 8.0
                add(a.x + (b.x - a.x) * t); add(a.y + (b.y - a.y) * t)
            }
            add(stops.last().x); add(stops.last().y)
        }.toDoubleArray()
        return decoder.read(handles, points, null, grid, null, 8)
    }

    @Test
    fun readsContractionsAndYourOwnWords() {
        val blocked = File("src/main/assets/typing/never-suggest.tsv").bufferedReader().use { SwipeDecoder.blocked(it, "en") }
        val decoder = SwipeDecoder(KeyboardCore.modelOpen(file.path), model, blocked)
        val personal = PersonalWords(model, listOf("hexkb"), mapOf("brb" to "be right back", "hx" to "Hexagonal"))
        val handles = decoder.sync(false, personal) { it.run() }!!
        val dont = swipe(decoder, handles, "dont")
        assertTrue(dont.toString(), "don't" in dont)
        val own = swipe(decoder, handles, "hexkb")
        assertTrue(own.toString(), "hexkb" in own)
        val replacement = swipe(decoder, handles, "hexagonal")
        assertTrue(replacement.toString(), "Hexagonal" in replacement)
        // Without them, neither of your words is read.
        val bare = decoder.sync(false, null) { it.run() }!!
        assertTrue("hexkb" !in swipe(decoder, bare, "hexkb"))
    }

    @Test
    fun aNameIsAWordTheBooksCapitaliseAtLeast95PercentOfTheTime() {
        val checker = SwipeDecoder(KeyboardCore.modelOpen(file.path), model, emptyList()).checker
        // en.capitals: "monday" 100%, "march" 92%.
        assertTrue(checker.isWordValid("Monday"))
        assertFalse(checker.isWordValid("monday"))
        assertTrue(checker.isWordValid("march"))
        assertFalse(checker.isWordValid("March"))
    }
}
