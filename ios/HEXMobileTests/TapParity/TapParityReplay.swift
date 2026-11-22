import CoreGraphics
import Foundation
@testable import HEX

/// Runs tap parity cases (the lines `crates/keyboard/core/tests/fixtures/tap-parity.jsonl`
/// holds) through the shipped decoder: `TypingDecoder` and `TypingDecoderModel` over the
/// keyboard's Rust core. `check` gives what the shipped path answers, in the fixture's form,
/// beside what the line expects.
@MainActor
final class TapParityReplay {
    struct Language {
        var model: TypingDecoderModel
        var blocked: [Bool: [String]]
        var clean: Set<String>
        var users: [[String]]
        var keys: [Int: (keys: TypingDecoder.Keys, truth: TypingDecoder.Keys)] = [:]
        var decoders: [Int: TypingDecoder] = [:]
    }

    private(set) var languages: [String: Language] = [:]

    enum Failure: Error {
        case malformed(String)
    }

    /// The shipped path's answer to `line` and the answer it expects; nil for a setup line.
    func check(_ line: [String: Any]) throws -> (ours: Any, expected: Any)? {
        guard let kind = line["kind"] as? String, let lang = line["lang"] as? String else { throw Failure.malformed("kind") }
        switch kind {
        case "setup":
            guard let model = TypingDecoderModel.bundled(lang),
                  let blocked = line["blocked"] as? [String: [String]],
                  let users = line["users"] as? [[String]] else { throw Failure.malformed("setup") }
            let clean = blocked["clean"] ?? []
            languages[lang] = Language(model: model, blocked: [true: blocked["swearing"] ?? [], false: clean], clean: Set(clean), users: users)
            return nil
        case "keys":
            return try keys(line, lang)
        case "model":
            guard let language = languages[lang] else { throw Failure.malformed("no setup") }
            return (Self.model(language, line), line["out"] ?? NSNull())
        default:
            guard var language = languages[lang],
                  let set = line["keys"] as? Int, let user = line["user"] as? Int, let swearing = line["swear"] as? Bool,
                  let keys = language.keys[set] else { throw Failure.malformed("case") }
            let id = user * 2 + (swearing ? 1 : 0)
            let decoder = language.decoders[id] ?? {
                var d = TypingDecoder(model: language.model)
                d.userWords = TypingDecoder.UserWords(language.users[user], model: language.model)
                d.neverSuggest = TypingDecoder.NeverSuggest(language.blocked[swearing] ?? [], model: language.model)
                return d
            }()
            language.decoders[id] = decoder
            languages[lang] = language
            return (try Self.run(kind, line, decoder, keys), line["fixed"] ?? NSNull())
        }
    }

    private func keys(_ line: [String: Any], _ lang: String) throws -> (ours: Any, expected: Any) {
        guard var language = languages[lang], let id = line["id"] as? Int,
              let width = line["width"] as? Double, let duo = line["duo"] as? Bool,
              let rawStats = line["stats"] as? [String: [Any]],
              let out = line["out"] as? [String: Any], let truth = line["truth"] as? [String: Any],
              let keyboard = KeyboardLanguage(rawValue: lang) else { throw Failure.malformed("keys") }
        let layout = duo
            ? KeyboardLayout(width: CGFloat(width), language: keyboard, posture: .unfoldedWide, windowInset: 75)
            : KeyboardLayout(width: CGFloat(width), language: keyboard)
        var stats: [String: TypingJournalKeyStats] = [:]
        for (c, s) in rawStats {
            guard s.count == 3, let n = s[0] as? Int, let dx = s[1] as? String, let dy = s[2] as? String else { throw Failure.malformed("stats") }
            stats[c] = TypingJournalKeyStats(count: n, dx: Self.double(dx), dy: Self.double(dy))
        }
        let offsets = TypingDecoder.offsets(from: stats, layout: layout)
        guard let keys = TypingDecoder.Keys(layout: layout, offsets: offsets),
              let plain = TypingDecoder.Keys(layout: layout) else { throw Failure.malformed("no keys") }
        language.keys[id] = (keys, plain)
        languages[lang] = language
        let ours: [String: Any] = [
            "offsets": Dictionary(uniqueKeysWithValues: offsets.map { (String($0.key), [bits(Double($0.value.dx)), bits(Double($0.value.dy))]) }),
            "out": Self.json(keys),
            "truth": Self.json(plain),
        ]
        let expected: [String: Any] = ["offsets": line["offsets"] ?? NSNull(), "out": out, "truth": truth]
        return (ours, expected)
    }

    static func json(_ keys: TypingDecoder.Keys) -> [String: Any] {
        [
            "centres": keys.centres.sorted { $0.key < $1.key }.map { [String($0.key)] + bits($0.value) },
            "pitch": [bits(Double(keys.pitch.width)), bits(Double(keys.pitch.height))],
            "bottom": bits(Double(keys.bottomRowY)),
            "space": keys.space.map { [$0.origin.x, $0.origin.y, $0.size.width, $0.size.height].map { bits(Double($0)) } } ?? NSNull(),
        ]
    }

    static func json(_ d: TypingDecoder.Decision?) -> Any {
        guard let d else { return NSNull() }
        return ["replacement": d.replacement ?? NSNull(), "readings": d.readings.map { [$0.text, bits($0.cost)] }, "literal": bits(d.literalCost)] as [String: Any]
    }

    static func double(_ hex: String) -> Double { Double(bitPattern: UInt64(hex, radix: 16) ?? 0) }

    static func points(_ value: Any?) -> [CGPoint] {
        ((value as? [[String]]) ?? []).map { CGPoint(x: double($0[0]), y: double($0[1])) }
    }

    private static func run(_ kind: String, _ line: [String: Any], _ d: TypingDecoder, _ keys: (keys: TypingDecoder.Keys, truth: TypingDecoder.Keys)) throws -> Any {
        let previous = line["previous"] as? String
        switch kind {
        case "decide":
            guard let typed = line["typed"] as? String, let valid = line["valid"] as? Bool else { throw Failure.malformed("decide") }
            return json(d.decide(taps: points(line["taps"]), typed: typed, previous: previous, literalIsValid: valid, keys: keys.keys))
        case "merge":
            guard let first = line["first"] as? String, let second = line["second"] as? String, let space = line["space"] as? [String],
                  let firstValid = line["firstValid"] as? Bool, let secondValid = line["secondValid"] as? Bool else { throw Failure.malformed("merge") }
            let merged = d.merge(
                first: first, firstTaps: points(line["firstTaps"]), space: CGPoint(x: double(space[0]), y: double(space[1])),
                second: second, secondTaps: points(line["secondTaps"]), previous: previous,
                firstIsValid: firstValid, secondIsValid: secondValid, keys: keys.keys
            )
            return merged ?? NSNull()
        case "replaces":
            guard let reading = line["reading"] as? String, let typed = line["typed"] as? String,
                  let valid = line["valid"] as? Bool, let truth = line["truth"] as? Bool else { throw Failure.malformed("replaces") }
            let answer = d.replaces(with: reading, taps: points(line["taps"]), typed: typed, previous: previous, literalIsValid: valid, keys: truth ? keys.truth : keys.keys)
            return answer ?? NSNull()
        default:
            throw Failure.malformed(kind)
        }
    }

    private static func model(_ language: Language, _ line: [String: Any]) -> Any {
        let model = language.model
        let word = line["word"] as? String ?? ""
        let previous = line["previous"] as? String
        let clean = language.clean
        let allowClean = line["clean"] as? Bool ?? false
        let allowing: (String) -> Bool = { allowClean ? !clean.contains($0) : true }
        switch line["fn"] as? String {
        case "cost": return model.cost(of: word, after: previous).map(bits) ?? NSNull()
        case "hasPair": return model.hasPair(word, line["second"] as? String ?? "")
        case "nextWords": return model.nextWords(after: word, allowing: allowing)
        case "completions": return model.completions(of: word, after: previous, allowing: allowing)
        default:
            guard let codes = model.encode(word), let mass = model.continuations(after: codes) else { return NSNull() }
            return mass.enumerated().filter { $0.element != 0 }.map { [$0.offset, bits($0.element)] as [Any] }
        }
    }

    /// Whether two answers in the fixture's form are the same, as JSON.
    static func same(_ a: Any, _ b: Any) -> Bool {
        let options: JSONSerialization.WritingOptions = [.sortedKeys, .withoutEscapingSlashes, .fragmentsAllowed]
        guard let x = try? JSONSerialization.data(withJSONObject: [a], options: options),
              let y = try? JSONSerialization.data(withJSONObject: [b], options: options) else { return false }
        return x == y
    }
}
