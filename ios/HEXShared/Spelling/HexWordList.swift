import Darwin
import Foundation

/// One of Hex's own sorted word lists beside a language's typing model, the same files Android
/// ships: `<code>.valid`, the real words outside the model (`build-valid-words.py`), and
/// `<code>.capitals`, how often the books write a word with a capital (`build-capitals.py`).
/// One lowercase NFC spelling per line, alone or followed by a tab and a number (a cost, a
/// percent), sorted by its UTF-8 bytes, newline-terminated. The file is memory-mapped read-only, like the `.hextm` models,
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
        line(of: word) != nil
    }

    /// The number after `word`'s tab (a `.capitals` percent, a `.valid` cost), or nil when the
    /// list lacks the word or its line has none.
    func value(of word: String) -> Int? {
        guard let (end, next) = line(of: word), end < next else { return nil }
        var value = 0
        for i in (end + 1)..<next {
            let digit = Int(base[i]) - 48
            guard (0...9).contains(digit) else { return nil }
            value = value * 10 + digit
        }
        return value
    }

    /// Where `word`'s line ends (its tab or newline) and where its newline is.
    private func line(of word: String) -> (end: Int, next: Int)? {
        var word = word
        return word.withUTF8 { key -> (Int, Int)? in
            guard !key.isEmpty else { return nil }
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
                if order == 0 { return (end, next) }
                if order < 0 { lo = next + 1 } else { hi = start }
            }
            return nil
        }
    }

    /// A language's list in the keyboard's bundle (`kind` "valid" or "capitals"), mapped once per
    /// process; nil when the build has none, as `TypingDecoderModel.bundled` finds the model.
    @MainActor static func bundled(_ language: String, _ kind: String = "valid") -> HexWordList? {
        let name = language + "." + kind
        if let cached = loaded[name] { return cached }
        var url = Bundle.main.url(forResource: language, withExtension: kind)
        if url == nil, let plugins = Bundle.main.builtInPlugInsURL,
           let keyboard = Bundle(url: plugins.appendingPathComponent("HEXKeyboard.appex")) {
            url = keyboard.url(forResource: language, withExtension: kind)
        }
        let list = url.flatMap { HexWordList(url: $0) }
        loaded[name] = list
        return list
    }

    @MainActor private static var loaded: [String: HexWordList?] = [:]
}

/// A language's own words: its typing model's and its `HexWordList`'s. Where Hex has both, they
/// alone say whether a spelling is a word, not the phone's dictionary, which iOS may not have
/// for the language or may have take any letters for one (iOS 27.2 simulator: `nb_NO`, `pl_PL`).
nonisolated struct HexOwnWords: Sendable {
    /// A word the books write with a capital this often is a name: typed all lowercase it is no
    /// word, so autocorrect gives it its capital ("paris" -> "Paris", "jean" -> "Jean"), as
    /// `UITextChecker` does with a word it knows only capitalised (Android's `TypingWord`: 95).
    static let nameShare = 95

    let language: String
    let model: TypingDecoderModel
    let list: HexWordList
    /// `<code>.capitals`, when the keyboard ships one: names and other words the books write with
    /// a capital, with how often they do (50–100).
    var capitals: HexWordList?

    /// Whether `word` is one, case-folded ("Żółw" is "żółw"; lists are lowercase) and with ’ read
    /// as '; a name typed all lowercase isn't ("paris"), its capitalised form is ("Paris"). The
    /// list lookups allocate nothing; folding a word with a capital or a ’ copies it.
    func knows(_ word: String) -> Bool {
        guard !word.isEmpty else { return true }
        let typedLower = !word.contains(where: \.isUppercase)
        var lower = typedLower ? word : word.lowercased()
        if lower.contains("\u{2019}") { lower = lower.replacingOccurrences(of: "\u{2019}", with: "'") }
        let share = capitals?.value(of: lower)
        if typedLower, lower.count >= 2, let share, share >= Self.nameShare { return false }
        return model.index(of: lower) != nil || list.contains(lower) || share != nil
    }

    /// Whether `word`, typed all lowercase, is one the books capitalise only part of the time
    /// (50–94%: "tom", "mary", also ordinary words or capped as such): the phone's dictionary,
    /// where it works, decides whether it is a name (`SystemTextChecker.isWordValid`).
    func mayBeName(_ word: String) -> Bool {
        guard word.count >= 2, !word.contains(where: \.isUppercase), let share = capitals?.value(of: word) else { return false }
        return share < Self.nameShare
    }
}

@MainActor
extension AutocorrectEngine {
    /// Gives this engine's spelling dictionary the decoder model's language's own words, when the
    /// keyboard ships a list for it, or takes them away. Called whenever `decoder` changes.
    func useOwnWords(_ model: TypingDecoderModel?) {
        guard let system = checker as? SystemTextChecker else { return }
        system.ownWords = model.flatMap { model in
            HexWordList.bundled(model.language).map {
                HexOwnWords(language: model.language, model: model, list: $0, capitals: HexWordList.bundled(model.language, "capitals"))
            }
        }
    }

    /// Whether this language has words to correct with: the phone's dictionary, or Hex's own
    /// model for it. A missing system dictionary never switches a Hex model off.
    var hasWords: Bool {
        hasDictionary || decoder?.model.language == language.rawValue
    }
}
