import CoreGraphics
import CryptoKit
import XCTest
@testable import HEX

/// The app's swipe decoder (`SwipeDecoder`, the Rust core) still reads the committed parity
/// fixture's swipes as recorded (`crates/keyboard/core/tests/fixtures/swipe-parity.jsonl`,
/// bundled with the tests). Every input comes from the fixture, as in the Rust test: each
/// context's never-suggest and extra words, each key set's centres, pitch and split join, and
/// each case's touches, times, word before and limit (its inputs hash as recorded, so they're
/// rebuilt exactly), so a change to the app's key positions can't move them. Every reading must
/// have the fixture's candidates, order, cost bits and `scored`. The Rust test checks the core
/// alone against the same file; this checks the path the keyboard ships, through the C
/// interface.
@MainActor
final class SwipeFixtureTests: XCTestCase {
    private struct KeySet {
        var keys: TypingDecoder.Keys
        /// The split layout's left edge, right edge and gap (`KeyboardLayout.joinedForSwipe`).
        var join: (left: Double, right: Double, gap: Double)?
    }

    func testTheShippedDecoderReadsTheFixtureAsRecorded() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "swipe-parity", withExtension: "jsonl"))
        let text = try String(contentsOf: url, encoding: .utf8)
        var models: [String: TypingDecoderModel] = [:]
        var decoders: [Int: SwipeDecoder] = [:]
        var keySets: [Int: KeySet] = [:]
        var cases = 0
        var differences: [String] = []
        for line in text.split(separator: "\n") {
            let object = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
            switch object["kind"] as? String {
            case "header":
                for (language, digest) in try XCTUnwrap(object["models"] as? [String: String]) {
                    let model = try XCTUnwrap(TypingDecoderModel.bundled(language), "the \(language) model")
                    let bytes = try Data(contentsOf: model.url)
                    let ours = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
                    XCTAssertEqual(ours, digest, "the fixture was written for another \(language) model: export it again")
                    models[language] = model
                }
            case "context":
                let model = try XCTUnwrap(models[try XCTUnwrap(object["language"] as? String)])
                var decoder = SwipeDecoder(model: model)
                decoder.neverSuggest = TypingDecoder.NeverSuggest(try XCTUnwrap(object["blocked"] as? [String]), model: model)
                var extra = SwipeDecoder.ExtraWords()
                for word in try XCTUnwrap(object["extra"] as? [[Any]]) {
                    let cost = try Self.bits(word[1])
                    extra.add([try XCTUnwrap(word[0] as? String)], cost: { _ in cost }, model: model, contractions: try XCTUnwrap(word[2] as? Bool))
                }
                decoder.extraWords = extra
                decoders[try XCTUnwrap(object["id"] as? Int)] = decoder
            case "keys":
                var centres: [Character: CGPoint] = [:]
                for centre in try XCTUnwrap(object["centres"] as? [[Any]]) {
                    let letter = try XCTUnwrap(centre[0] as? String)
                    XCTAssertEqual(letter.count, 1, "one letter a key")
                    centres[Character(letter)] = CGPoint(x: try Self.decimal(centre[1]), y: try Self.decimal(centre[2]))
                }
                let pitch = try XCTUnwrap(object["pitch"] as? [Any])
                let join = try (object["join"] as? [Any]).map { (left: try Self.decimal($0[0]), right: try Self.decimal($0[1]), gap: try Self.decimal($0[2])) }
                keySets[try XCTUnwrap(object["id"] as? Int)] = KeySet(
                    keys: TypingDecoder.Keys(centres: centres, pitch: CGSize(width: try Self.decimal(pitch[0]), height: try Self.decimal(pitch[1])), bottomRowY: 0),
                    join: join
                )
            case "case":
                let id = try XCTUnwrap(object["id"] as? Int)
                let decoder = try XCTUnwrap(decoders[try XCTUnwrap(object["context"] as? Int)])
                let keySet = try XCTUnwrap(keySets[try XCTUnwrap(object["keys"] as? Int)])
                let points = Self.screen(try XCTUnwrap(object["path"] as? String)).map { Self.joined($0, keySet.join) }
                var times: [TimeInterval]?
                if let ticks = object["ticks"] as? String {
                    let base = try Self.decimal(object["t0"] as Any)
                    var tick = 0
                    // `SwipeParity.Case.times`.
                    times = Self.unpack(ticks).map { step in
                        tick += step
                        return base + Double(tick) * 0.001
                    }
                }
                XCTAssertEqual(Self.inputs(points, times), object["inputs"] as? String, "case \(id): the path and times rebuilt from the fixture aren't the fixture's")
                let limit = try XCTUnwrap(object["limit"] as? Int)
                let reading = decoder.read(points, times: times, keys: keySet.keys, previous: object["previous"] as? String, limit: limit)
                let got = reading.candidates.map { "\($0.text) \(String(format: "%016llx", $0.cost.bitPattern))" }
                let want = try XCTUnwrap(object["out"] as? [[String]]).map { "\($0[0]) \($0[1])" }
                let scored = try XCTUnwrap(object["scored"] as? Int)
                if got != want || reading.scored != scored {
                    differences.append("case \(id): fixture \(want) of \(scored), read \(got) of \(reading.scored)")
                }
                cases += 1
            default:
                XCTFail("an unknown line in the fixture: \(line.prefix(40))")
            }
        }
        print("swipe fixture: \(cases) cases, \(differences.count) differences")
        XCTAssertGreaterThan(cases, 0, "the fixture has no cases")
        XCTAssertEqual(differences, [], "the decoder no longer reads the fixture as recorded")
    }

    /// Swift's shortest round-trip decimal, which `Double(_:)` reads back exactly.
    private static func decimal(_ value: Any) throws -> Double {
        try XCTUnwrap((value as? String).flatMap(Double.init), "a decimal: \(value)")
    }

    /// An IEEE 754 bit pattern in 16 hex digits.
    private static func bits(_ value: Any) throws -> Double {
        Double(bitPattern: try XCTUnwrap((value as? String).flatMap { UInt64($0, radix: 16) }, "bits: \(value)"))
    }

    /// The touches, from whole quarter points packed as steps (`SwipeParityExportTests.path`).
    private static func screen(_ packed: String) -> [CGPoint] {
        let steps = unpack(packed)
        var x = 0, y = 0
        return stride(from: 0, to: steps.count - 1, by: 2).map { i in
            x += steps[i]
            y += steps[i + 1]
            return CGPoint(x: Double(x) / 4, y: Double(y) / 4)
        }
    }

    /// `KeyboardLayout.joinedForSwipe`, with the split's edges and gap from the fixture.
    private static func joined(_ point: CGPoint, _ join: (left: Double, right: Double, gap: Double)?) -> CGPoint {
        guard let join else { return point }
        let x = Double(point.x)
        if x <= join.left { return point }
        if x >= join.right { return CGPoint(x: x - (join.right - join.left - join.gap), y: point.y) }
        let across = (x - join.left) / max(1, join.right - join.left)
        return CGPoint(x: join.left + across * join.gap, y: point.y)
    }

    /// `JSONLines.packed`: base64 of zigzag LEB128 numbers.
    private static func unpack(_ packed: String) -> [Int] {
        var text = packed
        while text.count % 4 != 0 { text += "=" }
        var out: [Int] = []
        var value: UInt64 = 0, shift: UInt64 = 0
        for byte in Data(base64Encoded: text) ?? Data() {
            value |= UInt64(byte & 0x7F) << shift
            shift += 7
            if byte & 0x80 == 0 {
                out.append(Int(value >> 1) ^ -Int(value & 1))
                value = 0
                shift = 0
            }
        }
        return out
    }

    /// `SwipeParityExportTests.inputs`: FNV-1a (64-bit) of the points' and times' bits.
    private static func inputs(_ points: [CGPoint], _ times: [TimeInterval]?) -> String {
        var hash: UInt64 = 0xCBF2_9CE4_8422_2325
        func add(_ value: Double) {
            var bits = value.bitPattern
            for _ in 0..<8 {
                hash = (hash ^ (bits & 0xFF)) &* 0x0000_0100_0000_01B3
                bits >>= 8
            }
        }
        for point in points {
            add(Double(point.x))
            add(Double(point.y))
        }
        for time in times ?? [] { add(time) }
        return String(format: "%016llx", hash)
    }
}
