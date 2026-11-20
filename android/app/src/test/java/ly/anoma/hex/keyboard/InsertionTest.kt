package ly.anoma.hex.keyboard

import org.junit.Assert.assertEquals
import org.junit.Test
import ly.anoma.hex.speech.Spoken
import ly.anoma.hex.speech.SpokenCleanup
import ly.anoma.hex.speech.SpokenText
import ly.anoma.hex.speech.SpeechOptions
import ly.anoma.hex.speech.Style

class InsertionTest {
    @Test fun replacesASelectedWordWithoutJoiningItsNeighbours() {
        assertEquals(" hello ", insertion(" hello ", 'a', 'b'))
    }
    @Test fun respectsWhitespaceAndClosingPunctuationInTheEditor() {
        assertEquals("Hello", insertion("  Hello ", '\n', '!'))
        assertEquals("hello", insertion("hello", '(', ')'))
        assertEquals(", yes", insertion(", yes", 'e', null))
    }
    @Test fun emptyResultMakesNoEdit() {
        assertEquals("", insertion(" \n ", 'a', 'b'))
    }

    @Test fun stylesChangeWhatReachesTheEditor() {
        fun render(text: String, style: Style) = insertion(
            SpokenText.format(text, SpeechOptions(style, Spoken.English, true, false)), null, null)
        assertEquals("Can you send it Thursday.", render("Can you, um, send it Tuesday, no wait, Thursday.", Style.Polished))
        assertEquals("see you soon", render("Um, see you soon.", Style.Casual))
        assertEquals("- Buy milk\n- Call Sam", render("bullet buy milk next bullet call Sam", Style.Notes))
        assertEquals("Um, Tuesday, sorry, Thursday.", render("Um, Tuesday, sorry, Thursday.", Style.Verbatim))
        assertEquals("Das ist eh egal.", SpokenText.format("Äh, das ist eh egal.",
            SpeechOptions(Style.Polished, Spoken.German, false, false)))
    }
    @Test fun removesOnlyAnchoredRepairsAndPreservesNegation() {
        assertEquals("Meet at 4pm?", SpokenCleanup.clean("Meet at 3pm, actually 4pm?", "en"))
        assertEquals("I didn't say Friday.", SpokenCleanup.clean("I didn't say Friday.", "en"))
        assertEquals("I want the red one, sorry, eventually.", SpokenCleanup.clean("I want the red one, sorry, eventually.", "en"))
        assertEquals("I sent the invoice, sorry I am late.", SpokenCleanup.clean("I sent the invoice, sorry I am late.", "en"))
        assertEquals("It isn’t blue, I mean, it is green.", SpokenCleanup.clean("It isn’t blue, I mean, it is green.", "en"))
        assertEquals("Uno\ndos", SpokenCleanup.clean("Uno\ndos", "es"))
        assertEquals("eBay works.", SpokenCleanup.clean("Um, eBay works.", "en"))
        assertEquals("Te digo que no puedo ir.", SpokenCleanup.clean("Te digo que no puedo ir.", "es"))
        assertEquals("¿El miércoles?", SpokenCleanup.clean("¿El martes, perdón, el miércoles?", "es"))
        assertEquals("Am Mittwoch.", SpokenCleanup.clean("Am Dienstag, ich meine, Mittwoch.", "de"))
        assertEquals("Det er torsdag.", SpokenCleanup.clean("Det er tirsdag, nei, torsdag.", "no"))
    }
}
