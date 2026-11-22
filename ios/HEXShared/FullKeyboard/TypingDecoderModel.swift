import Foundation
import HexKeyboardCore

/// The typing model: a word list and bigram language model in the file
/// `scripts/build-typing-model.py` writes, read by the keyboard's Rust core
/// (`crates/keyboard`, `hk_model_*`), which maps it read-only. Mapped pages are clean and
/// file-backed, so iOS can drop them under pressure instead of counting them against the
/// keyboard. Every lookup is the core's; this class only carries its answers into Swift.
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
        self.bigramCount = info.pair_count
        self.byteCount = info.byte_count
        self.language = withUnsafeBytes(of: info.language) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
        self.codes = codes
        self.characters = characters
        self.baseCodes = bases
        self.apostropheCode = codes["'"]
    }

    /// The word's letter codes, read in place from the mapped file (Swipe typing reads
    /// thousands a swipe).
    @inline(__always) func withCodes<R>(_ i: Int, _ body: (UnsafeRawBufferPointer) -> R) -> R {
        var length = 0
        let codes = i >= 0 ? hk_model_word_codes(pointer, i, &length) : nil
        return body(UnsafeRawBufferPointer(start: codes, count: codes == nil ? 0 : length))
    }

    @inline(__always) func length(_ i: Int) -> Int { withCodes(i) { $0.count } }

    /// The word's byte at `depth`, or -1 past its end.
    @inline(__always) func byte(_ i: Int, _ depth: Int) -> Int {
        withCodes(i) { depth < $0.count ? Int($0[depth]) : -1 }
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

    /// -ln P(b | a), in nats, when the pair is in the model.
    func bigramCost(_ a: Int, _ b: Int) -> Double? {
        guard a >= 0, b >= 0 else { return nil }
        var cost = 0.0
        return hk_model_bigram_cost(pointer, a, b, &cost) == 1 ? cost : nil
    }

    func index(of word: String) -> Int? {
        let i = hk_model_index(pointer, word)
        return i >= 0 ? Int(i) : nil
    }

    func index(ofCodes bytes: [UInt8]) -> Int? {
        let i = hk_model_index_of_codes(pointer, bytes, bytes.count)
        return i >= 0 ? Int(i) : nil
    }

    /// The words in `lo..<hi` (which share their first `depth` codes) whose next code is `byte`.
    func narrow(_ lo: Int, _ hi: Int, depth: Int, byte target: Int) -> (Int, Int) {
        guard lo >= 0, hi >= lo, depth >= 0 else { return (lo, lo) }
        var a = 0, b = 0
        hk_model_narrow(pointer, lo, hi, depth, Int32(clamping: target), &a, &b)
        return (a, b)
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
