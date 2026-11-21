import CoreGraphics
import Darwin
import Foundation

/// The spatial decoder's word list and bigram language model, memory-mapped read-only from
/// a file `scripts/build-typing-model.py` writes. Mapped pages are clean and file-backed, so
/// iOS can drop them under pressure instead of counting them against the keyboard.
nonisolated final class TypingDecoderModel: @unchecked Sendable {
    let wordCount: Int
    let bigramCount: Int
    let byteCount: Int
    /// The language it was built for ("en", "es").
    let language: String
    /// Letter codes (1…) by character, each letter's character, and its unaccented letter's code.
    let codes: [Character: UInt8]
    let characters: [Character]
    let baseCodes: [UInt8]
    let apostropheCode: UInt8?
    private let base: UnsafeRawPointer
    private let offsetsAt: Int
    private let unigramAt: Int
    private let pairRowsAt: Int
    private let pairWordsAt: Int
    private let pairCostsAt: Int
    private let blobAt: Int

    init?(url: URL) {
        let fd = open(url.path, O_RDONLY)
        guard fd >= 0 else { return nil }
        defer { close(fd) }
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_size >= 28 else { return nil }
        let size = Int(info.st_size)
        guard let mapped = mmap(nil, size, PROT_READ, MAP_PRIVATE, fd, 0), mapped != MAP_FAILED else { return nil }
        let base = UnsafeRawPointer(mapped)
        func u32(_ at: Int) -> Int { Int(base.loadUnaligned(fromByteOffset: at, as: UInt32.self)) }
        let magic = base.loadUnaligned(as: UInt32.self)
        guard magic == 0x4D54_5848, u32(4) == 3 else { munmap(mapped, size); return nil }  // "HXTM" v3
        let words = u32(8), bigrams = u32(12), blob = u32(16), letters = u32(20)
        guard letters < 255, 28 + 8 * letters <= size else { munmap(mapped, size); return nil }
        let languageBytes = (0..<4).map { base.load(fromByteOffset: 24 + $0, as: UInt8.self) }.filter { $0 != 0 }
        var codes: [Character: UInt8] = [:]
        var characters = [Character](repeating: " ", count: letters + 1)
        var baseCodes = [UInt8](repeating: 0, count: letters + 1)
        for i in 0..<letters {
            let at = 28 + 8 * i
            guard let scalar = Unicode.Scalar(u32(at)) else { munmap(mapped, size); return nil }
            let code = Int(base.load(fromByteOffset: at + 4, as: UInt8.self))
            let baseCode = base.load(fromByteOffset: at + 5, as: UInt8.self)
            guard code >= 1, code <= letters, baseCode >= 1, Int(baseCode) <= letters else { munmap(mapped, size); return nil }
            codes[Character(scalar)] = UInt8(code)
            characters[code] = Character(scalar)
            baseCodes[code] = baseCode
        }
        let offsetsAt = 28 + 8 * letters
        let unigramAt = offsetsAt + 4 * (words + 1)
        var rowsAt = unigramAt + words
        rowsAt += (4 - rowsAt % 4) % 4
        let pairWordsAt = rowsAt + 4 * (words + 1)
        let pairCostsAt = pairWordsAt + 4 * bigrams
        let blobAt = pairCostsAt + bigrams
        guard blobAt + blob == size else { munmap(mapped, size); return nil }
        self.base = base
        self.byteCount = size
        self.language = String(decoding: languageBytes, as: UTF8.self)
        self.codes = codes
        self.characters = characters
        self.baseCodes = baseCodes
        self.apostropheCode = codes["'"]
        self.wordCount = words
        self.bigramCount = bigrams
        self.offsetsAt = offsetsAt
        self.unigramAt = unigramAt
        self.pairRowsAt = rowsAt
        self.pairWordsAt = pairWordsAt
        self.pairCostsAt = pairCostsAt
        self.blobAt = blobAt
    }

    deinit {
        munmap(UnsafeMutableRawPointer(mutating: base), byteCount)
    }

    @inline(__always) private func start(_ i: Int) -> Int {
        Int(base.loadUnaligned(fromByteOffset: offsetsAt + 4 * i, as: UInt32.self))
    }

    @inline(__always) func length(_ i: Int) -> Int { start(i + 1) - start(i) }

    /// The word's byte at `depth`, or -1 past its end.
    @inline(__always) func byte(_ i: Int, _ depth: Int) -> Int {
        let s = start(i)
        guard depth < start(i + 1) - s else { return -1 }
        return Int(base.load(fromByteOffset: blobAt + s + depth, as: UInt8.self))
    }

    func word(_ i: Int) -> String {
        let s = start(i)
        return String((s..<start(i + 1)).map { characters[Int(base.load(fromByteOffset: blobAt + $0, as: UInt8.self))] })
    }

    /// The word's letter codes, read in place from the mapped file (Swipe typing reads
    /// thousands a swipe).
    @inline(__always) func withCodes<R>(_ i: Int, _ body: (UnsafeRawBufferPointer) -> R) -> R {
        let s = start(i)
        return body(UnsafeRawBufferPointer(start: base.advanced(by: blobAt + s), count: start(i + 1) - s))
    }

    /// The word's letter codes, or nil when it has a letter the model doesn't.
    func encode(_ word: String) -> [UInt8]? {
        var out: [UInt8] = []
        for c in word {
            guard let code = codes[c] else { return nil }
            out.append(code)
        }
        return out
    }

    /// -ln P(word), in nats.
    @inline(__always) func unigramCost(_ i: Int) -> Double {
        Double(base.load(fromByteOffset: unigramAt + i, as: UInt8.self)) / 8
    }

    /// -ln P(b | a), in nats, when the pair is in the model.
    func bigramCost(_ a: Int, _ b: Int) -> Double? {
        guard a >= 0, a < wordCount else { return nil }
        var lo = Int(base.loadUnaligned(fromByteOffset: pairRowsAt + 4 * a, as: UInt32.self))
        let end = Int(base.loadUnaligned(fromByteOffset: pairRowsAt + 4 * (a + 1), as: UInt32.self))
        guard lo <= end, end <= bigramCount else { return nil }
        var hi = end
        let target = UInt32(b)
        while lo < hi {
            let mid = (lo + hi) / 2
            if base.loadUnaligned(fromByteOffset: pairWordsAt + 4 * mid, as: UInt32.self) < target { lo = mid + 1 } else { hi = mid }
        }
        guard lo < end, base.loadUnaligned(fromByteOffset: pairWordsAt + 4 * lo, as: UInt32.self) == target else { return nil }
        return Double(base.load(fromByteOffset: pairCostsAt + lo, as: UInt8.self)) / 8
    }

    func index(of word: String) -> Int? {
        guard let bytes = encode(word) else { return nil }
        return index(ofCodes: bytes)
    }

    /// Whether the model has `second` following `first` as a pair.
    func hasPair(_ first: String, _ second: String) -> Bool {
        guard let a = index(of: first), let b = index(of: second) else { return false }
        return bigramCost(a, b) != nil
    }

    /// -ln P(word | previous) with the decoder's backoff to the unigram (0.92 nats), or nil
    /// when the model lacks the word.
    func cost(of word: String, after previous: String?) -> Double? {
        guard let i = index(of: word) else { return nil }
        let backoff = 0.92 + unigramCost(i)
        guard let previous, let a = index(of: previous), let pair = bigramCost(a, i) else { return backoff }
        return min(pair, backoff)
    }

    /// The three most probable allowed continuations of a completed word. Scan just its
    /// mapped bigram row and keep three entries, even for a word with thousands of followers.
    func nextWords(after previous: String, allowing: (String) -> Bool) -> [String] {
        guard let a = index(of: previous) else { return [] }
        let start = Int(base.loadUnaligned(fromByteOffset: pairRowsAt + 4 * a, as: UInt32.self))
        let end = Int(base.loadUnaligned(fromByteOffset: pairRowsAt + 4 * (a + 1), as: UInt32.self))
        guard start <= end, end <= bigramCount else { return [] }
        var best: [(rank: Int, text: String)] = []
        for i in start..<end {
            let cost = Int(base.load(fromByteOffset: pairCostsAt + i, as: UInt8.self))
            let b = Int(base.loadUnaligned(fromByteOffset: pairWordsAt + 4 * i, as: UInt32.self))
            guard b < wordCount else { continue }
            // Prefer the commoner word when two pair probabilities quantize to the same byte.
            let rank = cost * 256 + Int(base.load(fromByteOffset: unigramAt + b, as: UInt8.self))
            if best.count == 3, rank >= best[2].rank { continue }
            let text = word(b)
            guard allowing(text) else { continue }
            let at = best.firstIndex { rank < $0.rank } ?? best.count
            best.insert((rank, text), at: at)
            if best.count > 3 { best.removeLast() }
        }
        return best.map(\.text)
    }

    /// Complete a short prefix with the word frequencies, backed by the previous word's
    /// bigrams when available. Word indices and each bigram row have the same sort order:
    /// walk only the matching prefix range, creating strings only for contenders.
    func completions(of prefix: String, after previous: String?, allowing: (String) -> Bool) -> [String] {
        guard let codes = encode(prefix), (1...3).contains(codes.count) else { return [] }
        var lo = 0, hi = wordCount
        for (depth, code) in codes.enumerated() {
            (lo, hi) = narrow(lo, hi, depth: depth, byte: Int(code))
            if lo == hi { return [] }
        }
        var link = 0, end = 0
        if let previous, let a = index(of: previous) {
            link = Int(base.loadUnaligned(fromByteOffset: pairRowsAt + 4 * a, as: UInt32.self))
            end = Int(base.loadUnaligned(fromByteOffset: pairRowsAt + 4 * (a + 1), as: UInt32.self))
            guard link <= end, end <= bigramCount else { return [] }
            var upper = end
            while link < upper {
                let mid = (link + upper) / 2
                if follower(mid) < lo { link = mid + 1 } else { upper = mid }
            }
        }
        var best: [(rank: Int, text: String)] = []
        var next = link < end ? follower(link) : hi
        for i in lo..<hi {
            let unigram = Int(base.load(fromByteOffset: unigramAt + i, as: UInt8.self))
            var cost = unigram + 7 // 0.92 nats of unigram backoff, in eighths of a nat.
            if i == next {
                cost = min(cost, Int(base.load(fromByteOffset: pairCostsAt + link, as: UInt8.self)))
                link += 1
                next = link < end ? follower(link) : hi
            }
            let rank = cost * 256 + unigram
            guard length(i) > codes.count, length(i) <= codes.count + 8,
                  best.count < 2 || rank < best[1].rank else { continue }
            let text = word(i)
            guard allowing(text) else { continue }
            let at = best.firstIndex { rank < $0.rank } ?? best.count
            best.insert((rank, text), at: at)
            if best.count > 2 { best.removeLast() }
        }
        return best.map(\.text)
    }

    @inline(__always) private func follower(_ link: Int) -> Int {
        Int(base.loadUnaligned(fromByteOffset: pairWordsAt + 4 * link, as: UInt32.self))
    }

    /// How much of the word list's frequency continues `prefix` with each letter, by letter
    /// code. Nil when no word starts with `prefix`, or one ends there and none goes on.
    func continuations(after prefix: [UInt8]) -> [Double]? {
        var lo = 0, hi = wordCount
        for (depth, b) in prefix.enumerated() {
            (lo, hi) = narrow(lo, hi, depth: depth, byte: Int(b))
            if lo >= hi { return nil }
        }
        var mass = [Double](repeating: 0, count: characters.count)
        let depth = prefix.count
        var any = false
        for i in lo..<hi {
            let code = byte(i, depth)
            guard code > 0, code != Int(apostropheCode ?? 0) else { continue }
            mass[code] += Self.frequency[Int(base.load(fromByteOffset: unigramAt + i, as: UInt8.self))]
            any = true
        }
        return any ? mass : nil
    }

    /// exp(-cost) for each stored unigram cost byte (cost = byte / 8 nats).
    private static let frequency: [Double] = (0..<256).map { exp(-Double($0) / 8) }

    func index(ofCodes bytes: [UInt8]) -> Int? {
        var lo = 0, hi = wordCount
        for (depth, b) in bytes.enumerated() {
            (lo, hi) = narrow(lo, hi, depth: depth, byte: Int(b))
            if lo >= hi { return nil }
        }
        return length(lo) == bytes.count ? lo : nil
    }

}

/// A word list sorted by letter codes, searchable letter by letter: the model's, or yours.
nonisolated protocol TypingDecoderWords {
    var wordCount: Int { get }
    func length(_ i: Int) -> Int
    /// The word's letter code at `depth`, or -1 past its end.
    func byte(_ i: Int, _ depth: Int) -> Int
}

extension TypingDecoderWords {
    /// The words in `lo..<hi` (which share their first `depth` codes) whose next code is `byte`.
    func narrow(_ lo: Int, _ hi: Int, depth: Int, byte target: Int) -> (Int, Int) {
        var a = lo, b = hi
        while a < b {
            let mid = (a + b) / 2
            if byte(mid, depth) < target { a = mid + 1 } else { b = mid }
        }
        var c = a, d = hi
        while c < d {
            let mid = (c + d) / 2
            if byte(mid, depth) <= target { c = mid + 1 } else { d = mid }
        }
        return (a, c)
    }
}

extension TypingDecoderModel: TypingDecoderWords {}

extension TypingDecoder {
    /// Your own words for the decoder (Contacts names and other `UILexicon` terms, text
    /// replacements, words you kept, fixes Hex learned), as you spell them, each scored as a
    /// moderately common word. Words with a letter the model lacks are left out.
    struct UserWords: TypingDecoderWords, Sendable {
        private(set) var codes: [[UInt8]] = []
        private(set) var texts: [String] = []
        private var lookup: [[UInt8]: Int] = [:]

        init() {}

        init(_ words: [String], model: TypingDecoderModel) {
            var byCodes: [[UInt8]: String] = [:]
            for word in words {
                guard word.count >= 2, word.allSatisfy({ $0.isLetter || $0 == "'" }),
                      let encoded = model.encode(word.lowercased()) else { continue }
                if byCodes[encoded] == nil { byCodes[encoded] = word }
            }
            codes = byCodes.keys.sorted { $0.lexicographicallyPrecedes($1) }
            texts = codes.map { byCodes[$0]! }
            for (i, c) in codes.enumerated() { lookup[c] = i }
        }

        var wordCount: Int { codes.count }
        var isEmpty: Bool { codes.isEmpty }
        func length(_ i: Int) -> Int { codes[i].count }
        func byte(_ i: Int, _ depth: Int) -> Int { depth < codes[i].count ? Int(codes[i][depth]) : -1 }
        func index(of encoded: [UInt8]) -> Int? { lookup[encoded] }
    }
}

extension TypingDecoderModel {
    /// The model for a language in the keyboard's bundle (`HEXKeyboard/TypingModel/<code>.hextm`,
    /// built by `scripts/build-typing-model.py`), mapped once per process; nil when the build
    /// has none. The app's keyboard preview finds it inside the embedded keyboard.
    @MainActor static func bundled(_ language: String) -> TypingDecoderModel? {
        if let cached = loaded[language] { return cached }
        var url = Bundle.main.url(forResource: language, withExtension: "hextm")
        if url == nil, let plugins = Bundle.main.builtInPlugInsURL,
           let keyboard = Bundle(url: plugins.appendingPathComponent("HEXKeyboard.appex")) {
            url = keyboard.url(forResource: language, withExtension: "hextm")
        }
        let model = url.flatMap { TypingDecoderModel(url: $0) }
        loaded[language] = model
        return model
    }

    @MainActor private static var loaded: [String: TypingDecoderModel?] = [:]
}

/// Experiment (Spatial autocorrect): a noisy-channel word decoder. Each tap is scored against
/// every letter with a 2D Gaussian around the key's centre (shifted by where you tend to tap,
/// when known), words are searched in the model's sorted word list with a beam, and a bigram
/// language model on the word before weighs them. A missed space is tried by splitting the
/// taps in two. What you typed is replaced only when the best reading beats it by a margin.
nonisolated struct TypingDecoder {
    struct Parameters: Equatable, Sendable {
        // Tuned on the benchmark's dev sentences (experiment folder 2026-11-20-hex-typing-logs),
        // keeping wrong corrections of correctly typed words no higher than today's engine.
        /// Touch spread as a share of the key pitch (key width plus gap; row height plus gap).
        var sigmaX = 0.29
        var sigmaY = 0.26
        var lmWeight = 1.25
        var deletionCost = 7.0
        var insertionCost = 7.0
        var transpositionCost = 5.0
        var spaceOmissionCost = 6.0
        /// A tap on the bottom row's upper half read as a space the finger missed ("ofbthe"):
        /// this, plus `spaceTapWeight` times how much nearer its letter row than the space bar
        /// it landed (the bar's Gaussian likelihood against the row's). Below the row's centre
        /// such a tap costs `spaceOmissionCost`.
        var spaceTapCost = 5.0
        var spaceTapWeight = 0.25
        /// A space tap is read as a letter meant on the row above ("deli ate" for "delicate")
        /// only when the row is at most this many nats less likely than the space bar for it:
        /// within about the bar's top fifth, a little deeper than the system keyboard merges.
        var mergeReach = 3.0
        var maxEdits = 1
        var beam = 48
        /// Nats the best reading must beat what you typed by to replace it on space.
        var autoMargin = 2.0
        /// The same when what you typed is itself a word.
        var validMargin = 7.0
        /// Language-model cost of a word the model lacks that the dictionary, your learned
        /// words or the shorthand list accept, and of one nothing accepts.
        var unknownValidCost = 15.0
        var unknownCost = 22.0
        var maxTapCost = 9.0
        var splits = true
        /// Extra cost of an accented letter typed as its base letter's key (é from e).
        var accentCost = 1.0
        /// A one-letter word stands alone in a split only when commoner than this (-ln p).
        var oneLetterWordCost = 7.0
        /// Language-model cost of one of your own words the model lacks (-ln p, about a word
        /// ranked 10,000th).
        var userWordCost = 11.0
        /// The margin when the best reading only adds or moves accents on what you typed
        /// ("esta" -> "está"); ñ for n isn't one ("ano", "año").
        var accentMargin = 2.0

        /// The settings tuned for a language's model on the benchmark's dev sentences: English
        /// leans more on the word before (Google Books' English pairs are richer).
        static func tuned(for language: String) -> Parameters {
            var p = Parameters()
            if language == "en" { p.lmWeight = 1.5 }
            // Spanish's many one- and two-letter words ("a", "es", "va") make a letter read as
            // a space too cheap a split ("vaxes" -> "va es"): there, only below the row's centre.
            if language == "es" { p.spaceTapCost = .infinity }
            return p
        }
    }

    struct Keys: Equatable, Sendable {
        /// Each letter key's centre, shifted by your tap offsets when there are any.
        var centres: [Character: CGPoint]
        var pitch: CGSize
        /// The bottom letter row's centre, below which a tap may have been meant for space.
        var bottomRowY: CGFloat
        /// The space bar's keycap as on the unsplit keyboard (both bars on a split one), or
        /// nil when unknown: a bottom-row tap above it may have been meant for space.
        var space: CGRect?

        init(centres: [Character: CGPoint], pitch: CGSize, bottomRowY: CGFloat, space: CGRect? = nil) {
            self.centres = centres
            self.pitch = pitch
            self.bottomRowY = bottomRowY
            self.space = space
        }

        /// The letter keys of `layout` (nil off the letters page). `offsets` (in points, by
        /// letter) moves a centre to where you tend to tap it.
        init?(layout: KeyboardLayout, offsets: [Character: CGVector] = [:]) {
            guard layout.layer == .letters else { return nil }
            var centres: [Character: CGPoint] = [:]
            var bottom: CGFloat = 0
            var space: CGRect?
            for key in layout.keys where key.kind == .space {
                let minX = layout.unsplitX(key.frame.minX), maxX = layout.unsplitX(key.frame.maxX)
                let bar = CGRect(x: minX, y: key.frame.minY, width: max(0, maxX - minX), height: key.frame.height)
                space = space.map { $0.union(bar) } ?? bar
            }
            for key in layout.keys {
                guard let c = key.character, c.count == 1, let letter = c.first, letter.isLetter else { continue }
                let shift = offsets[letter] ?? .zero
                // A split layout's halves are pushed together, so a letter on both halves (G, V)
                // has one centre, where the unsplit keyboard has it.
                centres[letter] = CGPoint(x: layout.unsplitX(key.frame.midX) + shift.dx, y: key.frame.midY + shift.dy)
                bottom = max(bottom, key.frame.midY)
            }
            let m = layout.metrics
            self.init(
                centres: centres,
                pitch: CGSize(
                    width: layout.isSplit ? SplitGeometry.pitch : layout.letterWidth + m.gap,
                    height: m.keyHeight + m.rowGap
                ),
                bottomRowY: bottom,
                space: space
            )
        }
    }

    struct Reading: Equatable, Sendable {
        var text: String
        var cost: Double
    }

    struct Decision: Equatable, Sendable {
        /// What space should put in place of what you typed, or nil to keep it.
        var replacement: String?
        /// Readings better than what you typed, best first, for the suggestion strip.
        var readings: [Reading]
        var literalCost: Double
    }

    /// Model words and two-word readings never offered (`OffensiveWords`): still scored as
    /// words, so what you typed is read right, but never a reading or a replacement.
    struct NeverSuggest: Equatable, Sendable {
        struct Pair: Hashable, Sendable {
            var first: Int
            var second: Int
        }

        var words: Set<Int> = []
        var pairs: Set<Pair> = []

        init() {}

        /// Whether `word` after `previous` makes a blocked two-word expression.
        func follows(_ word: Int, after previous: Int?) -> Bool {
            guard let previous, !pairs.isEmpty else { return false }
            return pairs.contains(Pair(first: previous, second: word))
        }

        /// `blocked`'s words and two-word phrases that are in `model`.
        init(_ blocked: [String], model: TypingDecoderModel) {
            for entry in blocked {
                let parts = entry.split(separator: " ").map(String.init)
                switch parts.count {
                case 1:
                    if let i = model.index(of: entry) { words.insert(i) }
                case 2:
                    if let a = model.index(of: parts[0]), let b = model.index(of: parts[1]) {
                        pairs.insert(Pair(first: a, second: b))
                    }
                default:
                    break
                }
            }
        }
    }

    let model: TypingDecoderModel
    var parameters = Parameters()
    /// Your own words, searched beside the model's.
    var userWords = UserWords()
    /// Words never offered; set by `AutocorrectEngine` for the language and Suggest swear words.
    var neverSuggest = NeverSuggest()

    /// Learn how I type's average tap offsets (in keycap widths and heights) as points, for
    /// letters with 20 taps or more, shrunk toward the centre while there are few.
    static func offsets(from stats: [String: TypingJournalKeyStats], layout: KeyboardLayout) -> [Character: CGVector] {
        var out: [Character: CGVector] = [:]
        for key in layout.keys {
            guard let c = key.character, c.count == 1, let letter = c.first, letter.isLetter,
                  let s = stats[c], s.count >= 20 else { continue }
            let weight = Double(s.count) / Double(s.count + 20)
            let mean = s.meanOffset
            out[letter] = CGVector(dx: mean.dx * key.frame.width * weight, dy: mean.dy * key.frame.height * weight)
        }
        return out
    }

    init(model: TypingDecoderModel, parameters: Parameters? = nil) {
        self.model = model
        self.parameters = parameters ?? Parameters.tuned(for: model.language)
    }

    /// `typed` is the lowercase letters the keys resolved, one per tap. Nil when the model
    /// can't read it (a letter it lacks, or taps that don't match).
    func decide(taps: [CGPoint], typed: String, previous: String?, literalIsValid: Bool, keys: Keys) -> Decision? {
        guard taps.count == typed.count, !typed.isEmpty, let letters = model.encode(typed),
              !letters.contains(where: { $0 == model.apostropheCode }) else { return nil }
        let centres = codeCentres(keys)
        let costs = taps.map { tapCosts($0, centres: centres, keys: keys) }
        let prev = previous.flatMap { model.index(of: $0) }

        var literalCost = 0.0
        for (i, code) in letters.enumerated() {
            let c = costs[i][Int(code)]
            guard c.isFinite else { return nil }
            literalCost += c
        }
        let literalIndex = model.index(ofCodes: letters)
        let literalIsYours = userWords.index(of: letters) != nil
        var literalLM = literalIndex.map { lmCost($0, after: prev) }
            ?? (literalIsValid ? parameters.unknownValidCost : parameters.unknownCost)
        if literalIsYours { literalLM = min(literalLM, parameters.userWordCost) }
        literalCost += parameters.lmWeight * literalLM

        var readings: [Reading] = []
        for (word, spatial) in search(model, costs, range: 0..<costs.count, limit: 5, maxEdits: parameters.maxEdits, skipping: neverSuggest.words)
        where word != literalIndex && !neverSuggest.follows(word, after: prev) {
            readings.append(Reading(text: model.word(word), cost: spatial + parameters.lmWeight * lmCost(word, after: prev)))
        }
        if !userWords.isEmpty {
            for (word, spatial) in search(userWords, costs, range: 0..<costs.count, limit: 3, maxEdits: parameters.maxEdits)
            where userWords.codes[word] != letters {
                readings.append(Reading(text: userWords.texts[word], cost: spatial + parameters.lmWeight * parameters.userWordCost))
            }
        }
        if parameters.splits, letters.count >= 3 {
            readings += splitReadings(costs, taps: taps, letters: letters, keys: keys, prev: prev)
        }
        readings.sort { $0.cost < $1.cost }
        var seen = Set<String>()
        readings = readings.filter {
            $0.text.lowercased() != typed && $0.cost < literalCost && seen.insert($0.text.lowercased()).inserted
        }

        var replacement: String?
        if let best = readings.first {
            var margin = (literalIndex != nil || literalIsValid || literalIsYours) ? parameters.validMargin : parameters.autoMargin
            if Self.withoutAccents(best.text.lowercased()) == Self.withoutAccents(typed) {
                margin = min(margin, parameters.accentMargin)
            }
            if literalCost - best.cost >= margin { replacement = best.text }
        }
        return Decision(replacement: replacement, readings: Array(readings.prefix(3)), literalCost: literalCost)
    }

    /// The text with acute, grave, circumflex and diaeresis marks removed (é -> e, ü -> u),
    /// keeping ñ.
    static func withoutAccents(_ text: String) -> String {
        let marks: ClosedRange<UInt32> = 0x300...0x308
        let scalars = text.decomposedStringWithCanonicalMapping.unicodeScalars
            .filter { !(marks.contains($0.value) && $0.value != 0x303) }
        return String(String.UnicodeScalarView(scalars)).precomposedStringWithCanonicalMapping
    }

    /// -ln P(word | previous) with stupid backoff (0.4) to the unigram.
    private func lmCost(_ word: Int, after prev: Int?) -> Double {
        let backoff = 0.92 + model.unigramCost(word)
        guard let prev, let bigram = model.bigramCost(prev, word) else { return backoff }
        return min(bigram, backoff)
    }

    /// Key centres by the model's letter codes; nil for letters without a key.
    private func codeCentres(_ keys: Keys) -> [CGPoint?] {
        var out = [CGPoint?](repeating: nil, count: model.characters.count)
        for (c, p) in keys.centres {
            if let code = model.codes[c] { out[Int(code)] = p }
        }
        return out
    }

    /// Each letter code's cost for one tap, in nats above the likeliest letter's. An accented
    /// letter costs at most its base letter's plus `accentCost` (é on an English or Spanish
    /// layout, ñ typed with n); one with neither key is impossible (infinity).
    private func tapCosts(_ p: CGPoint, centres: [CGPoint?], keys: Keys) -> [Double] {
        let sx = Double(keys.pitch.width) * parameters.sigmaX
        let sy = Double(keys.pitch.height) * parameters.sigmaY
        var out = centres.map { centre -> Double in
            guard let c = centre else { return .infinity }
            let dx = (Double(p.x) - Double(c.x)) / sx
            let dy = (Double(p.y) - Double(c.y)) / sy
            return 0.5 * (dx * dx + dy * dy)
        }
        // Also from its base key even when it has its own (ñ from n on the Spanish layout):
        // whichever is cheaper.
        let raw = out
        for code in out.indices where code > 0 {
            let base = Int(model.baseCodes[code])
            if base != code, raw[base].isFinite { out[code] = min(raw[code], raw[base] + parameters.accentCost) }
        }
        let best = out.filter(\.isFinite).min() ?? 0
        for i in out.indices { out[i] -= best }
        return out
    }

    private struct State {
        var lo: Int32
        var hi: Int32
        var tap: Int16
        var edits: Int8
        var cost: Float
    }

    /// The best words of `words` for `costs[range]`, as (word index, spatial cost incl. edits),
    /// leaving out `skipping` (searched through, never found).
    private func search<Words: TypingDecoderWords>(
        _ words: Words, _ costs: [[Double]], range: Range<Int>, limit: Int, maxEdits: Int, skipping: Set<Int> = []
    ) -> [(Int, Double)] {
        let n = range.count
        let maxDepth = n + maxEdits + 2
        var layers = [[State]](repeating: [], count: maxDepth + 2)
        layers[0] = [State(lo: 0, hi: Int32(words.wordCount), tap: 0, edits: 0, cost: 0)]
        var found: [Int: Double] = [:]
        let letterChoices: [[(Int, Double)]] = range.map { i in
            costs[i].enumerated().filter { $0.offset > 0 && $0.element <= parameters.maxTapCost }
                .sorted { $0.element < $1.element }.map { ($0.offset, $0.element) }
        }
        // Dropping a clearly aimed, distinct tap needs more evidence than dropping an
        // accidental repeat or a tap between keys. Accents share their physical key.
        let extraTapCosts: [Double] = maxEdits == 0 ? [] : letterChoices.indices.map { tap in
            guard let code = letterChoices[tap].first?.0 else { return parameters.insertionCost }
            let key = model.baseCodes[code]
            for neighbor in [tap - 1, tap + 1] where letterChoices.indices.contains(neighbor) {
                if let other = letterChoices[neighbor].first?.0, model.baseCodes[other] == key {
                    return parameters.insertionCost
                }
            }
            let gap = letterChoices[tap].first { model.baseCodes[$0.0] != key }?.1 ?? parameters.maxTapCost
            return parameters.insertionCost + min(4, gap)
        }
        let apostrophe = model.apostropheCode.map(Int.init)
        let allCodes = (1..<model.characters.count).filter { $0 != apostrophe }
        for depth in 0...maxDepth {
            var layer = layers[depth]
            if layer.count > parameters.beam {
                layer.sort { $0.cost < $1.cost }
                layer.removeLast(layer.count - parameters.beam)
            }
            var k = 0
            while k < layer.count {
                let s = layer[k]
                k += 1
                let lo = Int(s.lo), hi = Int(s.hi), tap = Int(s.tap)
                if tap == n, words.length(lo) == depth, !skipping.contains(lo) {
                    let c = Double(s.cost)
                    if c < found[lo] ?? .infinity { found[lo] = c }
                }
                guard depth < maxDepth else { continue }
                func push(_ code: Int, tap nextTap: Int, edits: Int8, cost: Double) {
                    let (a, b) = words.narrow(lo, hi, depth: depth, byte: code)
                    guard a < b else { return }
                    layers[depth + 1].append(State(lo: Int32(a), hi: Int32(b), tap: Int16(nextTap), edits: edits, cost: Float(cost)))
                }
                let base = Double(s.cost)
                if tap < n {
                    for (code, c) in letterChoices[tap] {
                        push(code, tap: tap + 1, edits: s.edits, cost: base + c)
                    }
                }
                // Apostrophes are never typed: "dont" reads as "don't" for free.
                if depth > 0, let apostrophe { push(apostrophe, tap: tap, edits: s.edits, cost: base) }
                guard Int(s.edits) < maxEdits else { continue }
                let e = s.edits + 1
                // A letter you didn't type.
                for code in allCodes {
                    push(code, tap: tap, edits: e, cost: base + parameters.deletionCost)
                }
                // A tap that isn't in the word.
                if tap < n {
                    layer.append(State(lo: s.lo, hi: s.hi, tap: s.tap + 1, edits: e, cost: Float(base + extraTapCosts[tap])))
                }
                // Two letters typed in the wrong order.
                if tap + 1 < n, depth + 2 <= maxDepth {
                    for (a, ca) in letterChoices[tap + 1].prefix(3) {
                        let (lo2, hi2) = words.narrow(lo, hi, depth: depth, byte: a)
                        guard lo2 < hi2 else { continue }
                        for (b, cb) in letterChoices[tap].prefix(3) where b != a {
                            let (lo3, hi3) = words.narrow(lo2, hi2, depth: depth + 1, byte: b)
                            guard lo3 < hi3 else { continue }
                            layers[depth + 2].append(State(
                                lo: Int32(lo3), hi: Int32(hi3), tap: Int16(tap + 2), edits: e,
                                cost: Float(base + ca + cb + parameters.transpositionCost)
                            ))
                        }
                    }
                }
            }
        }
        return found.sorted { $0.value < $1.value }.prefix(limit).map { ($0.key, $0.value) }
    }

    /// Two words where the taps had no space, or one meant for space ("seehow", "seenhow").
    /// Each half is read with substitutions only, which keeps this cheap; around a tap on
    /// the bottom row's upper half read as a space ("ofbthe"), only as the letters typed, so
    /// a name's middle letter ("rima") is never a space with slips around it.
    private func splitReadings(_ costs: [[Double]], taps: [CGPoint], letters: [UInt8], keys: Keys, prev: Int?) -> [Reading] {
        let n = costs.count
        var out: [Reading] = []
        func pair(left: Range<Int>, right: Range<Int>, extra: Double, exact: Bool = false) {
            guard !left.isEmpty, !right.isEmpty else { return }
            let never = neverSuggest
            func halves(_ range: Range<Int>) -> [(Int, Double)] {
                guard exact else { return search(model, costs, range: range, limit: 3, maxEdits: 0, skipping: never.words) }
                guard let word = model.index(ofCodes: Array(letters[range])), !never.words.contains(word) else { return [] }
                return [(word, range.reduce(0) { $0 + costs[$1][Int(letters[$1])] })]
            }
            let lefts = halves(left).filter { canStandAlone($0.0) }
            guard !lefts.isEmpty else { return }
            let rights = halves(right).filter { canStandAlone($0.0) }
            for (l, ls) in lefts {
                let lcost = ls + parameters.lmWeight * lmCost(l, after: prev)
                for (r, rs) in rights where !never.follows(r, after: l) {
                    let cost = lcost + rs + parameters.lmWeight * lmCost(r, after: l) + extra
                    out.append(Reading(text: model.word(l) + " " + model.word(r), cost: cost))
                }
            }
        }
        for k in 1..<n {
            pair(left: 0..<k, right: k..<n, extra: parameters.spaceOmissionCost)
            guard k < n - 1 else { continue }
            if taps[k].y > keys.bottomRowY {
                pair(left: 0..<k, right: (k + 1)..<n, extra: parameters.spaceOmissionCost)
            } else if let extra = spaceCost(of: taps[k], keys: keys) {
                pair(left: 0..<k, right: (k + 1)..<n, extra: extra, exact: true)
            }
        }
        return out
    }

    /// The cost of reading a tap on the bottom row's upper half as a space the finger missed,
    /// or nil when it can't be one: above the space bar, priced by how much nearer the row's
    /// centre than the bar's it landed.
    private func spaceCost(of tap: CGPoint, keys: Keys) -> Double? {
        guard parameters.spaceTapCost.isFinite, let bar = keys.space, bar.width > 0 else { return nil }
        let slack = keys.pitch.width / 2
        guard tap.x >= bar.minX - slack, tap.x <= bar.maxX + slack,
              tap.y > keys.bottomRowY - keys.pitch.height / 2 else { return nil }
        let sy = Double(keys.pitch.height) * parameters.sigmaY
        let toBar = (Double(tap.y) - Double(bar.midY)) / sy
        let toRow = (Double(tap.y) - Double(keys.bottomRowY)) / sy
        let likelierRow = max(0, 0.5 * (toBar * toBar - toRow * toRow))
        return parameters.spaceTapCost + parameters.spaceTapWeight * likelierRow
    }

    /// How much likelier the space bar than the bottom letter row a space tap was, in nats
    /// (0 when the row is likelier), or nil when it isn't above a bottom-row letter.
    private func rowCost(of tap: CGPoint, keys: Keys) -> Double? {
        guard let bar = keys.space, bar.width > 0 else { return nil }
        let slack = keys.pitch.width / 2
        guard tap.x >= bar.minX - slack, tap.x <= bar.maxX + slack else { return nil }
        let sy = Double(keys.pitch.height) * parameters.sigmaY
        let toBar = (Double(tap.y) - Double(bar.midY)) / sy
        let toRow = (Double(tap.y) - Double(keys.bottomRowY)) / sy
        return max(0, 0.5 * (toRow * toRow - toBar * toBar))
    }

    /// One word where a space was tapped for a letter on the row above ("deli ate" for
    /// "delicate", "a out" for "about"): the likeliest reading of the first word's taps, the
    /// space tap as a letter and the second word's taps, when it beats the two words as they
    /// read by the margin and the tap was near enough the row (`mergeReach`). Nil to keep them.
    func merge(
        first: String, firstTaps: [CGPoint], space: CGPoint, second: String, secondTaps: [CGPoint],
        previous: String?, firstIsValid: Bool, secondIsValid: Bool, keys: Keys
    ) -> String? {
        guard firstTaps.count == first.count, secondTaps.count == second.count, !second.isEmpty,
              let firstCodes = model.encode(first.lowercased()), let secondCodes = model.encode(second.lowercased()),
              let reach = rowCost(of: space, keys: keys), reach <= parameters.mergeReach else { return nil }
        let taps = firstTaps + [space] + secondTaps
        let centres = codeCentres(keys)
        let costs = taps.map { tapCosts($0, centres: centres, keys: keys) }
        let prev = previous.flatMap { model.index(of: $0) }
        // The two words as space would leave them: each as typed or its likeliest reading
        // ("i dkdnt" is "I didn't", never "obedient").
        func reading(_ codes: [UInt8], at range: Range<Int>, after before: Int?, valid: Bool) -> (cost: Double, word: Int?) {
            let typed = model.index(ofCodes: codes)
            var spatial = 0.0
            for (i, code) in codes.enumerated() { spatial += costs[range.lowerBound + i][Int(code)] }
            var best = (cost: spatial + parameters.lmWeight * (typed.map { lmCost($0, after: before) }
                ?? (valid ? parameters.unknownValidCost : parameters.unknownCost)), word: typed)
            for (word, s) in search(model, costs, range: range, limit: 3, maxEdits: parameters.maxEdits, skipping: neverSuggest.words) {
                let cost = s + parameters.lmWeight * lmCost(word, after: before)
                if cost < best.cost { best = (cost, word) }
            }
            return best
        }
        let firstRange = 0..<firstCodes.count
        let firstReading = reading(firstCodes, at: firstRange, after: prev, valid: firstIsValid)
        let secondReading = reading(secondCodes, at: (firstCodes.count + 1)..<taps.count, after: firstReading.word, valid: secondIsValid)
        let split = firstReading.cost + secondReading.cost
        guard split.isFinite else { return nil }
        var best: (word: Int, cost: Double)?
        for (word, spatial) in search(model, costs, range: 0..<costs.count, limit: 3, maxEdits: parameters.maxEdits, skipping: neverSuggest.words)
        where model.length(word) > max(first.count, second.count) && !neverSuggest.follows(word, after: prev) {
            let cost = spatial + parameters.lmWeight * lmCost(word, after: prev) + reach
            if cost < best?.cost ?? .infinity { best = (word, cost) }
        }
        guard let best else { return nil }
        let margin = firstIsValid && secondIsValid ? parameters.validMargin : parameters.autoMargin
        return split - best.cost >= margin ? model.word(best.word) : nil
    }

    /// A word that may stand alone in a split: two or more letters, or a common one-letter
    /// word ("a" and "i" in English; "y", "a", "o" in Spanish).
    private func canStandAlone(_ i: Int) -> Bool {
        model.length(i) >= 2 || model.unigramCost(i) < parameters.oneLetterWordCost
    }
}
