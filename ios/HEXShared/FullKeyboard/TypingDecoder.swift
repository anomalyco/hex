import CoreGraphics
import Foundation
import HexKeyboardCore

extension TypingDecoder {
    /// Your own words for the decoder (Contacts names and other `UILexicon` terms, text
    /// replacements, words you kept, fixes Hex learned), as you spell them, each scored as a
    /// moderately common word. The core leaves out words with a letter the model lacks, and
    /// of two with the same letters keeps the first.
    struct UserWords: Sendable {
        private(set) var words: [String] = []

        init() {}

        init(_ words: [String], model: TypingDecoderModel) {
            self.words = words
        }

        var isEmpty: Bool { words.isEmpty }
    }
}

/// Experiment (Spatial autocorrect): a noisy-channel word decoder. Each tap is scored against
/// every letter with a 2D Gaussian around the key's centre (shifted by where you tend to tap,
/// when known), words are searched in the model's sorted word list with a beam, and a bigram
/// language model on the word before weighs them. A missed space is tried by splitting the
/// taps in two. What you typed is replaced only when the best reading beats it by a margin.
///
/// The decoder is the keyboard's Rust core (`crates/keyboard/core/src/decoder`, `hk_tap_*`),
/// which documents and tunes it; this struct carries its settings and answers into Swift.
nonisolated struct TypingDecoder {
    /// `hk_tap_default_parameters`, field for field. Tune them in the core.
    struct Parameters: Equatable, Sendable {
        /// Touch spread as a share of the key pitch (key width plus gap; row height plus gap).
        var sigmaX: Double
        var sigmaY: Double
        var lmWeight: Double
        var deletionCost: Double
        var insertionCost: Double
        var transpositionCost: Double
        var spaceOmissionCost: Double
        /// A tap on the bottom row's upper half read as a space the finger missed ("ofbthe").
        var spaceTapCost: Double
        var spaceTapWeight: Double
        /// How much less likely than the space bar the row above may be for a space tap read
        /// as one of its letters ("deli ate" for "delicate").
        var mergeReach: Double
        var maxEdits: Int
        var beam: Int
        /// Nats the best reading must beat what you typed by to replace it on space.
        var autoMargin: Double
        /// The same when what you typed is itself a word.
        var validMargin: Double
        var unknownValidCost: Double
        var unknownCost: Double
        var maxTapCost: Double
        var splits: Bool
        var accentCost: Double
        var oneLetterWordCost: Double
        var userWordCost: Double
        var accentMargin: Double

        init() {
            var p = HkTapParameters()
            hk_tap_default_parameters(&p)
            self.init(p)
        }

        /// The settings tuned for a language's model on the benchmark's dev sentences.
        static func tuned(for language: String) -> Parameters {
            var p = HkTapParameters()
            hk_tap_tuned_parameters(language, &p)
            return Parameters(p)
        }

        init(_ p: HkTapParameters) {
            sigmaX = p.sigma_x
            sigmaY = p.sigma_y
            lmWeight = p.lm_weight
            deletionCost = p.deletion_cost
            insertionCost = p.insertion_cost
            transpositionCost = p.transposition_cost
            spaceOmissionCost = p.space_omission_cost
            spaceTapCost = p.space_tap_cost
            spaceTapWeight = p.space_tap_weight
            mergeReach = p.merge_reach
            maxEdits = Int(p.max_edits)
            beam = Int(p.beam)
            autoMargin = p.auto_margin
            validMargin = p.valid_margin
            unknownValidCost = p.unknown_valid_cost
            unknownCost = p.unknown_cost
            maxTapCost = p.max_tap_cost
            splits = p.splits != 0
            accentCost = p.accent_cost
            oneLetterWordCost = p.one_letter_word_cost
            userWordCost = p.user_word_cost
            accentMargin = p.accent_margin
        }

        var core: HkTapParameters {
            HkTapParameters(
                sigma_x: sigmaX, sigma_y: sigmaY, lm_weight: lmWeight, deletion_cost: deletionCost,
                insertion_cost: insertionCost, transposition_cost: transpositionCost,
                space_omission_cost: spaceOmissionCost, space_tap_cost: spaceTapCost,
                space_tap_weight: spaceTapWeight, merge_reach: mergeReach, max_edits: max(0, maxEdits),
                beam: max(0, beam), auto_margin: autoMargin, valid_margin: validMargin,
                unknown_valid_cost: unknownValidCost, unknown_cost: unknownCost, max_tap_cost: maxTapCost,
                splits: splits ? 1 : 0, accent_cost: accentCost, one_letter_word_cost: oneLetterWordCost,
                user_word_cost: userWordCost, accent_margin: accentMargin
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
        /// Readings for the suggestion strip, best first: better than what you typed, or a
        /// missed-space fallback that costs more than the flat cost of a word nothing knows.
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
    var parameters = Parameters() { didSet { core = Core() } }
    /// Your own words, searched beside the model's.
    var userWords = UserWords() { didSet { core = Core() } }
    /// Words never offered; set by `AutocorrectEngine` for the language and Suggest swear words.
    var neverSuggest = NeverSuggest() { didSet { core = Core() } }
    /// The core's decoder for these settings, made on first use; a change makes a new one.
    private var core = Core()

    init(model: TypingDecoderModel, parameters: Parameters? = nil) {
        self.model = model
        self.parameters = parameters ?? Parameters.tuned(for: model.language)
    }

    /// `typed` is the lowercase letters the keys resolved, one per tap. Nil when the model
    /// can't read it (a letter it lacks, or taps that don't match).
    func decide(taps: [CGPoint], typed: String, previous: String?, literalIsValid: Bool, keys: Keys) -> Decision? {
        guard let decoder = handle() else { return nil }
        let points = Self.flat(taps)
        var out = HkTapDecision()
        defer { hk_tap_decision_free(&out) }
        let status = keys.withCore { keys in
            hk_tap_decide(decoder, points, taps.count, typed, previous, literalIsValid ? 1 : 0, keys, &out)
        }
        guard status == 0, out.decided != 0 else { return nil }
        let readings = (0..<out.reading_count).map { i in
            Reading(text: String(cString: out.readings[i].text), cost: out.readings[i].cost)
        }
        return Decision(replacement: out.replacement.map { String(cString: $0) }, readings: readings, literalCost: out.literal_cost)
    }

    /// One word where a space was tapped for a letter on the row above ("deli ate" for
    /// "delicate", "a out" for "about"), when it beats the two words as they read by the
    /// margin and the tap was near enough the row (`mergeReach`). Nil to keep them.
    func merge(
        first: String, firstTaps: [CGPoint], space: CGPoint, second: String, secondTaps: [CGPoint],
        previous: String?, firstIsValid: Bool, secondIsValid: Bool, keys: Keys
    ) -> String? {
        guard let decoder = handle() else { return nil }
        let (a, b) = (Self.flat(firstTaps), Self.flat(secondTaps))
        var out: UnsafeMutablePointer<CChar>?
        defer { hk_string_free(out) }
        let status = keys.withCore { keys in
            hk_tap_merge(
                decoder, first, a, firstTaps.count, Double(space.x), Double(space.y), second, b, secondTaps.count,
                previous, firstIsValid ? 1 : 0, secondIsValid ? 1 : 0, keys, &out
            )
        }
        guard status == 0, let out else { return nil }
        return String(cString: out)
    }

    /// The text with acute, grave, circumflex and diaeresis marks removed (é -> e, ü -> u),
    /// keeping ñ.
    static func withoutAccents(_ text: String) -> String {
        let marks: ClosedRange<UInt32> = 0x300...0x308
        let scalars = text.decomposedStringWithCanonicalMapping.unicodeScalars
            .filter { !(marks.contains($0.value) && $0.value != 0x303) }
        return String(String.UnicodeScalarView(scalars)).precomposedStringWithCanonicalMapping
    }

    private static func flat(_ taps: [CGPoint]) -> [Double] {
        taps.flatMap { [Double($0.x), Double($0.y)] }
    }

    /// The core's decoder, made from these settings on first use.
    private func handle() -> OpaquePointer? {
        core.handle { [model, parameters, userWords, neverSuggest] in
            var p = parameters.core
            let never = Array(neverSuggest.words).map(UInt32.init)
            let pairs = neverSuggest.pairs.flatMap { [UInt32($0.first), UInt32($0.second)] }
            let words = userWords.words.map { strdup($0) }
            defer { words.forEach { free($0) } }
            let pointers = words.map { UnsafePointer($0) }
            return hk_tap_new(model.pointer, &p, pointers, pointers.count, never, never.count, pairs, pairs.count / 2)
        }
    }

    /// The core's decoder for one set of settings, shared by copies of the decoder until one
    /// of them changes a setting.
    private final class Core: @unchecked Sendable {
        private let lock = NSLock()
        private var made = false
        private var decoder: OpaquePointer?

        func handle(_ make: () -> OpaquePointer?) -> OpaquePointer? {
            lock.withLock {
                if !made {
                    decoder = make()
                    made = true
                }
                return decoder
            }
        }

        deinit {
            hk_tap_free(decoder)
        }
    }
}

extension TypingDecoder {
    /// Whether `decide` with `keys` would put `reading` (one word) in place of what you typed:
    /// it beats the literal by the margin. Nil for a two-word reading, which only `decide` can
    /// tell. Adaptive touch asks with the centres read without it, so a correction it applies
    /// is one the keyboard would apply anyway.
    func replaces(with reading: String, taps: [CGPoint], typed: String, previous: String?, literalIsValid: Bool, keys: Keys) -> Bool? {
        guard !reading.contains(" ") else { return nil }
        guard let decoder = handle() else { return false }
        let points = Self.flat(taps)
        var answer: Int32 = 0
        let status = keys.withCore { keys in
            hk_tap_replaces(decoder, reading, points, taps.count, typed, previous, literalIsValid ? 1 : 0, keys, &answer)
        }
        guard status == 0 else { return false }
        return answer < 0 ? nil : answer == 1
    }
}
