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

    static func derive(from corrections: [TypingJournalCorrection]) -> TypingLearning {
        var rejections: [String: Int] = [:]
        for correction in corrections where correction.kind == .undone || correction.kind == .keptTyped {
            rejections[correction.final.lowercased(), default: 0] += 1
        }
        let rejected = Set(rejections.filter { $0.value >= minimumCount }.keys)

        // typo → lowercased word → (spelling, count)
        var fixes: [String: [String: (spelling: String, count: Int)]] = [:]
        for correction in corrections where correction.kind == .retyped || correction.kind == .picked {
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

    private static func isWord(_ text: String) -> Bool {
        !text.isEmpty && text.allSatisfy { $0.isLetter || $0 == "'" || $0 == "’" }
    }
}
