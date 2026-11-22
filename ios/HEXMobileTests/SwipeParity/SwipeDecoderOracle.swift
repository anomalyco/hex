// The Swift swipe decoder as it shipped before the Rust port, kept as the parity oracle:
// `ios/HEXShared/FullKeyboard/SwipeDecoder.swift` copied verbatim from dev at
// b719d45e5361ae04e220ec8b6822a40ea0dc1cd4 (`git log -1 --format=%H --
// ios/HEXShared/FullKeyboard/SwipeDecoder.swift`), with `SwipeDecoder` renamed
// `SwipeDecoderOracle` (its nested types unchanged) and its `extension KeyboardLayout`
// (`joinedForSwipe`, `swipePitch`), which the app keeps, left out. Never edit it to match the
// port: the port matches it. Delete it once the app's decoder is the Rust one and the
// in-process parity test has passed.
import CoreGraphics
import Foundation
@testable import HEX

/// Experiment (Swipe typing): reads a finger's path across the letter keys as a word.
///
/// A shape-matching decoder after SHARK2 (Kristensson and Zhai, 2004) and the template
/// matching of Gboard's published gesture decoders, written for Hex:
/// 1. The path is resampled to evenly spaced points in key pitches (a key's width plus a
///    gap across, a row's height plus a gap down), so a slip of half a key counts the same
///    either way.
/// 2. Words come from the typing model's word list, which is sorted letter by letter: the
///    words starting on a key near where the finger went down are one range of it per
///    letter. Of those, only words ending on a key near where it lifted, with every key near
///    the path and an ideal path (the polyline through their keys' centres) about as long,
///    are scored.
/// 3. Each is scored on where its ideal path runs against the gesture (location: a quick
///    point-by-point pass, then dynamic time warping for the best few, which follows a
///    finger that hurries and slows), on its shape alone (both paths moved to one centre
///    and size), and on how near the finger went down and lifted to its first and last keys;
/// 4. the best few also on how their keys account for where the finger turned and where it
///    slowed (`Evidence`, from the touches' times): a turn or a pause needs a key of the
///    word near it, and a key the word goes straight through needs a pause on it. A
///    straight stroke runs over the letters between its ends, so the path alone can't tell
///    "tired" from "tied" (one line from i to e) or "wire" from "write"; the finger slowing
///    on r can,
/// 5. and weighed with the model's frequency of the word after the word before.
///
/// Nothing is built ahead or kept: each swipe reads the memory-mapped model in place
/// (`TypingDecoderModel.withCodes`), so the decoder holds no memory between swipes.
nonisolated struct SwipeDecoderOracle: Sendable {
    /// Tuned on synthetic swipes of the autocorrect benchmark's held-out dev sentences
    /// (English and Spanish; clean, neat, typical and sloppy fingers; experiment folder
    /// 2026-11-20-hex-swipe-typing), never on its test sentences; the turn and pause
    /// parameters also on mined dev sets of words that share a path, with fingers that stop
    /// at every key, keep most of their speed through a straight-on key, or hesitate between
    /// keys.
    struct Parameters: Equatable, Sendable {
        /// Points the gesture and each word's ideal path are resampled to.
        var samples = 32
        /// Keys this many pitches from where the finger went down (lifted) may start (end)
        /// the word: the nearest few.
        var startRadius = 1.6
        var endRadius = 2.1
        var maxStartKeys = 4
        var maxEndKeys = 8
        /// Every key of a word passes at most this far from the path (pitches).
        var nearRadius = 1.5
        /// How much shorter or longer than the gesture a word's ideal path may be.
        var minLengthRatio = 0.4
        var maxLengthRatio = 1.8
        /// How far from its first and last keys' centres a finger goes down and lifts: most
        /// within the first spread, the rest (`stray`) within the wider second, so a near
        /// miss costs much and a far one no more than the path's shape can make up.
        var startSigma = 0.3
        var endSigma = 0.3
        var startStraySigma = 0.8
        var endStraySigma = 0.9
        var stray = 0.25
        /// Nats per mean squared pitch between the paths, and per mean squared distance
        /// between their shapes (each scaled to a unit box).
        var locationWeight = 30.0
        var shapeWeight = 60.0
        /// Weight of the word model's cost, -ln P(word | word before).
        var lmWeight = 1.0
        /// Words of the quick pass the aligned pass scores again.
        var shortlist = 80
        /// Past this many words, only the likeliest by how near their keys lie to the path
        /// (and their frequency) get the quick pass, which keeps a long, loose swipe fast.
        var prefilter = 1500
        /// How far (a share of the samples) the aligned pass may run ahead or behind.
        var band = 0.1
        /// The shortest path read as a swipe (pitches).
        var minimumLength = 0.6
        /// A contraction ("I'm") whose first word ("I") never follows the word before in the
        /// model's pairs costs this much more: those first words follow thousands of words, so
        /// a missing pair is evidence against it ("with us in", not "with us I'm").
        var contractionMissingPair = 3.0

        // Where the finger turned or slowed (`Evidence`): the keys of the word read must
        // account for them, in order.
        /// The path is read for turns and pauses every this many pitches.
        var evidenceStep = 0.1
        /// A turn is the change of direction between this much path before a point and after it:
        /// none under `turnFrom` degrees, full evidence from `turnFull`.
        var turnWindow = 0.2
        var turnFrom = 35.0
        var turnFull = 120.0
        /// A pause is time spent per pitch over this much path each side, against the swipe's
        /// median: none under `dwellFrom` times the median, full evidence from `dwellFull`.
        var dwellWindow = 0.1
        var dwellFrom = 1.3
        var dwellFull = 4.0
        /// Weaker turns and pauses are ignored, and of two nearer than `evidenceSpacing`
        /// (pitches along the path) only the stronger counts; none counts this near either end,
        /// which the first and last keys account for.
        var evidenceFloor = 0.1
        var evidenceSpacing = 0.8
        var evidenceGuard = 0.7
        /// How far a key may lie from the turn or pause it accounts for (pitches): a spread,
        /// and the farthest.
        var evidenceSigma = 0.35
        var evidenceReach = 1.4
        /// Nats for a full-strength turn or pause no key of the word accounts for (the finger
        /// slowed on a letter the word lacks), for a key the word goes straight through with
        /// no pause there (only when the path has times), and for a key the word turns at
        /// without a turn or pause there.
        var unexplainedCost = 6.0
        var straightKeyCost = 2.5
        var cornerKeyCost = 2.0
        /// A word's key it turns at by less than this (degrees) is one it goes straight through.
        var straightAngle = 20.0

        /// -ln of a point landing `squared` (pitches²) from a key's centre, under the mixture
        /// of a tight spread and a stray one; 0 at the centre.
        func cost(_ squared: Double, sigma: Double, stray strayingSigma: Double) -> Double {
            let near = (1 - stray) / (sigma * sigma)
            let far = stray / (strayingSigma * strayingSigma)
            let density = near * exp(-squared / (2 * sigma * sigma)) + far * exp(-squared / (2 * strayingSigma * strayingSigma))
            return log((near + far) / max(density, .leastNormalMagnitude))
        }
    }

    /// The cost (-ln P) of a common English contraction ("isn't", "we're"), about a word ranked
    /// 1,500th: Google Books splits contractions ("do n't"), so the model's counts for them
    /// are far too low.
    static let contractionCost = 7.5

    /// The cost of English contraction `word` by how often a chat uses it: the commonest ("I'm",
    /// "don't", "it's") like a word ranked about 300th, common ones at `base`, the rest ("he'll",
    /// "ma'am") about 10,000th.
    static func costOfContraction(_ word: String, base: Double = contractionCost) -> Double {
        let lower = word.lowercased()
        if commonestContractions.contains(lower) { return base - 1.5 }
        if commonContractions.contains(lower) { return base }
        return base + 2
    }

    private static let commonestContractions: Set<String> = [
        "i'm", "don't", "it's", "i'll", "can't", "that's", "didn't", "you're", "i've", "doesn't",
    ]
    private static let commonContractions: Set<String> = [
        "isn't", "there's", "let's", "won't", "what's", "he's", "she's", "wasn't", "i'd", "we're",
        "they're", "haven't", "couldn't", "wouldn't", "aren't", "you'll", "we'll", "shouldn't", "you've",
    ]
    /// The cost of one of your own words (as Spatial autocorrect's `userWordCost`).
    static let ownWordCost = 11.0

    struct Candidate: Equatable, Sendable {
        /// The word as the model spells it: lowercase, with its accents and apostrophes.
        var text: String
        var cost: Double
    }

    struct Reading: Equatable, Sendable {
        /// Best first.
        var candidates: [Candidate]
        /// Words that passed the key filters and were scored.
        var scored: Int
    }

    /// Words read beside the model's, each with its own cost (-ln P(word)): words the model
    /// lacks or barely counts, such as English contractions (Google Books splits "don't" into
    /// "do n't", so its "don't" is as rare as "tom"), and your own words.
    struct ExtraWords: Sendable {
        private(set) var codes: [[UInt8]] = []
        private(set) var texts: [String] = []
        private(set) var costs: [Double] = []
        /// The model's index of the same word, for its pairs; nil when it lacks it.
        private(set) var modelIndex: [Int?] = []
        /// A contraction's first word in the model ("i" for "I'm", "do" for "don't"), whose
        /// pairs weigh it after the word before; nil for other words.
        private(set) var heads: [Int?] = []

        init() {}

        /// `words` as you'd see them ("I'm", "don't"), each with `cost` unless the model's
        /// own is lower; a word with a letter the model lacks is left out, and a repeat once.
        init(_ words: [String], cost: Double, model: TypingDecoderModel) {
            add(words, cost: cost, model: model)
        }

        init(_ words: [String], cost: (String) -> Double, model: TypingDecoderModel) {
            add(words, cost: cost, model: model)
        }

        var isEmpty: Bool { texts.isEmpty }
        var count: Int { texts.count }

        mutating func add(_ words: [String], cost: Double, model: TypingDecoderModel) {
            add(words, cost: { _ in cost }, model: model)
        }

        mutating func add(_ words: [String], cost: (String) -> Double, model: TypingDecoderModel, contractions: Bool = false) {
            var seen = Set(codes)
            for word in words {
                guard word.count >= 2, let encoded = model.encode(word.lowercased()), seen.insert(encoded).inserted else { continue }
                codes.append(encoded)
                texts.append(word)
                costs.append(cost(word))
                modelIndex.append(model.index(ofCodes: encoded))
                heads.append(contractions ? Self.head(of: word.lowercased()).flatMap { model.index(of: $0) } : nil)
            }
        }

        /// A contraction's first word: "do" for "don't", "i" for "I'm".
        static func head(of contraction: String) -> String? {
            if contraction.hasSuffix("n't") { return String(contraction.dropLast(3)) }
            guard let apostrophe = contraction.firstIndex(of: "'") else { return nil }
            return String(contraction[..<apostrophe])
        }
    }

    let model: TypingDecoderModel
    var parameters: Parameters
    /// Words and two-word expressions never offered (`OffensiveWords`), as for Spatial
    /// autocorrect: never read from a swipe.
    var neverSuggest = TypingDecoder.NeverSuggest()
    var extraWords = ExtraWords() {
        didSet { extraIndices = Set(extraWords.modelIndex.compactMap { $0 }) }
    }
    /// The model's words that `extraWords` also has, read only as extra words.
    private var extraIndices: Set<Int> = []

    init(model: TypingDecoderModel, parameters: Parameters = Parameters()) {
        self.model = model
        self.parameters = parameters
    }

    /// The likeliest words for a swipe along `path` (the touch's points, first to last, in
    /// the keys' coordinates), best first; empty when it can't be read as one. `times` (each
    /// point's `UITouch.timestamp`) show where the finger paused; without them only its turns
    /// count.
    func decode(_ path: [CGPoint], times: [TimeInterval]? = nil, keys: TypingDecoder.Keys, previous: String? = nil, limit: Int = 4) -> [Candidate] {
        read(path, times: times, keys: keys, previous: previous, limit: limit).candidates
    }

    func read(_ path: [CGPoint], times: [TimeInterval]? = nil, keys: TypingDecoder.Keys, previous: String? = nil, limit: Int = 4) -> Reading {
        let p = parameters
        let none = Reading(candidates: [], scored: 0)
        let pitch = Vec(x: Double(keys.pitch.width), y: Double(keys.pitch.height))
        guard pitch.x > 0, pitch.y > 0, path.count >= 2, limit > 0, p.samples >= 4 else { return none }
        let grid = KeyGrid(keys, model: model, pitch: pitch)
        guard grid.centres.count >= 2, grid.centres.count <= 64 else { return none }
        guard let gesture = Gesture(path, times: times, pitch: pitch, parameters: p), gesture.length >= p.minimumLength,
              let start = gesture.raw.first, let end = gesture.raw.last else { return none }

        // 1. The keys the word may start and end on, and how near the path passes each key.
        let keyCount = grid.centres.count
        let near = grid.centres.map { Self.distance(from: $0, toPolyline: gesture.raw) }
        var startCost = [Double](repeating: .infinity, count: keyCount)
        var endCost = [Double](repeating: .infinity, count: keyCount)
        let starts = Self.nearest(grid.centres, to: start, radius: p.startRadius, limit: p.maxStartKeys)
        for (k, squared) in starts { startCost[k] = p.cost(squared, sigma: p.startSigma, stray: p.startStraySigma) }
        var endMask: UInt64 = 0
        for (k, squared) in Self.nearest(grid.centres, to: end, radius: p.endRadius, limit: p.maxEndKeys) {
            endCost[k] = p.cost(squared, sigma: p.endSigma, stray: p.endStraySigma)
            endMask |= 1 << UInt64(k)
        }
        guard !starts.isEmpty, endMask != 0 else { return none }

        // 2. Words that start on a start key, end on an end key, keep near the path and are
        // about as long: their keys (repeats run together, apostrophes left out). An id is
        // a model word's index, or -1 - n for extra word n.
        var words: [Int32] = []
        var offsets: [Int32] = [0]
        var lengths: [Double] = []
        var sequence: [UInt8] = []
        /// Mean squared distance of each word's keys from the path, for the prefilter.
        var nearness: [Double] = []
        var scratch = [UInt8](repeating: 0, count: 64)
        func consider<Codes: RandomAccessCollection>(_ codes: Codes, id: Int) where Codes.Element == UInt8, Codes.Index == Int {
            var j = codes.endIndex - 1
            while j > codes.startIndex, grid.keyOfCode[Int(codes[j])] == KeyGrid.skip { j -= 1 }
            guard j > codes.startIndex else { return }
            let endKey = grid.keyOfCode[Int(codes[j])]
            guard endKey >= 0, endMask & (1 << UInt64(endKey)) != 0 else { return }
            var count = 0
            var last = -1
            var length = 0.0
            var squares = 0.0
            for c in codes {
                let k = Int(grid.keyOfCode[Int(c)])
                if k == Int(KeyGrid.skip) || k == last { continue }
                guard k >= 0, count < scratch.count, near[k] <= p.nearRadius else { return }
                if last >= 0 { length += Self.distance(grid.centres[last], grid.centres[k]) }
                squares += near[k] * near[k]
                scratch[count] = UInt8(k)
                count += 1
                last = k
            }
            guard count >= 2, startCost[Int(scratch[0])].isFinite else { return }
            let ratio = length / gesture.length
            guard ratio >= p.minLengthRatio, ratio <= p.maxLengthRatio else { return }
            sequence.append(contentsOf: scratch[0..<count])
            words.append(Int32(id))
            lengths.append(length)
            offsets.append(Int32(sequence.count))
            nearness.append(squares / Double(count))
        }
        for (s, _) in starts {
            for code in grid.codesOfKey[s] {
                let (lo, hi) = model.narrow(0, model.wordCount, depth: 0, byte: Int(code))
                guard lo < hi else { continue }
                for i in lo..<hi where !neverSuggest.words.contains(i) && !extraIndices.contains(i) {
                    model.withCodes(i) { consider($0, id: i) }
                }
            }
        }
        for n in 0..<extraWords.count {
            consider(extraWords.codes[n], id: -1 - n)
        }
        guard !words.isEmpty else { return none }

        // 3. A quick point-by-point pass over them all, then the aligned pass for the best.
        let n = p.samples
        var template = [Vec](repeating: Vec(x: 0, y: 0), count: n)
        var vertices = [Vec](repeating: Vec(x: 0, y: 0), count: 64)
        var rows = ([Double](repeating: 0, count: n), [Double](repeating: 0, count: n))
        var evidenceRows = ([Double](repeating: 0, count: gesture.evidence.count + 1), [Double](repeating: 0, count: gesture.evidence.count + 1))
        func spatial(_ c: Int, aligned: Bool) -> Double {
            let from = Int(offsets[c]), to = Int(offsets[c + 1])
            for v in from..<to { vertices[v - from] = grid.centres[Int(sequence[v])] }
            Self.resample(vertices, count: to - from, length: lengths[c], into: &template)
            let location = aligned
                ? Self.warpedDistance(gesture.points, template, band: max(1, Int(p.band * Double(n))), rows: &rows)
                : Self.meanSquaredDistance(gesture.points, template)
            let shape = Self.shapeDistance(gesture.shape, template)
            let evidence = aligned
                ? Self.evidenceCost(vertices, count: to - from, gesture: gesture, parameters: p, rows: &evidenceRows)
                : 0
            return startCost[Int(sequence[from])] + endCost[Int(sequence[to - 1])]
                + p.locationWeight * location + p.shapeWeight * shape + evidence
        }
        var pool = Array(words.indices)
        if pool.count > p.prefilter {
            let rough = pool.map { c -> Double in
                let from = Int(offsets[c]), to = Int(offsets[c + 1])
                return startCost[Int(sequence[from])] + endCost[Int(sequence[to - 1])]
                    + p.locationWeight * nearness[c] + p.lmWeight * lmCost(Int(words[c]), after: nil)
            }
            pool.sort { rough[$0] < rough[$1] }
            pool.removeLast(pool.count - p.prefilter)
        }
        var quick: [(candidate: Int, cost: Double)] = []
        quick.reserveCapacity(pool.count)
        for c in pool {
            quick.append((c, spatial(c, aligned: false) + p.lmWeight * lmCost(Int(words[c]), after: nil)))
        }
        quick.sort { $0.cost < $1.cost }

        let previousWord = previous.flatMap { model.index(of: $0.lowercased()) }
        var scored: [(word: Int, cost: Double)] = []
        for (c, _) in quick.prefix(max(limit, p.shortlist)) {
            let word = Int(words[c])
            if word >= 0, neverSuggest.follows(word, after: previousWord) { continue }
            scored.append((word, spatial(c, aligned: true) + p.lmWeight * lmCost(word, after: previousWord)))
        }
        scored.sort { $0.cost < $1.cost }
        var candidates: [Candidate] = []
        for (word, cost) in scored.prefix(limit) {
            candidates.append(Candidate(text: word >= 0 ? model.word(word) : extraWords.texts[-1 - word], cost: cost))
        }
        return Reading(candidates: candidates, scored: words.count)
    }

    /// -ln P(word | previous) with stupid backoff (0.4) to the unigram, as Spatial autocorrect's;
    /// an extra word's own cost when lower.
    private func lmCost(_ id: Int, after previous: Int?) -> Double {
        func model(_ word: Int) -> Double {
            let backoff = 0.92 + self.model.unigramCost(word)
            guard let previous, let bigram = self.model.bigramCost(previous, word) else { return backoff }
            return min(bigram, backoff)
        }
        guard id < 0 else { return model(id) }
        var own = 0.92 + extraWords.costs[-1 - id]
        // A contraction is as much likelier after the word before as its first word is.
        if let head = extraWords.heads[-1 - id], let previous {
            let alone = 0.92 + self.model.unigramCost(head)
            own += (self.model.bigramCost(previous, head) ?? alone + parameters.contractionMissingPair) - alone
        }
        return extraWords.modelIndex[-1 - id].map { min(own, model($0)) } ?? own
    }

    // MARK: - Geometry, in key pitches

    struct Vec: Equatable, Sendable {
        var x: Double
        var y: Double
    }

    /// The layout's letter keys in pitches, and which key types each of the model's letters:
    /// its own, else its unaccented letter's (é on e); the apostrophe has none and is skipped.
    private struct KeyGrid {
        static let skip: Int8 = -1
        static let missing: Int8 = -2
        var centres: [Vec] = []
        var keyOfCode: [Int8]
        var codesOfKey: [[UInt8]]

        init(_ keys: TypingDecoder.Keys, model: TypingDecoderModel, pitch: Vec) {
            var index: [Character: Int] = [:]
            for (character, point) in keys.centres.sorted(by: { $0.key < $1.key }) {
                index[character] = centres.count
                centres.append(Vec(x: Double(point.x) / pitch.x, y: Double(point.y) / pitch.y))
            }
            keyOfCode = [Int8](repeating: Self.missing, count: model.characters.count)
            codesOfKey = [[UInt8]](repeating: [], count: centres.count)
            for code in 1..<model.characters.count {
                if UInt8(code) == model.apostropheCode {
                    keyOfCode[code] = Self.skip
                    continue
                }
                let own = index[model.characters[code]]
                guard let key = own ?? index[model.characters[Int(model.baseCodes[code])]], key < 64 else { continue }
                keyOfCode[code] = Int8(key)
                codesOfKey[key].append(UInt8(code))
            }
        }
    }

    /// The finger's path in pitches without repeated points, resampled, its shape, and where
    /// it turned or paused.
    private struct Gesture {
        var raw: [Vec] = []
        var length = 0.0
        var points: [Vec]
        var shape: [Vec]
        var evidence: [Evidence] = []
        /// Whether the path's times showed where it paused: without them a key the word goes
        /// straight through can't have shown itself.
        var timed = false

        init?(_ path: [CGPoint], times: [TimeInterval]?, pitch: Vec, parameters p: Parameters) {
            let times = times?.count == path.count ? times : nil
            var stamps: [Double] = []
            for (i, point) in path.enumerated() {
                let v = Vec(x: Double(point.x) / pitch.x, y: Double(point.y) / pitch.y)
                if let last = raw.last {
                    let step = SwipeDecoderOracle.distance(last, v)
                    guard step > 1e-4 else { continue }
                    length += step
                }
                raw.append(v)
                // A still finger sends no points: the time it stayed goes to the next one's step.
                if let times { stamps.append(max(times[i], stamps.last ?? times[i])) }
            }
            guard raw.count >= 2, length > 0 else { return nil }
            points = [Vec](repeating: Vec(x: 0, y: 0), count: p.samples)
            SwipeDecoderOracle.resample(raw, count: raw.count, length: length, into: &points)
            shape = SwipeDecoderOracle.normalized(points)
            let duration = (stamps.last ?? 0) - (stamps.first ?? 0)
            timed = duration > 0.02
            evidence = SwipeDecoderOracle.evidence(raw, times: timed ? stamps : nil, length: length, parameters: p)
        }
    }

    /// A point where the finger turned or slowed: a key it meant, most likely. `strength`
    /// is 0 to 1.
    struct Evidence: Equatable, Sendable {
        var point: Vec
        var strength: Double
    }

    /// The path's turns and pauses, in order along it: the path is read every
    /// `evidenceStep` pitches; a point scores by how sharply the path turns there and how
    /// long the finger took over it against the rest of the swipe; the strongest points
    /// at least `evidenceSpacing` apart, away from the ends, count.
    static func evidence(_ raw: [Vec], times: [Double]?, length: Double, parameters p: Parameters) -> [Evidence] {
        let count = Int(length / p.evidenceStep) + 1
        guard count >= 5 else { return [] }
        var points = [Vec](repeating: Vec(x: 0, y: 0), count: count)
        resample(raw, count: raw.count, length: length, into: &points)
        let step = length / Double(count - 1)
        var score = [Double](repeating: 0, count: count)
        let k = max(1, Int((p.turnWindow / step).rounded()))
        if count > 2 * k {
            for i in k..<(count - k) {
                let a = Vec(x: points[i].x - points[i - k].x, y: points[i].y - points[i - k].y)
                let b = Vec(x: points[i + k].x - points[i].x, y: points[i + k].y - points[i].y)
                let angle = atan2(abs(a.x * b.y - a.y * b.x), a.x * b.x + a.y * b.y) * 180 / .pi
                score[i] = min(1, max(0, (angle - p.turnFrom) / (p.turnFull - p.turnFrom)))
            }
        }
        let guardCount = Int((p.evidenceGuard / step).rounded(.up))
        guard count > 2 * guardCount + 1 else { return [] }
        if let times {
            // The time at each point read, from the raw points' times along the path.
            var at = [Double](repeating: 0, count: count)
            var segment = 0
            var walked = 0.0
            var span = distance(raw[0], raw[1])
            for i in 0..<count {
                let target = step * Double(i)
                while walked + span < target, segment < raw.count - 2 {
                    walked += span
                    segment += 1
                    span = distance(raw[segment], raw[segment + 1])
                }
                let t = span > 0 ? min(1, max(0, (target - walked) / span)) : 0
                at[i] = times[segment] + t * (times[min(segment + 1, raw.count - 1)] - times[segment])
            }
            let w = max(1, Int((p.dwellWindow / step).rounded()))
            var perPitch = [Double](repeating: 0, count: count)
            for i in 0..<count {
                let lo = max(0, i - w), hi = min(count - 1, i + w)
                perPitch[i] = (at[hi] - at[lo]) / (step * Double(hi - lo))
            }
            var middle = Array(perPitch[guardCount..<(count - guardCount)])
            middle.sort()
            let median = middle[middle.count / 2]
            if median > 0 {
                let from = log(p.dwellFrom), full = log(p.dwellFull)
                for i in guardCount..<(count - guardCount) where perPitch[i] > 0 {
                    let dwell = min(1, max(0, (log(perPitch[i] / median) - from) / (full - from)))
                    score[i] = max(score[i], dwell)
                }
            }
        }
        // The strongest first, each keeping its neighbours within the spacing out.
        let spacing = max(1, Int((p.evidenceSpacing / step).rounded()))
        var order = Array(guardCount..<(count - guardCount)).filter { score[$0] >= p.evidenceFloor }
        order.sort { score[$0] != score[$1] ? score[$0] > score[$1] : $0 < $1 }
        var taken: [Int] = []
        for i in order where taken.allSatisfy({ abs($0 - i) >= spacing }) {
            taken.append(i)
        }
        taken.sort()
        return taken.map { Evidence(point: points[$0], strength: score[$0]) }
    }

    /// Nats for how well a word's keys (`keys[0..<count]`, in pitches) account for the
    /// path's turns and pauses, in order: an edit distance where a key accounts for the turns
    /// or pauses near it (by how far it lies from each), a turn or pause no key accounts for
    /// costs by its strength, and a key with none costs `cornerKeyCost` where the word
    /// turns, `straightKeyCost` where it goes straight on (and the path's times would have
    /// shown a pause). The first and last keys cost nothing without one.
    private static func evidenceCost(_ keys: [Vec], count m: Int, gesture: Gesture, parameters p: Parameters, rows: inout ([Double], [Double])) -> Double {
        let evidence = gesture.evidence
        let n = evidence.count
        guard m >= 2 else { return 0 }
        let reach = p.evidenceReach * p.evidenceReach
        let spread = 2 * p.evidenceSigma * p.evidenceSigma
        let straight = cos(p.straightAngle * .pi / 180)
        func missing(_ i: Int) -> Double {
            guard i > 0, i < m - 1 else { return 0 }
            let a = Vec(x: keys[i].x - keys[i - 1].x, y: keys[i].y - keys[i - 1].y)
            let b = Vec(x: keys[i + 1].x - keys[i].x, y: keys[i + 1].y - keys[i].y)
            let la = (a.x * a.x + a.y * a.y).squareRoot(), lb = (b.x * b.x + b.y * b.y).squareRoot()
            guard la > 1e-6, lb > 1e-6 else { return 0 }
            let goesStraight = (a.x * b.x + a.y * b.y) / (la * lb) >= straight
            return goesStraight ? (gesture.timed ? p.straightKeyCost : 0) : p.cornerKeyCost
        }
        func accounts(_ i: Int, _ j: Int) -> Double {
            let d = squared(keys[i], evidence[j].point)
            return d <= reach ? d / spread : .infinity
        }
        func unexplained(_ j: Int) -> Double { p.unexplainedCost * evidence[j].strength }
        // Row i: keys 0...i placed; column j: the first j turns and pauses read.
        rows.0[0] = 0
        for j in 0..<n { rows.0[j + 1] = rows.0[j] + min(unexplained(j), accounts(0, j)) }
        for i in 1..<m {
            let skip = missing(i)
            rows.1[0] = rows.0[0] + skip
            for j in 0..<n {
                let near = accounts(i, j)
                var best = rows.0[j + 1] + skip
                best = min(best, rows.0[j] + near)
                best = min(best, rows.1[j] + min(unexplained(j), near))
                rows.1[j + 1] = best
            }
            swap(&rows.0, &rows.1)
        }
        return rows.0[n]
    }

    @inline(__always) static func distance(_ a: Vec, _ b: Vec) -> Double {
        ((a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)).squareRoot()
    }

    @inline(__always) private static func squared(_ a: Vec, _ b: Vec) -> Double {
        (a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)
    }

    /// The keys within `radius` of `point`, nearest first, at most `limit`, with their
    /// squared distances; the nearest two however far.
    private static func nearest(_ centres: [Vec], to point: Vec, radius: Double, limit: Int) -> [(Int, Double)] {
        let sorted = centres.indices.map { ($0, squared(centres[$0], point)) }.sorted { $0.1 < $1.1 }
        var found = Array(sorted.prefix(limit).filter { $0.1 <= radius * radius })
        if found.count < 2 { found = Array(sorted.prefix(min(2, limit))) }
        return found
    }

    static func distance(from point: Vec, toPolyline line: [Vec]) -> Double {
        var best = Double.infinity
        for i in 1..<line.count {
            let a = line[i - 1], b = line[i]
            let dx = b.x - a.x, dy = b.y - a.y
            let span = dx * dx + dy * dy
            var t = span > 0 ? ((point.x - a.x) * dx + (point.y - a.y) * dy) / span : 0
            t = min(1, max(0, t))
            best = min(best, squared(point, Vec(x: a.x + t * dx, y: a.y + t * dy)))
        }
        return best.squareRoot()
    }

    /// `count` points of `line` (`length` long) evenly spaced along it into `out`,
    /// first and last included.
    static func resample(_ line: [Vec], count: Int, length: Double, into out: inout [Vec]) {
        let samples = out.count
        out[0] = line[0]
        out[samples - 1] = line[count - 1]
        guard samples > 2 else { return }
        let step = length / Double(samples - 1)
        var segment = 0
        var walked = 0.0
        var span = distance(line[0], line[min(1, count - 1)])
        for i in 1..<(samples - 1) {
            let target = step * Double(i)
            while walked + span < target, segment < count - 2 {
                walked += span
                segment += 1
                span = distance(line[segment], line[segment + 1])
            }
            let a = line[segment], b = line[min(segment + 1, count - 1)]
            let t = span > 0 ? min(1, max(0, (target - walked) / span)) : 0
            out[i] = Vec(x: a.x + t * (b.x - a.x), y: a.y + t * (b.y - a.y))
        }
    }

    /// The points moved to their centroid and scaled so their bounding box's longer side is 1.
    static func normalized(_ points: [Vec]) -> [Vec] {
        let (centre, scale) = frame(points)
        return points.map { Vec(x: ($0.x - centre.x) * scale, y: ($0.y - centre.y) * scale) }
    }

    @inline(__always) private static func frame(_ points: [Vec]) -> (Vec, Double) {
        var sx = 0.0, sy = 0.0
        var minX = Double.infinity, maxX = -Double.infinity, minY = Double.infinity, maxY = -Double.infinity
        for v in points {
            sx += v.x
            sy += v.y
            minX = min(minX, v.x)
            maxX = max(maxX, v.x)
            minY = min(minY, v.y)
            maxY = max(maxY, v.y)
        }
        let side = max(maxX - minX, maxY - minY)
        let n = Double(points.count)
        return (Vec(x: sx / n, y: sy / n), side > 1e-6 ? 1 / side : 1)
    }

    /// Mean squared distance between the gesture's normalized shape and the template's.
    private static func shapeDistance(_ shape: [Vec], _ template: [Vec]) -> Double {
        let (centre, scale) = frame(template)
        var total = 0.0
        for i in shape.indices {
            let t = Vec(x: (template[i].x - centre.x) * scale, y: (template[i].y - centre.y) * scale)
            total += squared(shape[i], t)
        }
        return total / Double(shape.count)
    }

    private static func meanSquaredDistance(_ a: [Vec], _ b: [Vec]) -> Double {
        var total = 0.0
        for i in a.indices { total += squared(a[i], b[i]) }
        return total / Double(a.count)
    }

    /// Dynamic time warping of two equally long point lists within a band, symmetric steps
    /// (a diagonal step counts twice), as a mean squared distance.
    private static func warpedDistance(_ a: [Vec], _ b: [Vec], band: Int, rows: inout ([Double], [Double])) -> Double {
        let n = a.count
        for j in 0..<n { rows.0[j] = .infinity }
        for i in 0..<n {
            for j in 0..<n { rows.1[j] = .infinity }
            let lo = max(0, i - band), hi = min(n - 1, i + band)
            for j in lo...hi {
                let c = squared(a[i], b[j])
                var best = i == 0 && j == 0 ? 2 * c : Double.infinity
                if i > 0, j > 0 { best = min(best, rows.0[j - 1] + 2 * c) }
                if i > 0 { best = min(best, rows.0[j] + c) }
                if j > 0 { best = min(best, rows.1[j - 1] + c) }
                rows.1[j] = best
            }
            swap(&rows.0, &rows.1)
        }
        return rows.0[n - 1] / Double(2 * n)
    }
}
