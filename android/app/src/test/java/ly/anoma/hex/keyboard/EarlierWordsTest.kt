package ly.anoma.hex.keyboard

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ly.anoma.hex.typing.SpatialDecoder
import ly.anoma.hex.typing.TypingModel

/** Labs › Keyboard › Fix earlier words (#3019): the word around the cursor and what the strip offers for it. */
class EarlierWordsTest {
    private val decoder = SpatialDecoder(TypingModel.file(File("src/main/assets/typing/en.hextm"), "en"))
    private val grid = EarlierWords.grid(KeyboardLetters.rows("en"))

    private fun offer(before: String, after: String, words: EarlierWords = EarlierWords()) =
        EarlierWords.spot(before, after)?.let { words.offer(it, "en", decoder, grid, swears = false) }?.words

    @Test fun findsTheWordAroundTheCursorWithItsNeighbours() {
        assertEquals(EarlierWords.Spot("by", "", "to", "a"), EarlierWords.spot("I want to by", " a car"))
        assertEquals(EarlierWords.Spot("b", "y", "to", "a"), EarlierWords.spot("I want to b", "y a car"))
        assertEquals(EarlierWords.Spot("", "form", null, "the"), EarlierWords.spot("Hello. ", "form the"))
        assertEquals(EarlierWords.Spot("don'", "t", null, null), EarlierWords.spot("don'", "t."))
        // Right before a word is in it too, as a tap at its start leaves the cursor.
        assertEquals(EarlierWords.Spot("", "by", "to", null), EarlierWords.spot("I want to ", "by"))
        // Right after a closing quote or a possessive's apostrophe the cursor is past the word, so the quote is never
        // replaced with it; inside the quotes the word is found without them.
        for (b in listOf("He said 'teh'", "the dogs'", "He said ‘teh’", "the dogs’")) assertNull(b, EarlierWords.spot(b, ""))
        // The quote before it is punctuation: no word before.
        assertEquals(EarlierWords.Spot("teh", "", null, null), EarlierWords.spot("He said 'teh", "' to me"))
        // A lone apostrophe before the word is no word before it.
        assertNull(EarlierWords.spot("' ", "teh")?.previous)
        assertNull(EarlierWords.spot("’ teh", "")?.previous)
        // Not touching a word.
        assertNull(EarlierWords.spot("I want to ", " by"))
        assertNull(EarlierWords.spot("", ""))
        // Handles, hashtags, paths, numbers, addresses and links are left alone.
        for ((b, a) in listOf("@dgun" to "ay", "#tod" to "o", "src/main" to "", "abc12" to "", "me@exam" to "ple.com",
            "github" to ".com", "snake_ca" to "se", "x" to "2"))
            assertNull("$b|$a", EarlierWords.spot(b, a))
    }

    @Test fun offersOtherWordsOnlyWhereTheWordDoesNotFit() {
        assertEquals("to", offer("going too", " the store")?.first())
        assertEquals("from", offer("came form", " the store")?.first())
        assertEquals("read", offer("I red", " the book")?.first())
        // Capitals follow the word.
        assertEquals("To", offer("Going. Too", " the store")?.first())
        // Words that already fit offer nothing.
        assertNull(offer("I love", " it"))
        assertNull(offer("the car", " is red"))
        assertNull(offer("it was", " fun"))
        assertNull(offer("in form", " of"))
        // A word next to a likelier one-letter neighbour still offers nothing where it fits ("cat" beside "car").
        assertNull(offer("a cat", " sat on the mat"))
        assertNull(offer("the cat", " is"))
        assertNull(offer("my cat", " is"))
        assertNull(offer("a cat", " and a dog"))
        // A word written with capitals inside it is left as written.
        assertNull(offer("my iPhoen", ""))
    }

    @Test fun offersWhatAutocorrectChangedFirstAndForgetsItOnceUndone() {
        val words = EarlierWords()
        words.fixed("en", "Hex", "Hey")
        assertEquals("Hex", offer("Hey", " there", words)?.first())
        assertTrue(EarlierWords.spot("Hey", " there")!!.let { words.offer(it, "en", decoder, grid, false)?.fix } != null)
        // Another language's fix, or a word it didn't make, offers no original.
        assertTrue(offer("Hey", " there", EarlierWords().also { it.fixed("es", "Hex", "Hey") })?.contains("Hex") != true)
        words.forget(EarlierWords.Fix("en", "Hex", "Hey"))
        assertTrue(offer("Hey", " there", words)?.contains("Hex") != true)
        // Two-word fixes are not kept; only the last KEPT are.
        val many = EarlierWords()
        many.fixed("en", "well se", "we'll see")
        assertTrue(offer("see", "", many)?.contains("se") != true)
        val letters = ('a'..'z').take(EarlierWords.KEPT + 1)
        for (c in letters) many.fixed("en", "zq$c", "qz$c")
        assertTrue(offer("qz${letters.first()}", "", many)?.contains("zq${letters.first()}") != true)
        assertEquals("zq${letters.last()}", offer("qz${letters.last()}", "", many)?.first())
    }
}
