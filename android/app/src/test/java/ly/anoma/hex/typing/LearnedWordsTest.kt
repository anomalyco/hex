package ly.anoma.hex.typing

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LearnedWordsTest {
    private val known = setOf("the", "github", "don't")
    private val english: (Char) -> Boolean = { it == '\'' || it in 'a'..'z' }
    private val german: (Char) -> Boolean = { it in 'a'..'z' || it in "äöüß" }
    private fun candidate(typed: String, before: String = "I use ", spells: (Char) -> Boolean = english) =
        LearnedWords.candidate(typed, before, spells, knows = { it.lowercase() in known })

    @Test fun keepsASpellingNoDictionaryKnows() {
        assertEquals("tailscale", candidate("tailscale"))
        assertEquals("TestFlight", candidate("TestFlight"))
        assertEquals("o'neill", candidate("o\u2019neill"))
    }

    @Test fun leavesKnownWordsAlone() {
        assertNull(candidate("the"))
        assertNull(candidate("GitHub"))
        assertNull(candidate("don\u2019t"))
    }

    @Test fun keepsOnlyLettersAndApostrophesOfAReadableLength() {
        assertNull(candidate("x"))
        assertNull(candidate("a".repeat(TypingWord.MAX_LENGTH + 1)))
        assertEquals("a".repeat(TypingWord.MAX_LENGTH), candidate("a".repeat(TypingWord.MAX_LENGTH)))
        assertNull(candidate("abc123"))
        assertNull(candidate("wi-fi"))
        assertNull(candidate("'tis"))
        assertNull(candidate("users'"))
        assertNull(candidate("James\u2019"))
    }

    @Test fun dropsACapitalOnlyTheSentenceStartGave() {
        assertEquals("tailscale", candidate("Tailscale", ""))
        assertEquals("tailscale", candidate("Tailscale", "It works. "))
        assertEquals("Tailscale", candidate("Tailscale", "I use "))
        assertEquals("TestFlight", candidate("TestFlight", ""))
    }

    @Test fun keepsOnlyWhatTheModelsAlphabetReadsBack() {
        assertNull(candidate("café"))
        assertNull(candidate("Zürich", "I live in "))
        assertEquals("Zürich", candidate("Zürich", "Ich wohne in ", german))
        assertNull(candidate("o'neill", spells = german))
    }

    @Test fun neverLearnsASpellingOnTheNeverSuggestList() {
        val listed = setOf("slurword")
        assertNull(LearnedWords.candidate("Slurword", "I use ", english, { false }, { it in listed }))
        assertEquals("tailscale", LearnedWords.candidate("tailscale", "I use ", english, { false }, { it in listed }))
    }

    // An undone autocorrection of "tailscale" leaves "I use tailscale" before the cursor.
    private fun undone() = PendingKeep().apply { undone("tailscale", "tailscale", "en", "I use ", "I use tailscale") }

    @Test fun anUndoneWordEndedUnchangedIsKept() {
        val pending = undone()
        assertEquals(PendingKeep.Kept("tailscale", "en", "I use "), pending.ended("I use tailscale", "tailscale", "en"))
        // Once ended, nothing waits: a second space keeps nothing.
        assertNull(pending.ended("I use tailscale", "tailscale", "en"))
    }

    @Test fun aDeleteOrLetterAfterTheUndoKeepsNothing() {
        // Holding Delete (the first press undoes, the next drops it), or a letter typed and deleted again, which leaves
        // the same text: the engine drops it on either key.
        assertNull(undone().apply { drop() }.ended("I use tailscale", "tailscale", "en"))
        // The word or the text before the cursor changed.
        assertNull(undone().ended("I use tailscalex", "tailscalex", "en"))
        assertNull(undone().ended("We use tailscale", "tailscale", "en"))
        assertNull(undone().ended("I use tailscale", "tailscale", "de"))
    }

    @Test fun neitherOfTwoJoinedWordsIsKept() {
        val pending = PendingKeep().apply { undone("tail scale", "scale", "en", "I use tail ", "I use tail scale") }
        assertNull(pending.ended("I use tail scale", "scale", "en"))
    }

    @Test fun learnsOnlyWithTheLabOnInAFieldThatLearns() {
        assertTrue(LearnedWords.allowed(lab = true, secure = false, exact = false, keyEvents = false, learns = true, standIn = false))
        assertFalse(LearnedWords.allowed(lab = false, secure = false, exact = false, keyEvents = false, learns = true, standIn = false))
        assertFalse(LearnedWords.allowed(lab = true, secure = true, exact = false, keyEvents = false, learns = true, standIn = false))
        assertFalse(LearnedWords.allowed(lab = true, secure = false, exact = true, keyEvents = false, learns = true, standIn = false))
        assertFalse(LearnedWords.allowed(lab = true, secure = false, exact = false, keyEvents = true, learns = true, standIn = false))
        assertFalse(LearnedWords.allowed(lab = true, secure = false, exact = false, keyEvents = false, learns = false, standIn = false))
        assertFalse(LearnedWords.allowed(lab = true, secure = false, exact = false, keyEvents = false, learns = true, standIn = true))
    }
}
