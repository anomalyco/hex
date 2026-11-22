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
}

/// Which reading supplied most of a merged dictation's duration. Logged, never inserted.
nonisolated enum TwoLanguageWinner: String, Equatable, Sendable {
    case primary
    case secondary
}

nonisolated struct TwoLanguageMerged: Equatable, Sendable {
    /// The chosen words joined with single spaces.
    let text: String
    /// Which reading held the most audio, for the log only.
    let winner: TwoLanguageWinner
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
            return TwoLanguageMerged(text: "", winner: .primary)
        }
        let cuts = cutPoints(primary: primary, secondary: secondary)
        guard cuts.count > 1 else {
            return TwoLanguageMerged(text: "", winner: .primary)
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
        // Each reading's score in each segment, weighted by how much audio the segment holds.
        let scores = segments.indices.map { index in
            readings.indices.map { state in
                score(of: held[state][index]) * segments[index].duration
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

        var kept: [String] = []
        var lastKept: TwoLanguageWinner?
        for index in segments.indices {
            let winner: TwoLanguageWinner = chosen[index] == 0 ? .primary : .secondary
            var piece = held[chosen[index]][index]
            // The same word can close one reading's piece and open the other's: keep it once.
            if winner != lastKept,
               let first = piece.first,
               let last = kept.last,
               !normalized(first.text).isEmpty,
               normalized(first.text) == normalized(last) {
                piece.removeFirst()
            }
            guard !piece.isEmpty else { continue }
            kept.append(contentsOf: piece.map(\.text))
            lastKept = winner
        }

        var duration = [0.0, 0.0]
        for (index, segment) in segments.enumerated() {
            duration[chosen[index]] += segment.duration
        }
        return TwoLanguageMerged(
            text: kept.joined(separator: " "),
            winner: duration[0] >= duration[1] ? .primary : .secondary
        )
    }

    /// Every time the two readings agree on where a word ends, plus the start and end of the
    /// audio. A word belongs to the segment its midpoint falls in.
    private static func cutPoints(
        primary: [TwoLanguageTimedWord],
        secondary: [TwoLanguageTimedWord]
    ) -> [TimeInterval] {
        var cuts: [TimeInterval] = [0]
        for left in primary {
            for right in secondary where abs(left.end - right.end) <= cutTolerance {
                cuts.append((left.end + right.end) / 2)
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

    /// A reading's duration-weighted mean confidence in a segment, or 0 when it heard nothing
    /// there. Words with no length fall back to their plain mean.
    private static func score(of words: [TwoLanguageTimedWord]) -> Double {
        guard !words.isEmpty else { return 0 }
        let duration = words.reduce(0) { $0 + $1.duration }
        if duration <= 0 {
            return words.reduce(0) { $0 + $1.confidence } / Double(words.count)
        }
        return words.reduce(0) { $0 + $1.confidence * $1.duration } / duration
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
