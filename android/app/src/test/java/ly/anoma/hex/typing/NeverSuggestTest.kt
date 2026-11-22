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
            "fr" -> listOf("azertyuiop", "qsdfghjklm", "wxcvbn")
            "nb" -> listOf("qwertyuiopå", "asdfghjkløæ", "zxcvbnm")
            else -> listOf("qwertyuiop", "asdfghjkl", "zxcvbnm")
        }
        val pitch = if (language == "de" || language == "nb") 36.0 else 39.5
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
            Triple("de", "scheiße", "neger"), Triple("nb", "faen", "neger"),
            Triple("fr", "merde", "nègre"), Triple("it", "cazzo", "frocio")
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
        for (lang in listOf("en", "es", "de", "nb", "fr", "it")) {
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
            Triple("de", "dick", "dick"), Triple("nb", "fag", "fag"),
            Triple("fr", "pédal", "pédale"), Triple("it", "finocch", "finocchio")
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
        // Without the space bar's place, a bottom-row tap at its centre is only ever a letter. Four
        // letters: from five, a run no word reads may still be a missed space and an extra tap (#2244).
        val plain = word()
        for (typed in listOf("ofba", "isba")) {
            w.reset("is this "); for (c in typed) w.tap(g, c)
            assertEquals(typed, typed.substring(0, 2) + " a", w.replacement())
            plain.reset("is this "); for (c in typed) plain.tap(KeyGrid(g.centres, g.pitchX, g.pitchY), c)
            assertNull(typed, plain.replacement())
        }
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
        assertEquals("Mädchen", g.replacement())
    }

    // Types `text` at the key centres as the keyboard would: a space or closing mark finishes the
    // word (`finish`) and its correction goes in before it. `spaces` moves the space tap at an index.
    private fun TypingWord.typed(g: KeyGrid, text: String, spaces: Map<Int, KeyPoint> = emptyMap()): String {
        val out = StringBuilder()
        reset("")
        for ((i, c) in text.withIndex()) {
            if (c == ' ' || TypingWord.endsWord(c)) {
                val fix = if (c == ' ') finish(' ', spaces[i] ?: KeyPoint(183.93, 234.5), g) else finish(c)
                if (fix != null) {
                    assertTrue(text, out.endsWith(fix.original))
                    out.setLength(out.length - fix.original.length); out.append(fix.replacement)
                }
                out.append(c); reset(out.toString())
            } else if (c == '\'') { explicit("'"); out.append(c) }
            else { tap(g, c); out.append(c) }
        }
        return out.toString()
    }

    // iOS testContractionsWithoutTheApostropheFollowTheWordsAroundThem and
    // testSoundAlikeWordsFollowTheWordsAroundThem: the word before decides at the word's own space,
    // else the next word revises it.
    @Test fun wordsTheContextDecides() {
        val g = layout("en")
        for ((typed, expected) in listOf(
            "id like to go " to "I'd like to go ", "yes id love to " to "yes I'd love to ",
            "i think id help " to "I think I'd help ", "the id in this " to "the id in this ",
            "my id is here " to "my id is here ", "ill be there " to "I'll be there ",
            "shell be here " to "she'll be here ", "the shell is " to "the shell is ",
            "wed love to " to "we'd love to ", "lets go " to "let's go ", "well see " to "we'll see ",
            "well done " to "well done ", "as well as " to "as well as ",
            "till hell freezes over " to "till hell freezes over ", "hell be here " to "he'll be here ",
            "were going home " to "were going home ",
            "its a nice day " to "it's a nice day ", "i think its broken " to "I think it's broken ",
            "the dog wagged its tail " to "the dog wagged its tail ", "winter is on its way " to "winter is on its way ",
            "your welcome " to "you're welcome ", "i like your car " to "I like your car ",
            "there going home " to "they're going home ", "their is a dog " to "there is a dog ",
            "they lost there keys " to "they lost their keys ", "more then that " to "more than that ",
            "and than we left " to "and then we left ", "youre mother loves you " to "your mother loves you ",
            "you're mother " to "you're mother ",
        )) {
            for (spatial in listOf(false, true)) assertEquals("$typed spatial=$spatial", expected, word(spatial = spatial).typed(g, typed))
        }
        // Only English decides by context.
        assertEquals("es ill ", word("es", spatial = false).typed(layout("es"), "es ill "))
    }

    // iOS testASpaceTappedAtTheSpaceBarsEdgeForALetterJoinsTheWords: a space tapped at the bar's top
    // edge, under a letter the word needed, joins the words at the next word's space.
    @Test fun aSpaceTappedAtTheBarsEdgeForALetterJoinsTheWords() {
        val g = layout("en")
        fun edge(letter: Char) = KeyPoint(g.centres.getValue(letter).x, 207.5)
        assertEquals("the delicate ", word().typed(g, "the deli ate ", mapOf(8 to edge('c'))))
        assertEquals("so handed ", word().typed(g, "so ha ded ", mapOf(5 to edge('n'))))
        assertEquals("a space inside the bar", "deli ate ", word().typed(g, "deli ate "))
        // Two words space fixes on their own stay two.
        assertEquals("I didn't ", word().typed(g, "i didnt ", mapOf(1 to edge('b'))))
        // Without the bar's place, nothing joins.
        assertEquals("the deli ate ", word().typed(KeyGrid(g.centres, g.pitchX, g.pitchY), "the deli ate ", mapOf(8 to edge('c'))))
    }

    // iOS testPunctuationAfterATypoFixesItFirst and the "iwant" case: a closing mark corrects the
    // word as space does, never after an abbreviation's period; a split "i" is "I".
    @Test fun aClosingMarkCorrectsTheWordFirst() {
        val g = layout("en")
        for (mark in ",.!?;:)\"") assertEquals("$mark", "see the$mark ok", word().typed(g, "see teh$mark ok"))
        assertEquals("a receive. approx. a,", word().typed(g, "a recieve. approx. a,"))
        assertEquals("I want ", word().typed(g, "iwant "))
        assertEquals("look at ", word().typed(g, "lookat "))
    }

    // iOS testSpatialAutocorrectCapitalisesANameTypedInLowercase: a name, day or place typed in
    // lowercase takes its capital; a word that is also an ordinary lowercase word doesn't.
    @Test fun aNameTypedInLowercaseTakesItsCapital() {
        val g = layout("en")
        assertEquals("see Mary on Monday in Boston ", word().typed(g, "see mary on monday in boston "))
        assertEquals("we stood united in china ", word().typed(g, "we stood united in china "))
        // A name outside the Spanish model is a word as typed, not a typo ("tom" stays, not "toma").
        val es = layout("es")
        assertEquals("vi a tom ayer ", word("es").typed(es, "vi a tom ayer "))
        assertEquals("con Mary en Madrid ", word("es").typed(es, "con mary en madrid "))
    }

    // iOS testGermanSpatialAutocorrectCapitalisesTheNounItCorrects, with the shipped de.capitals.
    @Test fun germanNounsTakeTheirCapital() {
        val g = layout("de")
        fun fix(before: String, taps: String): String? = word("de").run {
            reset(before); for (c in taps) tap(g, c); replacement()
        }
        assertEquals("Haus", fix("das ", "haua"))
        assertEquals("Haus", fix("das ", "hsus"))
        assertEquals("Haus", fix("das ", "haus"))
        // Written with a capital only some of the time: a noun after an article or determiner.
        assertEquals("Leben", fix("das ", "leben"))
        assertNull(fix("zu ", "leben"))
        assertNull(fix("ich ", "frage"))
        assertNull(fix("wollt ihr ", "essen"))
        assertNull(fix("ich ", "kauf"))
        assertEquals("Ruhe", fix("lass mich in ", "ruhe"))
        assertEquals("Stadt", fix("die ", "stadt"))
    }

    // iOS testSentenceStartersAndNextWordsFillAllThreeSlots and
    // testNextWordPredictionsUseTheActiveLanguageAndRespectBoundaries.
    @Test fun sentenceStartersAndNextWordsFillAllThreeSlots() {
        val w = word()
        for (context in listOf("Done. ", "Really? ", "Wow! ", "Hi\n", "")) assertEquals(context, listOf("I", "The", "I'm"), w.predictions(context))
        assertEquals(listOf("I", "the", "I'm"), w.predictions("Done. ", capitalize = false))
        assertEquals("an unknown word: the commonest words", listOf("the", "to", "and"), w.predictions("zzqx "))
        assertEquals("after a comma, words that start a clause", listOf("I", "but", "and"), w.predictions("Running late, "))
        assertEquals("not after a number", emptyList<String>(), w.predictions("at 7 "))
        assertEquals("no space after the period yet", emptyList<String>(), w.predictions("Done."))
        val hi = w.predictions("say hi ")
        assertEquals(3, hi.size)
        assertTrue(hi.toString(), hi.all { it.first().isLowerCase() || it == "I" || it.startsWith("I'") })
        val es = word("es")
        assertEquals(3, es.predictions("por ").size)
        for (context in listOf("@por ", "todo/por ", "por  ")) assertEquals(context, emptyList<String>(), es.predictions(context))
        for (context in listOf("por. ", "hola\n")) assertEquals(context, listOf("Yo", "El", "La"), es.predictions(context))
        assertEquals(listOf("yo", "el", "la"), es.predictions("por. ", capitalize = false))
    }

    // iOS testShortCompletionsFollowThePhraseAndKeepTheTypedCase, testTheStripNeverRepeatsWhatYouTyped
    // and testAmbiguousContractionsSuggestWithoutAutoReplacing: the strip shows what space will do.
    @Test fun theStripShowsWhatSpaceWillDo() {
        fun strip(language: String, before: String, typed: String, spatial: Boolean = false) = word(language, spatial).run {
            reset(before); explicitOrTapped(this, typed, spatial, language); strip()
        }
        fun first(s: List<Suggestion>) = s.first { it.kind != Suggestion.Kind.Typed }.text
        val like = strip("en", "I would ", "li")
        assertEquals("like", first(like))
        assertTrue(like.toString(), like.none { it.kind == Suggestion.Kind.Fix })
        assertEquals("Like", first(strip("en", "I would ", "Li")))
        assertEquals("gracias", first(strip("es", "muchas ", "gr")))
        // A word space leaves alone shows itself first; an unfinished one gets three candidates.
        val well = strip("en", "", "well")
        assertEquals(Suggestion("well", Suggestion.Kind.Typed), well[0])
        assertEquals(Suggestion("we'll", Suggestion.Kind.Word), well[1])
        val beca = strip("en", "", "beca")
        assertEquals(beca.toString(), 3, beca.size)
        assertTrue(beca.toString(), beca.none { it.kind == Suggestion.Kind.Typed || it.text.equals("beca", ignoreCase = true) })
        // A fix space will apply sits after what was typed.
        val teh = strip("en", "", "teh", spatial = true)
        assertEquals(listOf(Suggestion("teh", Suggestion.Kind.Typed), Suggestion("the", Suggestion.Kind.Fix)), teh.take(2))
        assertTrue(teh.toString(), teh.drop(1).none { it.text == "teh" })
        // Nothing to offer: only what was typed.
        assertEquals(listOf(Suggestion("kqzx", Suggestion.Kind.Typed)), strip("en", "", "kqzx"))
        // Autocorrect off: completions, never a fix.
        val off = word(spatial = true).run { reset(""); explicitOrTapped(this, "teh", true, "en"); strip(autocorrect = false) }
        assertTrue(off.toString(), off.none { it.kind == Suggestion.Kind.Fix })
    }

    private fun explicitOrTapped(w: TypingWord, text: String, spatial: Boolean, language: String) {
        if (!spatial) { w.explicit(text); return }
        val g = layout(language)
        for (c in text) { val p = g.centres.getValue(c.lowercaseChar()); w.letter(c.toString(), p.x, p.y, g) }
    }

    private fun personal(w: TypingWord, words: List<String>, shortcuts: Map<String, String> = emptyMap()) {
        w.decoder.personal = PersonalWords(w.decoder.model, words, shortcuts)
    }

    // iOS testTextReplacementsGoInAsOnApplesKeyboard, testTextReplacementsTakeTheShortcutsLeadingCapital
    // and testYourTextReplacementWinsOverShorthand, with the personal dictionary's shortcuts.
    @Test fun textReplacementsGoInAsOnApplesKeyboard() {
        val w = word()
        personal(w, emptyList(), mapOf("myem" to "jason@example.com", "ad2" to "second address", "s.o" to "significant other",
            "home" to "at my house", "brb" to "be right back", "omw" to "On my way!", "mip" to "My iPhone"))
        for ((before, typed, phrase) in listOf(
            Triple("hi myem", "myem", "jason@example.com"), Triple("ad2", "ad2", "second address"),
            Triple("s.o", "s.o", "significant other"), Triple("home", "home", "at my house"),
            Triple("ok (myem", "myem", "jason@example.com"), Triple("hi,myem", "myem", "jason@example.com"),
            Triple("ok Brb", "Brb", "Be right back"), Triple("ok brb", "brb", "be right back"),
            Triple("ok BRB", "BRB", "be right back"), Triple("omw", "omw", "On my way!"), Triple("OMW", "OMW", "On my way!"),
            Triple("mip", "mip", "My iPhone"), Triple("Mip", "Mip", "My iPhone"), Triple("MIP", "MIP", "My iPhone"),
        )) assertEquals(before, Correction(typed, phrase), w.textReplacement(before))
        assertNull("never the end of a longer word", w.textReplacement("xmyem"))
        assertNull("not once a space follows", w.textReplacement("myem "))
        // The strip shows it, and space puts it in, before chat shorthand is left alone.
        w.reset("hi "); w.explicit("myem")
        assertEquals(listOf(Suggestion("myem", Suggestion.Kind.Typed), Suggestion("jason@example.com", Suggestion.Kind.Fix)), w.strip())
        w.reset(""); w.explicit("brb")
        assertEquals("be right back", w.replacement())
        w.reset(""); w.explicit("idk")
        assertNull("shorthand without a replacement stays", w.replacement())
    }

    // iOS testSpatialAutocorrectKnowsYourOwnWords, testYourCommonWordsStayLowercaseAndYourVocabulary
    // KeepsItsCapitals and testYourOwnContactsAndReplacementsAreStillOffered, with personal words.
    @Test fun yourOwnWordsAreReadSpelledYourWayAndOffered() {
        val g = layout("en")
        val w = word()
        personal(w, listOf("Zorbix", "GitHub", "World", "Will", "Kate", "Dick"), mapOf("wtf" to "what the fuck"))
        assertEquals("a slip in your word, and your word typed in lowercase", "Zorbix Zorbix ", w.typed(g, "zorbiz zorbix "))
        assertEquals("GitHub ", w.typed(g, "github "))
        assertEquals("your word that is also a common word stays lowercase", "will world Kate ", w.typed(g, "will world kate "))
        w.reset("so "); w.explicit("wor")
        assertTrue(w.strip().toString(), w.strip().none { it.text == "World" })
        // Your word wins a slot as a completion once two letters match, beside the fix space puts in.
        for (typed in listOf("zo", "zor")) {
            w.reset("so "); for (c in typed) w.tap(g, c)
            assertTrue(typed + " " + w.strip(), w.strip().any { it.text == "Zorbix" })
        }
        w.reset(""); w.explicit("Dic")
        assertTrue("your own words are offered, even listed ones", w.strip().any { it.text == "Dick" })
        w.reset(""); w.explicit("wtf")
        assertEquals("what the fuck", w.replacement())
        // Entries for another language stay out; Bokmål takes Norwegian's.
        assertTrue(PersonalWords.applies(null, "en") && PersonalWords.applies("en_US", "en") && PersonalWords.applies("no_NO", "nb"))
        assertFalse(PersonalWords.applies("es_ES", "en") || PersonalWords.applies("nn_NO", "nb"))
        // Without your words nothing of this applies.
        val plain = word()
        assertEquals("zorbiz github ", plain.typed(g, "zorbiz github "))
    }

    // iOS FullKeyboardEngineTests.testTheLikelyNextLetterClaimsAFewPointsOfItsNeighbour: within 5 dp of
    // its touch area, the letter likelier to come next takes a touch on the border.
    @Test fun theLikelyNextLetterClaimsAFewPointsOfItsNeighbour() {
        val keys = layout("en").centres.keys
        val w = word()
        assertEquals("the", 'e', w.likelierKey("th", 'd', mapOf('e' to 2.0), keys))
        assertEquals("past reach", 'd', w.likelierKey("th", 'd', mapOf('e' to 6.0), keys))
        assertEquals("and", 'd', w.likelierKey("the an", 'e', mapOf('d' to 2.0), keys))
        assertEquals('E', w.likelierKey("TH", 'D', mapOf('e' to 2.0), keys))
        assertEquals("inside a number", 'd', w.likelierKey("th3", 'd', mapOf('e' to 2.0), keys))
    }

    // French and Italian elide: a word starts right after "l'", "qu'" or "dell'", and is read there.
    @Test fun aWordStartsAfterAnElision() {
        val fr = word("fr")
        assertTrue(fr.startsWord("le chat de l'"))
        assertTrue(fr.startsWord("qu'"))
        assertFalse("not after any apostrophe", fr.startsWord("aujourd'"))
        assertFalse("English has none", word("en").startsWord("l'"))
        assertTrue(word("it").startsWord("dell'"))
        assertEquals("l", TypingWord.previousWord("le chat de l'"))
        val g = KeyGrid(buildMap {
            for ((row, letters) in listOf("azertyuiop", "qsdfghjklm", "wxcvbn").withIndex())
                for ((i, c) in letters.withIndex()) put(c, KeyPoint(201 + (i - (letters.length - 1) / 2.0) * 39.5, 66.5 + 56 * row))
        }, 39.5, 56.0)
        fr.reset("le chat de l'")
        for ((c, aimed) in "hpmme".zip("homme")) { val p = g.centres.getValue(aimed); fr.letter(c.toString(), p.x, p.y, g) }
        assertEquals("homme", fr.replacement())
        // The same letters run together with the apostrophe left out ("cest") are left to the decoder.
        fr.reset("")
        for (c in "cest") { val p = g.centres.getValue(c); fr.letter(c.toString(), p.x, p.y, g) }
        assertEquals("c'est", fr.replacement())
    }

    // French and Italian words the same letters spell with and without an accent ("a"/"à", "e"/"è"):
    // the words around them decide, as English's apostrophes.
    @Test fun accentsTheWordsAroundDecide() {
        val fr = layout("fr")
        assertEquals("je vais à Paris ", word("fr").typed(fr, "je vais a paris "))
        assertEquals("il a faim ", word("fr").typed(fr, "il a faim "))
        assertEquals("faire ça sera facile ", word("fr").typed(fr, "faire ca sera facile "))
        val it = layout("it")
        assertEquals("non è esagerato ", word("it").typed(it, "non e esagerato "))
        assertEquals("il vostro indirizzo è cambiato ", word("it").typed(it, "il vostro indirizzo e cambiato "))
        // At a sentence's start, its capital and all, once the next word says so.
        val e = word("it"); e.reset("")
        val p = it.centres.getValue('e'); e.letter("E", p.x, p.y, it)
        e.finish(' ', KeyPoint(183.93, 234.5), it)
        e.reset("E "); for (c in "andato") e.tap(it, c)
        assertEquals(Correction("E andato", "È andato"), e.finish(' ', KeyPoint(183.93, 234.5), it))
        assertEquals("pane e burro ", word("it").typed(it, "pane e burro "))
    }

    // iOS #2244: a run no word reads may still be a missed space with a slip in the second word, or
    // three or more words typed with no spaces; never a one-word typo or a likelier reading.
    @Test fun aMissedSpaceWithASlipOrWordsRunTogether() {
        val g = layout("en")
        for ((typed, expected) in listOf(
            "specialchsracter " to "special character ", "goodmotning " to "good morning ",
            "nextwerk " to "next week ", "lookingforwsrd " to "looking forward ", "emailaddtess " to "email address ",
            "seeyoutomorrow " to "see you tomorrow ", "thankyouverymuch " to "thank you very much ",
            "haveaniceday " to "have a nice day ", "inthemorning " to "in the morning ",
            "understsnd " to "understand ", "informstion " to "information ",
        )) assertEquals(typed, expected, word("en").typed(g, typed))
        assertFalse(word("en").typed(g, "tommorow ").contains("tom morrow"))
        val w = word("en")
        w.reset("")
        for (c in "theheron") w.tap(g, c)
        assertNotEquals("the her on", w.replacement())
        w.reset("")
        for (c in "specialchsracter") w.tap(g, c)
        assertTrue(w.suggestions().joinToString(), w.suggestions().any { it.equals("special character", ignoreCase = true) })
    }

    // Rare words the books use, outside the models (<lang>.valid), kept as typed: each of these is read
    // as a commoner word, or words run together, without its list. A model word typed without its
    // accent still gets it.
    @Test fun rareWordsTheBooksUseAreKept() {
        for ((lang, typed) in listOf("es" to "caminad", "es" to "vanesa", "it" to "scherzate", "it" to "antidepressivo",
            "de" to "analysiere", "de" to "handelst", "nb" to "navar", "nb" to "støyet")) {
            val w = word(lang)
            w.reset("")
            for (c in typed) w.tap(layout(lang), c)
            assertNull("$lang $typed", w.replacement())
        }
        val es = word("es")
        es.reset("")
        for (c in "volvere") es.tap(layout("es"), c)
        assertEquals("volveré", es.replacement())
    }

    // iOS: Auto-capitalisation off (Settings › Keyboard), autocorrect adds no capital you didn't type;
    // your own words and a capital you typed keep theirs.
    @Test fun autoCapitalisationOffAddsNoCapitals() {
        val g = layout("en")
        val on = word()
        val off = word().also { it.addsCapitals = false }
        for ((typed, capitalised, plain) in listOf(Triple("i", "I", null), Triple("im", "I'm", "i'm"), Triple("monday", "Monday", null))) {
            for ((w, expected) in listOf(on to capitalised, off to plain)) {
                w.reset("so "); for (c in typed) w.tap(g, c)
                assertEquals(typed, expected, w.replacement())
            }
        }
        assertEquals("I think I'd help ", on.typed(g, "i think id help "))
        assertEquals("i think i'd help ", off.typed(g, "i think id help "))
        assertEquals(listOf("I", "The", "I'm"), on.predictions(""))
        assertTrue(off.predictions("").toString(), off.predictions("").none { it != it.lowercase() })
        off.reset(""); for (c in "mon") off.tap(g, c)
        assertTrue(off.strip().toString(), off.strip().none { it.text != it.text.lowercase() })
        off.reset("so "); off.letter("M", g.centres.getValue('m').x, g.centres.getValue('m').y, g); for (c in "onday") off.tap(g, c)
        assertNull("a capital you typed stays", off.replacement())
        personal(off, listOf("GitHub"))
        off.reset("so "); for (c in "github") off.tap(g, c)
        assertEquals("GitHub", off.replacement())
        val de = layout("de")
        for ((w, expected) in listOf(word("de") to "Haus", word("de").also { it.addsCapitals = false } to null)) {
            w.reset("das "); for (c in "haus") w.tap(de, c)
            assertEquals(expected, w.replacement())
        }
    }

    // Every language has words for the strip between words: at a sentence's start, after a comma and
    // after a word the pairs don't know (0.2.0's French and Italian showed nothing there).
    @Test fun everyLanguageOffersWordsBetweenWords() {
        for (lang in listOf("en", "es", "de", "nb", "fr", "it")) {
            val w = word(lang)
            for (before in listOf("", "zzqx, ", "zzqx ")) assertEquals("$lang [$before]", 3, w.predictions(before).size)
        }
        assertEquals(listOf("Je", "Le", "La"), word("fr").predictions(""))
        assertEquals(listOf("e", "ma", "che"), word("it").predictions("ciao, "))
    }
}
