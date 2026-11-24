package ly.anoma.hex.typing

import java.io.File
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ly.anoma.hex.keyboard.KeyboardLetters

/** An Italian or French elision typed without its apostrophe is offered joined up ("laltro" ->
 * "l'altro"), as English's contractions are ("im" -> "I'm"), from the models the keyboard ships. */
class ElisionSuggestionTest {
    private fun word(language: String): TypingWord {
        val file = File("src/main/assets/typing/$language.hextm")
        return TypingWord(SpatialDecoder(TypingModel.file(file, language)), spatial = true)
    }

    // Gboard's rows as KeyPlaneView draws them, with the space bar the decoder reads missed spaces over.
    private fun layout(language: String): KeyGrid {
        val rows = KeyboardLetters.rows(language)
        val columns = rows.maxOf { it.length }
        return KeyGrid(buildMap {
            for ((row, letters) in rows.withIndex()) for ((i, c) in letters.withIndex())
                if (c.isLetter()) put(c, KeyPoint(((columns - letters.length) / 2.0 + i + 0.5) * 40.0, (row + 0.5) * 56.0))
        }, 40.0, 56.0, KeyRect(64.76, 212.0, 303.1, 257.0))
    }

    private fun TypingWord.type(grid: KeyGrid, text: String) = also {
        reset("")
        for (c in text) { val p = grid.centres.getValue(c); letter(c.toString(), p.x, p.y, grid) }
    }
    private fun TypingWord.slots() = strip().map { it.text }
    private fun TypingWord.words() = strip().filter { it.kind == Suggestion.Kind.Word }.map { it.text }

    @Test fun italianElisionsTypedWithoutTheirApostropheAreOffered() {
        val keys = layout("it")
        val w = word("it")
        // "dell'anno" and "un'altra" are words of the model, which the decoder can already offer as its fix;
        // "l'altro" and "l'ho" are not, so the elision rule is the only thing that offers them. Either way
        // the strip shows the joined-up form.
        for ((typed, offered) in listOf("laltro" to "l'altro", "dellanno" to "dell'anno",
                "unaltra" to "un'altra", "lho" to "l'ho")) {
            w.type(keys, typed)
            assertTrue("$typed -> ${w.slots()}", offered in w.slots())
        }
        // Neither is a word of the model, so no decoder fix can be offering them: this is the elision rule.
        for ((typed, offered) in listOf("laltro" to "l'altro", "lho" to "l'ho")) {
            w.type(keys, typed)
            assertTrue("$typed -> ${w.strip()}", offered in w.words())
        }
        // A suggestion only: space never puts it in for you.
        w.type(keys, "laltro")
        assertNull(w.replacement())
        w.type(keys, "dellanno")
        assertFalse("the longest elision wins", "d'ellanno" in w.slots())
    }

    @Test fun aWordTheModelKnowsIsNoElision() {
        val keys = layout("it")
        val w = word("it")
        for (typed in listOf("lago", "della", "dove", "sanno")) {
            w.type(keys, typed)
            assertFalse("$typed -> ${w.slots()}", w.slots().any { '\'' in it })
        }
    }

    @Test fun aFrenchElisionTypedWithoutItsApostropheIsOffered() {
        val keys = layout("fr")
        val w = word("fr")
        w.type(keys, "lheure")
        assertTrue("lheure -> ${w.slots()}", "l'heure" in w.words())
        w.type(keys, "daccord")
        assertTrue("daccord -> ${w.slots()}", "d'accord" in w.words())
        // "lhomme" is already one of the French model's spellings, so it is no elision to join up.
        w.type(keys, "lhomme")
        assertFalse("l'homme" in w.words())
    }

    @Test fun englishKeepsItsContractionsAndGetsNoElisionRule() {
        val keys = layout("en")
        val w = word("en")
        w.type(keys, "im")
        assertTrue("im -> ${w.slots()}", "I'm" in w.slots())
        w.type(keys, "dont")
        assertTrue("dont -> ${w.slots()}", "don't" in w.slots())
        w.type(keys, "lheure")
        assertFalse("l'heure" in w.slots())
    }

    @Test fun aPickedElisionIsLeftAsItIs() {
        val keys = layout("it")
        val w = word("it")
        w.type(keys, "laltro")
        assertTrue("l'altro" in w.words())
        // A pick commits the whole word and resets it; with the elision in the text, space and a mark
        // leave it alone: finish's revision and withCapitals skip a word with an apostrophe.
        w.reset("")
        w.explicit("l'altro")
        assertNull(w.finish(' '))
        assertNull(w.finish(','))
        assertNull(w.replacement())
    }
}
