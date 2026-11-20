import Foundation

/// One learned dictation fix: words the speech model keeps getting wrong, and what you meant.
nonisolated struct DictationCorrection: Equatable, Sendable {
    let heard: String
    let meant: String
}

/// Applies learned dictation fixes after transcription, with word boundaries and case.
nonisolated enum DictationCorrections {
    static let maxPhraseWords = DictationEditClassifier.maxPairWords
    private static let joiners: Set<Character> = ["-", "_", "/", "\\", "@", "#", "&", "+", "=", "~", "*", "."]

    /// Replaces each heard word or phrase (matched ignoring case, whole words only, never
    /// inside a longer word or a hyphenated, dotted, or slashed token) with what you meant.
    /// The meant spelling is kept when it has capitals; otherwise it follows the original's
    /// capitalization, so a fix at a sentence start still starts the sentence.
    static func apply(_ corrections: [DictationCorrection], to text: String) -> String {
        guard !corrections.isEmpty, !text.isEmpty else { return text }
        var table: [[String]: String] = [:]
        var longest = 1
        for correction in corrections {
            let keys = DictationText.words(in: correction.heard).map { DictationText.key($0.text) }
            let meant = correction.meant.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !keys.isEmpty, keys.count <= maxPhraseWords, !meant.isEmpty, table[keys] == nil else { continue }
            table[keys] = meant
            longest = max(longest, keys.count)
        }
        guard !table.isEmpty else { return text }

        let words = DictationText.words(in: text)
        var output = ""
        output.reserveCapacity(text.count)
        var copied = text.startIndex
        var index = 0
        while index < words.count {
            var matched = 0
            for count in stride(from: min(longest, words.count - index), through: 1, by: -1) {
                let window = Array(words[index..<(index + count)])
                guard isJoinedBySpaces(window, in: text) else { continue }
                let lower = window[0].range.lowerBound
                var upper = window[count - 1].range.upperBound
                var keys = window.map { DictationText.key($0.text) }
                var suffix = ""
                var meant = table[keys]
                if meant == nil, let last = keys.last, last.count > 2, last.hasSuffix("'s") {
                    keys[count - 1] = String(last.dropLast(2))
                    if let stemMeant = table[keys] {
                        meant = stemMeant
                        suffix = String(window[count - 1].text.suffix(2))
                        upper = text.index(upper, offsetBy: -2)
                    }
                }
                guard let meant, isBounded(lower..<window[count - 1].range.upperBound, in: text) else { continue }
                output.append(contentsOf: text[copied..<lower])
                output.append(matchCase(meant, like: String(text[lower..<upper])))
                output.append(suffix)
                copied = window[count - 1].range.upperBound
                matched = count
                break
            }
            index += max(1, matched)
        }
        output.append(contentsOf: text[copied...])
        return output
    }

    /// The custom vocabulary terms, then the meant words of learned fixes, without
    /// duplicates and within Apple's contextual-string limit.
    static func recognitionHints(vocabulary: [String], corrections: [DictationCorrection]) -> [String] {
        var seen = Set<String>()
        var hints: [String] = []
        for term in vocabulary + corrections.map(\.meant) {
            let key = VocabularySpelling.normalizedKey(term)
            guard key.count >= 2, seen.insert(key).inserted else { continue }
            hints.append(term)
            if hints.count == CustomVocabularySettings.maxTermCount { break }
        }
        return hints
    }

    /// Learned spellings with capitals ("Claude", "OpenCode"), which formatting must not undo.
    static func spellings(_ corrections: [DictationCorrection]) -> [String] {
        corrections.map(\.meant).filter { $0.contains(where: \.isUppercase) }
    }

    static func matchCase(_ meant: String, like original: String) -> String {
        let letters = original.filter(\.isLetter)
        if letters.count >= 2, letters == letters.uppercased(), letters != letters.lowercased() {
            return meant.uppercased()
        }
        if meant.contains(where: \.isUppercase) { return meant }
        if let first = original.first(where: \.isLetter), first.isUppercase, let head = meant.first {
            return head.uppercased() + meant.dropFirst()
        }
        return meant
    }

    private static func isJoinedBySpaces(_ window: [DictationWord], in text: String) -> Bool {
        for index in window.indices.dropFirst() {
            let gap = text[window[index - 1].range.upperBound..<window[index].range.lowerBound]
            guard !gap.isEmpty, gap.allSatisfy({ $0 == " " || $0 == "\t" }) else { return false }
        }
        return true
    }

    /// Not part of a longer token: no joiner right before, and none right after except a
    /// sentence-ending period.
    private static func isBounded(_ range: Range<String.Index>, in text: String) -> Bool {
        if range.lowerBound > text.startIndex, joiners.contains(text[text.index(before: range.lowerBound)]) {
            return false
        }
        guard range.upperBound < text.endIndex else { return true }
        let next = text[range.upperBound]
        guard joiners.contains(next) else { return true }
        guard next == "." else { return false }
        let afterDot = text.index(after: range.upperBound)
        guard afterDot < text.endIndex else { return true }
        let following = text[afterDot]
        return following.isWhitespace || following == "." || "\"”’')]".contains(following)
    }
}

/// A learned dictation fix as Typing insights shows it.
nonisolated struct DictationLearnedFix: Equatable, Sendable, Identifiable {
    var id: String
    var heard: String
    var meant: String
    var count: Int
    var contradictions: Int

    var correction: DictationCorrection { DictationCorrection(heard: heard, meant: meant) }
}

/// When a (heard → meant) substitution becomes a fix Hex applies.
nonisolated enum DictationLearningRule {
    static let minimumCount = 2

    /// Every substitution you made (by editing or dictating again), grouped by heard
    /// words and meant words ignoring case. A capital you used is kept as the spelling.
    static func candidates(from records: [TypingJournalDictationFix]) -> [DictationLearnedFix] {
        var groups: [String: DictationLearnedFix] = [:]
        var order: [String] = []
        for record in records where record.kind == .substituted || record.kind == .redictated {
            let heardKey = DictationText.phraseKey(record.heard)
            let meantKey = DictationText.phraseKey(record.meant)
            guard !heardKey.isEmpty, !meantKey.isEmpty, record.meant != record.heard else { continue }
            let id = heardKey + " → " + meantKey
            var group = groups[id] ?? DictationLearnedFix(id: id, heard: heardKey, meant: record.meant, count: 0, contradictions: 0)
            if groups[id] == nil { order.append(id) }
            group.count += record.count
            group.contradictions += record.contradictions
            if record.meant.contains(where: \.isUppercase) { group.meant = record.meant }
            groups[id] = group
        }
        return order.compactMap { groups[$0] }
    }

    /// Fixes seen at least twice, never contradicted, and whose heard words you only ever
    /// fixed one way. Anything else stays a record and is never applied.
    static func learned(from records: [TypingJournalDictationFix]) -> [DictationLearnedFix] {
        let all = candidates(from: records)
        var meanings: [String: Int] = [:]
        for fix in all { meanings[fix.heard, default: 0] += 1 }
        return all.filter { fix in
            fix.count >= minimumCount && fix.contradictions == 0 && meanings[fix.heard] == 1
        }
    }
}
