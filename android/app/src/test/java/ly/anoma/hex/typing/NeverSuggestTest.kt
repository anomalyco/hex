package ly.anoma.hex.typing

import java.io.File
import org.junit.Assert.*
import org.junit.Test

class NeverSuggestTest {
    private fun word(language: String = "en", spatial: Boolean = true): TypingWord {
        val file = File("src/main/assets/typing/$language.hextm")
        return TypingWord(SpatialDecoder(TypingModel.file(file, language)), spatial)
    }

    private val grid = KeyGrid(buildMap {
        for ((row, letters) in listOf("qwertyuiop", "asdfghjkl", "zxcvbnm").withIndex())
            for ((i, c) in letters.withIndex()) put(c, KeyPoint(201 + (i - (letters.length - 1) / 2.0) * 39.5, 66.5 + 56 * row))
    }, 39.5, 56.0)
    private fun TypingWord.type(hit: String, aimed: String = hit) {
        require(hit.length == aimed.length)
        for ((i, c) in hit.withIndex()) {
            val p = grid.centres.getValue(aimed[i].lowercaseChar())
            letter(c.toString(), p.x, p.y, grid)
        }
    }

    // The benchmark's 402 pt layout with its space bar, per language (benchmark/typing/Replay.java).
    private fun layout(language: String): KeyGrid {
        val rows = when (language) {
            "de" -> listOf("qwertzuiopü", "asdfghjklöä", "yxcvbnm")
            "es" -> listOf("qwertyuiop", "asdfghjklñ", "zxcvbnm")
            else -> listOf("qwertyuiop", "asdfghjkl", "zxcvbnm")
        }
        val pitch = if (language == "de") 36.0 else 39.5
        return KeyGrid(buildMap {
            for ((row, letters) in rows.withIndex()) for ((i, c) in letters.withIndex())
                put(c, KeyPoint(201 + (i - (letters.length - 1) / 2.0) * pitch, 66.5 + 56 * row))
        }, pitch, 56.0, KeyRect(64.76, 212.0, 303.1, 257.0))
    }
    private fun TypingWord.tap(grid: KeyGrid, c: Char, dx: Double = 0.0, dy: Double = 0.0) {
        val p = grid.centres.getValue(c)
        letter(c.toString(), p.x + dx, p.y + dy, grid)
    }

    @Test fun completionsRespectBothCategoriesInEveryShippedModel() {
        for ((lang, swear, slur) in listOf(
            Triple("en", "fuck", "nigger"), Triple("es", "mierda", "maricón"),
            Triple("de", "scheiße", "neger"), Triple("nb", "faen", "neger")
        )) {
            val w = word(lang, spatial = false)
            assertTrue("$lang still knows the slur", w.decoder.model.index(slur) >= 0)
            w.explicit(swear.dropLast(1))
            assertFalse("$lang default", swear in w.suggestions())
            w.suggestSwearWords = true
            assertTrue("$lang opt-in", swear in w.suggestions())
            w.suggestSwearWords = false
            assertFalse("$lang cached prefix after opt-out", swear in w.suggestions())
            w.reset(); w.explicit(slur.dropLast(1))
            for (on in listOf(false, true)) {
                w.suggestSwearWords = on
                assertFalse("$lang slur, opt-in=$on", slur in w.suggestions())
            }
        }
    }

    @Test fun nextWordRowsSkipBlockedTargetsAndBackfill() {
        val nb = word("nb")
        assertEquals(3, nb.predictions("fy ").size)
        assertFalse("faen" in nb.predictions("fy "))
        nb.suggestSwearWords = true
        assertEquals("faen", nb.predictions("fy ").first())
        val en = word()
        for (on in listOf(false, true)) {
            en.suggestSwearWords = on
            val predictions = en.predictions("mentally ")
            assertEquals(3, predictions.size)
            assertTrue("ill" in predictions)
            assertFalse("retarded" in predictions)
        }
        // A blocked word that the user typed is still useful as language-model context.
        assertEquals("you", en.predictions("fuck ").first())
    }

    @Test fun spaceAndSpatialSuggestionsFollowLiveOptInButSlursNeverReturn() {
        val w = word()
        w.type("fukc")
        for (on in listOf(false, true, false)) {
            w.suggestSwearWords = on
            assertEquals(on, "fuck" in w.suggestions())
            assertEquals(on, w.replacement() == "fuck")
        }
        w.reset(); w.type("faggto")
        for (on in listOf(false, true)) {
            w.suggestSwearWords = on
            assertFalse("faggot" in w.suggestions())
            assertNotEquals("faggot", w.replacement())
        }
        w.reset(); w.type("teh")
        assertEquals("the", w.replacement())
    }

    @Test fun fullBlockedSpellingsKeepTheirCaseEvenWithStrongEvidenceForAnotherWord() {
        for ((literal, aimed) in listOf("Fuck" to "duck", "FUCK" to "duck", "faggot" to "forgot", "fucktard" to "backward")) {
            val w = word()
            w.type(literal, aimed)
            assertNull(literal, w.replacement())
            val options = w.suggestions()
            assertFalse(literal, options.any { it.equals(literal, ignoreCase = true) })
            assertEquals(literal, w.literalChip(options))
            assertEquals(literal, w.token)
            if (literal == "fucktard") assertEquals(-1, w.decoder.model.index(literal))
        }
    }

    @Test fun everyListedLiteralIsKeptSeparatelyFromRankedSuggestions() {
        val rows = File("src/main/assets/typing/never-suggest.tsv").readLines()
            .filterNot { it.isBlank() || it.startsWith('#') }.map { it.split('\t') }
        for (lang in listOf("en", "es", "de", "nb")) {
            val w = word(lang)
            for ((layout, kind, entry) in rows) {
                if (layout != "*" && layout != lang) continue
                val before = entry.substringBefore(' ', "")
                val token = entry.substringAfter(' ').replaceFirstChar { it.uppercase() }
                for (on in listOf(false, true)) {
                    if (kind == "swear" && on) continue
                    w.suggestSwearWords = on; w.reset(before); w.explicit(token)
                    val ranked = w.suggestions()
                    assertFalse("$lang:$entry", ranked.any { it.equals(token, ignoreCase = true) })
                    assertEquals("$lang:$entry", token, w.literalChip(ranked))
                }
            }
        }
    }

    @Test fun noBlockedHalfOfASpatialSplitCanBeOfferedOrCommitted() {
        val w = word()
        for ((miss, aimed, split) in listOf(Triple("fukkyou", "fuckyou", "fuck you"),
                                            Triple("ohfukk", "ohfuck", "oh fuck"))) {
            w.reset(); w.type(miss, aimed)
            w.suggestSwearWords = true
            assertTrue("$miss opt-in can reach $split", w.replacement() == split || split in w.suggestions())
            w.suggestSwearWords = false
            assertFalse("$miss split chip", w.suggestions().any { "fuck" in it.split(' ') })
            assertFalse("$miss split commit", w.replacement()?.split(' ')?.contains("fuck") == true)
        }
    }

    @Test fun restrictedExpressionsNeverBanTheirOrdinaryComponents() {
        val w = word(spatial = false)
        w.explicit("job"); assertTrue("job" in w.suggestions())
        w.reset("hand "); w.explicit("job")
        assertFalse("job" in w.suggestions())
        assertEquals("job", w.literalChip(w.suggestions()))
        w.suggestSwearWords = true
        assertTrue("job" in w.suggestions())
        w.reset("fudge "); w.explicit("packer")
        assertFalse("packer" in w.suggestions())
        w.reset("a "); w.explicit("packer")
        assertTrue("packer" in w.suggestions())
        w.reset("nig "); w.explicit("nog") // First word is outside HXTM.
        assertFalse("nog" in w.suggestions())
        val split = word()
        split.type("handjib", aimed = "handjob")
        assertNotEquals("hand job", split.replacement())
        assertFalse("hand job" in split.suggestions())
        split.suggestSwearWords = true
        assertTrue(split.replacement() == "hand job" || "hand job" in split.suggestions())
    }

    @Test fun ordinaryPrefixesAnatomyAndFalseFriendsRemainAvailable() {
        for ((lang, prefix, expected) in listOf(
            Triple("en", "assu", "assume"), Triple("en", "vulv", "vulva"),
            Triple("en", "fuc", "fuchsia"), Triple("es", "negro", "negro"),
            Triple("de", "dick", "dick"), Triple("nb", "fag", "fag")
        )) {
            val w = word(lang, spatial = false)
            w.explicit(prefix)
            assertTrue("$lang:$prefix", expected in w.suggestions())
        }
    }

    // iOS AutocorrectEngineTests.testACommonBigramDoesNotSwallowADistinctWellAimedTap.
    @Test fun aCommonBigramDoesNotSwallowADistinctWellAimedTap() {
        val g = layout("en")
        val w = word()
        w.reset("I promised ")
        for ((c, dx, dy) in listOf(Triple('t', -7.2, 6.6), Triple('o', 14.5, 12.5), Triple('m', -10.0, 3.9))) w.tap(g, c, dx, dy)
        assertEquals("tom", w.token)
        assertNull(w.replacement())
    }

    // iOS testASpaceTappedAsALetterAboveTheSpaceBarOrMissedSplitsTheWords.
    @Test fun aBottomRowTapAboveTheSpaceBarOrBelowTheLettersSplitsTheWords() {
        val g = layout("en")
        val w = word()
        for (letter in "bnvc") for (dy in listOf(0.0, -8.0, 10.0)) {
            w.reset("is this ")
            for (c in "of") w.tap(g, c)
            w.tap(g, letter, dy = dy)
            for (c in "the") w.tap(g, c)
            assertEquals("$letter $dy", "of the", w.replacement())
        }
        for (typed in listOf("number", "cabin", "seven", "become")) {
            w.reset("is this ")
            for (c in typed) w.tap(g, c)
            assertNull(typed, w.replacement())
        }
        // Without the space bar's place, a bottom-row tap at its centre is only ever a letter.
        val plain = word()
        plain.reset("is this ")
        for (c in "ofbthe") plain.tap(KeyGrid(g.centres, g.pitchX, g.pitchY), c)
        assertNotEquals("of the", plain.replacement())
    }

    // iOS testSpatialAutocorrectAddsSpanishAccents: an accented letter with a key of its own is
    // also typed from its base letter's key (ñ from n, ä from a).
    @Test fun anAccentedLetterAlsoComesFromItsBaseKey() {
        val es = layout("es")
        val w = word("es")
        for ((typed, fixed) in listOf("esta" to null, "aqui" to "aquí", "manana" to "mañana")) {
            w.reset("")
            for (c in typed) w.tap(es, c)
            assertEquals(typed, fixed, w.replacement())
        }
        val de = layout("de")
        val g = word("de")
        g.reset("das ")
        for (c in "madchen") g.tap(de, c)
        assertEquals("mädchen", g.replacement())
    }
}
