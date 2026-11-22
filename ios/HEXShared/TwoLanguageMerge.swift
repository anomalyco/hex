import Foundation

/// One word of one reading of a recording: what it says, when it was said, and how sure
/// Apple Speech was (0…1). The merge needs nothing from Speech, so it is testable on its own.
nonisolated struct TwoLanguageTimedWord: Equatable, Sendable {
    let text: String
    let start: TimeInterval
    let end: TimeInterval
    let confidence: Double

    init(text: String, start: TimeInterval, end: TimeInterval, confidence: Double) {
        self.text = text
        self.start = start
        self.end = end
        self.confidence = confidence
    }

    /// Where the word sits on the recording, which is what a segment holds.
    var midpoint: TimeInterval { (start + end) / 2 }

    /// How much audio it covers, so a long word counts more than a short one.
    var duration: TimeInterval { max(0, end - start) }

    /// The same word with a punctuation run's characters appended, its end stretched to cover
    /// them. A run Apple gave no time range for adds its characters and no time.
    func appending(_ characters: String, endingAt end: TimeInterval?) -> TwoLanguageTimedWord {
        TwoLanguageTimedWord(
            text: text + characters,
            start: start,
            end: max(self.end, end ?? self.end),
            confidence: confidence
        )
    }
}

/// Which reading supplied most of a merged dictation's duration. Logged, never inserted.
nonisolated enum TwoLanguageWinner: String, Equatable, Sendable {
    case primary
    case secondary
}

nonisolated struct TwoLanguageMerged: Equatable, Sendable {
    /// The chosen text: the primary's own characters, with a second reading's words where
    /// that reading won, its runs' spacing and punctuation kept.
    let text: String
    /// Which reading held the most audio, for the log only.
    let winner: TwoLanguageWinner
    /// Whether any word from the second reading survived into `text`. When false the caller
    /// keeps the primary's original text, so nothing about it changes.
    let usedSecondary: Bool
}

/// Experiments › Two-language dictation's merge: two readings of one recording, piece by
/// piece, whichever is more confident. Pure and free of Speech so it can be unit-tested;
/// `AppleSpeechTwoLanguage` extracts the words from Apple Speech's results.
nonisolated enum TwoLanguageMerger {
    /// Two readings that end a word this close together end it at the same moment: a place
    /// the language can change without cutting a word in half.
    static let cutTolerance: TimeInterval = 0.15

    /// What one language change costs, as seconds of confident audio. A single word that is
    /// only slightly better in the other language never pays for the change.
    static let switchCost: TimeInterval = 0.35

    /// The two readings merged into the text to insert and which one held most of the audio.
    /// Ties go to the primary reading.
    static func merge(
        primary: [TwoLanguageTimedWord],
        secondary: [TwoLanguageTimedWord]
    ) -> TwoLanguageMerged {
        let readings = [primary, secondary]
        guard !(primary.isEmpty && secondary.isEmpty) else {
            return TwoLanguageMerged(text: "", winner: .primary, usedSecondary: false)
        }
        let cuts = cutPoints(primary: primary, secondary: secondary)
        guard cuts.count > 1 else {
            return TwoLanguageMerged(text: "", winner: .primary, usedSecondary: false)
        }
        let segments = zip(cuts, cuts.dropFirst()).map {
            Segment(start: $0.0, end: $0.1)
        }
        let held = readings.map { reading in
            segments.enumerated().map { item in
                words(
                    in: reading,
                    segment: item.element,
                    isLast: item.offset == segments.count - 1
                )
            }
        }
        // Each reading's confident seconds in each segment: a word's confidence times the
        // part of it the segment holds, so covering less of the audio never wins by itself.
        let scores = segments.indices.map { index in
            readings.indices.map { state in
                score(of: held[state][index], in: segments[index])
            }
        }

        // A two-state Viterbi: the best total for ending each segment in either reading,
        // having paid `switchCost` for every change. The first segment is free to choose.
        var value = scores[0]
        var previous = [[Int]](repeating: [0, 0], count: segments.count)
        previous[0] = [-1, -1]
        if segments.count > 1 {
            for index in 1..<segments.count {
                var next = [-Double.infinity, -Double.infinity]
                for state in 0...1 {
                    for prior in 0...1 {
                        let candidate = value[prior] + scores[index][state]
                            - (prior == state ? 0 : switchCost)
                        if candidate > next[state] {
                            next[state] = candidate
                            previous[index][state] = prior
                        }
                    }
                }
                value = next
            }
        }

        var chosen = [Int](repeating: 0, count: segments.count)
        chosen[segments.count - 1] = value[0] >= value[1] ? 0 : 1
        if segments.count > 1 {
            for index in stride(from: segments.count - 2, through: 0, by: -1) {
                chosen[index] = previous[index + 1][chosen[index + 1]]
            }
        }

        // Keep each chosen piece's timed words until the join, so a seam duplicate can be
        // judged on the times of the words on both sides of it.
        var pieces: [(language: TwoLanguageWinner, words: [TwoLanguageTimedWord])] = []
        var lastKept: TwoLanguageTimedWord?
        var lastLanguage: TwoLanguageWinner?
        for index in segments.indices {
            let language: TwoLanguageWinner = chosen[index] == 0 ? .primary : .secondary
            var piece = held[chosen[index]][index]
            // The same spoken word can close one piece and open the other's; keep it once,
            // and only when its times really are the same word (half the shorter one).
            if language != lastLanguage,
               let first = piece.first,
               let last = lastKept,
               !normalized(first.text).isEmpty,
               normalized(first.text) == normalized(last.text),
               overlapsAtLeastHalf(first, last) {
                piece.removeFirst()
            }
            guard !piece.isEmpty else { continue }
            pieces.append((language: language, words: piece))
            lastKept = piece.last
            lastLanguage = language
        }

        var text = ""
        var previous: TwoLanguageWinner?
        for piece in pieces {
            if previous != nil, previous != piece.language,
               !(text.last?.isWhitespace ?? true),
               !(piece.words.first?.text.first?.isWhitespace ?? true) {
                text += " "
            }
            text += piece.words.map(\.text).joined()
            previous = piece.language
        }
        let usedSecondary = pieces.contains { $0.language == .secondary }

        var duration = [0.0, 0.0]
        for (index, segment) in segments.enumerated() {
            duration[chosen[index]] += segment.duration
        }
        return TwoLanguageMerged(
            text: text,
            winner: duration[0] >= duration[1] ? .primary : .secondary,
            usedSecondary: usedSecondary
        )
    }

    /// Every time the two readings agree on where a word ends, plus the start and end of the
    /// audio, at the earlier of the two ends. A word belongs to the segment its midpoint
    /// falls in.
    private static func cutPoints(
        primary: [TwoLanguageTimedWord],
        secondary: [TwoLanguageTimedWord]
    ) -> [TimeInterval] {
        var cuts: [TimeInterval] = [0]
        for left in primary {
            for right in secondary where abs(left.end - right.end) <= cutTolerance {
                cuts.append(min(left.end, right.end))
            }
        }
        cuts.append(max((primary + secondary).map(\.end).max() ?? 0, 0))
        cuts.sort()
        var unique: [TimeInterval] = []
        for cut in cuts where cut >= 0 && (unique.last.map { cut - $0 > 1e-6 } ?? true) {
            unique.append(cut)
        }
        return unique
    }

    private static func words(
        in reading: [TwoLanguageTimedWord],
        segment: Segment,
        isLast: Bool
    ) -> [TwoLanguageTimedWord] {
        reading.filter { word in
            let midpoint = word.midpoint
            if isLast {
                return midpoint >= segment.start && midpoint <= segment.end
            }
            return midpoint >= segment.start && midpoint < segment.end
        }
    }

    /// A reading's confidence-weighted seconds in a segment: each word's confidence times the
    /// part of its span the segment holds.
    private static func score(of words: [TwoLanguageTimedWord], in segment: Segment) -> Double {
        words.reduce(0) { total, word in
            let covered = max(0, min(word.end, segment.end) - max(word.start, segment.start))
            return total + word.confidence * covered
        }
    }

    /// Whether two readings timed the same spoken word: their spans overlap by at least half
    /// of the shorter one. Two words that only repeat the same text apart in time are both
    /// kept.
    private static func overlapsAtLeastHalf(
        _ first: TwoLanguageTimedWord,
        _ last: TwoLanguageTimedWord
    ) -> Bool {
        let overlap = min(first.end, last.end) - max(first.start, last.start)
        let shorter = min(first.duration, last.duration)
        guard shorter > 0 else { return false }
        return overlap >= shorter * 0.5
    }

    /// Case and punctuation do not matter when the same word sits on both sides of a change
    /// ("Anna," and "anna").
    private static func normalized(_ word: String) -> String {
        word.lowercased().filter { $0.isLetter || $0.isNumber }
    }

    private struct Segment {
        let start: TimeInterval
        let end: TimeInterval

        var duration: TimeInterval { max(0, end - start) }
    }
}
