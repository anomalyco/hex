import CoreGraphics
import Darwin
import Foundation
@testable import HEX

/// One language's tap parity cases (`TapParityExport`, `TapParityGolden`), answered by the
/// shipped decoder (`TypingDecoder` over the Rust core): key geometry from the real `KeyboardLayout`
/// (375, 390, 402 and 430 pt wide, and the Duo's split keyboard), each with and without Learn
/// how I type offsets; words drawn from the model by frequency band and typed as key centres
/// plus Gaussian noise at several spreads, with slips (a neighbour key, two taps swapped, a
/// tap too many or too few), missed spaces, taps near the space bar, accents typed as their
/// base letters, three to six words run together, your own words and never-suggest words.
@MainActor
struct TapParityExporter {
    /// The user word sets a case may use: none, or these (in this order; two differ only in
    /// case, and some can't be words at all).
    static let userSets: [[String]] = [
        [],
        [
            "Kaitlyn", "kaitlyn", "Saoirse", "Tchaikovsky", "Llewellyn", "Ngozi", "Xiomara",
            "Jalapeño", "Müllerin", "O'Neill", "Brexitland", "Hexkey", "Dogecoin", "Zuzanna",
            "Bjørn", "a", "C3PO", "Joaquín", "Grüße", "señora",
        ],
    ]

    struct KeySet {
        var id: Int
        /// What the decoder reads with: the layout's centres, moved by the offsets.
        var keys: TypingDecoder.Keys
        /// The layout's own centres, where the generated fingers aim.
        var truth: TypingDecoder.Keys
        var letters: [(Character, CGPoint)]
    }

    let language: KeyboardLanguage
    let model: TypingDecoderModel
    var rng: TapParityRandom
    let byFrequency: [Int]
    /// The never-suggest lists by Suggest swear words (on: slurs only).
    let blocked: [Bool: [String]]
    let clean: Set<String>
    var decoders: [Int: TypingDecoder] = [:]
    var keysets: [KeySet] = []
    var counts: [String: Int] = [:]
    var decideNanos: [UInt64] = []

    init(language: KeyboardLanguage, seed: UInt64) throws {
        self.language = language
        rng = TapParityRandom(state: seed)
        let name = language.rawValue
        guard let model = TypingDecoderModel.bundled(name) else { throw TapParityError.noModel(name) }
        self.model = model
        byFrequency = (0..<model.wordCount).sorted { model.unigramCost($0) < model.unigramCost($1) }
        let words = OffensiveWords.bundled(name)
        blocked = [true: words.blocked(allowSwearing: true).sorted(), false: words.blocked(allowSwearing: false).sorted()]
        clean = Set(blocked[false]!)
    }

    var summary: String {
        let kinds = counts.keys.sorted().map { "\($0)=\(counts[$0]!)" }
        let sorted = decideNanos.sorted()
        func at(_ q: Double) -> Double { sorted.isEmpty ? 0 : Double(sorted[min(sorted.count - 1, Int(q * Double(sorted.count)))]) / 1000 }
        let mean = sorted.isEmpty ? 0 : Double(sorted.reduce(0, +)) / Double(sorted.count) / 1000
        return kinds.joined(separator: " ") + String(format: " decide-us median=%.1f p95=%.1f mean=%.1f", at(0.5), at(0.95), mean)
    }

    mutating func run(count: Int, into writer: inout TapParityWriter) throws {
        try run(count: count) { try writer.write($0) }
    }

    /// The setup line, the key sets, then `count` cases, each handed to `each`.
    mutating func run(count: Int, each: ([String: Any]) throws -> Void) throws {
        try each(["kind": "setup", "lang": language.rawValue, "blocked": ["swearing": blocked[true]!, "clean": blocked[false]!], "users": Self.userSets])
        try makeKeysets(each)
        var made = 0
        while made < count {
            let roll = rng.uniform()
            let line: [String: Any]?
            if roll < 0.70 { line = decideCase() } else if roll < 0.80 { line = mergeCase() } else if roll < 0.90 { line = replacesCase() } else { line = modelCase() }
            guard let line else { continue }
            try each(line)
            made += 1
        }
    }

    // MARK: Keys

    private mutating func makeKeysets(_ each: ([String: Any]) throws -> Void) throws {
        var layouts = [375, 390, 402, 430].map { (width: Double($0), duo: false, layout: KeyboardLayout(width: CGFloat($0), language: language)) }
        layouts.append((801, true, KeyboardLayout(width: 801, language: language, posture: .unfoldedWide, windowInset: 75)))
        for (width, duo, layout) in layouts {
            for learned in [false, true] {
                var stats: [String: TypingJournalKeyStats] = [:]
                if learned {
                    for key in layout.keys {
                        guard let c = key.character else { continue }
                        let n = rng.pick([0, 3, 19, 20, 21, 60, 400])
                        stats[c] = TypingJournalKeyStats(count: n, dx: rng.gaussian() * 0.2 * Double(n), dy: rng.gaussian() * 0.2 * Double(n))
                    }
                }
                let offsets = TypingDecoder.offsets(from: stats, layout: layout)
                guard let keys = TypingDecoder.Keys(layout: layout, offsets: offsets), let truth = TypingDecoder.Keys(layout: layout) else {
                    throw TapParityError.noKeys
                }
                let set = KeySet(id: keysets.count, keys: keys, truth: truth, letters: truth.centres.sorted { $0.key < $1.key }.map { ($0.key, $0.value) })
                keysets.append(set)
                let layoutKeys: [[String: Any]] = layout.keys.compactMap { key in
                    guard key.character != nil || key.kind == .space else { return nil }
                    let f = key.frame
                    return ["c": key.character ?? NSNull(), "space": key.kind == .space,
                            "frame": [f.origin.x, f.origin.y, f.size.width, f.size.height].map { bits(Double($0)) }]
                }
                let split: Any = layout.split.map { [bits(Double($0.middle)), bits(Double($0.shift))] } ?? NSNull()
                try each([
                    "kind": "keys", "lang": language.rawValue, "id": set.id, "width": width, "duo": duo,
                    "layout": ["keys": layoutKeys, "split": split, "pitch": [bits(Double(keys.pitch.width)), bits(Double(keys.pitch.height))]],
                    "stats": stats.mapValues { [$0.count, bits($0.dx), bits($0.dy)] as [Any] },
                    "offsets": Dictionary(uniqueKeysWithValues: offsets.map { (String($0.key), [bits(Double($0.value.dx)), bits(Double($0.value.dy))]) }),
                    "out": Self.json(keys),
                    "truth": Self.json(truth),
                ])
            }
        }
    }

    static func json(_ keys: TypingDecoder.Keys) -> [String: Any] {
        [
            "centres": keys.centres.sorted { $0.key < $1.key }.map { [String($0.key)] + bits($0.value) },
            "pitch": [bits(Double(keys.pitch.width)), bits(Double(keys.pitch.height))],
            "bottom": bits(Double(keys.bottomRowY)),
            "space": keys.space.map { [$0.origin.x, $0.origin.y, $0.size.width, $0.size.height].map { bits(Double($0)) } } ?? NSNull(),
        ]
    }

    // MARK: Fingers

    private mutating func decoder(user: Int, swearing: Bool) -> TypingDecoder {
        let id = user * 2 + (swearing ? 1 : 0)
        if let d = decoders[id] { return d }
        var d = TypingDecoder(model: model)
        d.userWords = TypingDecoder.UserWords(Self.userSets[user], model: model)
        d.neverSuggest = TypingDecoder.NeverSuggest(blocked[swearing]!, model: model)
        decoders[id] = d
        return d
    }

    private mutating func wordIndex(band: Int? = nil) -> Int {
        let edges = [0, 200, 2000, 20000, byFrequency.count]
        let b = band ?? rng.int(4)
        let lo = min(edges[b], byFrequency.count - 1), hi = min(edges[b + 1], byFrequency.count)
        return byFrequency[lo + rng.int(max(1, hi - lo))]
    }

    private mutating func previous() -> String? {
        switch rng.int(4) {
        case 0: return nil
        case 1, 2: return model.word(wordIndex(band: rng.int(3)))
        default: return rng.pick(["qzxv", "Hexkey", "blorptastic", "The", "xyzzy", "don't", "über"])
        }
    }

    /// Where a finger aims for `c`: its key, or its base letter's (é on a layout without it).
    private func aim(_ c: Character, _ set: KeySet) -> CGPoint? {
        if let p = set.truth.centres[c] { return p }
        guard let base = Self.withoutAccents(String(c)).first, base != c else { return nil }
        return set.truth.centres[base]
    }

    private mutating func noisy(_ p: CGPoint, _ spread: Double, _ set: KeySet) -> CGPoint {
        CGPoint(x: Double(p.x) + rng.gaussian() * spread * Double(set.truth.pitch.width),
                y: Double(p.y) + rng.gaussian() * spread * Double(set.truth.pitch.height))
    }

    private mutating func taps(for word: String, _ spread: Double, _ set: KeySet) -> [CGPoint]? {
        var out: [CGPoint] = []
        for c in word where c != "'" {
            guard let p = aim(c, set) else { return nil }
            out.append(noisy(p, spread, set))
        }
        return out.isEmpty ? nil : out
    }

    /// The letter key nearest each tap: what the keys resolved.
    private func typed(_ taps: [CGPoint], _ set: KeySet) -> String {
        let pw = Double(set.truth.pitch.width), ph = Double(set.truth.pitch.height)
        return String(taps.map { t -> Character in
            var best = set.letters[0].0, distance = Double.infinity
            for (c, p) in set.letters {
                let dx = (Double(t.x) - Double(p.x)) / pw, dy = (Double(t.y) - Double(p.y)) / ph
                if dx * dx + dy * dy < distance { distance = dx * dx + dy * dy; best = c }
            }
            return best
        })
    }

    /// A key near the one aimed at `p`, other than it.
    private mutating func neighbour(of p: CGPoint, _ set: KeySet) -> CGPoint {
        let pw = Double(set.truth.pitch.width), ph = Double(set.truth.pitch.height)
        let near = set.letters.filter {
            let dx = (Double($0.1.x) - Double(p.x)) / pw, dy = (Double($0.1.y) - Double(p.y)) / ph
            let d = dx * dx + dy * dy
            return d > 0.01 && d < 2.6
        }
        return near.isEmpty ? p : rng.pick(near).1
    }

    /// A tap on or near the space bar, or on the bottom row's lower half, where the decoder
    /// may read a space the finger missed.
    private mutating func spaceTap(_ set: KeySet) -> CGPoint {
        let pw = Double(set.truth.pitch.width), ph = Double(set.truth.pitch.height)
        let bar = set.truth.space ?? CGRect(x: 100, y: Double(set.truth.bottomRowY) + ph, width: 180, height: ph * 0.8)
        let x = Double(bar.minX) - pw / 2 + rng.uniform() * (Double(bar.width) + pw)
        let y = Double(set.truth.bottomRowY) + (rng.uniform() * 1.3 - 0.6) * ph
        return CGPoint(x: x, y: y)
    }

    private mutating func slip(_ taps: inout [CGPoint], aims: [CGPoint], _ spread: Double, _ set: KeySet) {
        guard !taps.isEmpty else { return }
        let i = rng.int(taps.count)
        switch rng.int(4) {
        case 0: taps[i] = noisy(neighbour(of: aims[i], set), spread, set)
        case 1: if i + 1 < taps.count { taps.swapAt(i, i + 1) }
        case 2: taps.insert(noisy(neighbour(of: aims[i], set), spread, set), at: i + 1)
        default: if taps.count >= 3 { taps.remove(at: i) }
        }
    }

    private func aims(_ word: String, _ set: KeySet) -> [CGPoint] {
        word.filter { $0 != "'" }.compactMap { aim($0, set) }
    }

    // MARK: Cases

    private func base(_ kind: String, _ set: KeySet, user: Int, swearing: Bool) -> [String: Any] {
        ["kind": kind, "lang": language.rawValue, "keys": set.id, "user": user, "swear": swearing]
    }

    /// Runs `body` with the shipped decoder, counting the case.
    private mutating func answer<T>(_ kind: String, timed: Bool = false, _ body: () -> T) -> T {
        let start = clock_gettime_nsec_np(CLOCK_UPTIME_RAW)
        let out = body()
        if timed { decideNanos.append(clock_gettime_nsec_np(CLOCK_UPTIME_RAW) - start) }
        counts[kind, default: 0] += 1
        return out
    }

    /// The text with acute, grave, circumflex and diaeresis marks removed, keeping ñ: where a
    /// generated finger aims for an accented letter on a layout without its key.
    static func withoutAccents(_ text: String) -> String {
        let scalars = text.decomposedStringWithCanonicalMapping.unicodeScalars
            .filter { !((0x300...0x308).contains($0.value) && $0.value != 0x303) }
        return String(String.UnicodeScalarView(scalars)).precomposedStringWithCanonicalMapping
    }

    private mutating func decideCase() -> [String: Any]? {
        let set = rng.pick(keysets)
        let spread = rng.pick([0, 0.08, 0.15, 0.25, 0.35, 0.5])
        let user = rng.chance(0.4) ? 1 : 0, swearing = rng.chance(0.5)
        let roll = rng.uniform()
        var taps: [CGPoint]
        if roll < 0.38 {
            // One word, perhaps with a slip.
            let word = model.word(wordIndex())
            guard let t = self.taps(for: word, spread, set) else { return nil }
            taps = t
            if roll >= 0.18 { slip(&taps, aims: aims(word, set), spread, set) }
        } else if roll < 0.50 {
            // Two words, no space.
            guard let a = self.taps(for: model.word(wordIndex(band: rng.int(3))), spread, set),
                  let b = self.taps(for: model.word(wordIndex(band: rng.int(3))), spread, set) else { return nil }
            taps = a + b
        } else if roll < 0.62 {
            // Two words with a tap near the space bar between them.
            guard let a = self.taps(for: model.word(wordIndex(band: rng.int(3))), spread, set),
                  let b = self.taps(for: model.word(wordIndex(band: rng.int(3))), spread, set) else { return nil }
            taps = a + [spaceTap(set)] + b
        } else if roll < 0.72 {
            // Three to six words run together.
            taps = []
            for _ in 0..<(3 + rng.int(4)) {
                guard let t = self.taps(for: model.word(wordIndex(band: rng.int(2))), spread * 0.6, set) else { return nil }
                taps += t
            }
            if taps.count > 32 { return nil }
            if rng.chance(0.3) {
                let words = taps
                slip(&taps, aims: words, spread, set)
            }
        } else if roll < 0.80 {
            // A slip after a first word ("specialchsracter").
            let first = model.word(wordIndex(band: rng.int(3)))
            let second = model.word(wordIndex(band: 1 + rng.int(2)))
            guard let a = self.taps(for: first, spread, set), var b = self.taps(for: second, spread, set) else { return nil }
            slip(&b, aims: aims(second, set), spread, set)
            taps = a + b
        } else if roll < 0.88 {
            // One of your own words.
            let word = rng.pick(Self.userSets[1])
            guard let t = self.taps(for: word.lowercased(), spread, set) else { return nil }
            taps = t
            if rng.chance(0.6) { slip(&taps, aims: aims(word.lowercased(), set), spread, set) }
            return decide(taps: taps, typed: typed(taps, set), set: set, user: 1, swearing: swearing)
        } else if roll < 0.95 {
            // A word never suggested, or one a letter from one.
            let words = blocked[false]!.filter { !$0.contains(" ") && model.index(of: $0) != nil }
            guard !words.isEmpty else { return nil }
            let word = rng.pick(words)
            guard let t = self.taps(for: word, spread, set) else { return nil }
            taps = t
            if rng.chance(0.6) { slip(&taps, aims: aims(word, set), spread, set) }
        } else {
            // What the model can't read: taps that don't match, a letter it lacks, nothing.
            let word = model.word(wordIndex(band: 0))
            guard let t = self.taps(for: word, spread, set) else { return nil }
            var typed = typed(t, set)
            switch rng.int(4) {
            case 0: typed += "e"
            case 1: typed = "1" + typed.dropFirst()
            case 2: typed = ""
            default: typed = String(typed.prefix(1)) + "'" + typed.dropFirst()
            }
            return decide(taps: t, typed: typed, set: set, user: user, swearing: swearing)
        }
        return decide(taps: taps, typed: typed(taps, set), set: set, user: user, swearing: swearing)
    }

    private mutating func decide(taps: [CGPoint], typed: String, set: KeySet, user: Int, swearing: Bool) -> [String: Any] {
        let d = decoder(user: user, swearing: swearing)
        let previous = previous()
        let valid = rng.chance(0.3)
        let keys = set.keys
        let decision = answer("decide", timed: true) { d.decide(taps: taps, typed: typed, previous: previous, literalIsValid: valid, keys: keys) }
        var line = base("decide", set, user: user, swearing: swearing)
        line["taps"] = taps.map(bits)
        line["typed"] = typed
        line["previous"] = previous ?? NSNull()
        line["valid"] = valid
        line["fixed"] = Self.json(decision)
        return line
    }

    static func json(_ d: TypingDecoder.Decision?) -> Any {
        guard let d else { return NSNull() }
        return ["replacement": d.replacement ?? NSNull(), "readings": d.readings.map { [$0.text, bits($0.cost)] }, "literal": bits(d.literalCost)] as [String: Any]
    }

    private mutating func mergeCase() -> [String: Any]? {
        let set = rng.pick(keysets)
        let spread = rng.pick([0, 0.08, 0.15, 0.25, 0.35])
        let user = rng.chance(0.3) ? 1 : 0, swearing = rng.chance(0.5)
        var first: String, second: String, space: CGPoint
        if rng.chance(0.6) {
            // One word with a space tapped for one of its letters ("deli ate").
            let word = model.word(wordIndex(band: rng.int(3))).filter { $0 != "'" }
            guard word.count >= 3 else { return nil }
            let chars = Array(word)
            let k = 1 + rng.int(chars.count - 2)
            first = String(chars[..<k])
            second = String(chars[(k + 1)...])
            guard let p = aim(chars[k], set) else { return nil }
            let bar = set.truth.space ?? CGRect(x: 0, y: Double(set.truth.bottomRowY) + Double(set.truth.pitch.height), width: 0, height: 40)
            space = CGPoint(x: Double(p.x) + rng.gaussian() * spread * Double(set.truth.pitch.width),
                            y: Double(bar.minY) + rng.uniform() * 0.7 * Double(bar.height))
        } else {
            first = model.word(wordIndex(band: rng.int(3))).filter { $0 != "'" }
            second = model.word(wordIndex(band: rng.int(3))).filter { $0 != "'" }
            space = spaceTap(set)
        }
        guard let firstTaps = taps(for: first, spread, set), let secondTaps = taps(for: second, spread, set) else { return nil }
        var typedFirst = typed(firstTaps, set)
        let typedSecond = typed(secondTaps, set)
        if rng.chance(0.1) { typedFirst = typedFirst.capitalized }
        let d = decoder(user: user, swearing: swearing)
        let previous = previous()
        let firstValid = rng.chance(0.5), secondValid = rng.chance(0.5)
        let keys = set.keys
        let merged = answer("merge") {
            d.merge(first: typedFirst, firstTaps: firstTaps, space: space, second: typedSecond, secondTaps: secondTaps,
                    previous: previous, firstIsValid: firstValid, secondIsValid: secondValid, keys: keys)
        }
        var line = base("merge", set, user: user, swearing: swearing)
        line["first"] = typedFirst
        line["firstTaps"] = firstTaps.map(bits)
        line["space"] = bits(space)
        line["second"] = typedSecond
        line["secondTaps"] = secondTaps.map(bits)
        line["previous"] = previous ?? NSNull()
        line["firstValid"] = firstValid
        line["secondValid"] = secondValid
        line["fixed"] = merged ?? NSNull()
        return line
    }

    private mutating func replacesCase() -> [String: Any]? {
        let set = rng.pick(keysets)
        let spread = rng.pick([0, 0.08, 0.15, 0.25, 0.35])
        let user = rng.chance(0.4) ? 1 : 0, swearing = rng.chance(0.5)
        let word = model.word(wordIndex())
        guard var taps = taps(for: word, spread, set) else { return nil }
        if rng.chance(0.6) { slip(&taps, aims: aims(word, set), spread, set) }
        let typed = typed(taps, set)
        let d = decoder(user: user, swearing: swearing)
        let previous = previous()
        let valid = rng.chance(0.3)
        let decision = d.decide(taps: taps, typed: typed, previous: previous, literalIsValid: valid, keys: set.keys)
        var readings = [word, model.word(wordIndex()), word + " " + model.word(wordIndex(band: 0)), rng.pick(Self.userSets[1]), typed]
        if let best = decision?.readings.first { readings.append(best.text) }
        let reading = rng.pick(readings)
        let keys = rng.chance(0.8) ? set.keys : set.truth
        let replaces = answer("replaces") { d.replaces(with: reading, taps: taps, typed: typed, previous: previous, literalIsValid: valid, keys: keys) }
        var line = base("replaces", set, user: user, swearing: swearing)
        line["reading"] = reading
        line["taps"] = taps.map(bits)
        line["typed"] = typed
        line["previous"] = previous ?? NSNull()
        line["valid"] = valid
        line["truth"] = keys == set.truth && keys != set.keys
        line["fixed"] = replaces ?? NSNull()
        return line
    }

    private mutating func modelCase() -> [String: Any]? {
        var line: [String: Any] = ["kind": "model", "lang": language.rawValue]
        let clean = self.clean
        let allowClean = rng.chance(0.5)
        let allowing: (String) -> Bool = { allowClean ? !clean.contains($0) : true }
        let word = rng.chance(0.92) ? model.word(wordIndex()) : rng.pick(["qzxv", "The", "dont"])
        switch rng.int(5) {
        case 0:
            let previous = previous()
            line["fn"] = "cost"
            line["word"] = word
            line["previous"] = previous ?? NSNull()
            line["out"] = model.cost(of: word, after: previous).map(bits) ?? NSNull()
        case 1:
            let followers = model.nextWords(after: word, allowing: { _ in true })
            let second = !followers.isEmpty && rng.chance(0.5) ? rng.pick(followers) : model.word(wordIndex(band: rng.int(3)))
            line["fn"] = "hasPair"
            line["word"] = word
            line["second"] = second
            line["out"] = model.hasPair(word, second)
        case 2:
            line["fn"] = "nextWords"
            line["word"] = word
            line["clean"] = allowClean
            line["out"] = model.nextWords(after: word, allowing: allowing)
        case 3:
            var prefix = String(word.prefix(1 + rng.int(4)))
            if rng.chance(0.2) { prefix = Self.withoutAccents(prefix) }
            let previous = rng.chance(0.3) ? nil : previous()
            line["fn"] = "completions"
            line["word"] = prefix
            line["previous"] = previous ?? NSNull()
            line["clean"] = allowClean
            line["out"] = model.completions(of: prefix, after: previous, allowing: allowing)
        default:
            let prefix = rng.chance(0.05) ? "zzq" : String(word.prefix(rng.int(5)))
            line["fn"] = "continuations"
            line["word"] = prefix
            if let codes = model.encode(prefix), let mass = model.continuations(after: codes) {
                line["out"] = mass.enumerated().filter { $0.element != 0 }.map { [$0.offset, bits($0.element)] as [Any] }
            } else {
                line["out"] = NSNull()
            }
        }
        counts["model", default: 0] += 1
        return line
    }
}

enum TapParityError: Error {
    case noModel(String)
    case noKeys
}
