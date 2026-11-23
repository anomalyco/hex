import CoreGraphics
import Foundation
import HexKeyboardCore

/// A typing model opened by the keyboard's Rust core (`crates/keyboard`, `hk_model_*`): its
/// own read-only map of the file. It never changes once open, so any thread may use it.
nonisolated final class KeyboardCoreModel: @unchecked Sendable {
    /// The core's handle, for the other wrappers of its interface.
    let pointer: OpaquePointer

    init?(url: URL) {
        guard let pointer = hk_model_open(url.path) else { return nil }
        self.pointer = pointer
    }

    deinit {
        hk_model_free(pointer)
    }

    var wordCount: Int { Int(hk_model_word_count(pointer)) }
}

/// Strings across the core's C interface: Swift's copied in for the length of a call, the
/// core's copied out and released. A string with a NUL in it ends at the NUL in C.
nonisolated enum KeyboardCoreText {
    /// `strings` as an array of C strings, for the length of `body`.
    static func with<R>(_ strings: [String], _ body: (UnsafePointer<UnsafePointer<CChar>?>?, Int) -> R) -> R {
        let copies = strings.map { strdup($0) }
        defer { copies.forEach { free($0) } }
        let pointers = copies.map { UnsafePointer($0) }
        return pointers.withUnsafeBufferPointer { body($0.baseAddress, strings.count) }
    }

    /// The core's string, released; nil for NULL.
    static func take(_ text: UnsafeMutablePointer<CChar>?) -> String? {
        guard let text else { return nil }
        defer { hk_string_free(text) }
        return String(cString: text)
    }

    /// Two of the core's strings, released; nil unless both are there.
    static func pair(_ a: UnsafeMutablePointer<CChar>?, _ b: UnsafeMutablePointer<CChar>?) -> (String, String)? {
        let (first, second) = (take(a), take(b))
        guard let first, let second else { return nil }
        return (first, second)
    }

    /// The core's list, released.
    static func take(_ list: inout HkStringList) -> [String] {
        defer { hk_string_list_free(&list) }
        return (0..<list.count).compactMap { i in list.items[i].map { String(cString: $0) } }
    }

    /// A string from text alone (`hk_text`).
    static func text(_ which: UInt8, _ a: String?, _ b: String?) -> String? {
        var out: UnsafeMutablePointer<CChar>?
        guard hk_text(which, a, b, &out) == 0 else { return nil }
        return take(out)
    }

    /// A language's words from its manifest (`hk_language_words`).
    static func languageWords(_ tag: String, _ which: UInt8) -> [String] {
        var out = HkStringList()
        guard hk_language_words(tag, which, &out) == 0 else { return [] }
        return take(&out)
    }

    /// What Learn how I type learned, as the core reads it, for the length of `body`.
    static func learning<R>(_ learning: TypingLearning, _ body: (UnsafePointer<HkLearning>) -> R) -> R {
        let typos = Array(learning.typos)
        return with(typos.map(\.key)) { typoList, count in
            with(typos.map(\.value)) { fixList, _ in
                with(Array(learning.rejected)) { rejected, rejectedCount in
                    var made = HkLearning(typos: typoList, fixes: fixList, typo_count: count, rejected: rejected, rejected_count: rejectedCount)
                    return body(&made)
                }
            }
        }
    }
}

/// The never-suggest list (`never-suggest.tsv`) as the core reads it, made once per process.
nonisolated final class KeyboardCoreNeverSuggest: @unchecked Sendable {
    let pointer: OpaquePointer

    init?(tsv: String) {
        guard let pointer = hk_never_suggest_new(tsv) else { return nil }
        self.pointer = pointer
    }

    deinit {
        hk_never_suggest_free(pointer)
    }
}

/// German nouns' capitals (`de.capitals`) as the core reads them: mapped in place, or a copy of
/// the list's bytes.
nonisolated final class KeyboardCoreGermanCapitals: @unchecked Sendable {
    let pointer: OpaquePointer

    init?(url: URL) {
        guard let pointer = hk_german_capitals_open(url.path) else { return nil }
        self.pointer = pointer
    }

    init?(bytes: [UInt8]) {
        guard let pointer = bytes.withUnsafeBufferPointer({ hk_german_capitals_new($0.baseAddress, $0.count) }) else { return nil }
        self.pointer = pointer
    }

    deinit {
        hk_german_capitals_free(pointer)
    }
}

/// One keyboard's autocorrect in the core (`hk_autocorrect_*`), freed with this object. Used
/// from the main thread, which the dictionary's callbacks run on.
@MainActor
final class KeyboardCoreAutocorrect {
    /// Frees the handle when the last reference goes, on whatever thread that is.
    private nonisolated final class Owner: @unchecked Sendable {
        let pointer: OpaquePointer

        init(_ pointer: OpaquePointer) {
            self.pointer = pointer
        }

        deinit {
            hk_autocorrect_free(pointer)
        }
    }

    private let owner: Owner
    var pointer: OpaquePointer { owner.pointer }
    /// The kept words last handed over, by list: which store, and its revision then.
    private var mine: (ObjectIdentifier, Int)?
    private var partners: (ObjectIdentifier, Int)?

    init(neverSuggest: KeyboardCoreNeverSuggest?, germanCapitals: KeyboardCoreGermanCapitals?, checker: HkTextChecker) {
        // NULL only on a panic, which making a handle never has.
        owner = Owner(hk_autocorrect_new(neverSuggest?.pointer, germanCapitals?.pointer, checker)!)
    }

    /// A yes-or-no question (`hk_autocorrect_is`).
    func ask(_ which: UInt8, _ word: String? = nil, _ other: String? = nil) -> Bool {
        hk_autocorrect_is(pointer, which, word, other) == 1
    }

    /// A string answer (`hk_autocorrect_text`).
    func text(_ which: UInt8, _ word: String? = nil, _ other: String? = nil) -> String? {
        var out: UnsafeMutablePointer<CChar>?
        guard hk_autocorrect_text(pointer, which, word, other, &out) == 0 else { return nil }
        return KeyboardCoreText.take(out)
    }

    /// A list (`hk_autocorrect_list`).
    func list(_ which: UInt8, _ items: [String] = [], _ word: String? = nil, flag: Bool = false, model: OpaquePointer? = nil) -> [String] {
        var out = HkStringList()
        let status = KeyboardCoreText.with(items) { list, count in
            hk_autocorrect_list(pointer, which, list, count, word, flag ? 1 : 0, model, &out)
        }
        guard status == 0 else { return [] }
        return KeyboardCoreText.take(&out)
    }

    /// Words of one kind (`hk_autocorrect_set_words`).
    func words(_ which: UInt8, _ words: [String]) {
        KeyboardCoreText.with(words) { list, count in
            _ = hk_autocorrect_set_words(pointer, which, list, count)
        }
    }

    /// Hands over a list's kept words when it is another list, or changed, since the last time.
    func learnedWords(_ store: LearnedWordsStore, partner: LearnedWordsStore?) {
        store.refresh()
        if mine.map({ $0 != (ObjectIdentifier(store), store.revision) }) ?? true {
            mine = (ObjectIdentifier(store), store.revision)
            words(UInt8(HK_AUTOCORRECT_LEARNED), store.words)
        }
        guard let partner else {
            partners = nil
            return
        }
        partner.refresh()
        if partners.map({ $0 != (ObjectIdentifier(partner), partner.revision) }) ?? true {
            partners = (ObjectIdentifier(partner), partner.revision)
            words(UInt8(HK_AUTOCORRECT_PARTNER_LEARNED), partner.words)
        }
    }

    /// A new partner's list goes over before its next question.
    func forgetPartnerWords() {
        partners = nil
    }

    /// The letter grid of `language`'s keys in `letters`' arrangement, a cell a letter.
    func grid(_ language: KeyboardLanguage, _ letters: KeyboardLetterLayout?) {
        let cells = Self.cells(language, letters)
        cells.withUnsafeBufferPointer { buffer in
            _ = hk_autocorrect_set_keys(pointer, language.rawValue, buffer.baseAddress, buffer.count)
        }
    }

    /// The grid's cells: x the column, y the row (`KeyboardLanguage.keyCoordinates`).
    static func cells(_ language: KeyboardLanguage, _ letters: KeyboardLetterLayout?) -> [HkKeyCentre] {
        language.keyCoordinates(letters).compactMap { letter, place in
            TypingDecoder.Keys.scalar(of: letter).map { HkKeyCentre(letter: $0, x: place.1, y: place.0) }
        }
        .sorted { ($0.y, $0.x, $0.letter) < ($1.y, $1.x, $1.letter) }
    }

    /// Spatial autocorrect's keys (`baseline` 0) or the centres without Adaptive touch (1).
    func keys(_ baseline: UInt8, _ keys: TypingDecoder.Keys?) {
        guard let keys else {
            _ = hk_autocorrect_set_decoder_keys(pointer, baseline, nil)
            return
        }
        keys.withCore { _ = hk_autocorrect_set_decoder_keys(pointer, baseline, $0) }
    }

    /// A model (`HK_AUTOCORRECT_DECODER`, `_PARTNER_DECODER` or `_CONTEXT`), or none.
    func model(_ which: Int32, _ model: TypingDecoderModel?) {
        _ = hk_autocorrect_set_model(pointer, UInt8(which), model?.pointer)
    }

    func taps(_ taps: [CGPoint]?) {
        guard let taps else {
            _ = hk_autocorrect_set_taps(pointer, nil, 0)
            return
        }
        let points = taps.flatMap { [Double($0.x), Double($0.y)] }
        _ = hk_autocorrect_set_taps(pointer, points, taps.count)
    }
}
