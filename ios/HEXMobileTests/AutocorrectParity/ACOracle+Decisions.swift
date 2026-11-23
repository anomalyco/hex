// The autocorrect oracle for the Rust port's parity fixture (`AutocorrectParityExport`):
// what space puts in, context and the decoder, a copy of `AutocorrectEngine.swift` (from `replacementOnSpace`) as of dev commit 8b879d8a, renamed (`ACOracle`,
// `TapOracle`, `ACO…`) so it runs on its own copies, independent of the app's. Changed from
// the copy: the dictionary is `ACOChecking` (no `SystemTextChecker`), learned words live in
// `ACOLearnedWords` (an in-memory copy of `LearnedWordsStore`), the bundled lists are read
// from the repository, and the two orders the owner fixed in the port go through
// `ACOracle.orderedValues` and `ACOOffensiveWords.bundled`. The keys' grid is the app's
// (`KeyboardLanguage.keyCoordinates`, which stays the host's). Delete with the exporter.

import CoreGraphics
import Foundation
import UIKit
@testable import HEX

@MainActor
extension ACOracle {
    /// Returns `(tokenToDelete, replacement)` only when cautious autocorrect is confident, or
    /// for a text replacement (with Autocorrect off too).
    func replacementOnSpace(
        beforeInput: String?,
        field: KeyboardFieldKind,
        autocorrectionType: UITextAutocorrectionType?
    ) -> (original: String, replacement: String)? {
        lastFixLanguage = nil
        guard field == .text, autocorrectionType != .no else { return nil }
        // Your own text replacement comes first, as on Apple's keyboard: before the chat
        // shorthand Hex leaves alone ("brb"), with Autocorrect off or no dictionary, and
        // however often you kept the shortcut.
        if let replaced = textReplacement(before: beforeInput) { return (replaced.typed, replaced.replacement) }
        guard let token = Self.currentToken(in: beforeInput) else { return nil }
        self.beforeInput = beforeInput
        partner?.beforeInput = beforeInput
        let lower = token.lowercased()
        guard !suppressedInSession.contains(lower),
              !learning.rejected.contains(lower),
              !learnedWords.contains(lower) else { return nil }
        guard isEnabled, hasDictionary, !Self.shorthandAllowlist.contains(lower) else { return nil }
        guard let fix = cautiousReplacement(for: token) else { return nil }
        return (token, fix)
    }

    /// What space puts in place of `token`, if anything. With a partner (bilingual
    /// autocorrect), a word either language knows as typed stays as it is (your text
    /// replacements still apply), except a sure fix of this language's (`isSureFix`) once
    /// the words before are in it: the other dictionary takes many of its typos for words
    /// ("dont", "im", "dk" in French, "im", "tkm" in German), and kept them. A typo either can
    /// fix gets the fix, though never the other's guess of a name ("Zuri" for "Ziri"); when both
    /// can, the fix in the language of the words before it, else the cheaper slip on these
    /// keys, else the keyboard's own language's. Held-out English benchmark, French beside it:
    /// 7.56% WER before, 5.79% now, against 5.75% alone.
    func cautiousReplacement(for token: String) -> String? {
        if let shortcut = shortcut(for: token) {
            lastFixLanguage = nil
            return shortcut
        }
        if let term = vocabularyTerm(for: token) {
            lastFixLanguage = nil
            return Self.vocabularySpelling(of: token, as: term)
        }
        // This language's own accented spelling decides before any partner logic: the table
        // only holds plain spellings that are no word of this language ("perche" -> "perché"),
        // so an English lean must not hand it to the other dictionary's guess.
        if let accented = accentRestoration(for: token) {
            lastFixLanguage = language
            return accented
        }
        guard let partner = workingPartner else {
            lastFixLanguage = language
            return ownReplacement(for: token)
        }
        let lean = languageLean(partner, token: token)
        // One-letter words are left to the keyboard's own language: `UITextChecker` takes
        // every letter as a word, so English "i" would never become "I" beside French.
        if token.count >= 2, partner.knowsAsTyped(token), Self.hasVowel(token) {
            if lean < 0, isSureFix(token), let own = ownReplacement(for: token) {
                lastFixLanguage = language
                return own
            }
            // Spatial autocorrect may still read the taps as another word, counting the other
            // language's word as one it changes only on a clear margin, as it does its own
            // words: "tkm" (German) becomes "Tom", "merci" stays. Not a sure fix's word, which
            // the next word decides ("Im" stays for "I'm going", not "In").
            guard lean <= 0, !isAllCapsOrMixed(token),
                  !(language.acoUsesEnglishFixes && Self.unambiguousFixes[token.lowercased()] != nil),
                  let replacement = decoderDecision(for: token, partnerWord: true)?.replacement,
                  mayOffer(replacement, replacing: token) else {
                lastFixLanguage = nil
                return nil
            }
            lastFixLanguage = language
            return withoutAddedCapitals(cased(replacement, typed: token), typed: token)
        }
        let own = ownReplacement(for: token)
        // A name the other dictionary guesses ("Zuri" for "Ziri") is no fix of yours.
        guard token.count >= 2, !knowsAsTyped(token), let theirs = partner.ownReplacement(for: token),
              !partner.isName(theirs) else {
            lastFixLanguage = own == nil ? nil : language
            return own
        }
        guard let own, !ACOracle.sameIgnoringCase(own, theirs) else {
            lastFixLanguage = own == nil ? partner.language : language
            return own ?? theirs
        }
        if prefersPartner(lean, token: token, own: own, theirs: theirs) {
            lastFixLanguage = partner.language
            return theirs
        }
        lastFixLanguage = language
        return own
    }

    /// Whether this language's dictionary knows `word` only with a capital, and German's noun
    /// list doesn't call it a noun: a name.
    func isName(_ word: String) -> Bool {
        let lower = word.lowercased()
        guard lower.count >= 2, let first = lower.first else { return false }
        guard checker.isWordValid(first.uppercased() + lower.dropFirst()), !checker.isWordValid(lower) else { return false }
        return !(language == .german && germanNouns?.capitalises(lower, after: nil) == true)
    }

    /// Whether `token` has a vowel. iOS's French and German dictionaries take abbreviations
    /// without one ("dk", "dl", "tkm"), which beside English kept its slips ("Tkm" for "Tom"),
    /// so a partner's word without a vowel doesn't count as a word. Serbian's vowelless words
    /// ("prst") never meet this: it has no dictionary, so it is never a partner.
    static func hasVowel(_ token: String) -> Bool {
        token.lowercased().folding(options: .diacriticInsensitive, locale: nil).contains { "aeiouy".contains($0) }
    }

    /// ALL-CAPS ("API") or with capitals inside ("iOS"), which Spatial autocorrect leaves alone.
    func isAllCapsOrMixed(_ token: String) -> Bool {
        token.dropFirst().contains(where: \.isUppercase)
    }

    /// A fix this language makes of `token` even where the other one knows the word: a
    /// contraction typed without its apostrophe or a common slip (`unambiguousFixes`: "dont",
    /// "im"), or a two-letter word ("dk" -> "do"), which in the other dictionary is mostly an
    /// abbreviation. Longer words of the other language ("est", "merci") stay as typed.
    func isSureFix(_ token: String) -> Bool {
        token.count == 2 || (language.acoUsesEnglishFixes && Self.unambiguousFixes[token.lowercased()] != nil)
    }

    /// Bilingual autocorrect: `word`, which the partner's dictionary knew, was left as typed
    /// at a sentence's start (no words before to tell the languages apart), and `next` now
    /// leans this language (`languageLean`): its sure fix (`unambiguousFixes`), as "Im
    /// going" becomes "I'm going" and "Dont do" "Don't do", while German "Im Haus" stays.
    func bilingualRevision(of word: String, before: String, next: String) -> String? {
        guard let partner = workingPartner, language.acoUsesEnglishFixes, !word.contains("'"),
              let fixed = Self.unambiguousFixes[word.lowercased()], word.count >= 2 else { return nil }
        let lower = word.lowercased()
        guard !suppressedInSession.contains(lower), !learning.rejected.contains(lower), !learnedWords.contains(lower),
              partner.knowsAsTyped(word), next.count >= 2, languageLean(partner, before: next, commonFrom: 2) < 0 else { return nil }
        let rest = before.hasSuffix(word) ? String(before.dropLast(word.count)) : before
        guard languageLean(partner, before: rest) == 0, mayOffer(fixed, after: Self.previousWord(before, token: word)) else { return nil }
        return withoutAddedCapitals(Self.matchCase(of: word, onto: fixed), typed: word)
    }

    /// Whether `token` is a word in this language as typed: the dictionary's, one of your
    /// own (`UILexicon`, kept twice) or chat shorthand. A name it knows only capitalised
    /// doesn't count ("paris").
    func knowsAsTyped(_ token: String) -> Bool {
        let lower = token.lowercased()
        return Self.shorthandAllowlist.contains(lower) || learnedWords.contains(lower)
            || lexiconTerms.contains { $0.lowercased() == lower } || checker.isWordValid(token)
    }

    /// Which language the words before `token` are in: above zero the partner's, below this
    /// one's, counting the last four that only one of the dictionaries knows, or, of three
    /// letters or more, that both know but only one language's typing model has among its
    /// common words: iOS's French and German dictionaries take "the", "you", "your", "not"
    /// and "please" (iOS 27.2 simulator), so English sentences rarely leaned English.
    func languageLean(_ partner: ACOracle, token: String) -> Int {
        let before = (beforeInput ?? "").suffix(120)
        return languageLean(partner, before: String(before.hasSuffix(token) ? before.dropLast(token.count) : before))
    }

    /// `commonFrom`: the fewest letters a word both dictionaries know needs to count by the
    /// typing models. Three, as French "de", "la" and "et" are common in the English model's
    /// books, except for the word after a sentence's first (`bilingualRevision`: "Im in").
    func languageLean(_ partner: ACOracle, before: String, commonFrom: Int = 3) -> Int {
        var lean = 0
        for word in before.suffix(120).split(whereSeparator: { !$0.isLetter && $0 != "'" }).suffix(4) where word.count >= 2 {
            let mine = checker.isWordValid(String(word))
            let theirs = partner.checker.isWordValid(String(word))
            if theirs && !mine {
                lean += 1
            } else if mine && !theirs {
                lean -= 1
            } else if mine, word.count >= commonFrom {
                let myCommon = isCommon(String(word))
                if myCommon != partner.isCommon(String(word)) { lean += myCommon ? -1 : 1 }
            }
        }
        return lean
    }

    /// Whether this language's typing model (`contextModel`, else Spatial autocorrect's)
    /// has `word` among its commonest, about the top 2,000 (-ln P at most 10 nats: English
    /// "please" 9.25, "capable" 9.9). False without a model for the language.
    func isCommon(_ word: String) -> Bool {
        guard let model = contextModel ?? decoder?.model, model.language == language.rawValue,
              let index = model.index(of: word.lowercased()) else { return false }
        return model.unigramCost(index) <= Self.commonWordCost
    }

    static let commonWordCost = 10.0

    /// Both languages can fix the typo: the partner's fix when the words before are in its
    /// language, or, without a lean, when its fix is clearly the cheaper slip on these keys.
    func prefersPartner(_ lean: Int, token: String, own: String, theirs: String) -> Bool {
        if lean != 0 { return lean > 0 }
        let lower = token.lowercased()
        func cost(_ fix: String) -> Double {
            let normalized = fix.replacingOccurrences(of: "'", with: "").lowercased()
            return Self.singleEditKeyboardCost(from: lower, to: normalized, language: layoutLanguage, letters: layoutLetters) ?? 1.0
        }
        return cost(theirs) + 0.2 < cost(own)
    }

    /// `cautiousReplacement` in this language alone.
    func ownReplacement(for token: String) -> String? {
        let lower = token.lowercased()
        // 1. Explicit iOS Text Replacement (`omw` -> `On my way!`).
        if let shortcut = shortcut(for: token) {
            return shortcut
        }
        // A word that may not be offered, typed in full, is a word you meant: never corrected
        // into another (as on Android: blocked by the current setting, or after its expression's
        // first word).
        if offensiveWords.blocks(lower, allowSwearing: allowsSwearing, after: Self.previousWord(beforeInput ?? "", token: token)) {
            return nil
        }
        // A typo you fixed the same way at least twice (Learn how I type), unless what you
        // typed is a real word: then it is only suggested.
        if let learned = learning.typos[lower], mayOffer(learned, replacing: token), !checker.isWordValid(token) {
            return cased(learned, typed: token)
        }
        // Leave ALL-CAPS acronyms (`API`, `PR`, `NYC`) alone unless matched in `UILexicon` above,
        // except unambiguous all-caps contractions/typos of 3+ letters (`DONT` -> `DON'T`, `TEH` -> `THE`).
        let isAllCaps = token.count >= 2 && token == token.uppercased() && token != token.lowercased()
        // Leave camelCase / PascalCase identifiers (`HexKey`, `iOS`) alone.
        let hasInternalCaps = token.dropFirst().contains(where: \.isUppercase) && !isAllCaps
        if hasInternalCaps { return nil }

        // A word the same letters spell with an apostrophe ("id", "I'd"), chosen by the word
        // before when it says so clearly ("yes id" -> "yes I'd", "the id" stays). The next
        // word may still revise it (`revision(of:before:next:)`).
        if !isAllCaps, !token.contains("'"), let choice = contextChoice(
            for: token, previous: Self.previousWord(beforeInput ?? "", token: token), next: nil,
            startsSentence: Self.startsSentence(before: beforeInput ?? "", token: token)
        ), mayOffer(choice, replacing: token) {
            return withoutAddedCapitals(choice, typed: token)
        }

        // 2. Deterministic contractions and common QWERTY transpositions (English only:
        // Norwegian "i" means "in" and must not become "I").
        if language.acoUsesEnglishFixes, let fixed = Self.unambiguousFixes[lower] {
            if isAllCaps && token.count < 3 { return nil }
            return withoutAddedCapitals(Self.matchCase(of: token, onto: fixed), typed: token)
        }

        if isAllCaps { return nil }

        // Spatial autocorrect, when on, decides alone: your taps, the word before, and a margin.
        if let decision = decoderDecision(for: token) {
            if let replacement = decision.replacement, mayOffer(replacement, replacing: token) {
                var cased = self.cased(replacement, typed: token)
                // A missed space after English "i" ("iwant") gives "I want", as "i" alone does.
                if language.acoUsesEnglishFixes, cased.contains(" ") {
                    cased = cased.split(separator: " ").map { $0 == "i" ? "I" : String($0) }.joined(separator: " ")
                }
                // German spells every noun with a capital, and the German model is lowercase
                // throughout like the English one: a corrected word the dictionary knows only
                // capitalised, or `de.capitals` calls a noun, takes its capital ("hsus" -> "haus"
                // -> "Haus"), as the no-replacement path does below. One word only: a split
                // reading keeps its second word as read.
                if addsCapitals, !cased.contains(" "), decoder?.model.language == KeyboardLanguage.german.rawValue {
                    cased = nounCapitalised(for: cased, typed: token) ?? cased
                }
                return withoutAddedCapitals(cased, typed: token)
            }
            // A word the model knows lowercase that the dictionary knows only capitalised
            // (a name, day or place), or that German's `de.capitals` calls a noun, typed in
            // lowercase, is capitalised as the dictionary path does ("mary" -> "Mary",
            // "haus" -> "Haus"), unless Auto-capitalization is off.
            return addsCapitals ? nounCapitalised(for: token, typed: token) : nil
        }

        // 3. A two-letter non-word one neighbouring key from a very common two-letter word
        // ("tk" -> "to", Spanish "nl" -> "no"); `UITextChecker` guesses skip two letters.
        if token.count == 2, !token.contains("'"), let fix = commonTwoLetterFix(for: lower),
           !checker.isWordValid(token) {
            return Self.matchCase(of: token, onto: fix)
        }

        // 4. A word typed without its accent where the plain spelling is no word of the
        // language ("perche" -> "perché", "citta" -> "città"): the dictionary path below
        // needs three letters and never rewrites a word it knows, and a language's own list
        // is deterministic. Ambiguous pairs ("e"/"è", "ce"/"c'è") are suggestion-only.
        if let accented = accentRestoration(for: token) {
            return accented
        }

        // 5. Single-edit `UITextChecker` typo on words of 3+ characters, never rewriting valid words
        // (a French elision's sides each as a word, `frenchElisionFix`).
        if token.contains("'") { return frenchElisionFix(for: token) }
        guard token.count >= 3 else { return nil }
        guard !checker.isWordValid(token) else { return nil }

        var bestGuess: String?
        var bestScore = Double.greatestFiniteMagnitude
        let guesses = checker.guesses(for: token)
        for (rank, checkerGuess) in guesses.prefix(8).enumerated() {
            guard let rawGuess = usableGuess(checkerGuess, typed: token, among: guesses), mayOffer(rawGuess, replacing: token) else { continue }
            let normalizedGuess = rawGuess.replacingOccurrences(of: "'", with: "").lowercased()
            guard let editCost = Self.singleEditKeyboardCost(from: lower, to: normalizedGuess, language: layoutLanguage, letters: layoutLetters) else { continue }
            let totalScore = editCost + Double(rank) * 0.08
            if totalScore < bestScore {
                bestScore = totalScore
                bestGuess = rawGuess
            }
        }
        guard let bestGuess, bestScore <= 1.35 else { return nil }
        return withoutAddedCapitals(cased(bestGuess, typed: token), typed: token)
    }

    /// The language's own accented spelling of `token`, where the plain spelling is no word
    /// of it ("perche" -> "perché"), or nil. All-caps and mixed-case words are left alone,
    /// as the dictionary path leaves them, and the table only holds non-words of this
    /// language, so it may decide before a bilingual partner's dictionary does.
    func accentRestoration(for token: String) -> String? {
        let isAllCaps = token.count >= 2 && token == token.uppercased() && token != token.lowercased()
        guard !isAllCaps, !token.dropFirst().contains(where: \.isUppercase) else { return nil }
        let lower = token.lowercased()
        guard let accented = language.acoAccentRestorations[lower], mayOffer(accented, replacing: token) else {
            return nil
        }
        return withoutAddedCapitals(cased(accented, typed: token), typed: token)
    }

    /// `candidate` cased as `token` was typed (`matchCase`), less a capital it only has
    /// from where it was learned (`withoutStrayCapital`).
    func cased(_ candidate: String, typed token: String) -> String {
        withoutStrayCapital(Self.matchCase(of: token, onto: candidate), typed: token)
    }

    /// `fix` for `token` as autocorrect puts it in: with `addsCapitals` off and nothing
    /// typed in capitals, all lowercase unless it is your own spelling; nil when that leaves
    /// just what you typed ("i").
    func withoutAddedCapitals(_ fix: String, typed token: String) -> String? {
        guard !addsCapitals, !token.contains(where: \.isUppercase), !isOwnSpelling(fix) else { return fix }
        let lowered = fix.lowercased()
        return lowered == token ? nil : lowered
    }

    /// Whether `word`, capitals and all, is one of your own spellings: a Contacts name or
    /// other `UILexicon` term, a text replacement's output, a word you kept twice, or a fix
    /// Hex learned from you. Auto-capitalization off leaves their capitals alone, except a
    /// capital that `withoutStrayCapital` drops: a Contacts name or learned fix that is also a
    /// common word ("Will", "World") stays lowercase mid-sentence either way.
    func isOwnSpelling(_ word: String) -> Bool {
        lexiconShortcuts.values.contains(word)
            || ((lexiconTerms.contains(word) || learning.typos.values.contains(word) || learnedWords.words.contains(word))
                && withoutStrayCapital(word, typed: "") == word)
    }

    /// The very common two-letter word (the language's `commonTwoLetterWords`, commonest
    /// first) one neighbouring key or one swap away from `lower`, if any.
    func commonTwoLetterFix(for lower: String) -> String? {
        var best: (word: String, cost: Double)?
        for word in language.acoCommonTwoLetterWords {
            guard let cost = Self.singleEditKeyboardCost(from: lower, to: word, language: layoutLanguage, letters: layoutLetters),
                  cost > 0, cost <= 0.65 else { continue }
            if best == nil || cost < best!.cost { best = (word, cost) }
        }
        return best?.word
    }

    /// What spatial autocorrect makes of `token`'s taps, or nil when it doesn't apply: off,
    /// another language's model, an apostrophe or one letter typed, a letter the model lacks,
    /// or taps that don't match the word.
    /// `partnerWord`: the bilingual partner knows `token` as typed, so it counts as a word.
    func decoderDecision(for token: String, partnerWord: Bool = false) -> TapOracle.Decision? {
        syncUserWords()
        guard let decoder, let keys = decoderKeys, let taps = decoderTaps, decoder.model.language == language.rawValue,
              token.count >= 2, taps.count == token.count, !token.contains("'") else { return nil }
        let before = beforeInput ?? ""
        if let cache = decoderCache, cache.before == before, cache.taps == taps, cache.partnerWord == partnerWord { return cache.decision }
        let lower = token.lowercased()
        // Whether what you typed is a word at all, which `properNoun` already answers for a noun
        // the dictionary knows only capitalised. German's list only decides the capital its
        // callers apply to the reading; it never makes a word the dictionary lacks untouchable
        // here.
        let valid = Self.shorthandAllowlist.contains(lower) || learnedWords.contains(lower)
            || lexiconTerms.contains { $0.lowercased() == lower } || checker.isWordValid(token)
            || properNoun(for: token) != nil || partnerWord
        let previous = Self.previousWord(before, token: token)
        var decision = decoder.decide(taps: taps, typed: lower, previous: previous, literalIsValid: valid, keys: keys)
        if let baseline = decoderBaselineKeys, let replacement = decision?.replacement {
            func clears(_ reading: String) -> Bool? {
                decoder.replaces(with: reading, taps: taps, typed: lower, previous: previous, literalIsValid: valid, keys: baseline)
            }
            switch clears(replacement) {
            case true?:
                break
            case false?:
                // The next reading the baseline would also put in, if any: never a new rewrite,
                // and no second search on the letter.
                let readings = decision?.readings ?? []
                decision?.replacement = readings.lazy.map(\.text).first { $0 != replacement && clears($0) == true }
            case nil:
                decision?.replacement = decoder.decide(taps: taps, typed: lower, previous: previous, literalIsValid: valid, keys: baseline)?.replacement
            }
        }
        decoderCache = (before, taps, partnerWord, decision)
        return decision
    }

    /// `token` capitalised when it is all lowercase and the dictionary knows it only that way
    /// (a name, day or place: "mary" -> "Mary", "monday" -> "Monday"; a German noun:
    /// "haus" -> "Haus"), else nil.
    func properNoun(for token: String) -> String? {
        guard token.count >= 2, token == token.lowercased(), let first = token.first else { return nil }
        let capitalised = first.uppercased() + token.dropFirst()
        guard checker.isWordValid(capitalised), !checker.isWordValid(token) else { return nil }
        return capitalised
    }

    /// `properNoun`, with German's noun list in front of the dictionary: a word the books write
    /// with a capital almost always takes one anywhere ("haus" -> "Haus"), and one they write
    /// that way half the time takes one after an article or determiner ("das leben" -> "das
    /// Leben"; "zu leben" stays). Without a determiner a 50-94% word still takes a capital
    /// where only the capital is a word ("abbas" -> "Abbas", "heute abend" -> "heute Abend").
    /// `typed` is what you typed, which finds the word before it when a correction changed
    /// `word`. Words the list doesn't know, and every other language, keep the dictionary's
    /// rule.
    func nounCapitalised(for word: String, typed token: String) -> String? {
        guard language == .german, let capitals = germanNouns,
              let capitalise = capitals.capitalises(word, after: Self.previousWord(beforeInput ?? "", token: token)),
              capitalise, let first = word.first
        else { return properNoun(for: word) }
        return first.uppercased() + word.dropFirst()
    }

    // MARK: - Words the context decides

    /// English words chosen by the words around them rather than as typos: the same letters
    /// with and without an apostrophe ("id" or "I'd", "its" or "it's"), and words that sound
    /// alike ("your", "there", "then"). "were" and "we're" aren't here: the word pairs can't
    /// tell them apart, so the strip offers "we're" and space never changes it.
    static let contextGroups: [[String]] = [
        ["id", "I'd"], ["ill", "I'll"], ["hell", "he'll"], ["shell", "she'll"],
        ["wed", "we'd"], ["lets", "let's"], ["well", "we'll"],
        ["its", "it's"], ["your", "you're"], ["there", "their", "they're"], ["then", "than"],
    ]

    /// The typing model's word pairs barely spell contractions (its books split "I'd" into
    /// "I" and "'d"), so a contraction is scored as the words it stands for.
    static let contractionWords: [String: [[String]]] = [
        "i'd": [["i", "would"], ["i", "had"]], "i'll": [["i", "will"]], "he'll": [["he", "will"]],
        "she'll": [["she", "will"]], "we'd": [["we", "would"], ["we", "had"]], "let's": [["let", "us"]],
        "we'll": [["we", "will"]], "it's": [["it", "is"], ["it", "has"]], "you're": [["you", "are"]],
        "they're": [["they", "are"]],
    ]

    static let contextGroupOf: [String: [String]] = {
        var out: [String: [String]] = [:]
        for group in contextGroups { for word in group { out[word.lowercased()] = group } }
        return out
    }()

    /// Nats the better word must win by: with the word before only, and once the next word
    /// is known too. Changing only an apostrophe needs less than changing letters ("your"
    /// and "you're"). Tuned on Tatoeba sentences outside the benchmark's held-out ones.
    static let contextMargin = (previous: 3.0, next: 1.5)
    static let contextLetterMargin = (previous: 6.0, next: 3.0)
    /// -ln P of a word the model lacks.
    static let contextUnknownCost = 15.0
    /// How much likelier a sentence starts with a contraction's pronoun ("I'd", "It's") than
    /// the word list alone says: the pairs have no sentence starts.
    static let contextSentenceStart = 3.0
    /// "will", "would", "had" and "us", which thousands of words follow in the pairs: a next
    /// word that never follows a contraction's last one of them is evidence against it
    /// ("till hell freezes", not "till he'll freezes"). "are" and "is" are left out: the
    /// adjectives after them are too rare for the pairs ("you're funny").
    static let contextMissingFollower = 5.0
    static let contextRichFollowers: Set<String> = ["will", "would", "had", "us"]

    /// Whether `token`, at the end of `before`, starts a sentence.
    static func startsSentence(before: String, token: String) -> Bool {
        let rest = before.hasSuffix(token) ? String(before.dropLast(token.count)) : before
        return ACOAutoCapitalization.shouldCapitalize(mode: .sentences, before: rest, hasText: true)
    }

    /// -ln P(`word` | `previous`) + -ln P(`next` | `word`) by the model's pairs, a
    /// contraction as its words ("I'd like" as "I would like").
    func contextCost(_ word: String, previous: String?, next: String?, startsSentence: Bool, model: TapOracleModel) -> Double {
        let unknown = Self.contextUnknownCost
        let expanded = Self.contractionWords[word.lowercased()]
        var best = Double.infinity
        for words in expanded ?? [[word.lowercased()]] {
            var cost = model.cost(of: words[0], after: previous) ?? unknown
            for k in words.indices.dropFirst() { cost += model.cost(of: words[k], after: words[k - 1]) ?? unknown }
            if let next {
                let last = words[words.count - 1]
                cost += model.cost(of: next, after: last) ?? unknown
                if Self.contextRichFollowers.contains(last), !model.hasPair(last, next) {
                    cost += Self.contextMissingFollower
                }
            }
            best = min(best, cost)
        }
        return expanded != nil && startsSentence ? best - Self.contextSentenceStart : best
    }

    /// The word of `word`'s group (`contextGroups`) that the words around it make clearly
    /// likelier, cased as `word` is, or nil to keep it. English only, with the layout's
    /// model, and never a word you kept or Hex learned not to change.
    func contextChoice(for word: String, previous: String?, next: String?, startsSentence: Bool = false) -> String? {
        guard isEnabled, language.acoUsesEnglishFixes, let model = contextModel, model.language == language.rawValue,
              let group = Self.contextGroupOf[word.lowercased()] else { return nil }
        let lower = word.lowercased()
        guard !suppressedInSession.contains(lower), !learning.rejected.contains(lower), !learnedWords.contains(lower),
              !lexiconTerms.contains(where: { $0.lowercased() == lower }) else { return nil }
        let own = contextCost(word, previous: previous, next: next, startsSentence: startsSentence, model: model)
        var best: (word: String, cost: Double)?
        for other in group where other.lowercased() != lower {
            let cost = contextCost(other, previous: previous, next: next, startsSentence: startsSentence, model: model)
            if cost < best?.cost ?? .infinity { best = (other, cost) }
        }
        guard let best else { return nil }
        let sameLetters = best.word.replacingOccurrences(of: "'", with: "").lowercased() == lower.replacingOccurrences(of: "'", with: "")
        let margins = sameLetters ? Self.contextMargin : Self.contextLetterMargin
        guard own - best.cost >= (next == nil ? margins.previous : margins.next) else { return nil }
        return Self.matchContextCase(of: word, onto: best.word)
    }

    /// `target` cased as `word` reads: a capital first letter carries over (a sentence's
    /// start), except the "I" of "I'd" or "I'll", which comes off ("I'd" to "id").
    static func matchContextCase(of word: String, onto target: String) -> String {
        let startsWithI = word.hasPrefix("I'") && word.dropFirst(2).allSatisfy(\.isLowercase)
        guard let first = word.first, first.isUppercase, !startsWithI, let t = target.first else { return target }
        return t.uppercased() + target.dropFirst()
    }

    /// The word before the one being finished, revised now that `next` follows it: "well
    /// see" becomes "we'll see", as Apple's keyboard decides at the next word. `before` is
    /// the text up to the end of `word`. Nil to leave it.
    func revision(of word: String, before: String, next: String, field: KeyboardFieldKind, autocorrectionType: UITextAutocorrectionType?) -> String? {
        guard field == .text, autocorrectionType != .no, hasDictionary, word.count >= 2 else { return nil }
        let isAllCaps = word == word.uppercased() && word != word.lowercased() && !word.hasPrefix("I'")
        guard !isAllCaps, let nextWord = next.split(separator: " ").first.map(String.init) else { return nil }
        let previous = Self.previousWord(before, token: word)
        lastRevisionLanguage = language
        if let fixed = bilingualRevision(of: word, before: before, next: nextWord) { return fixed }
        if let (fixed, into) = accentRevision(of: word, before: before, next: nextWord) { lastRevisionLanguage = into; return fixed }
        guard let choice = contextChoice(
            for: word, previous: previous, next: nextWord.lowercased(),
            startsSentence: Self.startsSentence(before: before, token: word)
        ), mayOffer(choice, after: previous) else { return nil }
        return addsCapitals || word.first?.isUppercase == true ? choice : choice.lowercased()
    }

    /// One word in place of `first`, a space tapped near the letter row's edge (`space`) and
    /// the word being finished (`decoderTaps`), when Spatial autocorrect reads the space as a
    /// letter ("deli ate" -> "delicate"); nil to keep both. `before` is the text up to the
    /// end of `first`. `first` and `firstTaps` are as you typed them.
    func merge(first: String, firstTaps: [CGPoint], space: CGPoint, second: String, before: String) -> String? {
        // Adaptive touch's personal centres never join words: the centres read without it decide.
        guard isEnabled, hasDictionary, let decoder, let keys = decoderBaselineKeys ?? decoderKeys, let taps = decoderTaps,
              decoder.model.language == language.rawValue, !first.contains("'"), !second.contains("'") else { return nil }
        for word in [first, second] {
            let lower = word.lowercased()
            if suppressedInSession.contains(lower) || learning.rejected.contains(lower) || learnedWords.contains(lower)
                || Self.shorthandAllowlist.contains(lower) || lexiconTerms.contains(where: { $0.lowercased() == lower }) {
                return nil
            }
        }
        let previous = Self.previousWord(before, token: first)
        guard let merged = decoder.merge(
            first: first, firstTaps: firstTaps, space: space, second: second, secondTaps: taps,
            previous: previous, firstIsValid: knowsAsTyped(first), secondIsValid: knowsAsTyped(second), keys: keys
        ), mayOffer(merged, after: previous) else { return nil }
        return withoutAddedCapitals(Self.matchCase(of: first, onto: merged), typed: first + second)
    }

    /// Your own words for Swipe typing: Contacts names and other `UILexicon` terms, one-word
    /// text replacements, and words you kept twice that may be offered.
    var ownWords: [String] {
        lexiconTerms + Self.orderedValues(lexiconShortcuts).filter { !$0.contains(" ") } + learnedWords.words.filter { mayOffer($0) }
    }

    /// The English contractions Hex corrects to or offers ("I'm", "don't", "we'll"), which
    /// Swipe typing reads as common words.
    static let englishContractions: [String] = Set(
        unambiguousFixes.values.filter { $0.contains("'") } + suggestOnlyContractions.values
    ).sorted()

    /// Gives the decoder your own words: Contacts names and other `UILexicon` terms, the
    /// one-word outputs of text replacements, words you kept twice, and typo fixes Hex
    /// learned. Rebuilt only when one of them, or the model, changed.
    ///
    /// Also hands it the words never to offer: the language's offensive words, less the swear
    /// words while Suggest swear words is on. Words Hex learned are left out when offensive;
    /// your `UILexicon` words never are.
    func syncUserWords() {
        guard let model = decoder?.model else { return }
        let id = ObjectIdentifier(model)
        if let built = userWordsBuilt, built.revision == userWordsRevision, built.model == id { return }
        let learned = (learnedWords.words + Self.orderedValues(learning.typos)).filter { mayOffer($0) }
        let words = lexiconTerms + Self.orderedValues(lexiconShortcuts) + learned
        decoder?.userWords = TapOracle.UserWords(words, model: model)
        decoder?.neverSuggest = TapOracle.NeverSuggest(offensiveWords.blocked(allowSwearing: allowsSwearing), model: model)
        userWordsBuilt = (userWordsRevision, id)
        decoderCache = nil
    }

    /// A `UITextChecker` guess as autocorrect and the strip may use it, or nil. A guess adds
    /// no character you didn't type but letters and a contraction's apostrophe (never "t.b."
    /// for "tb"). One that splits what you typed into two words ("up-to" or "up to" for
    /// "upto") is used only as two words with a space, never hyphenated, and only when its
    /// letters are exactly yours (a missed space), both parts are words, and the checker
    /// offers the spaced form itself.
    func usableGuess(_ guess: String, typed token: String, among guesses: [String]) -> String? {
        let isSeparator: (Character) -> Bool = { $0 == " " || $0 == "-" }
        guard guess.contains(where: isSeparator) else {
            let typed = Set(token)
            let addsOnlyLetters = guess.allSatisfy { $0.isLetter || $0 == "'" || typed.contains($0) }
            return addsOnlyLetters ? guess : nil
        }
        let parts = guess.split(separator: " ", omittingEmptySubsequences: false)
            .flatMap { $0.split(separator: "-", omittingEmptySubsequences: false) }
            .map(String.init)
        guard parts.count == 2,
              parts.joined().lowercased() == token.lowercased(),
              parts.allSatisfy({ isSplitWord($0) })
        else { return nil }
        let spaced = parts.joined(separator: " ")
        guard guesses.contains(where: { ACOracle.sameIgnoringCase($0, spaced) }) else { return nil }
        return spaced
    }

    /// One half of a split guess: a word of two or more letters the dictionary knows, or
    /// English "a" and "I" (`UITextChecker` accepts every single letter as a word).
    func isSplitWord(_ part: String) -> Bool {
        guard !part.isEmpty, part.allSatisfy(\.isLetter) else { return false }
        if part.count == 1 {
            return language.acoUsesEnglishFixes && (part.lowercased() == "a" || part.lowercased() == "i")
        }
        return checker.isWordValid(part)
    }

    func rankedGuesses(for token: String) -> [String] {
        let lower = token.lowercased()
        let language = layoutLanguage
        let letters = layoutLetters
        let guesses = checker.guesses(for: token)
        return guesses
            .compactMap { usableGuess($0, typed: token, among: guesses) }
            .prefix(8)
            .enumerated()
            .sorted { a, b in
                let costA = Self.singleEditKeyboardCost(
                    from: lower,
                    to: a.element.replacingOccurrences(of: "'", with: "").lowercased(),
                    language: language,
                    letters: letters
                ) ?? (2.0 + Double(abs(a.element.count - token.count)) * 0.3)
                let costB = Self.singleEditKeyboardCost(
                    from: lower,
                    to: b.element.replacingOccurrences(of: "'", with: "").lowercased(),
                    language: language,
                    letters: letters
                ) ?? (2.0 + Double(abs(b.element.count - token.count)) * 0.3)
                return (costA + Double(a.offset) * 0.08) < (costB + Double(b.offset) * 0.08)
            }
            .map(\.element)
    }
}
