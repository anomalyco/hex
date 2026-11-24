package ly.anoma.hex.keyboard

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ly.anoma.hex.typing.KeyGrid
import ly.anoma.hex.typing.KeyPoint
import ly.anoma.hex.typing.PersonalWords
import ly.anoma.hex.typing.SpatialDecoder
import ly.anoma.hex.typing.Suggestion
import ly.anoma.hex.typing.TypingModel
import ly.anoma.hex.typing.TypingWord

/** English's word list standing in for a dictation language with no model of its own (Português, Nederlands,
 * Polski, Русский): your personal dictionary and its replacements stay, English's words do not. */
class StandInTypingTest {
    private fun word(standIn: Boolean = false): TypingWord {
        val model = TypingModel.file(File("src/main/assets/typing/en.hextm"), "en")
        return TypingWord(SpatialDecoder(model), spatial = true).also {
            it.decoder.personal = PersonalWords(model, listOf("Zorbex"), mapOf("myem" to "my@email.com"))
            it.standIn = standIn
        }
    }

    private val grid = KeyGrid(buildMap {
        for ((row, letters) in listOf("qwertyuiop", "asdfghjkl", "zxcvbnm").withIndex())
            for ((i, c) in letters.withIndex()) put(c, KeyPoint(201 + (i - (letters.length - 1) / 2.0) * 39.5, 66.5 + 56 * row))
    }, 39.5, 56.0)

    private fun TypingWord.type(text: String) {
        for (c in text) {
            val p = grid.centres.getValue(c)
            letter(c.toString(), p.x, p.y, grid)
        }
    }

    @Test fun standInOffersYourWordsAndShortcutsButNoEnglishCompletionsOrNextWords() {
        val english = word()
        english.explicit("hel")
        assertTrue("English completes a word being typed", english.strip().any { it.kind == Suggestion.Kind.Word })
        assertTrue("English predicts the next word", english.predictions("the ").isNotEmpty())

        val standIn = word(standIn = true)
        standIn.explicit("hel")
        assertEquals(listOf(Suggestion("hel", Suggestion.Kind.Typed)), standIn.strip())
        assertTrue(standIn.predictions("the ").isEmpty())
        // Your own word still completes, keeping the spelling you saved.
        standIn.reset(); standIn.explicit("zor")
        assertEquals(listOf(Suggestion("zor", Suggestion.Kind.Typed), Suggestion("Zorbex", Suggestion.Kind.Word)), standIn.strip())
        // Your shortcut still shows its phrase and still goes in on space.
        standIn.reset(); standIn.explicit("myem")
        assertEquals(listOf("myem", "my@email.com"), standIn.strip().map { it.text })
        assertEquals("my@email.com", standIn.textReplacement("myem")?.replacement)
    }

    @Test fun standInTurnsOffAutocorrectButKeepsYourTextReplacements() {
        val english = word()
        english.type("dont")
        assertEquals("don't", english.replacement())

        val standIn = word(standIn = true)
        standIn.type("dont")
        assertNull(standIn.replacement())
        assertNull(standIn.finish(' '))
        // Nor does a mark, from the letters or the ?123 page (Русский, Polski, Nederlands, Português: #2603).
        for (mark in ",.!?;:)\"") { standIn.reset(); standIn.type("teh"); assertNull("$mark", standIn.finish(mark)) }
        // Nothing changes what you typed on space or a mark, your own spellings included.
        standIn.reset(); standIn.type("zorbex")
        assertNull(standIn.replacement())
        // Your text replacement still goes in, whatever the language dictates.
        assertEquals("my@email.com", standIn.textReplacement("myem")?.replacement)
    }
}
