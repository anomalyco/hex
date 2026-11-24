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
    // Types [typed] and checks the elision rule offers none of [fakes]. A decoder fix of its own ("allodio"
    // reads as "all'odio", a word of the model) is not this rule's, and is left as it was.
    private fun TypingWord.assertNoneOf(keys: KeyGrid, typed: String, vararg fakes: String) {
        type(keys, typed)
        val offered = words()
        for (fake in fakes) assertFalse(typed + " -> " + strip(), fake in offered)
    }

    @Test fun italianElisionsTypedWithoutTheirApostropheAreOffered() {
        val keys = layout("it")
        val w = word("it")
        // "dell'anno" and "un'altra" are words of the model, which the decoder can already offer as its fix;
        // "l'altro" and "l'ho" are not, so the elision rule is the only thing that offers them. Either way
        // the strip shows the joined-up form.
        for ((typed, offered) in listOf("laltro" to "l'altro", "lho" to "l'ho", "lha" to "l'ha",
                "lhanno" to "l'hanno", "lacqua" to "l'acqua", "lamico" to "l'amico",
                "quellanno" to "quell'anno", "unaltra" to "un'altra", "dellanno" to "dell'anno")) {
            w.type(keys, typed)
            assertTrue(typed + " -> " + w.slots(), offered in w.slots())
        }
        // Neither is a word of the model, so no whole-word decoder fix can be offering them: this is the
        // elision rule, as a word suggestion.
        for ((typed, offered) in listOf("laltro" to "l'altro", "lho" to "l'ho")) {
            w.type(keys, typed)
            assertTrue(typed + " -> " + w.strip(), offered in w.words())
        }
        // A suggestion only: space never puts it in for you.
        w.type(keys, "laltro")
        assertNull(w.replacement())
        w.type(keys, "dellanno")
        assertFalse("the longest elision wins", "d'ellanno" in w.slots())
    }

    @Test fun aFrenchElisionTypedWithoutItsApostropheIsOffered() {
        val keys = layout("fr")
        val w = word("fr")
        w.type(keys, "lheure")
        assertTrue("lheure -> " + w.slots(), "l'heure" in w.words())
        w.type(keys, "daccord")
        assertTrue("daccord -> " + w.slots(), "d'accord" in w.words())
        // "lhomme" is already one of the French model's spellings, so it is no elision to join up.
        w.type(keys, "lhomme")
        assertFalse("l'homme" in w.words())
    }

    @Test fun aWordOfTheModelsOwnListIsNoElision() {
        val keys = layout("it")
        val w = word("it")
        for (typed in listOf("lago", "della", "dove", "sanno")) {
            w.type(keys, typed)
            assertFalse(typed + " -> " + w.slots(), w.slots().any { '\'' in it })
        }
    }

    // The books' spellings these come from are run together too ("alleno", "sensei"), but the word after
    // them is no likelier than the spelling, so they are left alone.
    @Test fun booksWordsStayWithoutAnElision() {
        val it = word("it"); val itKeys = layout("it")
        for ((typed, fake) in listOf("alleno" to "all'eno", "alludi" to "all'udi", "allodio" to "all'odio",
                "santita" to "sant'ita", "cavita" to "c'avita")) it.assertNoneOf(itKeys, typed, fake)
        val fr = word("fr"); val frKeys = layout("fr")
        for ((typed, fake) in listOf("sensei" to "s'ensei", "couvres" to "c'ouvres", "terrance" to "t'errance",
                "quantico" to "qu'antico", "dude" to "d'ude")) fr.assertNoneOf(frKeys, typed, fake)
    }

    // Accentless spellings the decoder already reads as one accented word are no elisions.
    @Test fun anAccentlessWordTheDecoderReadsGetsNoElision() {
        val it = word("it"); val keys = layout("it")
        for ((typed, fake) in listOf("diro" to "d'iro", "daro" to "d'aro", "sapra" to "s'apra",
                "terro" to "t'erro", "mangera" to "m'angera")) it.assertNoneOf(keys, typed, fake)
    }

    // A remainder has to be a reasonably common word of the model: filler and rare tokens are no elision.
    @Test fun junkRemaindersAreNoElision() {
        val it = word("it"); val itKeys = layout("it")
        for ((typed, fake) in listOf("lol" to "l'ol", "mom" to "m'om", "taco" to "t'aco", "tae" to "t'ae",
                "shy" to "s'hy", "chun" to "c'hun", "chopper" to "c'hopper", "shack" to "s'hack"))
            it.assertNoneOf(itKeys, typed, fake)
        val fr = word("fr"); val frKeys = layout("fr")
        for ((typed, fake) in listOf("nope" to "n'ope", "shh" to "s'hh", "calin" to "c'alin", "lache" to "l'ache",
                "noh" to "n'oh", "dok" to "d'ok")) fr.assertNoneOf(frKeys, typed, fake)
    }

    // A split fix is only the elision's own two words ("l ho" for "l'ho"), never another reading of the
    // same letters: "nonce" (fix "non c'è") gets no "n'once", and "lae" (fix "la e") no "l'ae".
    @Test fun aSplitFixOffersOnlyItsOwnElision() {
        val keys = layout("it")
        val w = word("it")
        w.type(keys, "nonce")
        assertFalse("nonce -> " + w.strip(), "n'once" in w.slots())
        assertTrue("nonce -> " + w.strip(), "non c'è" in w.slots())
        w.type(keys, "nonche")
        assertTrue("nonche -> " + w.strip(), "nonché" in w.slots())
        w.type(keys, "lae")
        assertFalse("lae -> " + w.strip(), "l'ae" in w.slots())
        for ((typed, offered) in listOf("lho" to "l'ho", "lha" to "l'ha", "lhai" to "l'hai", "lhanno" to "l'hanno")) {
            w.type(keys, typed)
            assertTrue(typed + " -> " + w.strip(), offered in w.slots())
        }
    }

    // Italian elides the h of the avere forms alone; French elides its h generally.
    @Test fun italianHTakesOnlyTheAvereForms() {
        val keys = layout("it")
        val w = word("it")
        w.assertNoneOf(keys, "lhotel", "l'hotel")
        for ((typed, offered) in listOf("lho" to "l'ho", "lhai" to "l'hai", "lha" to "l'ha", "lhanno" to "l'hanno")) {
            w.type(keys, typed)
            assertTrue(typed + " -> " + w.slots(), offered in w.slots())
        }
        val fr = word("fr")
        fr.type(layout("fr"), "lheure")
        assertTrue("l'heure" in fr.slots())
    }

    @Test fun englishKeepsItsContractionsAndGetsNoElisionRule() {
        val keys = layout("en")
        val w = word("en")
        w.type(keys, "im")
        assertTrue("im -> " + w.slots(), "I'm" in w.slots())
        w.type(keys, "dont")
        assertTrue("dont -> " + w.slots(), "don't" in w.slots())
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
