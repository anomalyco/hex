package ly.anoma.hex.typing

import org.junit.Assert.assertEquals
import org.junit.Test

class SmartQuotesTest {
    @Test fun anApostropheInsideAContractionIsSmart() {
        assertEquals("don’t", SmartQuotes.text("don't", null))
        assertEquals("I’m", SmartQuotes.text("I'm", ' '))
        assertEquals('’', SmartQuotes.typed('\'', 'n'))
    }

    @Test fun possessivesKeepTheirApostrophe() {
        assertEquals("Sam’s", SmartQuotes.text("Sam's", '('))
        assertEquals('’', SmartQuotes.typed('\'', 's'))
        // A number's feet mark is an apostrophe too, as iOS writes 5'10".
        assertEquals('’', SmartQuotes.typed('\'', '5'))
    }

    @Test fun aQuoteOpensAtTheStartAndAfterSpace() {
        assertEquals('‘', SmartQuotes.typed('\'', null))
        assertEquals('‘', SmartQuotes.typed('\'', ' '))
        assertEquals('“', SmartQuotes.typed('"', null))
        assertEquals('“', SmartQuotes.typed('"', '\n'))
    }

    @Test fun aQuoteAfterBracketsOpens() {
        assertEquals('“', SmartQuotes.typed('"', '('))
        assertEquals('“', SmartQuotes.typed('"', '['))
        assertEquals('‘', SmartQuotes.typed('\'', '{'))
        assertEquals('‘', SmartQuotes.typed('\'', '“'))
        assertEquals("(“this”)", SmartQuotes.text("(\"this\")", null))
    }

    @Test fun aQuoteAfterAWordCloses() {
        assertEquals('”', SmartQuotes.typed('"', 't'))
        assertEquals('”', SmartQuotes.typed('"', '.'))
        assertEquals('’', SmartQuotes.typed('\'', '!'))
        assertEquals('”', SmartQuotes.typed('"', '”'))
    }

    @Test fun quotePairsBecomeOpeningAndClosing() {
        assertEquals("“this”", SmartQuotes.text("\"this\"", null))
        assertEquals("He said “this”.", SmartQuotes.text("He said \"this\".", null))
        assertEquals("‘this’", SmartQuotes.text("'this'", null))
        assertEquals("‘n’", SmartQuotes.text("'n'", ' '))
    }

    @Test fun nestedQuotesNest() {
        assertEquals("He said “it’s ‘fine’”", SmartQuotes.text("He said \"it's 'fine'\"", null))
        assertEquals("\"don’t\"", SmartQuotes.word("\"don't\""))
    }

    @Test fun quotesAfterTheSpacingADictationAddsStillOpen() {
        // Dictation spaces its text before the quotes are converted, so the leading space is what the
        // first quote follows.
        assertEquals(" “this”", SmartQuotes.text(" \"this\"", 'a'))
        assertEquals("\n“this”", SmartQuotes.text("\n\"this\"", 'a'))
        assertEquals(" don’t", SmartQuotes.text(" don't", 'a'))
    }

    @Test fun aWholeWordGetsOnlyApostrophes() {
        assertEquals("don’t", SmartQuotes.word("don't"))
        assertEquals("’em", SmartQuotes.word("'em"))
        assertEquals("O’Brien", SmartQuotes.word("O'Brien"))
        assertEquals("hello", SmartQuotes.word("hello"))
        // A picked suggestion is mapped again as it goes in: a curly apostrophe stays as it is.
        assertEquals("don’t", SmartQuotes.word(SmartQuotes.word("don't")))
    }

    @Test fun emptyTextStaysEmpty() {
        assertEquals("", SmartQuotes.text("", null))
        assertEquals("", SmartQuotes.text("", 'a'))
        assertEquals("", SmartQuotes.word(""))
    }

    @Test fun alreadyCurlyTextIsUnchanged() {
        val curly = "He said “it’s ‘fine’”"
        assertEquals(curly, SmartQuotes.text(curly, null))
        assertEquals(curly, SmartQuotes.text(curly, ' '))
        assertEquals('’', SmartQuotes.typed('’', 'a'))
        assertEquals('“', SmartQuotes.typed('“', 'a'))
    }

    @Test fun anythingElseIsLeftAlone() {
        assertEquals('d', SmartQuotes.typed('d', 'n'))
        assertEquals("hello", SmartQuotes.text("hello", null))
    }
}
