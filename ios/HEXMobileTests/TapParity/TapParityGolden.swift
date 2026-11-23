import CoreGraphics
import XCTest
@testable import HEX

/// The shipped tap decoder (`TypingDecoder` and `TypingDecoderModel` over the Rust core)
/// against the committed fixture (`crates/keyboard/core/tests/fixtures/tap-parity.jsonl`,
/// bundled with the tests). Every input comes from the fixture: each language's never-suggest
/// lists and user words, each key set's centres, pitch, bottom row and space bar as recorded,
/// and each case's taps, letters, word before and flags, so a change to the app's key
/// positions (another iOS version's rows) can't move them. Every answer must be the fixture's,
/// costs bit for bit. The Rust test (`tap_parity.rs`) checks the core alone against the same
/// file, the keys made from the recorded layouts included; this checks the path the keyboard
/// ships, through the C interface and the Swift wrappers.
@MainActor
final class TapParityGolden: XCTestCase {
    private struct Language {
        var model: TypingDecoderModel
        var blocked: [Bool: [String]]
        var clean: Set<String>
        var users: [[String]]
        var keys: [Int: (keys: TypingDecoder.Keys, truth: TypingDecoder.Keys)] = [:]
        var decoders: [Int: TypingDecoder] = [:]
    }

    private enum Malformed: Error {
        case line(String)
    }

    func testTheShippedDecoderAnswersAsTheFixture() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "tap-parity", withExtension: "jsonl"), "the fixture is bundled")
        var languages: [String: Language] = [:]
        var cases = 0
        var failures: [String] = []
        for (n, raw) in try String(contentsOf: url, encoding: .utf8).split(separator: "\n").enumerated() {
            let line = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
            let lang = try XCTUnwrap(line["lang"] as? String)
            switch line["kind"] as? String {
            case "setup":
                let model = try XCTUnwrap(TypingDecoderModel.bundled(lang), "the \(lang) model")
                let blocked = try XCTUnwrap(line["blocked"] as? [String: [String]])
                languages[lang] = Language(
                    model: model, blocked: [true: blocked["swearing"] ?? [], false: blocked["clean"] ?? []],
                    clean: Set(blocked["clean"] ?? []), users: try XCTUnwrap(line["users"] as? [[String]])
                )
            case "keys":
                let id = try XCTUnwrap(line["id"] as? Int)
                languages[lang]?.keys[id] = (try Self.keys(line["out"]), try Self.keys(line["truth"]))
            default:
                var language = try XCTUnwrap(languages[lang], "setup first")
                let ours: Any, expected: Any
                if line["kind"] as? String == "model" {
                    (ours, expected) = (Self.model(language, line), line["out"] ?? NSNull())
                } else {
                    let set = try XCTUnwrap(line["keys"] as? Int), user = try XCTUnwrap(line["user"] as? Int)
                    let swearing = try XCTUnwrap(line["swear"] as? Bool)
                    let keys = try XCTUnwrap(language.keys[set])
                    let id = user * 2 + (swearing ? 1 : 0)
                    let decoder = language.decoders[id] ?? {
                        var d = TypingDecoder(model: language.model)
                        d.userWords = TypingDecoder.UserWords(language.users[user], model: language.model)
                        d.neverSuggest = TypingDecoder.NeverSuggest(language.blocked[swearing] ?? [], model: language.model)
                        return d
                    }()
                    language.decoders[id] = decoder
                    languages[lang] = language
                    (ours, expected) = (try Self.run(line, decoder, keys), line["fixed"] ?? NSNull())
                }
                cases += 1
                if !Self.same(ours, expected), failures.count < 10 {
                    failures.append("line \(n + 1) (\(line["kind"] ?? "")): shipped \(ours), fixture \(expected)")
                }
            }
        }
        XCTAssertEqual(cases, 3 * TapParityExport.count, "every case of the fixture's three languages")
        XCTAssert(failures.isEmpty, failures.joined(separator: "\n"))
    }

    private static func double(_ value: Any?) throws -> Double {
        guard let hex = value as? String, let bits = UInt64(hex, radix: 16) else { throw Malformed.line("bits") }
        return Double(bitPattern: bits)
    }

    private static func points(_ value: Any?) throws -> [CGPoint] {
        guard let pairs = value as? [[Any]] else { throw Malformed.line("points") }
        return try pairs.map { CGPoint(x: try double($0[0]), y: try double($0[1])) }
    }

    /// A key set as the fixture records it (`Keys`'s fields).
    private static func keys(_ value: Any?) throws -> TypingDecoder.Keys {
        guard let keys = value as? [String: Any], let centres = keys["centres"] as? [[Any]], let pitch = keys["pitch"] as? [Any] else {
            throw Malformed.line("keys")
        }
        var byLetter: [Character: CGPoint] = [:]
        for centre in centres {
            guard let letter = centre[0] as? String, letter.count == 1 else { throw Malformed.line("letter") }
            byLetter[Character(letter)] = CGPoint(x: try double(centre[1]), y: try double(centre[2]))
        }
        var space: CGRect?
        if let bar = keys["space"] as? [Any] {
            space = CGRect(x: try double(bar[0]), y: try double(bar[1]), width: try double(bar[2]), height: try double(bar[3]))
        }
        return TypingDecoder.Keys(
            centres: byLetter, pitch: CGSize(width: try double(pitch[0]), height: try double(pitch[1])),
            bottomRowY: try double(keys["bottom"]), space: space
        )
    }

    private static func run(_ line: [String: Any], _ d: TypingDecoder, _ keys: (keys: TypingDecoder.Keys, truth: TypingDecoder.Keys)) throws -> Any {
        let previous = line["previous"] as? String
        switch line["kind"] as? String {
        case "decide":
            guard let typed = line["typed"] as? String, let valid = line["valid"] as? Bool else { throw Malformed.line("decide") }
            return TapParityExporter.json(d.decide(taps: try points(line["taps"]), typed: typed, previous: previous, literalIsValid: valid, keys: keys.keys))
        case "merge":
            guard let first = line["first"] as? String, let second = line["second"] as? String, let space = line["space"] as? [Any],
                  let firstValid = line["firstValid"] as? Bool, let secondValid = line["secondValid"] as? Bool else { throw Malformed.line("merge") }
            let merged = d.merge(
                first: first, firstTaps: try points(line["firstTaps"]), space: CGPoint(x: try double(space[0]), y: try double(space[1])),
                second: second, secondTaps: try points(line["secondTaps"]), previous: previous,
                firstIsValid: firstValid, secondIsValid: secondValid, keys: keys.keys
            )
            return merged ?? NSNull()
        case "replaces":
            guard let reading = line["reading"] as? String, let typed = line["typed"] as? String,
                  let valid = line["valid"] as? Bool, let truth = line["truth"] as? Bool else { throw Malformed.line("replaces") }
            let answer = d.replaces(
                with: reading, taps: try points(line["taps"]), typed: typed, previous: previous, literalIsValid: valid,
                keys: truth ? keys.truth : keys.keys
            )
            return answer ?? NSNull()
        default:
            throw Malformed.line("kind")
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
    private static func same(_ a: Any, _ b: Any) -> Bool {
        let options: JSONSerialization.WritingOptions = [.sortedKeys, .withoutEscapingSlashes, .fragmentsAllowed]
        guard let x = try? JSONSerialization.data(withJSONObject: [a], options: options),
              let y = try? JSONSerialization.data(withJSONObject: [b], options: options) else { return false }
        return x == y
    }
}
