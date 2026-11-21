import CoreGraphics
import Foundation

/// One letter typed with a tap, for Adaptive touch: where the finger went down (layout
/// points), the letter its key typed, and, when it retyped a letter you had just erased at the
/// same place in the word, that erased tap.
nonisolated struct AdaptiveTouchTap: Equatable {
    var x: Float
    var y: Float
    var letter: Unicode.Scalar
    /// False when the touch slid onto its key or came down above the keys (the strip's lower
    /// half): its point says nothing about where you aim.
    var usable: Bool
    var erasedX: Float = 0
    var erasedY: Float = 0
    var erasedLetter: Unicode.Scalar?

    var point: CGPoint { CGPoint(x: CGFloat(x), y: CGFloat(y)) }
    var erasedPoint: CGPoint { CGPoint(x: CGFloat(erasedX), y: CGFloat(erasedY)) }
}

/// What Adaptive touch keeps of what you type, in memory only: the word being typed and at
/// most `pendingLimit` finished words, as taps and the letters they ended up as, until the
/// next word shows they stood (`AdaptiveTouch.finish`). Anything it can't follow (the cursor
/// moved, the host changed the text, a swipe, a new geometry, `timeout`) is forgotten, and
/// a tap teaches only with evidence of the letter you meant:
///
/// - a word left as typed (weak), but only letters the key under the finger typed;
/// - a suggestion of the same length picked in its place, changing at most two letters
///   (`AdaptiveTouch.pickedQuality`);
/// - a word kept as typed over an autocorrection, by undoing it or tapping it, and a letter
///   erased and retyped at the same place (strong): the erased tap is labelled with the
///   letter that replaced it.
///
/// An autocorrection left in place teaches nothing: a model can't confirm itself.
nonisolated struct AdaptiveTouchSpans {
    enum Outcome: Equatable {
        /// Left as typed.
        case typed
        /// Kept as typed over an autocorrection.
        case kept
        /// A suggestion of the same length went in its place.
        case picked
        /// Autocorrect replaced it: nothing to learn, kept only for its undo.
        case corrected
    }

    struct Span: Equatable {
        var taps: ContiguousArray<AdaptiveTouchTap>
        /// The letters the word ended as, one per tap (empty when corrected).
        var labels: [Unicode.Scalar]
        var outcome: Outcome
        var geometry: UInt64
        var at: TimeInterval
        /// The word was erased and retyped back to its length, changing at most two letters.
        var retyped: Bool
    }

    static let wordLimit = 32
    static let erasedLimit = 8
    static let pendingLimit = 2
    static let timeout: TimeInterval = 30
    static let changedLetterLimit = 2

    private(set) var word = ContiguousArray<AdaptiveTouchTap>()
    private(set) var pending: [Span] = []
    private var erased = ContiguousArray<(position: Int, tap: AdaptiveTouchTap)>()
    private var wordGeometry: UInt64 = 0
    private var wordKept = false
    private var wordBroken = false
    /// The word's length when the current run of erasing began.
    private var eraseBase: Int?
    private var lastAt: TimeInterval = 0

    init() {
        word.reserveCapacity(Self.wordLimit)
        erased.reserveCapacity(Self.erasedLimit)
    }

    var isEmpty: Bool { word.isEmpty && pending.isEmpty }

    mutating func forget() {
        word.removeAll(keepingCapacity: true)
        erased.removeAll(keepingCapacity: true)
        pending.removeAll(keepingCapacity: true)
        resetWordState()
    }

    private mutating func resetWordState() {
        wordKept = false
        wordBroken = false
        eraseBase = nil
    }

    /// A letter key typed `letter`. No allocation: the word's storage is reserved.
    mutating func typed(_ tap: AdaptiveTouchTap, geometry: UInt64, now: TimeInterval) {
        if !word.isEmpty, geometry != wordGeometry || now - lastAt > Self.timeout {
            word.removeAll(keepingCapacity: true)
            erased.removeAll(keepingCapacity: true)
            resetWordState()
        }
        if word.isEmpty { wordGeometry = geometry }
        lastAt = now
        guard word.count < Self.wordLimit else {
            wordBroken = true
            return
        }
        var tap = tap
        if let i = erased.lastIndex(where: { $0.position == word.count }) {
            let gone = erased[i].tap
            tap.erasedX = gone.x
            tap.erasedY = gone.y
            tap.erasedLetter = gone.letter
            erased.remove(at: i)
        }
        word.append(tap)
    }

    /// Backspace is about to delete the last character of `before`.
    mutating func willDelete(before: String?) {
        guard let last = before?.last else { return forget() }
        if last.isLetter {
            guard !word.isEmpty else { return forget() }
            if eraseBase == nil { eraseBase = word.count }
            var tap = word.removeLast()
            wordKept = false
            // A retyped letter erased again: its first attempt is the touch that meant it.
            if let letter = tap.erasedLetter {
                tap = AdaptiveTouchTap(x: tap.erasedX, y: tap.erasedY, letter: letter, usable: true)
            }
            guard tap.usable else { return }
            if erased.count == Self.erasedLimit { erased.remove(at: 0) }
            erased.append((word.count, tap))
        } else if last == " ", word.isEmpty, let span = pending.last,
                  span.outcome == .typed || span.outcome == .kept,
                  Self.endsWithWord(Substring(before!.dropLast()), span.labels) {
            // Back into the word just finished: it is being typed again.
            pending.removeLast()
            word.append(contentsOf: span.taps)
            wordGeometry = span.geometry
            erased.removeAll(keepingCapacity: true)
            resetWordState()
            wordKept = span.outcome == .kept
        } else {
            forget()
        }
    }

    /// Undo put back what you typed in place of the autocorrection just applied.
    mutating func undone(original: String) {
        guard word.isEmpty, let span = pending.last, span.outcome == .corrected,
              Self.matches(span.taps, original) else { return forget() }
        pending.removeLast()
        word.append(contentsOf: span.taps)
        wordGeometry = span.geometry
        erased.removeAll(keepingCapacity: true)
        resetWordState()
        wordKept = true
    }

    /// The word being typed ended as `token` (what the field shows; nil when it isn't a word)
    /// and, when autocorrect replaced it, `corrected`; or, with `picked`, a suggestion went in
    /// its place (`overCorrection`: you tapped what you typed over the fix space would apply).
    /// Returns the earlier words this one shows stood: `before`, the text before the cursor
    /// still ending in this word, has each of them just before it; none when autocorrect is
    /// revising the word before now (`previousRevised`, "well" to "we'll").
    mutating func finish(
        token: String?, corrected: String? = nil, picked: String? = nil, overCorrection: Bool = false,
        previousRevised: Bool = false, before: String?, now: TimeInterval
    ) -> [Span] {
        guard !word.isEmpty else { return [] }
        let previous = Self.withoutLastWord(before ?? "")
        var stood: [Span] = []
        for span in pending where !previousRevised && now - span.at <= Self.timeout && Self.endsWithWord(previous, span.labels) {
            stood.append(span)
        }
        pending.removeAll(keepingCapacity: true)
        if let span = span(token: token, corrected: corrected, picked: picked, overCorrection: overCorrection, now: now) {
            pending.append(span)
        }
        word.removeAll(keepingCapacity: true)
        erased.removeAll(keepingCapacity: true)
        resetWordState()
        return stood
    }

    /// The host changed the text or moved the cursor: what can't be followed any more goes.
    mutating func hostChanged(before: String?, token: String?) {
        if !word.isEmpty {
            if wordBroken || token == nil || !Self.matches(word, token!) { forget() }
        } else if let span = pending.last {
            let text = before ?? ""
            if !Self.endsWithWord(Self.trailingSeparatorsRemoved(Substring(text)), span.labels) { forget() }
        }
    }

    private func span(token: String?, corrected: String?, picked: String?, overCorrection: Bool, now: TimeInterval) -> Span? {
        guard !wordBroken, let token, Self.matches(word, token) else { return nil }
        let typed = word.map(\.letter)
        var outcome: Outcome = wordKept ? .kept : .typed
        var labels = typed
        if corrected != nil {
            outcome = .corrected
            labels = []
        } else if let picked {
            let scalars = Array(picked.lowercased().unicodeScalars)
            if scalars == typed {
                if overCorrection { outcome = .kept }
            } else {
                let changed = zip(scalars, typed).filter { $0 != $1 }.count
                guard scalars.count == typed.count, scalars.allSatisfy({ Character($0).isLetter }),
                      changed <= Self.changedLetterLimit else { return nil }
                outcome = .picked
                labels = scalars
            }
        }
        let changes = word.filter { $0.erasedLetter != nil && $0.erasedLetter != $0.letter }.count
        let retyped = eraseBase == word.count && changes <= Self.changedLetterLimit
        return Span(taps: word, labels: labels, outcome: outcome, geometry: wordGeometry, at: now, retyped: retyped)
    }

    /// Whether `taps` typed exactly `text`'s letters, ignoring case.
    static func matches(_ taps: ContiguousArray<AdaptiveTouchTap>, _ text: String) -> Bool {
        var i = 0
        for scalar in text.lowercased().unicodeScalars {
            guard i < taps.count, taps[i].letter == scalar else { return false }
            i += 1
        }
        return i == taps.count
    }

    /// `text` without the word it ends with, and the spaces and marks before that word.
    static func withoutLastWord(_ text: String) -> Substring {
        var s = Substring(text)
        while let last = s.last, last.isLetter || last == "'" { s = s.dropLast() }
        return trailingSeparatorsRemoved(s)
    }

    static func trailingSeparatorsRemoved(_ text: Substring) -> Substring {
        var s = text
        while let last = s.last, !last.isLetter { s = s.dropLast() }
        return s
    }

    /// Whether `text` ends with the word `letters` (ignoring case), standing alone.
    static func endsWithWord(_ text: Substring, _ letters: [Unicode.Scalar]) -> Bool {
        guard !letters.isEmpty else { return false }
        let tail = text.suffix(letters.count + 1)
        let scalars = Array(String(tail).lowercased().unicodeScalars)
        guard scalars.count >= letters.count, Array(scalars.suffix(letters.count)) == letters else { return false }
        if scalars.count == letters.count { return text.count == letters.count }
        return !Character(scalars[scalars.count - letters.count - 1]).isLetter
    }
}

@MainActor
extension AdaptiveTouch {
    /// Fields Adaptive touch may learn in: those Learn how I type may, and never a secure one.
    static func allows(field: KeyboardFieldKind, target: KeyboardTextTarget) -> Bool {
        TypingJournal.allows(field: field, target: target) && target.isSecureTextEntry != true
    }

    /// A letter key typed `character` (its key's, lowercase) on a tap that went down at `down`.
    func typed(_ character: String, down: CGPoint, slid: Bool) {
        guard mode == .learn || mode == .monitor, let geometry, let letter = character.unicodeScalars.first,
              character.unicodeScalars.count == 1 else { return }
        let usable = !slid && down.y >= geometry.touchTop
        spans.typed(
            AdaptiveTouchTap(x: Float(down.x), y: Float(down.y), letter: letter, usable: usable),
            geometry: geometry.key, now: clock()
        )
    }

    func willDelete(before: String?) {
        spans.willDelete(before: before)
    }

    func undone(original: String) {
        spans.undone(original: original)
    }

    func forget() {
        spans.forget()
    }

    func hostChanged(before: String?) {
        guard !spans.isEmpty else { return }
        spans.hostChanged(before: before, token: AutocorrectEngine.currentToken(in: before))
    }

    /// The word being typed ended (space, return, a mark); `corrected` when autocorrect
    /// replaced it, `previousRevised` when it is changing the word before too. Words before
    /// it that stood teach now; this one waits for the next.
    func finish(token: String?, corrected: String? = nil, previousRevised: Bool = false, before: String?) {
        admit(spans.finish(token: token, corrected: corrected, previousRevised: previousRevised, before: before, now: clock()))
    }

    /// A suggestion is about to replace `token`, the word being typed.
    func picked(token: String, replacement: String, overCorrection: Bool, before: String?) {
        admit(spans.finish(token: token, picked: replacement, overCorrection: overCorrection, before: before, now: clock()))
    }

    private func admit(_ stood: [AdaptiveTouchSpans.Span]) {
        guard let geometry else { return }
        for span in stood where span.geometry == geometry.key && span.outcome != .corrected {
            var weak = 0
            for (i, tap) in span.taps.enumerated() where i < span.labels.count {
                let label = span.labels[i]
                if span.retyped, let erased = tap.erasedLetter, erased != tap.letter {
                    // The erased tap meant the letter that replaced it: a neighbour, not a miss.
                    let at = tap.erasedPoint
                    if let slot = geometry.slot(for: label, near: at), geometry.slot(at: at) != slot,
                       Self.pitches(at, geometry.frames[slot], geometry) <= Self.neighbourPitches {
                        admit(slot: slot, point: at, quality: Self.strongQuality)
                    }
                }
                guard tap.usable, let slot = geometry.slot(for: label, near: tap.point) else { continue }
                switch span.outcome {
                case .typed:
                    // Only what the key under the finger typed: not a letter the next-letter odds
                    // claimed from a neighbour, whose label would be the machine's guess.
                    guard geometry.slot(at: tap.point) == slot, weak < 12 else { continue }
                    weak += 1
                    admit(slot: slot, point: tap.point, quality: Self.weakQuality)
                case .kept:
                    admit(slot: slot, point: tap.point, quality: Self.strongQuality)
                case .picked:
                    admit(slot: slot, point: tap.point, quality: Self.pickedQuality)
                case .corrected:
                    break
                }
            }
        }
    }

    /// How far an erased tap may be from the key of the letter that replaced it: a neighbour,
    /// diagonals included.
    static let neighbourPitches: CGFloat = 1.25

    private static func pitches(_ point: CGPoint, _ frame: CGRect, _ geometry: AdaptiveTouchGeometry) -> CGFloat {
        let x = (point.x - frame.midX) / geometry.pitch.width
        let y = (point.y - frame.midY) / geometry.pitch.height
        return (x * x + y * y).squareRoot()
    }
}
