package ly.anoma.hex.typing

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Labs › Keyboard › Emoji shortcodes: iPhone Hex's rules (EmojiShortcodeTests) on the bundled index. */
class EmojiShortcodeTest {
    private val lines = File("src/main/assets/emoji/shortcodes-en.txt").readLines(Charsets.UTF_8)
    private val index = EmojiShortcodeIndex.parse(lines.asSequence())
    private fun matches(before: String) = EmojiShortcode.token(before)?.let { EmojiShortcode.matches(it, index) }.orEmpty()
    private fun completion(before: String) = EmojiShortcode.token(before)?.let { EmojiShortcode.completion(it, index) }

    @Test fun aTokenIsAColonAndTwoLettersAfterWhitespaceOrTheStart() {
        assertEquals(":fire", EmojiShortcode.token(":fire")?.text)
        assertEquals(":pizza", EmojiShortcode.token("I want :pizza")?.text)
        assertEquals(":pi", EmojiShortcode.token("so\n:pi")?.text)
        for (text in listOf(":p", "abc:pizza", "10:30", ":-D", ":+1", ":_x", "", ":", "fire", ":fire ", null))
            assertNull(text, EmojiShortcode.token(text))
    }

    @Test fun anExactNameComesFirstThenWhatTheNameStarts() {
        assertEquals("🔥", matches(":fire").first())
        assertEquals("🍕", matches("lunch :pizza").first())
        assertTrue(matches(":piz").contains("🍕"))
        assertTrue(matches(":fire").size <= EmojiShortcode.LIMIT)
        assertEquals(emptyList<String>(), matches(":zzzzqx"))
    }

    @Test fun aliasesReadUnderscoresAsSpacesAndIgnoreCase() {
        assertEquals("👍", matches(":thumbs_up").first())
        assertEquals("🔥", matches(":FIRE").first())
    }

    @Test fun oneEmojiPerFamily() {
        val families = matches(":running").map { emoji -> (0 until index.size).first { index.emoji(it) == emoji } }
            .map(index::family).filter { it >= 0 }
        assertEquals(families.distinct(), families)
    }

    @Test fun theClosingColonTakesOnlyAWholeName() {
        assertEquals("🔥", completion(":fire"))
        assertEquals("🍕", completion("I want :pizza"))
        assertNull(completion(":piz"))
        assertNull(completion("abc:pizza"))
        assertNull(completion(":zzzzqx"))
    }

    @Test fun emojiThePhoneCannotDrawAreLeftOutWithoutShiftingTheOthers() {
        val drawable = EmojiShortcodeIndex.parse(lines.asSequence()) { it != "🔥" }
        assertNull(EmojiShortcode.completion(EmojiShortcode.Token("fire"), drawable))
        assertEquals("🍕", EmojiShortcode.completion(EmojiShortcode.Token("pizza"), drawable))
        assertTrue("🔥" !in EmojiShortcode.matches(EmojiShortcode.Token("fire"), drawable))
    }

    @Test fun shortcodesPlaceTheBestInTheMiddle() {
        val slots = listOf("🔥", "🧯", "🚒").map { Suggestion(it, Suggestion.Kind.Shortcode) }
        assertEquals(listOf("🧯", "🔥", "🚒"), Suggestion.place(slots).map { it?.text })
    }
}
