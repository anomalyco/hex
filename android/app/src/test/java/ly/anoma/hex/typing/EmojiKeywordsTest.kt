package ly.anoma.hex.typing

import java.io.File
import ly.anoma.hex.keyboard.EmojiQuery
import ly.anoma.hex.keyboard.emojiStarters
import ly.anoma.hex.keyboard.searchBoost
import ly.anoma.hex.speech.Spoken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
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

    // Labs › Keyboard › Emoji search: the iPhone panel's ranking over the bundled index.
    private val search = EmojiSearch.parse(File("src/main/assets/emoji/search-en.txt").readLines(Charsets.UTF_8).asSequence())

    @Test fun searchFindsEmojiByNameAndKeyword() {
        assertTrue(search.size > 1800)
        assertEquals("❤️", search.search("heart", boost = searchBoost(emptyList())).first())
        assertEquals("👍", search.search("thumbs up").first())
        assertTrue("😂" in search.search("laugh").take(10))
        assertTrue("🎉" in search.search("party").take(5))
        assertEquals("red heart", search.name("❤️"))
    }

    @Test fun searchMatchesWordStartsWhateverTheCase() {
        assertTrue("😂" in search.search("LAUG"))
        assertEquals(search.search("heart"), search.search("  Heart! "))
        assertEquals(listOf("cafe", "that"), EmojiSearch.words("Café that’s"))
        assertTrue("❤️" !in search.search("eart"))
        assertEquals(emptyList<String>(), search.search("wqzxqzx"))
        assertEquals(emptyList<String>(), search.search(""))
    }

    @Test fun searchRanksWholeNameWordsFirstAndYourUseAmongEquals() {
        val parsed = EmojiSearch.parse(sequenceOf("# comment", "🅰\tcat face|pet", "🅱\tcatalog|cat", "🅲\tdog|cat", "🅳\tsmall cat|kitten"))
        assertEquals(listOf("🅰", "🅳", "🅱", "🅲"), parsed.search("cat"))
        assertEquals(listOf("🅳", "🅰", "🅱", "🅲"), parsed.search("cat", boost = searchBoost(listOf("🅳"))))
        assertEquals(listOf("🅲"), EmojiSearch.parse(sequenceOf("🅲\tdog|cat", "🅰\tcat")) { it != "🅰" }.search("cat"))
    }

    @Test fun anEmptySearchOffersYourRecentEmojiFirst() {
        val parsed = EmojiSearch.parse(sequenceOf("🅰\ta", "🅱\tb", "🅲\tc", "😂\tface with tears of joy", "🅳\td"))
        assertEquals(listOf("🅲", "🅰", "😂"), emojiStarters(listOf("🅲", "✗", "🅰"), parsed, 3))
        assertEquals(listOf("😂", "🅰", "🅱"), emojiStarters(emptyList(), parsed, 3))
        assertEquals(listOf("😂", "❤️", "👍"), emojiStarters(emptyList(), search, 3))
    }

    @Test fun theSearchQueryTakesKeysAndReturnEndsIt() {
        val seen = ArrayList<String>(); var ended = 0
        val query = EmojiQuery({ seen.add(it) }, { ended++ })
        query.type("c"); query.type("a"); query.type(" "); query.type("t")
        assertEquals("ca t", query.text)
        query.delete(); query.delete()
        assertEquals("ca", query.text)
        query.type("\n"); query.enter()
        assertEquals(2, ended)
        assertEquals(listOf("c", "ca", "ca ", "ca t", "ca ", "ca"), seen)
    }
}
