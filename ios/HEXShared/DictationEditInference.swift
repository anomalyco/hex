import Foundation

// Learn how I type, dictation side: from what Hex inserted and what the field holds a
// little later, infer whether a dictation was kept, edited, or taken back, and which
// words were wrong. Everything here is pure so it can be tested without a keyboard.

/// A word in dictated or typed text: letters and digits, with apostrophes inside it.
nonisolated struct DictationWord: Equatable, Sendable {
    let text: String
    let range: Range<String.Index>
}

nonisolated enum DictationText {
    static func words(in text: String) -> [DictationWord] {
        var result: [DictationWord] = []
        var start: String.Index?
        var index = text.startIndex
        while index < text.endIndex {
            let character = text[index]
            let next = text.index(after: index)
            let isWordCharacter = character.isLetter || character.isNumber
                || (isApostrophe(character) && start != nil && next < text.endIndex && text[next].isLetter)
            if isWordCharacter {
                if start == nil { start = index }
            } else if let wordStart = start {
                result.append(DictationWord(text: String(text[wordStart..<index]), range: wordStart..<index))
                start = nil
            }
            index = next
        }
        if let wordStart = start {
            result.append(DictationWord(text: String(text[wordStart...]), range: wordStart..<text.endIndex))
        }
        return result
    }

    static func isApostrophe(_ character: Character) -> Bool {
        character == "'" || character == "’"
    }

    /// How two words compare: lowercased, with curly apostrophes made straight.
    static func key(_ word: String) -> String {
        word.lowercased().replacingOccurrences(of: "’", with: "'")
    }

    /// A phrase's word keys joined by single spaces.
    static func phraseKey(_ phrase: String) -> String {
        words(in: phrase).map { key($0.text) }.joined(separator: " ")
    }

    /// Letters and digits only, lowercased, so spacing and punctuation don't count.
    static func letters(_ text: String) -> String {
        text.lowercased().filter { $0.isLetter || $0.isNumber }
    }

    static func distance(_ a: String, _ b: String) -> Int {
        let a = Array(a), b = Array(b)
        if a.isEmpty { return b.count }
        if b.isEmpty { return a.count }
        var previous = Array(0...b.count)
        for i in 1...a.count {
            var row = [i] + Array(repeating: 0, count: b.count)
            for j in 1...b.count {
                row[j] = min(previous[j] + 1, row[j - 1] + 1, previous[j - 1] + (a[i - 1] == b[j - 1] ? 0 : 1))
            }
            previous = row
        }
        return previous[b.count]
    }
}

// MARK: - Where the dictation is now

/// A dictation Hex inserted, remembered by the text right around it rather than by
/// position, because the host only shows the text near the cursor and it moves.
nonisolated struct DictationInsertion: Equatable, Sendable {
    static let anchorLength = 32

    let inserted: String
    let anchorBefore: String
    let anchorAfter: String

    init(inserted: String, before: String?, after: String?) {
        self.inserted = inserted
        anchorBefore = String((before ?? "").suffix(Self.anchorLength))
        anchorAfter = String((after ?? "").prefix(Self.anchorLength))
    }
}

/// The text that now sits where the dictation went.
nonisolated struct DictationSpan: Equatable, Sendable {
    var text: String
    /// False when the start can't be told from the host cutting its context short
    /// (the dictation began the field and text now sits before the cursor).
    var startIsCertain: Bool
    /// False when the end can't be told from the host cutting its context short.
    var endIsCertain: Bool

    var hasWords: Bool { !DictationText.words(in: text).isEmpty }
}

nonisolated enum DictationSpanLocator {
    /// Finds the dictation between its anchors in the text around the cursor, or nil
    /// when the context is missing or the anchors are gone (another field, or edited).
    static func locate(_ insertion: DictationInsertion, before: String?, after: String?) -> DictationSpan? {
        guard before != nil || after != nil else { return nil }
        let currentBefore = before ?? ""
        let currentAfter = after ?? ""
        let text = currentBefore + currentAfter
        let starts: [String.Index] = insertion.anchorBefore.isEmpty
            ? [text.startIndex]
            : text.ranges(of: insertion.anchorBefore).map(\.upperBound)
        let ends: [String.Index] = insertion.anchorAfter.isEmpty
            ? [text.endIndex]
            : text.ranges(of: insertion.anchorAfter).map(\.lowerBound)
        guard !starts.isEmpty, !ends.isEmpty else { return nil }

        // With repeated anchors, take the pair whose gap is closest to the dictation's length.
        let expected = insertion.inserted.count
        var best: (range: Range<String.Index>, miss: Int)?
        for start in starts.prefix(16) {
            for end in ends.prefix(16) where start <= end {
                let miss = abs(text.distance(from: start, to: end) - expected)
                if best == nil || miss < best!.miss {
                    best = (start..<end, miss)
                }
            }
        }
        guard let range = best?.range,
              text.distance(from: range.lowerBound, to: range.upperBound) <= expected * 4 + 200
        else { return nil }
        return DictationSpan(
            text: String(text[range]),
            startIsCertain: !insertion.anchorBefore.isEmpty || currentBefore.isEmpty,
            endIsCertain: !insertion.anchorAfter.isEmpty || currentAfter.isEmpty
        )
    }
}

// MARK: - Word diff

nonisolated enum DictationWordDiff {
    /// A run of old words replaced by a run of new words; either side may be empty.
    struct Hunk: Equatable, Sendable {
        var old: Range<Int>
        var new: Range<Int>
    }

    static let maxCells = 250_000

    /// The changes from `old` to `new`, by longest common subsequence of whole words.
    static func hunks(_ old: [String], _ new: [String]) -> [Hunk] {
        var prefix = 0
        while prefix < old.count, prefix < new.count, old[prefix] == new[prefix] {
            prefix += 1
        }
        var suffix = 0
        while suffix < old.count - prefix, suffix < new.count - prefix,
              old[old.count - 1 - suffix] == new[new.count - 1 - suffix] {
            suffix += 1
        }
        let a = Array(old[prefix..<(old.count - suffix)])
        let b = Array(new[prefix..<(new.count - suffix)])
        if a.isEmpty && b.isEmpty { return [] }
        if a.isEmpty || b.isEmpty || a.count * b.count > maxCells {
            return [Hunk(old: prefix..<(prefix + a.count), new: prefix..<(prefix + b.count))]
        }

        let n = a.count, m = b.count
        var table = Array(repeating: Array(repeating: 0, count: m + 1), count: n + 1)
        for i in stride(from: n - 1, through: 0, by: -1) {
            for j in stride(from: m - 1, through: 0, by: -1) {
                table[i][j] = a[i] == b[j] ? table[i + 1][j + 1] + 1 : max(table[i + 1][j], table[i][j + 1])
            }
        }

        var hunks: [Hunk] = []
        var i = 0, j = 0
        var open: (i: Int, j: Int)?
        while i < n || j < m {
            if i < n, j < m, a[i] == b[j] {
                if let start = open {
                    hunks.append(Hunk(old: (prefix + start.i)..<(prefix + i), new: (prefix + start.j)..<(prefix + j)))
                    open = nil
                }
                i += 1
                j += 1
            } else {
                if open == nil { open = (i, j) }
                if j >= m || (i < n && table[i + 1][j] >= table[i][j + 1]) {
                    i += 1
                } else {
                    j += 1
                }
            }
        }
        if let start = open {
            hunks.append(Hunk(old: (prefix + start.i)..<(prefix + n), new: (prefix + start.j)..<(prefix + m)))
        }
        return hunks
    }
}

// MARK: - What happened to a dictation

nonisolated enum DictationOutcome: String, Codable, CaseIterable, Sendable {
    /// Sent or left as Hex inserted it.
    case kept
    /// Some words fixed.
    case edited
    /// More than half of it changed: not word fixes.
    case rewritten
    /// Taken back with Undo dictation.
    case undone
    /// Deleted soon after.
    case deleted
    /// Deleted and dictated again soon after.
    case redictated
}

/// One word-level change: never more than three words on a side, plus a word of context.
nonisolated struct DictationEditPair: Equatable, Sendable {
    enum Kind: String, Codable, Sendable {
        /// You replaced words Hex inserted.
        case substituted
        /// You added words inside the dictation.
        case inserted
        /// You removed words from it.
        case deleted
        /// You added back words the speech model heard but cleanup or formatting dropped.
        case restored
        /// You removed a filler Hex left in.
        case fillerRemoved
        /// You dictated it again and these words came out differently.
        case redictated
    }

    var kind: Kind
    /// The words as the speech model heard them (raw transcript), or as inserted when unknown.
    var heard: String
    /// The words as Hex inserted them, after cleanup, formatting, and spelling.
    var inserted: String
    /// The words you ended up with.
    var meant: String
    var before: String?
    var after: String?
}

nonisolated struct DictationEditResult: Equatable, Sendable {
    var outcome: DictationOutcome
    var pairs: [DictationEditPair] = []
    var wordCount: Int
    var changedWordCount: Int = 0
    /// Runs of heard word keys left exactly as heard in a dictation you corrected
    /// elsewhere, so you saw them and kept them. Never stored; used to contradict fixes.
    var keptHeard: [[String]] = []
}

nonisolated enum DictationEditClassifier {
    static let maxPairWords = 3
    static let maxWords = 400

    static let fillers: Set<String> = [
        "um", "umm", "uh", "uhh", "uhm", "er", "erm", "ah", "hmm", "mm", "like", "so",
        "basically", "actually", "literally", "you know", "i mean", "kind of", "sort of",
    ]

    /// Classifies a dictation against the text now in its place. Nil when there is
    /// nothing to judge (no words, or far too long).
    static func classify(heard: String?, inserted: String, final span: DictationSpan, undone: Bool = false) -> DictationEditResult? {
        let insertedWords = DictationText.words(in: inserted)
        guard !insertedWords.isEmpty, insertedWords.count <= maxWords else { return nil }
        let finalWords = DictationText.words(in: span.text)
        if finalWords.isEmpty {
            return DictationEditResult(
                outcome: undone ? .undone : .deleted,
                wordCount: insertedWords.count,
                changedWordCount: insertedWords.count
            )
        }
        guard finalWords.count <= maxWords * 2 else { return nil }

        let ins = insertedWords.map(\.text)
        let fin = finalWords.map(\.text)
        let allHunks = DictationWordDiff.hunks(ins, fin)
        let hunks = trimEdges(allHunks, old: ins, new: fin, span: span)
        guard !hunks.isEmpty else {
            return DictationEditResult(outcome: .kept, wordCount: ins.count)
        }

        let changed = hunks.reduce(0) { $0 + max($1.old.count, $1.new.count) }
        guard Double(changed) <= max(1, Double(ins.count) / 2) else {
            return DictationEditResult(outcome: .rewritten, wordCount: ins.count, changedWordCount: changed)
        }

        let heardWords = heard.map { DictationText.words(in: $0).map(\.text) } ?? ins
        let alignment = HeardAlignment(heard: heardWords.isEmpty ? ins : heardWords, inserted: ins)
        let startsSentence = ins.first?.first?.isUppercase ?? false
        var pairs: [DictationEditPair] = []
        for hunk in hunks where hunk.old.count <= maxPairWords && hunk.new.count <= maxPairWords {
            let insertedText = ins[hunk.old].joined(separator: " ")
            var meantText = fin[hunk.new].joined(separator: " ")
            let before = hunk.old.lowerBound > 0 ? ins[hunk.old.lowerBound - 1] : nil
            let after = hunk.old.upperBound < ins.count ? ins[hunk.old.upperBound] : nil

            if !hunk.old.isEmpty && !hunk.new.isEmpty {
                let heardText = alignment.heard(for: hunk.old) ?? insertedText
                guard isPlausibleFix(insertedText, meantText) || isPlausibleFix(heardText, meantText) else { continue }
                let sentenceStart = hunk.new.lowerBound == 0
                    ? startsSentence
                    : isSentenceStart(finalWords[hunk.new.lowerBound], in: span.text)
                meantText = normalizedMeant(meantText, sentenceStart: sentenceStart)
                guard meantText != insertedText else { continue }
                pairs.append(DictationEditPair(
                    kind: .substituted, heard: heardText, inserted: insertedText, meant: meantText,
                    before: before, after: after
                ))
            } else if hunk.old.isEmpty {
                let removed = alignment.removed[hunk.old.lowerBound] ?? []
                let added = fin[hunk.new].map(DictationText.key)
                let wasHeard = removed.map(DictationText.key).containsRun(added)
                pairs.append(DictationEditPair(
                    kind: wasHeard ? .restored : .inserted,
                    heard: wasHeard ? meantText : "", inserted: "", meant: meantText,
                    before: before, after: after
                ))
            } else {
                let heardText = alignment.heard(for: hunk.old) ?? insertedText
                let isFiller = fillers.contains(DictationText.phraseKey(insertedText))
                pairs.append(DictationEditPair(
                    kind: isFiller ? .fillerRemoved : .deleted,
                    heard: heardText, inserted: insertedText, meant: "",
                    before: before, after: after
                ))
            }
        }

        return DictationEditResult(
            outcome: .edited,
            pairs: pairs,
            wordCount: ins.count,
            changedWordCount: changed,
            keptHeard: alignment.keptRuns(excluding: allHunks, insertedCount: ins.count)
        )
    }

    /// A dictation you removed and dictated again within the redictation window: the
    /// words that came out differently the second time, when most of it is the same.
    static func redictation(previousHeard: String?, previousInserted: String, nextInserted: String) -> DictationEditResult? {
        let oldWords = DictationText.words(in: previousInserted)
        guard !oldWords.isEmpty, oldWords.count <= maxWords else { return nil }
        let newWords = DictationText.words(in: nextInserted)
        let old = oldWords.map(\.text)
        let new = newWords.map(\.text)
        var result = DictationEditResult(outcome: .redictated, wordCount: old.count, changedWordCount: old.count)
        guard !new.isEmpty, new.count <= maxWords * 2 else { return result }

        let hunks = DictationWordDiff.hunks(old.map(DictationText.key), new.map(DictationText.key))
        let changed = hunks.reduce(0) { $0 + max($1.old.count, $1.new.count) }
        result.changedWordCount = changed
        guard Double(changed) <= max(1, Double(old.count) / 2) else { return result }

        let heardWords = previousHeard.map { DictationText.words(in: $0).map(\.text) } ?? old
        let alignment = HeardAlignment(heard: heardWords.isEmpty ? old : heardWords, inserted: old)
        let startsSentence = new.first?.first?.isUppercase ?? false
        for hunk in hunks where !hunk.old.isEmpty && !hunk.new.isEmpty
            && hunk.old.count <= maxPairWords && hunk.new.count <= maxPairWords {
            let insertedText = old[hunk.old].joined(separator: " ")
            let heardText = alignment.heard(for: hunk.old) ?? insertedText
            var meantText = new[hunk.new].joined(separator: " ")
            guard isPlausibleFix(insertedText, meantText) || isPlausibleFix(heardText, meantText) else { continue }
            let sentenceStart = hunk.new.lowerBound == 0
                ? startsSentence
                : isSentenceStart(newWords[hunk.new.lowerBound], in: nextInserted)
            meantText = normalizedMeant(meantText, sentenceStart: sentenceStart)
            guard meantText != insertedText else { continue }
            result.pairs.append(DictationEditPair(
                kind: .redictated, heard: heardText, inserted: insertedText, meant: meantText,
                before: hunk.old.lowerBound > 0 ? old[hunk.old.lowerBound - 1] : nil,
                after: hunk.old.upperBound < old.count ? old[hunk.old.upperBound] : nil
            ))
        }
        return result
    }

    /// Whether `meant` could be a fix of `heard` rather than different words altogether.
    static func isPlausibleFix(_ heard: String, _ meant: String) -> Bool {
        let a = DictationText.letters(heard)
        let b = DictationText.letters(meant)
        guard !a.isEmpty, !b.isEmpty else { return false }
        if a == b { return true }
        return DictationText.distance(a, b) <= max(2, max(a.count, b.count) / 2)
    }

    /// Edits at the very edges of the span are the least certain: words typed before or
    /// after it are new writing, and when the host cut the context short a missing edge
    /// is truncation, not a deletion.
    private static func trimEdges(
        _ hunks: [DictationWordDiff.Hunk],
        old: [String],
        new: [String],
        span: DictationSpan
    ) -> [DictationWordDiff.Hunk] {
        var result = hunks
        if let first = result.first, first.old.lowerBound == 0, first.new.lowerBound == 0 {
            let truncated = !span.startIsCertain
                && (first.new.isEmpty || isPartialWord(new[first.new.lowerBound], of: old[first.old], suffix: true))
            if first.old.isEmpty || truncated {
                result.removeFirst()
            } else if first.new.count > first.old.count {
                let keep = bestLength(old: old[first.old], new: new[first.new], fromEnd: true)
                result[0].new = (first.new.upperBound - keep)..<first.new.upperBound
            }
        }
        if let last = result.last, last.old.upperBound == old.count, last.new.upperBound == new.count {
            let truncated = !span.endIsCertain
                && (last.new.isEmpty || isPartialWord(new[last.new.upperBound - 1], of: old[last.old], suffix: false))
            if last.old.isEmpty || truncated {
                result.removeLast()
            } else if last.new.count > last.old.count {
                let keep = bestLength(old: old[last.old], new: new[last.new], fromEnd: false)
                result[result.count - 1].new = last.new.lowerBound..<(last.new.lowerBound + keep)
            }
        }
        return result
    }

    /// A word the host cut in half: a proper suffix (at the start) or prefix (at the end) of an old word.
    private static func isPartialWord(_ word: String, of old: ArraySlice<String>, suffix: Bool) -> Bool {
        let key = DictationText.key(word)
        return old.contains { candidate in
            let full = DictationText.key(candidate)
            return full.count > key.count && (suffix ? full.hasSuffix(key) : full.hasPrefix(key))
        }
    }

    /// How many of the new words replace the old ones; the rest is new writing.
    private static func bestLength(old: ArraySlice<String>, new: ArraySlice<String>, fromEnd: Bool) -> Int {
        let target = DictationText.letters(old.joined())
        var best = 1
        var bestScore = Int.max
        for length in 1...min(new.count, maxPairWords + 1) {
            let slice = fromEnd ? new.suffix(length) : new.prefix(length)
            let score = DictationText.distance(target, DictationText.letters(slice.joined()))
            if score < bestScore {
                best = length
                bestScore = score
            }
        }
        return best
    }

    /// Whether a word begins a sentence: after a period, question or exclamation mark, or a line break.
    private static func isSentenceStart(_ word: DictationWord, in text: String) -> Bool {
        var index = word.range.lowerBound
        while index > text.startIndex {
            index = text.index(before: index)
            let character = text[index]
            if character.isNewline { return true }
            if character.isWhitespace || "\"“”'‘’([".contains(character) { continue }
            return ".!?…".contains(character)
        }
        return true
    }

    /// A capital that only marks a sentence start isn't part of how you spell the word.
    private static func normalizedMeant(_ meant: String, sentenceStart: Bool) -> String {
        guard sentenceStart, let first = meant.first, first.isUppercase,
              !meant.dropFirst().contains(where: \.isUppercase) else { return meant }
        let firstWord = DictationText.words(in: meant).first.map { DictationText.key($0.text) } ?? ""
        guard !["i", "i'm", "i'll", "i've", "i'd"].contains(firstWord) else { return meant }
        return first.lowercased() + meant.dropFirst()
    }
}

/// Which raw (heard) words each inserted word came from, by a case-insensitive diff.
private nonisolated struct HeardAlignment {
    let heard: [String]
    /// The heard words behind each inserted word; nil where processing added the word.
    private(set) var source: [Range<Int>?]
    /// Whether each inserted word is exactly a heard word (ignoring case).
    private(set) var same: [Bool]
    /// Heard words processing dropped just before each inserted position.
    private(set) var removed: [Int: [String]] = [:]

    init(heard: [String], inserted: [String]) {
        self.heard = heard
        source = Array(repeating: nil, count: inserted.count)
        same = Array(repeating: false, count: inserted.count)
        let hunks = DictationWordDiff.hunks(heard.map(DictationText.key), inserted.map(DictationText.key))
        var h = 0
        var i = 0
        for hunk in hunks {
            while i < hunk.new.lowerBound, h < hunk.old.lowerBound {
                source[i] = h..<(h + 1)
                same[i] = true
                h += 1
                i += 1
            }
            if hunk.new.isEmpty {
                removed[hunk.new.lowerBound] = Array(heard[hunk.old])
            } else if !hunk.old.isEmpty {
                for k in hunk.new { source[k] = hunk.old }
            }
            h = hunk.old.upperBound
            i = hunk.new.upperBound
        }
        while i < inserted.count, h < heard.count {
            source[i] = h..<(h + 1)
            same[i] = true
            h += 1
            i += 1
        }
    }

    func heard(for range: Range<Int>) -> String? {
        guard !range.isEmpty else { return nil }
        var lower = Int.max
        var upper = Int.min
        for index in range {
            guard let heardRange = source[index] else { return nil }
            lower = min(lower, heardRange.lowerBound)
            upper = max(upper, heardRange.upperBound)
        }
        return heard[lower..<upper].joined(separator: " ")
    }

    /// Runs of heard keys for inserted words that are exactly as heard and untouched by the edit.
    func keptRuns(excluding hunks: [DictationWordDiff.Hunk], insertedCount: Int) -> [[String]] {
        var touched = Array(repeating: false, count: insertedCount)
        for hunk in hunks {
            for index in hunk.old { touched[index] = true }
        }
        var runs: [[String]] = []
        var run: [String] = []
        for index in 0..<insertedCount {
            if !touched[index], same[index], let heardRange = source[index] {
                run.append(DictationText.key(heard[heardRange.lowerBound]))
            } else if !run.isEmpty {
                runs.append(run)
                run = []
            }
        }
        if !run.isEmpty { runs.append(run) }
        return runs
    }
}

extension Array where Element == String {
    /// Whether `run` appears in order, without gaps, inside this array.
    nonisolated func containsRun(_ run: [String]) -> Bool {
        guard !run.isEmpty, run.count <= count else { return false }
        for start in 0...(count - run.count) where Array(self[start..<(start + run.count)]) == run {
            return true
        }
        return false
    }
}
