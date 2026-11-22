import Darwin
import Foundation

/// Hex's own list of real words outside a language's typing model (`HEXKeyboard/TypingModel/
/// <code>.valid`, the same file Android ships: `android/benchmark/typing/build-valid-words.py`).
/// One lowercase NFC spelling per line, alone or followed by a tab and its cost, sorted by its
/// UTF-8 bytes, newline-terminated. The file is memory-mapped read-only, like the `.hextm` models,
/// and searched in place by bisection: no copy and no set, so a 5 MB list costs only the clean
/// pages its lookups touch, which iOS can drop under pressure.
nonisolated final class HexWordList: @unchecked Sendable {
    let byteCount: Int
    private let base: UnsafePointer<UInt8>

    init?(url: URL) {
        let fd = open(url.path, O_RDONLY)
        guard fd >= 0 else { return nil }
        defer { close(fd) }
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_size > 0 else { return nil }
        let size = Int(info.st_size)
        guard let mapped = mmap(nil, size, PROT_READ, MAP_PRIVATE, fd, 0), mapped != MAP_FAILED else { return nil }
        let bytes = UnsafePointer(mapped.assumingMemoryBound(to: UInt8.self))
        guard bytes[size - 1] == 10 else { munmap(mapped, size); return nil }
        base = bytes
        byteCount = size
    }

    deinit {
        munmap(UnsafeMutableRawPointer(mutating: base), byteCount)
    }

    /// Whether the list has `word` (lowercase NFC, as the builder writes them): one bisection over
    /// the mapped bytes, nothing allocated for a native string.
    func contains(_ word: String) -> Bool {
        var word = word
        return word.withUTF8 { key in
            guard !key.isEmpty else { return false }
            var lo = 0, hi = byteCount
            while lo < hi {
                // The line holding the middle byte: back to its start, on to its tab or newline.
                var start = (lo + hi) / 2
                while start > lo, base[start - 1] != 10 { start -= 1 }
                var end = start
                while base[end] != 10, base[end] != 9 { end += 1 }
                var next = end
                while base[next] != 10 { next += 1 }
                var order = 0
                var k = 0
                while order == 0, k < key.count, start + k < end {
                    order = Int(base[start + k]) - Int(key[k])
                    k += 1
                }
                if order == 0 { order = (end - start) - key.count }
                if order == 0 { return true }
                if order < 0 { lo = next + 1 } else { hi = start }
            }
            return false
        }
    }

    /// The list for a language in the keyboard's bundle, mapped once per process; nil when the
    /// build has none, as `TypingDecoderModel.bundled` finds the model.
    @MainActor static func bundled(_ language: String) -> HexWordList? {
        if let cached = loaded[language] { return cached }
        var url = Bundle.main.url(forResource: language, withExtension: "valid")
        if url == nil, let plugins = Bundle.main.builtInPlugInsURL,
           let keyboard = Bundle(url: plugins.appendingPathComponent("HEXKeyboard.appex")) {
            url = keyboard.url(forResource: language, withExtension: "valid")
        }
        let list = url.flatMap { HexWordList(url: $0) }
        loaded[language] = list
        return list
    }

    @MainActor private static var loaded: [String: HexWordList?] = [:]
}

/// A language's own words: its typing model's and its `HexWordList`'s. Where Hex has both, they
/// alone say whether a spelling is a word, not the phone's dictionary, which iOS may not have
/// for the language or may have take any letters for one (iOS 27.2 simulator: `nb_NO`, `pl_PL`).
nonisolated struct HexOwnWords: Sendable {
    let language: String
    let model: TypingDecoderModel
    let list: HexWordList

    /// Whether `word` is one, case-folded ("Żółw" is "żółw"; names are listed lowercase) and with ’
    /// read as '. The list lookup allocates nothing; folding a word with a capital or a ’ copies it.
    func knows(_ word: String) -> Bool {
        guard !word.isEmpty else { return true }
        var lower = word.contains(where: \.isUppercase) ? word.lowercased() : word
        if lower.contains("\u{2019}") { lower = lower.replacingOccurrences(of: "\u{2019}", with: "'") }
        return model.index(of: lower) != nil || list.contains(lower)
    }
}

@MainActor
extension AutocorrectEngine {
    /// Gives this engine's spelling dictionary the decoder model's language's own words, when the
    /// keyboard ships a list for it, or takes them away. Called whenever `decoder` changes.
    func useOwnWords(_ model: TypingDecoderModel?) {
        guard let system = checker as? SystemTextChecker else { return }
        system.ownWords = model.flatMap { model in HexWordList.bundled(model.language).map { HexOwnWords(language: model.language, model: model, list: $0) } }
    }

    /// Whether this language has words to correct with: the phone's dictionary, or Hex's own
    /// model for it. A missing system dictionary never switches a Hex model off.
    var hasWords: Bool {
        hasDictionary || decoder?.model.language == language.rawValue
    }
}
