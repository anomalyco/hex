import CoreGraphics
import Foundation
@testable import HEX

/// Swipe typing's parity cases: deterministic swipes (SplitMix64 from a fixed seed per
/// language) for the decoder's Swift oracle and its Rust port to read alike. They know no
/// decoder, so one stream of cases feeds either, or both in one process.
///
/// Words are drawn from the model by frequency (its commonest 2,000, the next 18,000, the
/// rest), plus the English contractions and your own words (names with capitals, words the
/// model lacks), and drawn through Hex's own keys (`KeyboardLayout` at 375, 390, 402 and
/// 430 pt and the Duo's split layout, its points joined as `SwipeTyping` joins them) with
/// per-key misses, a bias, wobble, curves, overshoot and undershoot at the ends, and pauses on
/// keys; with times or without (and sometimes as many times as points, sometimes not).
/// Paths too short to read, straight scribbles and random noise read as little or nothing.
/// The decoder's settings are those `SwipeTyping.syncWords` makes: the never-suggest list
/// with Suggest swear words off or on, English's contractions, and your own words or none.
enum SwipeParity {
    /// Bumped whenever the cases change.
    static let generatorVersion = 1
    static let languages: [KeyboardLanguage] = [.english, .spanish]

    static func seed(_ language: KeyboardLanguage) -> UInt64 {
        language == .english ? 0x4845_585F_5357_454E : 0x4845_585F_5357_4553
    }

    /// The decoder's words besides the model's, as `SwipeTyping.syncWords` makes them.
    struct Context {
        let id: Int
        let language: KeyboardLanguage
        let allowSwearing: Bool
        /// `OffensiveWords.blocked(allowSwearing:)`, sorted (a dictionary's order otherwise).
        let blocked: [String]
        /// English's contractions (added with `contractions: true`), none elsewhere.
        let contractions: [String]
        /// Added at `ownWordCost` after the contractions.
        let ownWords: [String]
    }

    struct Layout {
        let id: Int
        let name: String
        let layout: KeyboardLayout
        let keys: TypingDecoder.Keys
        /// Each letter's keycap centres where they're drawn: two for G and V on the split layout.
        let screen: [Character: [CGPoint]]
    }

    enum Kind: String {
        case common, frequent, rare, contraction, own, offKeys = "off-keys", short, scribble, noise
    }

    struct Case {
        let id: Int
        let context: Int
        let layout: Int
        let kind: Kind
        /// The word the path was drawn for, if any.
        let word: String
        /// Where the finger touched, on a quarter-point grid.
        let screen: [CGPoint]
        /// As the decoder gets them: `KeyboardLayout.joinedForSwipe` of `screen`.
        let points: [CGPoint]
        /// `timeBase + Double(tick) * timeUnit`, so a fixture can carry them as whole numbers.
        let timeBase: Double
        let ticks: [Int]?
        let previous: String?
        let limit: Int

        static let timeUnit = 0.001

        var times: [TimeInterval]? {
            ticks.map { $0.map { timeBase + Double($0) * Self.timeUnit } }
        }
    }

    /// A language's model, contexts and layouts. Ids are unique across languages.
    @MainActor
    struct Setup {
        let language: KeyboardLanguage
        let model: TypingDecoderModel
        let contexts: [Context]
        let layouts: [Layout]
        /// The model's words, commonest first.
        let ranked: [Int]

        init(_ language: KeyboardLanguage, firstContext: Int, firstLayout: Int) throws {
            self.language = language
            guard let model = TypingDecoderModel.bundled(language.rawValue) else {
                throw NSError(domain: "SwipeParity", code: 1, userInfo: [NSLocalizedDescriptionKey: "no \(language) model"])
            }
            self.model = model
            let offensive = OffensiveWords.bundled(language.rawValue)
            let contractions = language == .english ? AutocorrectEngine.englishContractions : []
            var contexts: [Context] = []
            for allowSwearing in [false, true] {
                for own in [[], SwipeParity.ownWords(language)] {
                    contexts.append(Context(
                        id: firstContext + contexts.count, language: language, allowSwearing: allowSwearing,
                        blocked: offensive.blocked(allowSwearing: allowSwearing).sorted(),
                        contractions: contractions, ownWords: own
                    ))
                }
            }
            self.contexts = contexts
            let built: [(String, KeyboardLayout)] = [
                ("375", KeyboardLayout(width: 375, language: language, phoneSize: .homeButton)),
                ("390", KeyboardLayout(width: 390, language: language)),
                ("402", KeyboardLayout(width: 402, language: language)),
                ("430", KeyboardLayout(width: 430, language: language, phoneSize: .large)),
                ("split", KeyboardLayout(width: 801, language: language, posture: .unfoldedWide, windowInset: 75)),
            ]
            var layouts: [Layout] = []
            for (name, layout) in built {
                guard let keys = TypingDecoder.Keys(layout: layout) else { continue }
                var screen: [Character: [CGPoint]] = [:]
                for key in layout.keys {
                    guard let c = key.character, c.count == 1, let letter = c.first, letter.isLetter else { continue }
                    screen[letter, default: []].append(CGPoint(x: key.frame.midX, y: key.frame.midY))
                }
                layouts.append(Layout(id: firstLayout + layouts.count, name: "\(language.rawValue)-\(name)", layout: layout, keys: keys, screen: screen))
            }
            self.layouts = layouts
            let costs = (0..<model.wordCount).map { model.unigramCost($0) }
            ranked = (0..<model.wordCount).sorted { costs[$0] != costs[$1] ? costs[$0] < costs[$1] : $0 < $1 }
        }
    }

    /// Your own words for the cases: Contacts names with capitals, words the model lacks or
    /// spells only lowercase, a repeat, a contraction again, and one too short to read.
    static func ownWords(_ language: KeyboardLanguage) -> [String] {
        switch language {
        case .spanish:
            ["Begoña", "Xóchitl", "José", "pibe", "Iñaki", "Muñoz", "güey", "Ainhoa", "chamba", "wasap", "Kevin",
             "iPhone", "Anomaly", "Ñandú", "x", "begoña", "Ryan", "Dax", "pingüino", "Sevilla", "Zoë", "McDonald's"]
        default:
            ["Kate", "Bell", "Will", "McDonald", "O'Brien", "iPhone", "Anomaly", "Dax", "Ryan", "hexkb", "opencode",
             "tallow", "Zoë", "kate", "I'm", "X", "Ångström", "naïve", "café", "Jürgen", "yeet", "Brexit", "covid",
             "Siobhan", "Nguyen", "Xiaoming", "qwerty", "asdf", "zzz", "y'all"]
        }
    }

    /// Words before a swipe that the model lacks, or only with another case or spelling.
    static let oddPrevious = ["", "xqzt", "Hexkb", "İstanbul", "ΟΔΟΣ", "don't", "I'm", "NASA", "123", "e\u{301}l", "ma\u{F1}ana"]
}

/// SplitMix64 (Steele, Lea and Flood): small, fast, and the same everywhere.
struct SplitMix64 {
    private(set) var state: UInt64

    init(seed: UInt64) { state = seed }

    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    mutating func unit() -> Double { Double(next() >> 11) * 0x1p-53 }
    mutating func uniform(_ lo: Double, _ hi: Double) -> Double { lo + (hi - lo) * unit() }
    mutating func int(_ n: Int) -> Int { Int(next() % UInt64(n)) }
    mutating func chance(_ p: Double) -> Bool { unit() < p }
    mutating func pick<T>(_ items: [T]) -> T { items[int(items.count)] }

    /// A standard normal (Box–Muller).
    mutating func normal() -> Double {
        let u = max(unit(), 1e-12), v = unit()
        return (-2 * log(u)).squareRoot() * cos(2 * .pi * v)
    }
}

/// The cases for one language, one after another.
@MainActor
struct SwipeParityGenerator {
    let setup: SwipeParity.Setup
    private var rng: SplitMix64
    private var nextID: Int

    init(_ setup: SwipeParity.Setup, firstID: Int = 0) {
        self.setup = setup
        rng = SplitMix64(seed: SwipeParity.seed(setup.language))
        nextID = firstID
    }

    /// A touch drawn on screen, with the time the finger spent still before it.
    private struct Touch {
        var point: CGPoint
        var pause: TimeInterval = 0
    }

    mutating func next() -> SwipeParity.Case {
        let id = nextID
        nextID += 1
        let context = rng.pick(setup.contexts)
        let layout = rng.pick(setup.layouts)
        let roll = rng.unit()
        var kind: SwipeParity.Kind
        var word = ""
        var touches: [Touch]
        switch roll {
        case ..<0.84:
            let pick = rng.unit()
            if pick < 0.40 {
                kind = .common
                word = setup.model.word(setup.ranked[rng.int(min(2000, setup.ranked.count))])
            } else if pick < 0.68 {
                kind = .frequent
                word = setup.model.word(setup.ranked[2000 + rng.int(18000)])
            } else if pick < 0.84 {
                kind = .rare
                word = setup.model.word(setup.ranked[20000 + rng.int(setup.ranked.count - 20000)])
            } else if pick < 0.92, !context.contractions.isEmpty {
                kind = .contraction
                word = rng.pick(context.contractions)
            } else {
                kind = .own
                word = rng.pick(SwipeParity.ownWords(setup.language))
            }
            touches = wordPath(word, on: layout, offKeys: false)
        case ..<0.89:
            kind = .offKeys
            word = setup.model.word(setup.ranked[rng.int(20000)])
            touches = wordPath(word, on: layout, offKeys: true)
        case ..<0.92:
            kind = .short
            touches = shortPath(on: layout)
        case ..<0.96:
            kind = .scribble
            touches = scribble(on: layout)
        default:
            kind = .noise
            touches = noise(on: layout)
        }
        if touches.isEmpty {
            kind = .noise
            touches = noise(on: layout)
        }
        // Touches land on a quarter-point grid, which a fixture carries as whole numbers (so
        // never at -0, which adding 0 makes 0).
        let screen = touches.map { CGPoint(x: ($0.point.x * 4).rounded() / 4 + 0, y: ($0.point.y * 4).rounded() / 4 + 0) }
        let points = screen.map(layout.layout.joinedForSwipe)
        let (timeBase, ticks) = times(for: touches, screen: screen)
        return SwipeParity.Case(
            id: id, context: context.id, layout: layout.id, kind: kind, word: word, screen: screen, points: points,
            timeBase: timeBase, ticks: ticks, previous: previous(), limit: limit()
        )
    }

    private mutating func limit() -> Int {
        let roll = rng.unit()
        return roll < 0.80 ? 4 : roll < 0.88 ? 1 : roll < 0.98 ? 8 : 3
    }

    private mutating func previous() -> String? {
        let roll = rng.unit()
        if roll < 0.35 { return nil }
        let common = setup.model.word(setup.ranked[rng.int(2000)])
        if roll < 0.72 { return common }
        if roll < 0.84 { return rng.chance(0.7) ? common.prefix(1).uppercased() + common.dropFirst() : common.uppercased() }
        if roll < 0.90 { return setup.model.word(setup.ranked[20000 + rng.int(setup.ranked.count - 20000)]) }
        return rng.pick(SwipeParity.oddPrevious)
    }

    private func bounds(_ layout: SwipeParity.Layout) -> CGRect {
        let all = layout.screen.values.flatMap { $0 }
        let xs = all.map(\.x), ys = all.map(\.y)
        return CGRect(x: xs.min()!, y: ys.min()!, width: xs.max()! - xs.min()!, height: ys.max()! - ys.min()!)
    }

    /// Through `word`'s keys (an accented letter on its base letter's, an apostrophe and a
    /// letter with no key on none) the way a finger goes: each key missed by its own bit, the
    /// whole path off by a bias, curved, wobbling, overshooting or stopping short at its ends,
    /// and sometimes pausing on a key. `offKeys` draws it a key or more away.
    private mutating func wordPath(_ word: String, on layout: SwipeParity.Layout, offKeys: Bool) -> [Touch] {
        let pitch = layout.keys.pitch
        var centres: [CGPoint] = []
        var lastLetter: Character?
        for letter in word.lowercased() where letter != "'" {
            let base = String(letter).decomposedStringWithCanonicalMapping.unicodeScalars.first.map(Character.init)
            let key = layout.screen[letter] != nil ? letter : base
            guard let key, let options = layout.screen[key] else { continue }
            if key == lastLetter { continue }
            lastLetter = key
            centres.append(rng.pick(options))
        }
        guard !centres.isEmpty else { return [] }
        let sigma = rng.pick([0, 0.05, 0.1, 0.15, 0.25, 0.35])
        var bias = CGVector.zero
        if offKeys {
            let angle = rng.uniform(0, 2 * .pi), far = rng.uniform(1, 3)
            bias = CGVector(dx: cos(angle) * far * pitch.width, dy: sin(angle) * far * pitch.height)
        } else if rng.chance(0.3) {
            bias = CGVector(dx: rng.normal() * 0.2 * pitch.width, dy: rng.normal() * 0.2 * pitch.height)
        }
        var targets = centres.map {
            CGPoint(x: $0.x + bias.dx + rng.normal() * sigma * pitch.width, y: $0.y + bias.dy + rng.normal() * sigma * pitch.height)
        }
        if targets.count == 1 {
            // One key: a small loop on it.
            let c = targets[0], r = rng.uniform(0.05, 0.4) * pitch.width, n = 3 + rng.int(8)
            return (0...n).map { i in
                let a = 2 * .pi * Double(i) / Double(n)
                return Touch(point: CGPoint(x: c.x + r * cos(a), y: c.y + r * sin(a)))
            }
        }
        // Overshooting past the first and last keys, or stopping short of them.
        func moved(_ point: CGPoint, awayFrom other: CGPoint, by share: Double) -> CGPoint {
            let dx = point.x - other.x, dy = point.y - other.y, length = max(1e-9, hypot(dx, dy))
            return CGPoint(x: point.x + dx / length * share * pitch.width, y: point.y + dy / length * share * pitch.width)
        }
        if rng.chance(0.5) { targets[0] = moved(targets[0], awayFrom: targets[1], by: rng.uniform(-0.3, 0.5)) }
        if rng.chance(0.5) {
            let n = targets.count
            targets[n - 1] = moved(targets[n - 1], awayFrom: targets[n - 2], by: rng.uniform(-0.3, 0.5))
        }
        // A touch every display frame (60 or 120 Hz) at the finger's speed.
        let step = rng.uniform(6, 30)
        let wobble = rng.pick([0, 0, 0.5, 1.5, 3])
        let curve = rng.chance(0.4) ? rng.uniform(-0.25, 0.25) : 0
        let pauses = rng.chance(0.5)
        var touches = [Touch(point: targets[0])]
        for (index, (a, b)) in zip(targets, targets.dropFirst()).enumerated() {
            let length = hypot(b.x - a.x, b.y - a.y)
            let n = max(1, Int((length / step).rounded(.up)))
            let bend = curve * rng.uniform(0.5, 1.5) * length
            let control = CGPoint(x: (a.x + b.x) / 2 - (b.y - a.y) / max(length, 1e-9) * bend, y: (a.y + b.y) / 2 + (b.x - a.x) / max(length, 1e-9) * bend)
            // A pause on the key this segment leaves: the finger stays, sending nothing or
            // a few still touches, then moves on.
            var pause: TimeInterval = 0
            if pauses, rng.chance(index == 0 ? 0.1 : 0.3) {
                for _ in 0..<rng.int(4) {
                    let jitter = rng.chance(0.5) ? 0 : 0.125
                    touches.append(Touch(point: CGPoint(x: a.x + jitter, y: a.y - jitter), pause: rng.uniform(0.01, 0.06)))
                }
                pause = rng.uniform(0.04, 0.4)
            }
            for i in 1...n {
                let t = Double(i) / Double(n)
                let x = (1 - t) * (1 - t) * a.x + 2 * (1 - t) * t * control.x + t * t * b.x
                let y = (1 - t) * (1 - t) * a.y + 2 * (1 - t) * t * control.y + t * t * b.y
                let shake = i == n ? 0 : wobble
                touches.append(Touch(point: CGPoint(x: x + rng.normal() * shake, y: y + rng.normal() * shake), pause: i == 1 ? pause : 0))
            }
        }
        if pauses, rng.chance(0.2) {
            // Resting on the last key before lifting.
            touches.append(Touch(point: touches[touches.count - 1].point, pause: rng.uniform(0.05, 0.3)))
        }
        return touches
    }

    /// Too short to read, or barely: one touch, two a few points apart, one spot touched
    /// again and again, a wiggle on one key.
    private mutating func shortPath(on layout: SwipeParity.Layout) -> [Touch] {
        // Never a dictionary's order, which changes from run to run.
        let c = layout.screen[rng.pick(layout.screen.keys.sorted())]![0], pitch = layout.keys.pitch
        switch rng.int(6) {
        case 0: return [Touch(point: c)]
        case 1: return [Touch(point: c), Touch(point: CGPoint(x: c.x + 4, y: c.y))]
        case 2: return Array(repeating: Touch(point: c), count: 2 + rng.int(6))
        case 3: return [Touch(point: c), Touch(point: c), Touch(point: CGPoint(x: c.x + 0.125, y: c.y))]
        case 4: return [Touch(point: c), Touch(point: CGPoint(x: c.x + 0.55 * pitch.width, y: c.y))]
        default:
            return (0..<6).map { _ in
                Touch(point: CGPoint(x: c.x + rng.uniform(-0.2, 0.2) * pitch.width, y: c.y + rng.uniform(-0.2, 0.2) * pitch.height))
            }
        }
    }

    /// Back and forth along a row: a straight scribble.
    private mutating func scribble(on layout: SwipeParity.Layout) -> [Touch] {
        let box = bounds(layout)
        let y = box.minY + Double(rng.int(3)) * box.height / 2 + rng.normal() * 3
        var x = rng.uniform(box.minX, box.maxX)
        var touches = [Touch(point: CGPoint(x: x, y: y))]
        for _ in 0..<(1 + rng.int(4)) {
            let to = rng.uniform(box.minX, box.maxX)
            let n = max(1, Int((abs(to - x) / rng.uniform(6, 30)).rounded(.up)))
            for i in 1...n {
                touches.append(Touch(point: CGPoint(x: x + (to - x) * Double(i) / Double(n), y: y + rng.normal())))
            }
            x = to
        }
        return touches
    }

    /// A random walk over and around the keys.
    private mutating func noise(on layout: SwipeParity.Layout) -> [Touch] {
        let box = bounds(layout).insetBy(dx: -layout.keys.pitch.width / 2, dy: -layout.keys.pitch.height / 2)
        var p = CGPoint(x: rng.uniform(box.minX, box.maxX), y: rng.uniform(box.minY, box.maxY))
        var touches = [Touch(point: p)]
        for _ in 0..<(4 + rng.int(60)) {
            let a = rng.uniform(0, 2 * .pi), r = rng.uniform(1, 30)
            p = CGPoint(x: p.x + r * cos(a), y: p.y + r * sin(a))
            touches.append(Touch(point: p))
        }
        return touches
    }

    /// The touches' times, as whole milliseconds after a base: at the finger's speed, with
    /// its pauses. A quarter of swipes have none, and a few have one too many or too few,
    /// all the same time, or some out of order.
    private mutating func times(for touches: [Touch], screen: [CGPoint]) -> (Double, [Int]?) {
        let base = Double(1000 + rng.int(200_000)) + 0.25 * Double(rng.int(4))
        let roll = rng.unit()
        guard roll >= 0.25 else { return (base, nil) }
        let speed = rng.uniform(300, 2500)
        var t = 0.0
        var ticks: [Int] = []
        for (i, touch) in touches.enumerated() {
            if i > 0 {
                let d = hypot(screen[i].x - screen[i - 1].x, screen[i].y - screen[i - 1].y)
                t += d / speed * rng.uniform(0.7, 1.3) + touch.pause
            }
            ticks.append(Int((t * 1000).rounded()))
        }
        if roll < 0.29 {
            if rng.chance(0.5), !ticks.isEmpty { ticks.removeLast() } else { ticks.append((ticks.last ?? 0) + 8) }
        } else if roll < 0.32 {
            ticks = ticks.map { _ in 0 }
        } else if roll < 0.35, ticks.count > 3 {
            for _ in 0..<(1 + ticks.count / 10) {
                let i = 1 + rng.int(ticks.count - 1)
                ticks[i] = max(0, ticks[i] - rng.int(40))
            }
        }
        return (base, ticks)
    }
}
