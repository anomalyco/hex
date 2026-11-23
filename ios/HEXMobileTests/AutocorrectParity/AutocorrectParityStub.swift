import Foundation
@testable import HEX

/// The parity fixture's dictionary: deterministic, so the Rust test answers every question the
/// same way from the same data (`crates/keyboard/core/tests/autocorrect_parity/stub.rs` is its
/// twin). Words are a language's typing model words, commonest first (the first 20,000, less one
/// in 41 by hash: words the model knows that the dictionary doesn't), or a short list for a
/// language without a model, plus names it knows only capitalised. `isWordValid` takes a word in
/// any case, every single letter (as `UITextChecker` does) and a name as spelled or in capitals;
/// `guesses` are the commonest words one edit away (six), the word split in two (hyphenated
/// first for half the words) and, for two letters, a dotted form; `completions` the commonest ten
/// longer words that start with the prefix.
@MainActor
final class ACStubDictionary {
    let ranked: [String]
    let rank: [String: Int]
    let names: Set<String>
    let upperNames: Set<String>
    let nameByLower: [String: (Int, String)]
    let alphabet: [Character]
    let byFirst: [Character: [(Int, String)]]
    /// By the word's bytes: a `String` key would answer "café" with what "cafe\u{301}" got.
    var guessMemo: [[UInt8]: [String]] = [:]

    init(ranked words: [String], names nameList: [String]) {
        var rank: [String: Int] = [:]
        var ranked: [String] = []
        let nameLowers = Set(nameList.map { $0.lowercased() })
        for word in words where rank[word] == nil && !nameLowers.contains(word) {
            rank[word] = ranked.count
            ranked.append(word)
        }
        var nameByLower: [String: (Int, String)] = [:]
        for (i, name) in nameList.enumerated() where nameByLower[name.lowercased()] == nil {
            nameByLower[name.lowercased()] = (ranked.count + i, name)
        }
        var letters = Set<Character>()
        var byFirst: [Character: [(Int, String)]] = [:]
        for (i, word) in ranked.enumerated() {
            letters.formUnion(word)
            if let f = word.first { byFirst[f, default: []].append((i, word)) }
        }
        for (lower, entry) in nameByLower {
            letters.formUnion(lower)
            if let f = lower.first { byFirst[f, default: []].append(entry) }
        }
        for key in byFirst.keys { byFirst[key]!.sort { $0.0 < $1.0 } }
        self.ranked = ranked
        self.rank = rank
        names = Set(nameList)
        upperNames = Set(nameList.map { $0.uppercased() })
        self.nameByLower = nameByLower
        alphabet = Array(letters)
        self.byFirst = byFirst
    }

    func isWordValid(_ word: String) -> Bool {
        if word.isEmpty { return true }
        if word.count == 1, word.first!.isLetter { return true }
        return rank[word.lowercased()] != nil || names.contains(word) || upperNames.contains(word)
    }

    func guesses(for word: String) -> [String] {
        let lower = word.lowercased()
        if let memo = guessMemo[Array(lower.utf8)] { return memo }
        let chars = Array(lower)
        let n = chars.count
        guard n > 0 else { return [] }
        var found: [Int: String] = [:]
        func consider(_ c: [Character]) {
            let s = String(c)
            guard s != lower else { return }
            if let r = rank[s] { found[r] = s } else if let (r, spelled) = nameByLower[s] { found[r] = spelled }
        }
        if n > 1 { for i in 0..<n { var c = chars; c.remove(at: i); consider(c) } }
        if n > 1 { for i in 0..<(n - 1) { var c = chars; c.swapAt(i, i + 1); consider(c) } }
        for i in 0..<n { for a in alphabet where a != chars[i] { var c = chars; c[i] = a; consider(c) } }
        for i in 0...n { for a in alphabet { var c = chars; c.insert(a, at: i); consider(c) } }
        var out = found.sorted { $0.key < $1.key }.prefix(6).map(\.value)
        var splits = 0
        if n >= 3 {
            for i in 1..<n where splits < 2 {
                let left = String(chars[..<i]), right = String(chars[i...])
                guard rank[left] != nil, rank[right] != nil else { continue }
                if ACStub.fnv(lower) % 2 == 0 { out.append(left + "-" + right) }
                out.append(left + " " + right)
                splits += 1
            }
        }
        if n == 2 { out.append("\(chars[0]).\(chars[1]).") }
        let result = Array(out.prefix(10))
        // Only a cache: a large export's typos would otherwise grow it without end.
        if guessMemo.count >= 100_000 { guessMemo.removeAll(keepingCapacity: true) }
        guessMemo[Array(lower.utf8)] = result
        return result
    }

    func completions(for prefix: String) -> [String] {
        let lower = prefix.lowercased()
        guard let f = lower.first, let list = byFirst[f] else { return [] }
        var out: [String] = []
        for (_, word) in list where out.count < 10 {
            let l = word.lowercased()
            if l != lower && l.hasPrefix(lower) { out.append(word) }
        }
        return out
    }
}

/// Every question the engines ask their dictionaries, hashed in order (FNV-1a 64 over
/// `tag|method|argument|answer\n`), so the Rust port must ask the same ones in the same order.
@MainActor
final class ACCallLog {
    var hash: UInt64 = 0xcbf2_9ce4_8422_2325
    var count = 0
    var lines: [String]?

    func record(_ line: String) {
        count += 1
        lines?.append(line)
        for b in (line + "\n").utf8 {
            hash ^= UInt64(b)
            hash = hash &* 0x0000_0100_0000_01b3
        }
    }
}

@MainActor
enum ACStub {
    static func fnv(_ s: String) -> UInt64 {
        var h: UInt64 = 0xcbf2_9ce4_8422_2325
        for b in s.utf8 {
            h ^= UInt64(b)
            h = h &* 0x0000_0100_0000_01b3
        }
        return h
    }

    /// The stub dictionaries from a parity set's setup line, as `stub.rs` builds them: a typing
    /// model's commonest `modelWords` words, less one in `dropEvery` by hash, and its extras, or
    /// the language's list; with its names.
    static func dictionaries(setup: [String: Any], models: [String: TypingDecoderModel]) -> [String: ACStubDictionary] {
        let modelWords = setup["modelWords"] as! Int, dropEvery = UInt64(setup["dropEvery"] as! Int)
        let names = setup["names"] as! [String: [String]], extras = setup["extras"] as! [String: [String]]
        var out: [String: ACStubDictionary] = [:]
        for (code, model) in models {
            let order = (0..<model.wordCount).sorted { (model.unigramCost($0), $0) < (model.unigramCost($1), $1) }
            var words = order.prefix(modelWords).map { model.word($0) }.filter { fnv($0) % dropEvery != 0 }
            words += extras[code] ?? []
            out[code] = ACStubDictionary(ranked: words, names: names[code] ?? [])
        }
        for (code, list) in setup["lists"] as! [String: [String]] where models[code] == nil {
            out[code] = ACStubDictionary(ranked: list, names: names[code] ?? [])
        }
        return out
    }

    /// The dictionary a `UITextChecker` code opens ("en_US" -> "en", "no" -> "nb").
    static func dictionaryCode(_ code: String) -> String {
        let base = String(code.prefix { $0 != "_" && $0 != "-" })
        return base == "no" ? "nb" : base
    }
}

/// One engine's dictionary (`TextChecking`), tagged for the call log.
@MainActor
final class ACStubChecker: ACOChecking, TextChecking {
    let tag: String
    let log: ACCallLog
    let dictionaries: [String: ACStubDictionary]
    let acceptsEverything: Bool
    var code = "en_US"

    init(tag: String, log: ACCallLog, dictionaries: [String: ACStubDictionary], acceptsEverything: Bool) {
        self.tag = tag
        self.log = log
        self.dictionaries = dictionaries
        self.acceptsEverything = acceptsEverything
    }

    var dictionary: ACStubDictionary? { dictionaries[ACStub.dictionaryCode(code)] }

    func isWordValid(_ word: String) -> Bool {
        let answer = acceptsEverything || (dictionary?.isWordValid(word) ?? false)
        log.record("\(tag)|v|\(word)|\(answer ? 1 : 0)")
        return answer
    }

    func guesses(for word: String) -> [String] {
        let answer = dictionary?.guesses(for: word) ?? []
        log.record("\(tag)|g|\(word)|\(answer.joined(separator: "\u{1F}"))")
        return answer
    }

    func completions(for prefix: String) -> [String] {
        let answer = dictionary?.completions(for: prefix) ?? []
        log.record("\(tag)|c|\(prefix)|\(answer.joined(separator: "\u{1F}"))")
        return answer
    }

    func useLanguage(_ code: String) {
        self.code = code
        log.record("\(tag)|u|\(code)|")
    }
}
