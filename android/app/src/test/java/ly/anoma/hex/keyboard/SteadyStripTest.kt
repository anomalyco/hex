package ly.anoma.hex.keyboard

import ly.anoma.hex.typing.Suggestion
import ly.anoma.hex.typing.Suggestion.Kind
import org.junit.Assert.assertEquals
import org.junit.Test

class SteadyStripTest {
    private fun typed(s: String) = Suggestion(s, Kind.Typed)
    private fun fix(s: String) = Suggestion(s, Kind.Fix)
    private fun word(s: String) = Suggestion(s, Kind.Word)
    private fun texts(slots: List<Suggestion?>) = slots.map { it?.text }

    @Test fun anUnknownWordIsLaidOutAsAfterThePause() {
        // Before the pause: typed on the left, the likeliest completion on the right, where the fix won't take it.
        val strip = SteadyStrip()
        val early = strip.arrange("gon", listOf(word("gonna"), word("gone"), word("going")), typed("gon"), early = true)
        assertEquals(listOf("gon", "gone", "gonna"), texts(early))
        // The pause brings the fix to the middle; "gonna" stays where it was.
        val late = strip.arrange("gon", listOf(typed("gon"), fix("going"), word("gonna")))
        assertEquals(listOf("gon", "going", "gonna"), texts(late))
        assertEquals(Kind.Fix, late[1]?.kind)
    }

    @Test fun aShownWordKeepsItsSlotWhileTheSameWordIsTyped() {
        val strip = SteadyStrip()
        strip.arrange("gon", listOf(typed("gon"), fix("gone"), word("gonna")))
        // "gonn": the model now ranks "gonna" first, which on its own would take the middle.
        val next = strip.arrange("gonn", listOf(typed("gonn"), word("gonna"), word("gonne")))
        assertEquals(listOf("gonn", "gonne", "gonna"), texts(next))
    }

    @Test fun anotherWordStartsAfresh() {
        val strip = SteadyStrip()
        strip.arrange("gon", listOf(typed("gon"), fix("gone"), word("gonna")))
        assertEquals(listOf("th", "then", "gone"),
            texts(strip.arrange("th", listOf(typed("th"), word("then"), word("gone")))))
        strip.forget()
        // After a forget, even a word that starts the same is laid out as the model places it.
        strip.arrange("go", listOf(typed("go"), word("good"), word("gone")))
        strip.forget()
        assertEquals(listOf("good", "gone", "going"),
            texts(strip.arrange("gon", listOf(word("gone"), word("good"), word("going")))))
    }

    @Test fun theEmojiKeepsTheLastSlot() {
        val strip = SteadyStrip()
        val emoji = Suggestion("\uD83D\uDE0A", Kind.Emoji)
        val early = strip.arrange("smil", listOf(word("smile"), word("smiling"), emoji), typed("smil"), early = true)
        assertEquals(listOf("smil", "smile", emoji.text), texts(early))
        val late = strip.arrange("smile", listOf(typed("smile"), word("smiled"), emoji))
        assertEquals(listOf("smile", "smiled", emoji.text), texts(late))
    }

    @Test fun aPauseWithoutAFixKeepsWhatWasTypedOnTheLeft() {
        // "hel" has no fix or near word, so the pause's strip has no typed slot of its own: it still keeps
        // what was typed on the left, where the strip before the pause showed it, and the next letter agrees.
        val strip = SteadyStrip()
        val early = strip.arrange("hel", listOf(word("hello"), word("help"), word("held")), typed("hel"), early = true)
        assertEquals(listOf("hel", "help", "hello"), texts(early))
        val late = strip.arrange("hel", listOf(word("hello"), word("help"), word("held")), typed("hel"))
        assertEquals(listOf("hel", "help", "hello"), texts(late))
        assertEquals(Kind.Typed, late[0]?.kind)
        val next = strip.arrange("help", listOf(word("helpful"), word("helped"), word("helping")), typed("help"), early = true)
        assertEquals(Kind.Typed, next[0]?.kind)
        assertEquals(listOf("help", "helped", "helpful"), texts(next))
    }

    @Test fun withoutAnEarlyTypedSlotThePauseIsLaidOutAsTheModelPlacesIt() {
        // A field with no pause strip never shows what was typed for an unknown word.
        val strip = SteadyStrip()
        assertEquals(listOf("help", "hello", "held"),
            texts(strip.arrange("hel", listOf(word("hello"), word("help"), word("held")))))
        assertEquals(listOf("help", "hello", "held"),
            texts(strip.arrange("hel", listOf(word("hello"), word("help"), word("held")), typed("hel"))))
    }

    @Test fun anEmptyOrEmojiOnlyStripGetsNoTypedSlot() {
        // A word over the model's length has no strip: it stays empty before and after the pause.
        val strip = SteadyStrip()
        val long = "a".repeat(33)
        assertEquals(listOf(null, null, null), texts(strip.arrange(long, emptyList(), typed(long), early = true)))
        assertEquals(listOf(null, null, null), texts(strip.arrange(long, emptyList(), typed(long))))
        val emoji = Suggestion("\uD83D\uDE0A", Kind.Emoji)
        val other = SteadyStrip()
        assertEquals(listOf(null, null, emoji.text), texts(other.arrange("smil", listOf(emoji), typed("smil"), early = true)))
        assertEquals(listOf(null, null, emoji.text), texts(other.arrange("smil", listOf(emoji), typed("smil"))))
    }

    @Test fun aKnownWordIsPlacedAsTheModelOrdersIt() {
        val strip = SteadyStrip()
        assertEquals(listOf("go", "good", "going"),
            texts(strip.arrange("go", listOf(typed("go"), word("good"), word("going")), typed("go"), early = true)))
    }
}
