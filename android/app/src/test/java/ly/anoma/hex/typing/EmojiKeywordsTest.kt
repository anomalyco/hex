package ly.anoma.hex.typing

import java.io.File
import ly.anoma.hex.speech.Spoken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

class EmojiKeywordsTest {
    private val keywords = EmojiKeywords.parse(File("src/main/assets/emoji/suggestions-en.txt").readLines(Charsets.UTF_8).asSequence())
    private fun match(word: String, language: String = Spoken.English.layoutCode) = keywords.match(word, language)

    @Test fun wordsNameTheirEmoji() {
        assertEquals("😊", match("smile"))
        assertEquals("😂", match("joy"))
        assertEquals("😂", match("haha"))
        assertEquals("❤️", match("heart"))
    }

    @Test fun lookupIgnoresCase() {
        assertEquals("😊", match("Smile"))
        assertEquals("😂", match("HAHA"))
        assertEquals("❤️", match("Heart"))
    }

    @Test fun onlyWholeWordsMatch() {
        assertNull(match("smil"))
        assertNull(match("wqzxqzx"))
        assertNull(match(""))
    }

    @Test fun onlyEnglishModelsOfferEmoji() {
        assertEquals("en", Spoken.English.layoutCode)
        assertEquals("😊", match("smile", "en"))
        for (language in listOf("de", "es", "nb", "fr", "it")) assertNull(language, match("smile", language))
    }

    @Test fun withEmojiAppendsTheEmojiAfterTheWords() {
        val typed = Suggestion("smile", Suggestion.Kind.Typed)
        val word = Suggestion("smiley", Suggestion.Kind.Word)
        assertEquals(listOf(typed, word, Suggestion("😊", Suggestion.Kind.Emoji)),
            Suggestion.withEmoji(listOf(typed, word), "😊"))
    }

    @Test fun theEmojiIsPaddedIntoTheLastSlot() {
        val typed = Suggestion("smile", Suggestion.Kind.Typed)
        val word = Suggestion("smiley", Suggestion.Kind.Word)
        assertEquals(listOf(typed, null, Suggestion("😊", Suggestion.Kind.Emoji)),
            Suggestion.place(Suggestion.withEmoji(listOf(typed), "😊")))
        assertEquals(listOf(typed, word, Suggestion("😊", Suggestion.Kind.Emoji)),
            Suggestion.place(Suggestion.withEmoji(listOf(typed, word), "😊")))
        assertEquals(listOf(null, word, Suggestion("😊", Suggestion.Kind.Emoji)),
            Suggestion.place(Suggestion.withEmoji(listOf(word), "😊")))
        assertEquals(listOf(typed, word, null), Suggestion.place(listOf(typed, word)))
    }

    @Test fun noEmojiLeavesTheStripAlone() {
        val slots = listOf(Suggestion("smile", Suggestion.Kind.Typed), Suggestion("smiley", Suggestion.Kind.Word))
        assertSame(slots, Suggestion.withEmoji(slots, null))
    }

    @Test fun theStripStillShowsThreeSlots() {
        val slots = (1..4).map { Suggestion("word$it", Suggestion.Kind.Word) }
        val shown = Suggestion.withEmoji(slots, "😊")
        assertEquals(3, shown.size)
        assertEquals(Suggestion("😊", Suggestion.Kind.Emoji), shown.last())
    }

    @Test fun parseSkipsCommentsAndKeepsTheFirstSpelling() {
        val parsed = EmojiKeywords.parse(sequenceOf("# comment", "", "smile\t😊", "smile\t🙂"))
        assertEquals("😊", parsed.match("smile", "en"))
    }

    @Test fun parsingDropsEmojiTheDeviceCannotDraw() {
        val parsed = EmojiKeywords.parse(sequenceOf("smile\t😊", "haha\t😂")) { it != "😂" }
        assertEquals("😊", parsed.match("smile", "en"))
        assertNull(parsed.match("haha", "en"))
    }
}
