import Foundation

/// A word-level dictation change with how often you made it. Holds at most three words a
/// side and a word of context on each, never the dictation itself.
nonisolated struct TypingJournalDictationFix: Codable, Equatable, Sendable {
    var kind: DictationEditPair.Kind
    /// As the speech model heard it.
    var heard: String
    /// As Hex inserted it, after cleanup, formatting, spelling, and learned fixes.
    var inserted: String
    /// As you left it.
    var meant: String
    var count: Int
    /// Times you showed this fix is wrong: you kept the heard words in a dictation you
    /// corrected, changed the fix back, or fixed the other way round.
    var contradictions: Int
    var before: String?
    var after: String?
    var lastAt: TimeInterval
    /// The language it was dictated in (`LanguageLearning.dictationCode`); nil for a journal
    /// written before fixes carried one, and for a fix made in more than one language.
    var language: String? = nil

    var isSubstitution: Bool { kind == .substituted || kind == .redictated }

    /// Folds one dictation into the records: contradictions first-hand from what you kept
    /// or took back, then a count for each pair.
    static func merging(
        _ result: DictationEditResult,
        into records: [TypingJournalDictationFix],
        at now: TimeInterval,
        language: String? = nil
    ) -> [TypingJournalDictationFix] {
        var records = records
        for pair in result.pairs {
            let heardKey = DictationText.phraseKey(pair.heard)
            let insertedKey = DictationText.phraseKey(pair.inserted)
            if pair.kind == .substituted || pair.kind == .redictated {
                // Putting back what was heard, or a case change that isn't a capital, teaches nothing.
                guard pair.meant != pair.heard,
                      heardKey != DictationText.phraseKey(pair.meant) || pair.meant.contains(where: \.isUppercase)
                else { continue }
            }
            if let index = records.firstIndex(where: {
                $0.kind == pair.kind && $0.meant == pair.meant
                    && DictationText.phraseKey($0.heard) == heardKey
                    && DictationText.phraseKey($0.inserted) == insertedKey
            }) {
                records[index].count += 1
                records[index].lastAt = now
                records[index].before = pair.before
                records[index].after = pair.after
                // Fixed in two languages: it belongs to every one of them.
                if records[index].language != language { records[index].language = nil }
            } else {
                records.append(TypingJournalDictationFix(
                    kind: pair.kind, heard: pair.heard, inserted: pair.inserted, meant: pair.meant,
                    count: 1, contradictions: 0, before: pair.before, after: pair.after, lastAt: now,
                    language: language
                ))
            }
        }

        for index in records.indices where records[index].isSubstitution {
            if contradicts(result, records[index]) {
                records[index].contradictions += 1
            }
        }

        if records.count > TypingJournalSnapshot.dictationFixLimit {
            let drop = records.indices
                .sorted { (records[$0].count, records[$0].lastAt) < (records[$1].count, records[$1].lastAt) }
                .prefix(records.count - TypingJournalSnapshot.dictationFixLimit)
            for index in drop.sorted(by: >) { records.remove(at: index) }
        }
        return records
    }

    private static func contradicts(_ result: DictationEditResult, _ fix: TypingJournalDictationFix) -> Bool {
        let heardKey = DictationText.phraseKey(fix.heard)
        let meantKey = DictationText.phraseKey(fix.meant)
        let heardRun = heardKey.split(separator: " ").map(String.init)
        if heardKey != meantKey, result.keptHeard.contains(where: { $0.containsRun(heardRun) }) {
            return true
        }
        for pair in result.pairs where pair.kind == .substituted || pair.kind == .redictated {
            let pairHeard = DictationText.phraseKey(pair.heard)
            let pairInserted = DictationText.phraseKey(pair.inserted)
            let pairMeant = DictationText.phraseKey(pair.meant)
            // Hex applied this fix and you changed it to something else (not just extended
            // it, as with a possessive).
            let pairLetters = DictationText.letters(pair.meant)
            let fixLetters = DictationText.letters(fix.meant)
            let extended = pairLetters.count > fixLetters.count && pairLetters.hasPrefix(fixLetters)
            let changedFix = pairHeard == heardKey && pairInserted == meantKey && pair.meant != fix.meant && !extended
            // You fixed it the other way round.
            let reversed = pairMeant == heardKey && pairMeant != meantKey
                && (pairInserted == meantKey || pairHeard == meantKey)
            if changedFix || reversed { return true }
        }
        return false
    }
}

nonisolated struct TypingJournalDictationOutcome: Codable, Equatable, Sendable {
    var outcome: DictationOutcome
    var words: Int
    var changedWords: Int
    var at: TimeInterval
}

extension TypingJournalSnapshot {
    /// Fixes Hex applies to dictation, before the ones you switched off.
    var learnedDictationFixes: [DictationLearnedFix] {
        DictationLearningRule.learned(from: dictationFixes)
    }

    /// Every language's learned fixes, one per id: with Learn each language separately on, a
    /// fix made only in one language is listed too, and one made in two is listed once. A
    /// journal with no tagged fixes is the shared list.
    func learnedDictationFixesByLanguage() -> [DictationLearnedFix] {
        let languages = Set(dictationFixes.compactMap(\.language))
        guard !languages.isEmpty else { return learnedDictationFixes }
        var fixes: [DictationLearnedFix] = []
        var seen: Set<String> = []
        for language in languages.sorted() {
            for fix in DictationLearningRule.learned(
                from: dictationFixes.filter { $0.language == nil || $0.language == language }
            ) where seen.insert(fix.id).inserted {
                fixes.append(fix)
            }
        }
        return fixes
    }

    /// `language` keeps a dictation's fixes to those made in it and the ones never tagged
    /// (`Learn each language separately`); nil keeps every language's.
    func dictationCorrections(disabled: Set<String>, language: String? = nil) -> [DictationCorrection] {
        let fixes = if let language {
            DictationLearningRule.learned(from: dictationFixes.filter { $0.language == nil || $0.language == language })
        } else {
            learnedDictationFixes
        }
        return fixes.filter { !disabled.contains($0.id) }.map(\.correction)
    }

    func count(of outcome: DictationOutcome) -> Int {
        dictationOutcomes.reduce(0) { $0 + ($1.outcome == outcome ? 1 : 0) }
    }
}

/// What the keys learn from your corrections: typos you fix the same way, and
/// autocorrections you keep turning down.
nonisolated struct TypingLearning: Equatable, Sendable {
    static let minimumCount = 2

    /// Lowercased typo → the word you fixed it to, at least twice and only that way.
    var typos: [String: String] = [:]
    /// Lowercased words you kept as typed at least twice (undoing autocorrect with
    /// backspace, or tapping what you typed); autocorrect never rewrites them again.
    var rejected: Set<String> = []
    /// A typing config's own words, as spelled (also in `rejected`): offered and read as yours.
    var keepWords: [String] = []
    /// A typing config's fixes it only offers: lowercased typo → word.
    var suggestedFixes: [String: String] = [:]
    /// Words a typing config says autocorrect may never put in place of what you typed.
    var neverCorrectTo: [String] = []

    /// Whether autocorrect may not replace a word with `fix` (`neverCorrectTo`): exactly, for
    /// one with capitals ("DONT"), else in any case.
    func blocksCorrection(to fix: String) -> Bool {
        neverCorrectTo.contains { $0 == $0.lowercased() ? $0 == fix.lowercased() : $0 == fix }
    }

    /// One of a typing config's own words (`keepWords`).
    func keeps(_ lower: String) -> Bool { !keepWords.isEmpty && keepWords.contains { $0.lowercased() == lower } }

    /// This learning with a typing config's words on top, in memory only: what you taught Hex
    /// yourself wins (a typo you fixed another way, a word you kept).
    func adding(_ words: TypingConfigReview.Applied) -> TypingLearning {
        guard !words.keep.isEmpty || !words.neverCorrectTo.isEmpty || !words.fixes.isEmpty else { return self }
        var out = self
        out.keepWords = words.keep
        out.rejected.formUnion(words.keep.map { $0.lowercased() })
        out.neverCorrectTo = words.neverCorrectTo
        for fix in words.fixes {
            let typo = fix.typo.lowercased()
            guard out.typos[typo] == nil, !out.rejected.contains(typo) else { continue }
            switch fix.mode {
            case .apply: out.typos[typo] = fix.word
            case .suggest: out.suggestedFixes[typo] = fix.word
            }
        }
        return out
    }

    static func derive(from corrections: [TypingJournalCorrection]) -> TypingLearning {
        var rejections: [String: Int] = [:]
        for correction in corrections where correction.kind == .undone || correction.kind == .keptTyped {
            rejections[correction.final.lowercased(), default: 0] += 1
        }
        let rejected = Set(rejections.filter { $0.value >= minimumCount }.keys)

        // typo → lowercased word → (spelling, count)
        var fixes: [String: [String: (spelling: String, count: Int)]] = [:]
        for correction in corrections where (correction.kind == .retyped || correction.kind == .picked) && correction.teaches {
            let typo = correction.typed.lowercased()
            let word = correction.final.lowercased()
            guard typo.count >= 2, isWord(typo), isWord(word), typo != word,
                  !word.hasPrefix(typo) // a completion, not a fix
            else { continue }
            var entry = fixes[typo, default: [:]][word] ?? (spelling: correction.final, count: 0)
            entry.count += 1
            // Prefer the spelling without a sentence-start capital.
            if correction.final == word { entry.spelling = word }
            fixes[typo, default: [:]][word] = entry
        }

        var typos: [String: String] = [:]
        for (typo, words) in fixes where words.count == 1 && !rejected.contains(typo) {
            guard let only = words.first, only.value.count >= minimumCount,
                  fixes[only.key]?[typo] == nil // you never fixed it the other way round
            else { continue }
            typos[typo] = only.value.spelling
        }
        return TypingLearning(typos: typos, rejected: rejected)
    }

    /// Whether a word you erased (`typed`) is plausibly a slip of the one you typed in its place
    /// (`final`), so the pair may teach a fix. Plausible: the same word with other capitals or
    /// an apostrophe ("Im" → "I'm"); or, both two letters or more, starting alike (the same
    /// letter, a neighbouring key, or its first letter missed or one typed before it) and at
    /// most one slip for every four letters of `final` (at least one), where a neighbouring
    /// key, a missing accent, two letters swapped and a doubled or undoubled letter count half
    /// ("Ciry" → "City", "Diaxuss" → "Discuss"). A change of mind ("Sunda" → "Saturday",
    /// "is" → "this") or an unfinished word finished ("reqe" → "request") is no slip.
    /// Neighbours are on `language`'s own layout (`KeyboardLanguage.rawValue`; English when
    /// unknown).
    static func isSlip(typed: String, final: String, language: String? = nil) -> Bool {
        func plain(_ word: String) -> [Character] { Array(word.lowercased().filter { $0 != "'" && $0 != "’" }) }
        let a = plain(typed), b = plain(final)
        if a == b { return true }
        guard a.count >= 2, b.count >= 2, a.allSatisfy(\.isLetter), b.allSatisfy(\.isLetter) else { return false }
        let keys = keyCoordinates[KeyboardLanguage(rawValue: language ?? "") ?? .english] ?? [:]
        func near(_ x: Character, _ y: Character) -> Bool {
            if x == y || base(x) == base(y) { return true }
            guard let p = keys[x], let q = keys[y] else { return false }
            return hypot(p.0 - q.0, p.1 - q.1) <= 1.6
        }
        guard near(a[0], b[0]) || a[0] == b[1] || a[1] == b[0] else { return false }
        return slipCost(a, b, near: near) <= max(1, Double(b.count) / 4)
    }

    /// The weighted edit distance `isSlip` bounds: 0.5 for a neighbouring key or an accent, two
    /// letters swapped, or a doubled letter added or dropped; 1 for any other change.
    private static func slipCost(_ a: [Character], _ b: [Character], near: (Character, Character) -> Bool) -> Double {
        func step(_ s: [Character], _ i: Int) -> Double { i > 0 && s[i] == s[i - 1] ? 0.5 : 1 }
        var d = Array(repeating: Array(repeating: 0.0, count: b.count + 1), count: a.count + 1)
        for i in 1...a.count { d[i][0] = d[i - 1][0] + step(a, i - 1) }
        for j in 1...b.count { d[0][j] = d[0][j - 1] + step(b, j - 1) }
        for i in 1...a.count {
            for j in 1...b.count {
                let substitution = a[i - 1] == b[j - 1] ? 0 : near(a[i - 1], b[j - 1]) ? 0.5 : 1
                var cost = min(d[i - 1][j - 1] + substitution, d[i - 1][j] + step(a, i - 1), d[i][j - 1] + step(b, j - 1))
                if i > 1, j > 1, a[i - 1] == b[j - 2], a[i - 2] == b[j - 1] { cost = min(cost, d[i - 2][j - 2] + 0.5) }
                d[i][j] = cost
            }
        }
        return d[a.count][b.count]
    }

    private static func base(_ c: Character) -> Unicode.Scalar? {
        String(c).decomposedStringWithCanonicalMapping.unicodeScalars.first
    }

    /// Each language's letters on its own layout, for `isSlip`'s neighbouring keys.
    private static let keyCoordinates: [KeyboardLanguage: [Character: (Double, Double)]] = Dictionary(
        uniqueKeysWithValues: KeyboardLanguage.allCases.map { ($0, $0.keyCoordinates()) }
    )

    private static func isWord(_ text: String) -> Bool {
        !text.isEmpty && text.allSatisfy { $0.isLetter || $0 == "'" || $0 == "’" }
    }
}

extension TypingJournalCorrection {
    /// Whether this correction may be counted, listed and learned from: a word you erased and
    /// retyped only when it was a slip of the new one (`TypingLearning.isSlip`), so a change of
    /// mind ("Sunda" → "Saturday") teaches nothing, in a journal written before this check too.
    var teaches: Bool {
        kind != .retyped || TypingLearning.isSlip(typed: typed, final: final, language: language)
    }
}
