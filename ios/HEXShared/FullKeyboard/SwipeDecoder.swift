import CoreGraphics
import Foundation
import HexKeyboardCore

/// Experiment (Swipe typing): reads a finger's path across the letter keys as a word.
///
/// The decoder is the keyboard's Rust core (`crates/keyboard/core/src/swipe`, through
/// `hk_swipe_*`), which documents the method and its parameters: tune swipe there. This is its
/// Swift face, unchanged from when the decoder was Swift: what a swipe reads, the words never
/// read and the words read beside the model's. The core's decoder can't change once made, so
/// one is made for each set of them, on the first read after a change, and kept with this
/// value (copies share it until one changes), so readings on the preview queue and the main
/// thread use it at once.
nonisolated struct SwipeDecoder: Sendable {
    /// The core's `swipe::Parameters`, field for field (`HkSwipeParameters`), with its defaults.
    struct Parameters: Equatable, Sendable {
        var samples: Int
        var startRadius: Double
        var endRadius: Double
        var maxStartKeys: Int
        var maxEndKeys: Int
        var nearRadius: Double
        var minLengthRatio: Double
        var maxLengthRatio: Double
        var startSigma: Double
        var endSigma: Double
        var startStraySigma: Double
        var endStraySigma: Double
        var stray: Double
        var locationWeight: Double
        var shapeWeight: Double
        var lmWeight: Double
        var shortlist: Int
        var prefilter: Int
        var band: Double
        var minimumLength: Double
        var contractionMissingPair: Double
        var evidenceStep: Double
        var turnWindow: Double
        var turnFrom: Double
        var turnFull: Double
        var dwellWindow: Double
        var dwellFrom: Double
        var dwellFull: Double
        var evidenceFloor: Double
        var evidenceSpacing: Double
        var evidenceGuard: Double
        var evidenceSigma: Double
        var evidenceReach: Double
        var unexplainedCost: Double
        var straightKeyCost: Double
        var cornerKeyCost: Double
        var straightAngle: Double

        /// The core's defaults (`hk_swipe_default_parameters`).
        init() {
            var c = HkSwipeParameters()
            hk_swipe_default_parameters(&c)
            self.init(c)
        }

        init(_ c: HkSwipeParameters) {
            samples = c.samples
            startRadius = c.start_radius
            endRadius = c.end_radius
            maxStartKeys = c.max_start_keys
            maxEndKeys = c.max_end_keys
            nearRadius = c.near_radius
            minLengthRatio = c.min_length_ratio
            maxLengthRatio = c.max_length_ratio
            startSigma = c.start_sigma
            endSigma = c.end_sigma
            startStraySigma = c.start_stray_sigma
            endStraySigma = c.end_stray_sigma
            stray = c.stray
            locationWeight = c.location_weight
            shapeWeight = c.shape_weight
            lmWeight = c.lm_weight
            shortlist = c.shortlist
            prefilter = c.prefilter
            band = c.band
            minimumLength = c.minimum_length
            contractionMissingPair = c.contraction_missing_pair
            evidenceStep = c.evidence_step
            turnWindow = c.turn_window
            turnFrom = c.turn_from
            turnFull = c.turn_full
            dwellWindow = c.dwell_window
            dwellFrom = c.dwell_from
            dwellFull = c.dwell_full
            evidenceFloor = c.evidence_floor
            evidenceSpacing = c.evidence_spacing
            evidenceGuard = c.evidence_guard
            evidenceSigma = c.evidence_sigma
            evidenceReach = c.evidence_reach
            unexplainedCost = c.unexplained_cost
            straightKeyCost = c.straight_key_cost
            cornerKeyCost = c.corner_key_cost
            straightAngle = c.straight_angle
        }

        var core: HkSwipeParameters {
            var c = HkSwipeParameters()
            c.samples = samples
            c.start_radius = startRadius
            c.end_radius = endRadius
            c.max_start_keys = maxStartKeys
            c.max_end_keys = maxEndKeys
            c.near_radius = nearRadius
            c.min_length_ratio = minLengthRatio
            c.max_length_ratio = maxLengthRatio
            c.start_sigma = startSigma
            c.end_sigma = endSigma
            c.start_stray_sigma = startStraySigma
            c.end_stray_sigma = endStraySigma
            c.stray = stray
            c.location_weight = locationWeight
            c.shape_weight = shapeWeight
            c.lm_weight = lmWeight
            c.shortlist = shortlist
            c.prefilter = prefilter
            c.band = band
            c.minimum_length = minimumLength
            c.contraction_missing_pair = contractionMissingPair
            c.evidence_step = evidenceStep
            c.turn_window = turnWindow
            c.turn_from = turnFrom
            c.turn_full = turnFull
            c.dwell_window = dwellWindow
            c.dwell_from = dwellFrom
            c.dwell_full = dwellFull
            c.evidence_floor = evidenceFloor
            c.evidence_spacing = evidenceSpacing
            c.evidence_guard = evidenceGuard
            c.evidence_sigma = evidenceSigma
            c.evidence_reach = evidenceReach
            c.unexplained_cost = unexplainedCost
            c.straight_key_cost = straightKeyCost
            c.corner_key_cost = cornerKeyCost
            c.straight_angle = straightAngle
            return c
        }
    }

    /// The cost (-ln P) of a common English contraction ("isn't", "we're"), about a word ranked
    /// 1,500th: Google Books splits contractions ("do n't"), so the model's counts for them
    /// are far too low. The core's (`hk_swipe_contraction_cost`).
    static let contractionCost = hk_swipe_contraction_cost()

    /// The cost of English contraction `word` by how often a chat uses it: the commonest ("I'm",
    /// "don't", "it's") like a word ranked about 300th, common ones at `base`, the rest ("he'll",
    /// "ma'am") about 10,000th. The core decides (`hk_swipe_cost_of_contraction`); a word with
    /// a NUL in it is none of them, which the core gives for no word.
    static func costOfContraction(_ word: String, base: Double = contractionCost) -> Double {
        if word.unicodeScalars.contains("\0") { return hk_swipe_cost_of_contraction(nil, base) }
        return word.withCString { hk_swipe_cost_of_contraction($0, base) }
    }

    /// The cost of one of your own words (as Spatial autocorrect's `userWordCost`): the core's
    /// (`hk_swipe_own_word_cost`).
    static let ownWordCost = hk_swipe_own_word_cost()

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
    /// "do n't", so its "don't" is as rare as "tom"), and your own words. Kept as given, in
    /// order; the core leaves out a word of fewer than two letters or with a letter the model
    /// lacks, and a repeat (by its letters) after the first, and uses the model's own cost when
    /// it's lower.
    struct ExtraWords: Sendable {
        struct Word: Equatable, Sendable {
            var text: String
            var cost: Double
            /// Whether its first word ("i" for "I'm", "do" for "don't") weighs it after the
            /// word before.
            var contraction: Bool
        }

        private(set) var words: [Word] = []

        init() {}

        /// `words` as you'd see them ("I'm", "don't"), each with `cost`. `model` is the
        /// decoder's, which the core reads them against.
        init(_ words: [String], cost: Double, model: TypingDecoderModel) {
            add(words, cost: cost, model: model)
        }

        init(_ words: [String], cost: (String) -> Double, model: TypingDecoderModel) {
            add(words, cost: cost, model: model)
        }

        mutating func add(_ words: [String], cost: Double, model: TypingDecoderModel) {
            add(words, cost: { _ in cost }, model: model)
        }

        mutating func add(_ words: [String], cost: (String) -> Double, model: TypingDecoderModel, contractions: Bool = false) {
            self.words += words.map { Word(text: $0, cost: cost($0), contraction: contractions) }
        }
    }

    let model: TypingDecoderModel
    var parameters: Parameters {
        didSet { core = Core() }
    }
    /// Words and two-word expressions never offered (`OffensiveWords`), as for Spatial
    /// autocorrect: never read from a swipe.
    var neverSuggest = TypingDecoder.NeverSuggest() {
        didSet { core = Core() }
    }
    var extraWords = ExtraWords() {
        didSet { core = Core() }
    }
    /// The core's decoder for these settings, made on the first read.
    private var core = Core()

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
        let none = Reading(candidates: [], scored: 0)
        // Without the core's model (a file it can't open) nothing can be read, as for a path
        // that reads as no word.
        guard limit > 0, let decoder = core.decoder(self) else { return none }
        var points: [Double] = []
        points.reserveCapacity(2 * path.count)
        for point in path {
            points.append(Double(point.x))
            points.append(Double(point.y))
        }
        // Times count only one per point, as the Swift decoder took them.
        let times = times?.count == path.count ? times : nil
        var centres: [HkKeyCentre] = []
        var others: [(Character, CGPoint)] = []
        for (character, point) in keys.centres {
            // A letter key is one character, which the core takes as one Unicode scalar: its
            // composed form, which a Swift `Character` equals.
            let scalars = String(character).precomposedStringWithCanonicalMapping.unicodeScalars
            guard scalars.count == 1, let scalar = scalars.first else {
                others.append((character, point))
                continue
            }
            centres.append(HkKeyCentre(letter: scalar.value, x: Double(point.x), y: Double(point.y)))
        }
        // A key of more than one scalar (none of Hex's layouts has one) still takes its place
        // among the keys, as it did in Swift, but types no model letter: a private-use scalar
        // of its own (plane 15, which no model has), in the keys' order among themselves.
        // These sort after the one-scalar keys, where Swift sorted them by their characters.
        for (i, (_, point)) in others.sorted(by: { $0.0 < $1.0 }).enumerated() {
            centres.append(HkKeyCentre(letter: 0xF0000 + UInt32(i), x: Double(point.x), y: Double(point.y)))
        }
        var reading = HkSwipeReading()
        let status = centres.withUnsafeBufferPointer { centres in
            var keys = HkKeys(
                centres: centres.baseAddress, centre_count: centres.count,
                pitch_width: Double(keys.pitch.width), pitch_height: Double(keys.pitch.height)
            )
            // A word with a NUL in it is in no model, as no word before.
            return withOptionalBuffer(times) { times in
                withOptionalCString(previous?.unicodeScalars.contains("\0") == true ? nil : previous) { previous in
                    hk_swipe_read(decoder, points, path.count, times, &keys, previous, limit, &reading)
                }
            }
        }
        defer { hk_swipe_reading_free(&reading) }
        guard status == 0 else { return none }
        let candidates = (0..<reading.candidate_count).map { i in
            let candidate = reading.candidates[i]
            return Candidate(text: String(cString: candidate.text), cost: candidate.cost)
        }
        return Reading(candidates: candidates, scored: reading.scored)
    }

    /// The core's decoder, made once from the decoder's settings when first read (on any
    /// thread), and freed with the last copy that has it.
    private final class Core: @unchecked Sendable {
        private let lock = NSLock()
        private var made = false
        private var pointer: OpaquePointer?

        func decoder(_ settings: SwipeDecoder) -> OpaquePointer? {
            lock.withLock {
                if !made {
                    made = true
                    pointer = Self.make(settings)
                }
                return pointer
            }
        }

        deinit {
            if let pointer { hk_swipe_free(pointer) }
        }

        private static func make(_ settings: SwipeDecoder) -> OpaquePointer? {
            guard let model = settings.model.core else { return nil }
            var parameters = settings.parameters.core
            let words = settings.neverSuggest.words.map { UInt32($0) }
            let pairs = settings.neverSuggest.pairs.flatMap { [UInt32($0.first), UInt32($0.second)] }
            // A word with a NUL in it has a letter no model has, so it's left out, as the core
            // leaves out the others.
            let extraWords = settings.extraWords.words.filter { !$0.text.unicodeScalars.contains("\0") }
            let texts = extraWords.map { strdup($0.text) }
            defer { texts.forEach { free($0) } }
            let extra = zip(extraWords, texts).map { word, text in
                HkExtraWord(text: text.map { UnsafePointer($0) }, cost: word.cost, contraction: word.contraction ? 1 : 0)
            }
            return hk_swipe_new(
                model.pointer, &parameters, words, words.count, pairs, pairs.count / 2, extra, extra.count
            )
        }
    }
}

// MARK: - What a swipe types

extension SwipeDecoder {
    /// Your text replacements and words as the core types a swipe with them
    /// (`hk_swipe_words_new`): made again whenever one of them changes.
    final class Words: @unchecked Sendable {
        let pointer: OpaquePointer

        /// `replacements` by shortcut (lowercased); `own` your other own spellings (lexicon
        /// terms, words you kept, fixes Hex learned); `vocabulary` your vocabulary's words.
        init?(language: String, hasDictionary: Bool, replacements: [String: String], own: [String], vocabulary: [String]) {
            let pairs = Array(replacements)
            let shortcuts = pairs.map { strdup($0.key) }, outputs = pairs.map { strdup($0.value) }
            let owned = own.map { strdup($0) }, terms = vocabulary.map { strdup($0) }
            defer { (shortcuts + outputs + owned + terms).forEach { free($0) } }
            let entries = zip(shortcuts, outputs).map { HkTextReplacement(shortcut: $0.map { UnsafePointer($0) }, replacement: $1.map { UnsafePointer($0) }) }
            let ownPointers = owned.map { $0.map { UnsafePointer($0) } }, termPointers = terms.map { $0.map { UnsafePointer($0) } }
            guard let pointer = hk_swipe_words_new(
                language, hasDictionary ? 1 : 0, entries, entries.count, ownPointers, ownPointers.count, termPointers, termPointers.count
            ) else { return nil }
            self.pointer = pointer
        }

        deinit {
            hk_swipe_words_free(pointer)
        }
    }

    /// The state a swipe is typed in: the text before the cursor, shift, the field's
    /// auto-capitalization (`.none` also with Settings › Auto-capitalization off), the setting,
    /// and whether the field allows autocorrection (text replacements show only then).
    struct TypingState: Equatable, Sendable {
        var before: String?
        var shift: ShiftState
        var shiftHeld: Bool
        var autoCapitalization: AutoCapitalization.Mode
        var autoCapitalizes: Bool
        var autocorrection: Bool
    }

    /// What a finished swipe types: a space first or not, the word, the strip's other readings.
    struct Typed: Equatable, Sendable {
        var space: Bool
        var word: String
        var alternatives: [String]
    }

    /// What a swipe whose readings are `readings` (best first, those that may be offered)
    /// types in `state` (`hk_swipe_typed`); nil when there are none.
    static func typed(_ readings: [String], in state: TypingState, words: Words, isWordValid: (String) -> Bool) -> Typed? {
        guard !readings.isEmpty else { return nil }
        let texts = readings.map { strdup($0) }
        defer { texts.forEach { free($0) } }
        let pointers = texts.map { $0.map { UnsafePointer($0) } }
        var typed = HkSwipeTyped()
        let status = withOptionalCString(state.before) { before in
            var context = HkSwipeContext(
                before: before,
                shift: state.shift == .locked ? UInt8(HK_SHIFT_LOCKED) : state.shift == .on ? UInt8(HK_SHIFT_ON) : UInt8(HK_SHIFT_OFF),
                shift_held: state.shiftHeld ? 1 : 0,
                auto_capitalization: state.autoCapitalization.core,
                auto_capitalizes: state.autoCapitalizes ? 1 : 0,
                autocorrection: state.autocorrection ? 1 : 0
            )
            return withWordCheck(isWordValid) { check, check_context in
                hk_swipe_typed(words.pointer, pointers, pointers.count, &context, check, check_context, &typed)
            }
        }
        defer { hk_swipe_typed_free(&typed) }
        guard status == 0 else { return nil }
        let alternatives = (0..<typed.alternative_count).map { String(cString: typed.alternatives[$0]!) }
        return Typed(space: typed.space != 0, word: String(cString: typed.word), alternatives: alternatives)
    }

    /// The order of `candidates` (a reading's, best first) once a rare name or abbreviation
    /// costs more (`hk_swipe_rank`); as they are without the core's model.
    func ranked(_ candidates: [Candidate], words: Words, isWordValid: (String) -> Bool) -> [Int] {
        guard let decoder = core.decoder(self), !candidates.isEmpty else { return Array(candidates.indices) }
        let texts = candidates.map { strdup($0.text) }
        defer { texts.forEach { free($0) } }
        let pointers = texts.map { $0.map { UnsafePointer($0) } }
        var order = [Int](repeating: 0, count: candidates.count)
        let status = withWordCheck(isWordValid) { check, context in
            hk_swipe_rank(decoder, words.pointer, pointers, candidates.map(\.cost), candidates.count, check, context, &order)
        }
        return status == 0 ? order : Array(candidates.indices)
    }

    /// Whether `word` (a reading, as the model spells it) is one the dictionary knows only
    /// with a capital: a name, day or place ("monday") (`hk_swipe_reads_as_name`).
    static func readsAsName(_ word: String, words: Words, isWordValid: (String) -> Bool) -> Bool {
        withWordCheck(isWordValid) { check, context in hk_swipe_reads_as_name(words.pointer, word, check, context) != 0 }
    }

    /// Whether a swiped word needs a space before it after `before` (`hk_swipe_needs_space`).
    static func needsSpace(before: String?) -> Bool {
        withOptionalCString(before) { hk_swipe_needs_space($0) != 0 }
    }

    /// Whether a swipe through `points` was a tap that slid (`hk_swipe_is_slid_tap`): it never
    /// went far from where it went down, it lifted on the key it went down on (`key`, that
    /// key's character), and its best reading doesn't start and end on that key.
    static func isSlidTap(_ points: [CGPoint], pitch: CGFloat, key: String?, firstReading: String?) -> Bool {
        let flat = points.flatMap { [Double($0.x), Double($0.y)] }
        return withOptionalCString(key) { key in
            withOptionalCString(firstReading) { first in hk_swipe_is_slid_tap(flat, points.count, Double(pitch), key, first) != 0 }
        }
    }

    /// Whether `word` begins and ends on `letter`'s key ("lol" on l) (`hk_swipe_starts_and_ends_with`).
    static func word(_ word: String, startsAndEndsWith letter: String) -> Bool {
        hk_swipe_starts_and_ends_with(word, letter) != 0
    }
}

private extension AutoCapitalization.Mode {
    var core: UInt8 {
        switch self {
        case .none: UInt8(HK_AUTOCAPITALIZATION_NONE)
        case .words: UInt8(HK_AUTOCAPITALIZATION_WORDS)
        case .sentences: UInt8(HK_AUTOCAPITALIZATION_SENTENCES)
        case .allCharacters: UInt8(HK_AUTOCAPITALIZATION_ALL_CHARACTERS)
        }
    }
}

/// `isWordValid` for the core to ask during `body`, as an `HkIsWordValid` and its context.
nonisolated private func withWordCheck<R>(_ isWordValid: (String) -> Bool, _ body: (HkIsWordValid, UnsafeMutableRawPointer) -> R) -> R {
    withoutActuallyEscaping(isWordValid) { isWordValid in
        let box = WordCheck(isWordValid)
        return withExtendedLifetime(box) {
            body({ context, word in
                guard let context, let word else { return 0 }
                return Unmanaged<WordCheck>.fromOpaque(context).takeUnretainedValue().isWordValid(String(cString: word)) ? 1 : 0
            }, Unmanaged.passUnretained(box).toOpaque())
        }
    }
}

nonisolated private final class WordCheck {
    let isWordValid: (String) -> Bool

    init(_ isWordValid: @escaping (String) -> Bool) {
        self.isWordValid = isWordValid
    }
}

/// `body` with `values`' storage, or NULL.
nonisolated private func withOptionalBuffer<R>(_ values: [Double]?, _ body: (UnsafePointer<Double>?) -> R) -> R {
    guard let values else { return body(nil) }
    return values.withUnsafeBufferPointer { body($0.baseAddress) }
}

/// `body` with `text` as a C string, or NULL.
nonisolated private func withOptionalCString<R>(_ text: String?, _ body: (UnsafePointer<CChar>?) -> R) -> R {
    guard let text else { return body(nil) }
    return text.withCString { body($0) }
}

extension KeyboardLayout {
    /// A swipe's point where the decoder's keys are (`TypingDecoder.Keys`): on the split
    /// keyboard the right half moves left against the left one (as `unsplitX`) and the middle
    /// between the halves shrinks to one key's gap, so a stroke across it stays one unbroken
    /// stroke, as Gboard joins the halves; elsewhere the point itself.
    func joinedForSwipe(_ point: CGPoint) -> CGPoint {
        guard let split else { return point }
        if point.x <= split.leftEdge { return point }
        if point.x >= split.rightEdge { return CGPoint(x: point.x - split.shift, y: point.y) }
        let across = (point.x - split.leftEdge) / max(1, split.rightEdge - split.leftEdge)
        return CGPoint(x: split.leftEdge + across * SplitGeometry.gap, y: point.y)
    }

    /// The distance between neighbouring letter keys' centres across: a split half's own pitch.
    var swipePitch: CGFloat { isSplit ? SplitGeometry.pitch : letterWidth + metrics.gap }
}
