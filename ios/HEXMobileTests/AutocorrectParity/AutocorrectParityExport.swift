import CoreGraphics
import Foundation
import XCTest
@testable import HEX

/// Writes the autocorrect parity fixture for the Rust port (`crates/keyboard/core/tests/
/// autocorrect_parity.rs`): cases of operations on the oracle (`ACOracle*.swift`, dev's
/// `AutocorrectEngine` renamed) with a deterministic dictionary (`ACStubDictionary`), each with
/// its inputs and outputs and a hash of every dictionary call in order. Opt-in: it runs only
/// when `HEX_AUTOCORRECT_PARITY_OUT` names the file to write (`TEST_RUNNER_…` for xcodebuild),
/// with `HEX_AUTOCORRECT_PARITY_COUNT` cases for English, Spanish, German and French (the
/// languages with a typing model) and a quarter as
/// many for each other language (200 by default), and every `HEX_AUTOCORRECT_PARITY_LITERALS`th
/// case from the iOS tests' inputs (all by default).
///
/// Each case runs five times: with your own words in the port's order (by shortcut and typo),
/// the decoder's fixed tie order and case-folded comparisons (`out`, `calls`: what Rust must
/// match); with Swift's dictionary order (`today`, written only where it differs: the cases
/// the port's order changes); with the decoder's own walk order (`tie` where it differs); with
/// Foundation's `caseInsensitiveCompare` (`foundation` where it differs); and with the shared
/// never-suggest list read by base language (`shared`, `sharedCalls` where they differ).
/// Lines: `setup` (the dictionaries), `chars` (Swift's answers for character properties and
/// Foundation calls), `keys` (letter keys by id), then `case`s.
@MainActor
final class AutocorrectParityExport: XCTestCase {
    func testExport() throws {
        let environment = ProcessInfo.processInfo.environment
        guard let out = environment["HEX_AUTOCORRECT_PARITY_OUT"] else {
            throw XCTSkip("set TEST_RUNNER_HEX_AUTOCORRECT_PARITY_OUT to write the autocorrect parity fixture")
        }
        let count = environment["HEX_AUTOCORRECT_PARITY_COUNT"].flatMap(Int.init) ?? 200
        let literalStride = environment["HEX_AUTOCORRECT_PARITY_LITERALS"].flatMap(Int.init) ?? 1
        // `HEX_AUTOCORRECT_PARITY_ONLY=12,40`: only these cases (made as always), to look into them.
        let only = environment["HEX_AUTOCORRECT_PARITY_ONLY"].map { Set($0.split(separator: ",").compactMap { Int($0) }) }
        let keepCalls = environment["HEX_AUTOCORRECT_PARITY_CALLS"] != nil
        let repo = environment["HEX_AUTOCORRECT_PARITY_REPO"]
            ?? URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().path
        ACOracleFiles.repo = repo
        FileManager.default.createFile(atPath: out, contents: nil)
        let handle = try XCTUnwrap(FileHandle(forWritingAtPath: out))
        defer { try? handle.close() }
        var writer = TapParityWriter(handle: handle)

        var models: [String: TapOracleModel] = [:]
        for code in ["en", "es", "de", "fr"] {
            let url = URL(fileURLWithPath: repo + "/ios/HEXKeyboard/TypingModel/\(code).hextm")
            models[code] = try XCTUnwrap(TapOracleModel(url: url), "no \(code) model")
        }
        let (dictionaries, setup) = Self.dictionaries(models: models)
        try writer.write(setup)
        try writer.write(ACParityChars.line())

        var keysets: [String: ACParityGenerator.KeySet] = [:]
        var keysetsByID: [Int: TapOracle.Keys] = [:]
        var nextID = 0
        var caseID = 0
        var grids = Set<String>()
        var totals: [String: Int] = [:]
        let languages: [KeyboardLanguage] = [.english, .spanish, .german, .french, .frenchCanadian, .frenchSwiss, .italian,
                                             .portuguese, .polish, .czech, .hungarian, .romanian, .norwegian, .russian, .ukrainian, .greek,
                                             .serbianLatin, .korean]
        // Cases are made, run and written one at a time; a case's keys are written before it.
        func emit(_ c: ACParityGenerator.Case, _ language: KeyboardLanguage, _ generator: inout ACParityGenerator) throws {
            for line in generator.keysetLines { try writer.write(line) }
            generator.keysetLines.removeAll()
            keysets = generator.keysets
            keysetsByID = generator.keysetsByID
            nextID = generator.nextKeysetID
            let runner = ACParityRunner(dictionaries: dictionaries, models: models, keysets: keysetsByID)
            var line: [String: Any] = ["kind": "case", "id": caseID, "lang": language.rawValue, "learned": c.learned, "ops": c.ops]
            caseID += 1
            if let only, !only.contains(caseID - 1) { return }
            func useGrid(_ ref: Any?) throws {
                guard let ref = ref as? String, grids.insert(ref).inserted else { return }
                try writer.write(ACParityGenerator.gridLine(ref))
            }
            for op in c.ops { try useGrid(op["grid"]) }
            let fixed = Self.run(runner, c, order: .byKey, tie: .ascending, shared: false, folding: true, keepCalls: keepCalls)
            line["out"] = fixed.outputs
            line["grid0"] = fixed.initialGrid
            try useGrid(fixed.initialGrid)
            for out in fixed.outputs { try useGrid((out as? [String: Any])?["g"]) }
            line["calls"] = String(fixed.hash, radix: 16)
            line["ncalls"] = fixed.calls
            if let lines = fixed.lines { line["log"] = lines }
            let today = Self.run(runner, c, order: .dictionary, tie: .ascending, shared: false, folding: true)
            if !Self.same(today.outputs, fixed.outputs) {
                line["today"] = today.outputs
                totals["order", default: 0] += 1
            }
            let swiftTies = Self.run(runner, c, order: .byKey, tie: .dictionary, shared: false, folding: true)
            if !Self.same(swiftTies.outputs, fixed.outputs) {
                line["tie"] = true
                totals["tie", default: 0] += 1
            }
            let foundation = Self.run(runner, c, order: .byKey, tie: .ascending, shared: false, folding: false)
            if !Self.same(foundation.outputs, fixed.outputs) {
                line["foundation"] = foundation.outputs
                totals["foundation", default: 0] += 1
            }
            let shared = Self.run(runner, c, order: .byKey, tie: .ascending, shared: true, folding: true)
            if !Self.same(shared.outputs, fixed.outputs) {
                line["shared"] = shared.outputs
                totals["shared", default: 0] += 1
            }
            if shared.hash != fixed.hash { line["sharedCalls"] = String(shared.hash, radix: 16) }
            totals[language.rawValue, default: 0] += 1
            try writer.write(line)
        }
        for (n, language) in languages.enumerated() {
            var generator = ACParityGenerator(
                language: language, dictionaries: dictionaries, models: models, rng: TapParityRandom(state: 0xAC_0C0E_0000 &+ UInt64(n)),
                keysets: keysets, keysetsByID: keysetsByID, nextKeysetID: nextID
            )
            let main = [KeyboardLanguage.english, .spanish, .german, .french].contains(language)
            for _ in 0..<(main ? count : max(1, count / 4)) {
                let c = generator.makeCase()
                try emit(c, language, &generator)
            }
        }
        var literalGenerator = ACParityGenerator(
            language: .english, dictionaries: dictionaries, models: models, rng: TapParityRandom(state: 0xAC_7E57),
            keysets: keysets, keysetsByID: keysetsByID, nextKeysetID: nextID
        )
        for c in ACParityLiterals.cases(repo: repo, generator: &literalGenerator, stride: literalStride) {
            try emit(c, .english, &literalGenerator)
        }
        try writer.flush()
        print("AUTOCORRECT-PARITY cases=\(caseID) " + totals.keys.sorted().map { "\($0)=\(totals[$0]!)" }.joined(separator: " "))
    }

    static func run(
        _ runner: ACParityRunner, _ c: ACParityGenerator.Case, order: ACOracle.OwnWordsOrder, tie: TapOracle.TieOrder,
        shared: Bool, folding: Bool, keepCalls: Bool = false
    ) -> ACParityRunner.Result {
        ACOracle.ownWordsOrder = order
        ACOracle.caseComparison = folding ? .folding : .foundation
        TapOracle.tieOrder = tie
        ACOOffensiveWords.sharedList = shared
        return runner.run(c.ops, initialLearned: c.learned, keepLines: keepCalls)
    }

    static func same(_ a: [Any], _ b: [Any]) -> Bool {
        let options: JSONSerialization.WritingOptions = [.sortedKeys]
        return (try? JSONSerialization.data(withJSONObject: a, options: options)) == (try? JSONSerialization.data(withJSONObject: b, options: options))
    }

    /// The stub dictionaries and the setup line the Rust test builds the same ones from.
    static func dictionaries(models: [String: TapOracleModel]) -> ([String: ACStubDictionary], [String: Any]) {
        let modelWords = 20_000, dropEvery: UInt64 = 41
        var out: [String: ACStubDictionary] = [:]
        for (code, model) in models {
            let order = (0..<model.wordCount).sorted { (model.unigramCost($0), $0) < (model.unigramCost($1), $1) }
            var words = order.prefix(modelWords).map { model.word($0) }.filter { ACStub.fnv($0) % dropEvery != 0 }
            words += ACParityWords.extras[code] ?? []
            out[code] = ACStubDictionary(ranked: words, names: ACParityWords.names[code] ?? [])
        }
        for (code, list) in ACParityWords.lists where models[code] == nil {
            out[code] = ACStubDictionary(ranked: list, names: ACParityWords.names[code] ?? [])
        }
        let setup: [String: Any] = [
            "kind": "setup", "modelWords": modelWords, "dropEvery": dropEvery, "lists": ACParityWords.lists,
            "names": ACParityWords.names, "extras": ACParityWords.extras, "available": ACParityGenerator.available,
            "models": models.keys.sorted(),
        ]
        return (out, setup)
    }
}

/// Swift's answers on characters and strings the port reads them by.
@MainActor
enum ACParityChars {
    static let ranges: [ClosedRange<UInt32>] = [
        0x09...0x0D, 0x20...0x24F, 0x2B0...0x2FF, 0x300...0x36F, 0x370...0x3FF, 0x400...0x52F, 0x1E00...0x1FFF,
        0x2000...0x206F, 0x2100...0x214F, 0x2160...0x2188, 0x3000...0x3007, 0xFB00...0xFB06, 0x4E00...0x4E10,
        0x1680...0x1680, 0x180E...0x180E, 0xFEFF...0xFEFF, 0x5341...0x5345, 0x767E...0x767E, 0x13A0...0x13F5, 0xAB70...0xABBF,
    ]

    static func line() -> [String: Any] {
        var rows: [[Any]] = []
        for range in ranges {
            for v in range {
                guard let scalar = Unicode.Scalar(v) else { continue }
                let c = Character(scalar), s = String(c)
                var flags = 0
                if c.isLetter { flags |= 1 }
                if c.isNumber { flags |= 2 }
                if c.isWhitespace { flags |= 4 }
                if c.isNewline { flags |= 8 }
                if c.isUppercase { flags |= 16 }
                if c.isLowercase { flags |= 32 }
                let fold = s.folding(options: .diacriticInsensitive, locale: nil)
                var ci = 0
                if s.caseInsensitiveCompare(s.lowercased()) == .orderedSame { ci |= 1 }
                if s.caseInsensitiveCompare(s.uppercased()) == .orderedSame { ci |= 2 }
                if s.caseInsensitiveCompare(fold) == .orderedSame { ci |= 4 }
                // The same, case-folded (the port's comparison).
                if ACOracle.caseFolded(s) == ACOracle.caseFolded(s.lowercased()) { ci |= 8 }
                if ACOracle.caseFolded(s) == ACOracle.caseFolded(s.uppercased()) { ci |= 16 }
                if ACOracle.caseFolded(s) == ACOracle.caseFolded(fold) { ci |= 32 }
                rows.append([String(v, radix: 16), flags, s.lowercased(), s.uppercased(), fold, ci,
                             s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty])
            }
        }
        let tricky = ACParityWords.tricky + ["Straße", "STRASSE", "ς", "σ", "Σ", "ı", "I", "i", "İ", "ﬁ", "fi", "K", "k", "Å", "å", "Ω", "ω",
                                            "ſ", "s", "ǰ", "J̌", "ẞ", "ß", "SS", "ǅ", "Ǆ", "ǆ", "e\u{301}", "\u{E9}", "É", "1\u{301}", "'\u{301}", " \u{301}", "\u{1FCD}\u{3B1}", "\u{301}x", "\u{1F80}", "\u{1F08}\u{399}", "Ꭰ", "ꭰ"]
        let strings: [[Any]] = tricky.map { s in
            [s, s.count, s.lowercased(), s.uppercased(), s.folding(options: .diacriticInsensitive, locale: nil),
             tricky.map { s.caseInsensitiveCompare($0) == .orderedSame ? "1" : "0" }.joined(),
             tricky.map { ACOracle.caseFolded(s) == ACOracle.caseFolded($0) ? "1" : "0" }.joined(),
             s.trimmingCharacters(in: .whitespacesAndNewlines), s.replacingOccurrences(of: "'", with: "")]
        }
        // `caseInsensitiveCompare` both ways on pairs where Foundation's folding is its own: a
        // receiver's ß against an umlaut before s, Greek's iota subscript.
        let pairs: [(String, String)] = [
            ("wußte", "wüste"), ("Maß", "mäs"), ("groß", "grös"), ("Straße", "Sträse"), ("aß", "äs"), ("aß", "a\u{308}s"),
            ("Aß", "ÄS"), ("aßb", "a\u{301}sb"), ("aß", "ass"), ("ẞ", "ss"), ("ﬃ", "ffi"), ("ŉ", "ʼn"), ("ǰ", "j\u{30C}"),
            ("İ", "i\u{307}"), ("İ", "i"), ("\u{345}", "ι"), ("ᾳ", "αι"), ("ᾼ", "ᾳ"), ("ΑΙ", "ᾳ"), ("ἈΙ", "ᾀ"),
        ]
        let pairRows: [[Any]] = pairs.flatMap { a, b in
            [[a, b, a.caseInsensitiveCompare(b) == .orderedSame, ACOracle.caseFolded(a) == ACOracle.caseFolded(b)],
             [b, a, b.caseInsensitiveCompare(a) == .orderedSame, ACOracle.caseFolded(b) == ACOracle.caseFolded(a)]]
        }
        return ["kind": "chars", "rows": rows, "strings": strings, "pairs": pairRows]
    }
}

/// Cases from the inputs of `AutocorrectEngineTests.swift`: every string literal there as the
/// text before the cursor, and its literal pairs as lexicon entries, across a few setups.
@MainActor
enum ACParityLiterals {
    static func cases(repo: String, generator g: inout ACParityGenerator, stride: Int) -> [ACParityGenerator.Case] {
        guard let source = try? String(contentsOfFile: repo + "/ios/HEXMobileTests/AutocorrectEngineTests.swift", encoding: .utf8) else {
            return []
        }
        let literal = try! NSRegularExpression(pattern: #""((?:[^"\\\n]|\\.)*)""#)
        var seen = Set<String>()
        var literals: [String] = []
        for m in literal.matches(in: source, range: NSRange(source.startIndex..., in: source)) {
            guard let r = Range(m.range(at: 1), in: source) else { continue }
            let raw = String(source[r])
            guard !raw.contains("\\("), raw.count <= 48 else { continue }
            let text = raw.replacingOccurrences(of: "\\\"", with: "\"").replacingOccurrences(of: "\\n", with: "\n")
                .replacingOccurrences(of: "\\u{2019}", with: "\u{2019}").replacingOccurrences(of: "\\\\", with: "\\")
            if seen.insert(text).inserted { literals.append(text) }
        }
        let pair = try! NSRegularExpression(pattern: #"\("([^"\\]*)", "([^"\\]*)"\)"#)
        var pairs: [[String]] = []
        for m in pair.matches(in: source, range: NSRange(source.startIndex..., in: source)) {
            if let a = Range(m.range(at: 1), in: source), let b = Range(m.range(at: 2), in: source) {
                pairs.append([String(source[a]), String(source[b])])
            }
        }
        let setups: [(KeyboardLanguage, KeyboardLanguage?)] = [
            (.english, nil), (.english, .french), (.french, .english), (.italian, nil), (.spanish, .english),
            (.german, .english), (.portuguese, .english), (.polish, nil), (.czech, .english), (.greek, nil), (.russian, nil),
            (.hungarian, .english), (.frenchCanadian, nil), (.ukrainian, nil), (.korean, nil),
        ]
        var out: [ACParityGenerator.Case] = []
        for (i, text) in literals.enumerated() where i % max(1, stride) == 0 {
            let (language, partner) = setups[(i / max(1, stride)) % setups.count]
            var c = ACParityGenerator.Case()
            c.ops.append(["op": "lang", "l": language.rawValue, "avail": "all"])
            c.ops.append(["op": "keys", "l": language.rawValue, "letters": language.letterLayouts[0].rawValue])
            c.ops.append(["op": "lexicon", "entries": [["omw", "On my way!"], ["addr", "123 Market St"], ["Kit", "Kit"]] + (i % 3 == 0 ? pairs : [])])
            if let partner { c.ops.append(g.partnerOp(partner, available: "all")) }
            if g.models[language.rawValue] != nil, i % 2 == 0 {
                c.ops.append(["op": "decoder", "model": language.rawValue])
                c.ops.append(["op": "context", "model": language.rawValue])
                if let set = g.keyset(language, language.letterLayouts[0]) {
                    c.ops.append(["op": "dkeys", "keys": set.stock])
                    let token = ACOracle.currentToken(in: text) ?? ""
                    let taps = g.taps(for: token, set, spread: 0).map { ACParityGenerator.json($0.1) }
                    c.ops.append(["op": "taps", "taps": taps ?? NSNull()])
                }
            }
            let model: Any = g.models[language.rawValue] != nil ? language.rawValue as Any : NSNull()
            c.ops.append(["op": "space", "before": text, "field": "text", "off": false])
            c.ops.append(["op": "suggest", "before": text, "field": "text", "off": false, "model": model, "cap": false])
            c.ops.append(["op": "suggest", "before": text + " ", "field": "text", "off": false, "model": model, "cap": true])
            c.ops.append(["op": "token", "before": text])
            let words = text.split(separator: " ").map(String.init)
            if words.count >= 2 {
                let before = words.dropLast().joined(separator: " ")
                c.ops.append(["op": "revision", "w": words[words.count - 2], "before": before, "next": words.last!, "field": "text", "off": false])
            }
            c.ops.append(["op": "caps", "on": false])
            c.ops.append(["op": "space", "before": text, "field": "text", "off": false])
            c.ops.append(["op": "enabled", "on": false])
            c.ops.append(["op": "suggest", "before": text, "field": "text", "off": false, "model": model, "cap": false])
            out.append(c)
        }
        return out
    }
}
