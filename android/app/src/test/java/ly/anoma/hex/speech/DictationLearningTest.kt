package ly.anoma.hex.speech

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DictationLearningTest {
    private fun pair(heard: String, inserted: String, meant: String) = DictationEditPair(heard, inserted, meant)

    private fun edited(vararg pairs: DictationEditPair, kept: List<List<String>> = emptyList(), words: Int = 5) =
        DictationEdit(DictationOutcome.Edited, pairs.toList(), words, pairs.size, kept)

    // Inference: iPhone Hex's DictationEditInference, dictation side only (no restored or filler pairs).
    @Test fun unchangedTextIsKept() {
        val kept = DictationEdits.classify("see you at noon", "See you at noon.", "See you at noon.")
        assertEquals(DictationOutcome.Kept, kept?.outcome)
        assertTrue(kept!!.pairs.isEmpty())
        // New writing after the dictation isn't a fix of it.
        val continued = DictationEdits.classify(null, "See you at noon.", "See you at noon. Bring snacks")
        assertEquals(DictationOutcome.Kept, continued?.outcome)
        assertTrue(continued!!.pairs.isEmpty())
    }

    @Test fun aFixedWordBecomesAPair() {
        val result = DictationEdits.classify("see you at cloud nine", "See you at cloud nine.", "See you at Claude nine.")
        assertEquals(DictationOutcome.Edited, result?.outcome)
        assertEquals(listOf(pair("cloud", "cloud", "Claude")), result!!.pairs)
        assertEquals(listOf(listOf("see", "you", "at"), listOf("nine")), result.keptHeard)
    }

    @Test fun changingMoreThanHalfIsARewriteWithoutPairs() {
        val result = DictationEdits.classify(null, "I will be there at noon", "Sorry I can't make it today")
        assertEquals(DictationOutcome.Rewritten, result?.outcome)
        assertTrue(result!!.pairs.isEmpty())
    }

    @Test fun differentWordsAreAnEditButNotAPair() {
        val result = DictationEdits.classify(null, "meet on Tuesday at noon", "meet on Friday at noon")
        assertEquals(DictationOutcome.Edited, result?.outcome)
        assertTrue(result!!.pairs.isEmpty())
    }

    @Test fun plausibilityIsALettersOnlyLevenshteinOfHalfTheWord() {
        assertTrue(DictationEdits.plausible("cloud", "Claude"))
        assertTrue(DictationEdits.plausible("teh", "the"))
        assertTrue(DictationEdits.plausible("a", "an"))
        assertTrue(DictationEdits.plausible("is", "this"))
        assertTrue(!DictationEdits.plausible("Tuesday", "Friday"))
    }

    @Test fun aHunkOfMoreThanThreeWordsIsNoPair() {
        val result = DictationEdits.classify(null, "one two three four five six seven eight nine ten",
            "one two three four alpha beta gamma delta nine ten")
        assertEquals(DictationOutcome.Edited, result?.outcome)
        assertTrue(result!!.pairs.isEmpty())
    }

    @Test fun aSentenceStartCapitalIsNotTheSpelling() {
        val result = DictationEdits.classify("their car is here", "Their car is here.", "There car is here.")
        assertEquals(pair("their", "Their", "there"), result?.pairs?.first())
    }

    @Test fun aSingleWordDictationCanBeFixed() {
        val result = DictationEdits.classify("cloud", "cloud", "Claude")
        assertEquals(DictationOutcome.Edited, result?.outcome)
        assertEquals("Claude", result!!.pairs.first().meant)
    }

    @Test fun dictatingAgainPairsTheWordsThatChanged() {
        val same = DictationEdits.redictation("meet me at cloud nine", "Meet me at cloud nine.", "Meet me at Claude nine.")
        assertEquals(DictationOutcome.Redictated, same?.outcome)
        assertEquals(listOf("cloud"), same!!.pairs.map { it.heard })
        assertEquals(listOf("Claude"), same.pairs.map { it.meant })
        val different = DictationEdits.redictation(null, "Meet me at noon.", "Actually let's skip lunch today.")
        assertEquals(DictationOutcome.Redictated, different?.outcome)
        assertTrue(different!!.pairs.isEmpty())
    }

    @Test fun aSpanThatIsTooLongOrEmptyIsNotJudged() {
        assertNull(DictationEdits.classify(null, "", "anything"))
        assertNull(DictationEdits.classify(null, (1..401).joinToString(" ") { "word" + it }, "word"))
    }

    // The rules: applied only after two sightings, never after a contradiction, and only one meaning.
    private fun record(heard: String, meant: String, count: Int = 1, contradictions: Int = 0, at: Long = 0) =
        DictationFix(heard, meant, count, contradictions, at)

    @Test fun aFixIsLearnedOnlyAfterTwoSightingsAndNoContradiction() {
        assertTrue(DictationFixes.learned(listOf(record("cloud", "Claude"))).isEmpty())
        val twice = DictationFixes.learned(listOf(record("cloud", "Claude"), record("Cloud", "Claude", at = 1)))
        assertEquals(1, twice.size)
        assertEquals("Claude", twice.first().meant)
        assertEquals(2, twice.first().count)
        assertTrue(DictationFixes.learned(listOf(record("cloud", "Claude", count = 5, contradictions = 1))).isEmpty())
        assertTrue(DictationFixes.learned(listOf(record("cloud", "Claude", count = 3), record("cloud", "crowd"))).isEmpty())
    }

    @Test fun aCapitalYouUsedIsKeptAsTheSpelling() {
        val learned = DictationFixes.learned(listOf(record("cloud", "claude"), record("cloud", "Claude")))
        assertEquals("Claude", learned.single().meant)
    }

    @Test fun aFixSwitchedOffInSettingsIsNotApplied() {
        val fix = record("cloud", "Claude", count = 2)
        assertEquals(listOf(fix), DictationFixes.active(listOf(fix), emptySet()))
        assertTrue(DictationFixes.active(listOf(fix), setOf(fix.id)).isEmpty())
    }

    @Test fun mergingCountsPairsAndContradictsFixesYouKeptOrTookBack() {
        val edit = edited(pair("cloud", "cloud", "Claude"))
        var records = DictationFixes.merge(emptyList(), edit, 1)
        records = DictationFixes.merge(records, edit, 2)
        assertEquals(1, records.size)
        assertEquals(2, records.first().count)
        assertEquals(1, DictationFixes.learned(records).size)

        // Hex applied it and you changed it back.
        val reverted = DictationFixes.merge(records, edited(pair("cloud", "Claude", "cloud")), 3)
        assertEquals(1, reverted.first().contradictions)
        assertTrue(DictationFixes.learned(reverted).isEmpty())

        // You fixed it the other way round.
        val reversed = DictationFixes.merge(records, edited(pair("Claude", "Claude", "cloud")), 4)
        assertEquals(1, reversed.first().contradictions)

        // You corrected a dictation and left the heard words alone in it.
        val kept = DictationFixes.merge(records, edited(kept = listOf(listOf("the", "cloud", "is", "grey"))), 5)
        assertEquals(1, kept.first().contradictions)
    }

    @Test fun theStoreKeepsAtMostAThousandRecordsAndDropsTheLeastUsed() {
        val pairs = (0..1000).map { pair("heard" + it, "heard" + it, "meant" + it) }
        val merged = DictationFixes.merge(emptyList(), DictationEdit(DictationOutcome.Edited, pairs, 1001), 7)
        assertEquals(DictationFixes.maxRecords, merged.size)
        assertTrue(merged.none { it.heard == "heard0" })
        assertTrue(merged.any { it.heard == "heard1000" })
    }

    // Applying fixes: whole words and phrases, never inside a longer token, following the original's case.
    private val cloud = DictationFix("cloud", "Claude", 2, 0)

    @Test fun fixesApplyToWholeWordsOnlyAndFollowCase() {
        val text = DictationFixes.apply(listOf(cloud), "cloud is great. Cloud rocks, cloudy day, cloud-based, CLOUD")
        assertEquals("Claude is great. Claude rocks, cloudy day, cloud-based, CLAUDE", text)
        assertEquals("There car and there dog, THERE cat",
            DictationFixes.apply(listOf(DictationFix("their", "there", 2, 0)), "Their car and their dog, THEIR cat"))
    }

    @Test fun fixesNeverTouchAddressesPathsOrTags() {
        val text = "cloud.com /cloud/ @cloud #cloud cloud_name"
        assertEquals(text, DictationFixes.apply(listOf(cloud), text))
        assertEquals("I love Claude.", DictationFixes.apply(listOf(cloud), "I love cloud."))
    }

    @Test fun fixesRunBeforeTheCustomVocabularyInFormatting() {
        val fixes = listOf(DictationFix("cloud", "Claude", 2, 0))
        val options = SpeechOptions(Style.Polished, DictationLanguage.English, false, false,
            vocabulary = listOf("OpenCode"), fixes = fixes)
        assertEquals("Claude ships with OpenCode.", SpokenText.format("cloud ships with open code.", options))
    }

    @Test fun phraseFixesNeedPlainSpacesBetweenWords() {
        val fix = DictationFix("open code", "OpenCode", 2, 0)
        assertEquals("i use OpenCode daily, not open, code",
            DictationFixes.apply(listOf(fix), "i use open code daily, not open, code"))
    }

    @Test fun hintsPutVocabularyFirstWithoutDuplicates() {
        val fixes = listOf(DictationFix("cloud", "Claude", 2, 0), DictationFix("hacks", "Hex", 2, 0))
        assertEquals(listOf("OpenCode", "Claude", "Hex"),
            DictationFixes.hints(listOf("OpenCode", "Claude"), fixes))
        assertEquals(listOf("Claude", "Hex"), DictationFixes.hints(emptyList(), fixes))
        assertEquals(listOf("OpenCode"), DictationFixes.hints(listOf("OpenCode"), emptyList()))
        assertEquals(listOf("Hex"), DictationFixes.hints(listOf("H"), listOf(DictationFix("hacks", "Hex", 2, 0))))
    }

    @Test fun hintsAreCappedAtTheVocabularyLimit() {
        val terms = (0 until 120).map { "term" + it }
        val hints = DictationFixes.hints(terms, listOf(DictationFix("cloud", "Claude", 2, 0)))
        assertEquals(VocabularySpelling.maxTermCount, hints.size)
        assertEquals("term0", hints.first())
        assertEquals("term98", hints[hints.lastIndex - 1])
    }

    @Test fun persistenceRoundTripsAwkwardWordsThroughOneLinePerFix() {
        val records = listOf(
            DictationFix("a\tb", "line\nbreak\\x", 2, 1, 123),
            DictationFix("open code", "OpenCode", 7, 0, 456),
        )
        assertEquals(records.map(::fields), DictationFixes.decode(DictationFixes.encode(records)).map(::fields))
        assertTrue(DictationFixes.decode("garbage").isEmpty())
    }

    private fun fields(fix: DictationFix) = listOf(fix.heard, fix.meant, fix.count.toString(), fix.contradictions.toString(),
        fix.lastAt.toString()).joinToString("|")
}
