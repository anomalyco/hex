import CoreGraphics
import Foundation
import HexKeyboardCore

/// The typing model: a word list and bigram language model in the file
/// `scripts/build-typing-model.py` writes, read by the keyboard's Rust core
/// (`crates/keyboard`, `hk_model_*`), which maps it read-only. Mapped pages are clean and
/// file-backed, so iOS can drop them under pressure instead of counting them against the
/// keyboard. Every lookup is the core's; this class only carries its answers into Swift.
nonisolated final class TypingDecoderModel: @unchecked Sendable {
    let wordCount: Int
    /// The language it was built for ("en", "es").
    let language: String
    /// Letter codes (1…) by character, each letter's character, and its unaccented letter's code.
    let codes: [Character: UInt8]
    let baseCodes: [UInt8]
    private let characters: [Character]
    /// The file it maps.
    let url: URL
    private let coreModel: KeyboardCoreModel

    /// The Rust core's model, for the other wrappers of its interface.
    var core: KeyboardCoreModel? { coreModel }
    var pointer: OpaquePointer { coreModel.pointer }

    init?(url: URL) {
        guard let core = KeyboardCoreModel(url: url) else { return nil }
        var info = HkModelInfo()
        guard hk_model_info(core.pointer, &info) == 0, info.code_count > 0 else { return nil }
        var scalars = [UInt32](repeating: 0, count: info.code_count)
        var bases = [UInt8](repeating: 0, count: info.code_count)
        guard hk_model_alphabet(core.pointer, &scalars, &bases, info.code_count) == 0 else { return nil }
        var codes: [Character: UInt8] = [:]
        var characters: [Character] = []
        for (code, scalar) in scalars.enumerated() {
            let character = Unicode.Scalar(scalar).map(Character.init) ?? " "
            characters.append(character)
            if code > 0 { codes[character] = UInt8(code) }
        }
        self.coreModel = core
        self.url = url
        self.wordCount = info.word_count
        self.language = withUnsafeBytes(of: info.language) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
        self.codes = codes
        self.characters = characters
        self.baseCodes = bases
    }

    /// The word's letter codes, read in place from the mapped file.
    private func withCodes<R>(_ i: Int, _ body: (UnsafeRawBufferPointer) -> R) -> R {
        var length = 0
        let codes = i >= 0 ? hk_model_word_codes(pointer, i, &length) : nil
        return body(UnsafeRawBufferPointer(start: codes, count: codes == nil ? 0 : length))
    }

    func word(_ i: Int) -> String {
        withCodes(i) { String($0.map { characters[Int($0)] }) }
    }

    /// The word's letter codes, or nil when it has a letter the model doesn't.
    func encode(_ word: String) -> [UInt8]? {
        let capacity = word.utf8.count
        var out = [UInt8](repeating: 0, count: max(1, capacity))
        let count = hk_model_encode(pointer, word, &out, capacity)
        guard count >= 0 else { return nil }
        return Array(out.prefix(Int(count)))
    }

    /// -ln P(word), in nats.
    @inline(__always) func unigramCost(_ i: Int) -> Double {
        hk_model_unigram_cost(pointer, i)
    }

    func index(of word: String) -> Int? {
        let i = hk_model_index(pointer, word)
        return i >= 0 ? Int(i) : nil
    }

    /// Whether the model has `second` following `first` as a pair.
    func hasPair(_ first: String, _ second: String) -> Bool {
        hk_model_has_pair(pointer, first, second) == 1
    }

    /// -ln P(word | previous) with the decoder's backoff to the unigram (0.92 nats), or nil
    /// when the model lacks the word.
    func cost(of word: String, after previous: String?) -> Double? {
        var cost = 0.0
        return hk_model_cost(pointer, word, previous, &cost) == 1 ? cost : nil
    }

    /// The three most probable allowed continuations of a completed word. The core scans just
    /// its bigram row and asks `allowing` only about words that would make the three.
    func nextWords(after previous: String, allowing: (String) -> Bool) -> [String] {
        offered(allowing, capacity: 3) { allow, context, out in
            hk_model_next_words(pointer, previous, allow, context, out, 3)
        }
    }

    /// Complete a short prefix with the word frequencies, backed by the previous word's
    /// bigrams when available; `allowing` is asked only about contenders.
    func completions(of prefix: String, after previous: String?, allowing: (String) -> Bool) -> [String] {
        offered(allowing, capacity: 2) { allow, context, out in
            hk_model_completions(pointer, prefix, previous, allow, context, out, 2)
        }
    }

    /// How much of the word list's frequency continues `prefix` with each letter, by letter
    /// code. Nil when no word starts with `prefix`, or one ends there and none goes on.
    func continuations(after prefix: [UInt8]) -> [Double]? {
        var mass = [Double](repeating: 0, count: characters.count)
        return hk_model_continuations(pointer, prefix, prefix.count, &mass, mass.count) == 1 ? mass : nil
    }

    /// Runs a prediction with `allowing` as the core's filter (`HkAllowWord`): the core asks
    /// about word indices, the closure about their text.
    private func offered(
        _ allowing: (String) -> Bool, capacity: Int,
        _ call: (HkAllowWord, UnsafeMutableRawPointer, UnsafeMutablePointer<UInt32>) -> Int
    ) -> [String] {
        withoutActuallyEscaping(allowing) { allowing in
            var filter = Filter(model: self, allowing: allowing)
            var out = [UInt32](repeating: 0, count: capacity)
            let count = withUnsafeMutablePointer(to: &filter) { context in
                out.withUnsafeMutableBufferPointer { call(Self.allow, UnsafeMutableRawPointer(context), $0.baseAddress!) }
            }
            return out.prefix(count).map { word(Int($0)) }
        }
    }

    private struct Filter {
        let model: TypingDecoderModel
        let allowing: (String) -> Bool
    }

    private static let allow: HkAllowWord = { context, word in
        guard let filter = context?.assumingMemoryBound(to: Filter.self).pointee else { return 1 }
        return filter.allowing(filter.model.word(Int(word))) ? 1 : 0
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

extension TypingDecoder {
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
        /// letter) moves a centre to where you tend to tap it. A split layout's halves are
        /// pushed together, so a letter on both halves (G, V) has one centre, where the
        /// unsplit keyboard has it (`hk_layout_tap_keys`).
        init?(layout: KeyboardLayout, offsets: [Character: CGVector] = [:]) {
            guard layout.layer == .letters else { return nil }
            let m = layout.metrics
            let pitch = CGSize(
                width: layout.isSplit ? SplitGeometry.pitch : layout.letterWidth + m.gap,
                height: m.keyHeight + m.rowGap
            )
            let shifts = offsets.compactMap { letter, shift in
                Self.scalar(of: letter).map { HkKeyOffset(letter: $0, dx: Double(shift.dx), dy: Double(shift.dy)) }
            }
            let split = layout.split.map { [Double($0.middle), Double($0.shift)] } ?? []
            var centres = [HkKeyCentre](repeating: HkKeyCentre(), count: max(1, layout.keys.count))
            var made = HkTapKeys()
            let status = TypingDecoder.withLayoutKeys(layout) { keys in
                split.withUnsafeBufferPointer { split in
                    centres.withUnsafeMutableBufferPointer { buffer in
                        hk_layout_tap_keys(
                            keys.baseAddress, keys.count, split.isEmpty ? nil : split.baseAddress,
                            Double(pitch.width), Double(pitch.height), shifts, shifts.count,
                            buffer.baseAddress, buffer.count, &made
                        )
                    }
                }
            }
            guard status == 0 else { return nil }
            var byLetter: [Character: CGPoint] = [:]
            for centre in centres.prefix(made.keys.centre_count) {
                guard let scalar = Unicode.Scalar(centre.letter) else { continue }
                byLetter[Character(scalar)] = CGPoint(x: centre.x, y: centre.y)
            }
            let space = made.space
            self.init(
                centres: byLetter,
                pitch: pitch,
                bottomRowY: CGFloat(made.bottom_row_y),
                space: made.has_space != 0 ? CGRect(x: space.x, y: space.y, width: space.width, height: space.height) : nil
            )
        }

        /// The keys as the core reads them, for the length of `body`.
        func withCore<R>(_ body: (UnsafePointer<HkTapKeys>) -> R) -> R {
            let letters = centres.compactMap { letter, point in
                Self.scalar(of: letter).map { HkKeyCentre(letter: $0, x: Double(point.x), y: Double(point.y)) }
            }
            return letters.withUnsafeBufferPointer { buffer in
                let bar = space ?? .zero
                var keys = HkTapKeys(
                    keys: HkKeys(
                        centres: buffer.baseAddress, centre_count: buffer.count,
                        pitch_width: Double(pitch.width), pitch_height: Double(pitch.height)
                    ),
                    bottom_row_y: Double(bottomRowY),
                    has_space: space == nil ? 0 : 1,
                    space: HkRect(x: Double(bar.minX), y: Double(bar.minY), width: Double(bar.width), height: Double(bar.height))
                )
                return body(&keys)
            }
        }

        /// A letter's one Unicode scalar, composed; nil for a character that isn't one (no
        /// model letter is).
        static func scalar(of letter: Character) -> UInt32? {
            let scalars = letter.unicodeScalars.count == 1
                ? letter.unicodeScalars
                : String(letter).precomposedStringWithCanonicalMapping.unicodeScalars
            return scalars.count == 1 ? scalars.first?.value : nil
        }
    }

    /// Learn how I type's average tap offsets (in keycap widths and heights) as points, for
    /// letters with 20 taps or more, shrunk toward the centre while there are few
    /// (`hk_layout_offsets`).
    static func offsets(from stats: [String: TypingJournalKeyStats], layout: KeyboardLayout) -> [Character: CGVector] {
        let perKey = layout.keys.map { key -> HkKeyStats in
            guard let c = key.character, let s = stats[c] else { return HkKeyStats() }
            return HkKeyStats(present: 1, count: UInt64(max(0, s.count)), dx: s.dx, dy: s.dy)
        }
        var found = [HkKeyOffset](repeating: HkKeyOffset(), count: max(1, layout.keys.count))
        let count = withLayoutKeys(layout) { keys in
            hk_layout_offsets(keys.baseAddress, perKey, keys.count, &found, found.count)
        }
        var out: [Character: CGVector] = [:]
        for offset in found.prefix(count) {
            guard let scalar = Unicode.Scalar(offset.letter) else { continue }
            out[Character(scalar)] = CGVector(dx: offset.dx, dy: offset.dy)
        }
        return out
    }

    /// `layout`'s keys as the core reads them, for the length of `body`.
    fileprivate static func withLayoutKeys<R>(_ layout: KeyboardLayout, _ body: (UnsafeBufferPointer<HkLayoutKey>) -> R) -> R {
        let characters = layout.keys.map { $0.character.flatMap { strdup($0) } }
        defer { characters.forEach { free($0) } }
        let keys = zip(layout.keys, characters).map { key, character in
            let f = key.frame
            return HkLayoutKey(
                character: character.map { UnsafePointer($0) },
                space: key.kind == .space ? 1 : 0,
                frame: HkRect(x: Double(f.origin.x), y: Double(f.origin.y), width: Double(f.size.width), height: Double(f.size.height))
            )
        }
        return keys.withUnsafeBufferPointer(body)
    }
}
