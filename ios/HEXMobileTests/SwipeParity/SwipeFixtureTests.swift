import CoreGraphics
import XCTest
@testable import HEX

/// The app's swipe decoder (`SwipeDecoder`, the Rust core) still reads the committed parity
/// fixture's swipes as recorded (`crates/keyboard/core/tests/fixtures/swipe-parity.jsonl`,
/// bundled with the tests): `SwipeParity` makes the same cases again (each one's inputs hash
/// as the fixture's do) and every reading has the fixture's candidates, order, cost bits and
/// `scored`. The Rust test checks the core alone against the same file; this checks the path
/// the keyboard ships, through the C interface.
@MainActor
final class SwipeFixtureTests: XCTestCase {
    private struct Expected {
        var inputs: String
        var scored: Int
        var candidates: [String]
    }

    func testTheShippedDecoderReadsTheFixtureAsRecorded() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "swipe-parity", withExtension: "jsonl"))
        let text = try String(contentsOf: url, encoding: .utf8)
        var count = 0
        var expected: [Int: Expected] = [:]
        for line in text.split(separator: "\n") {
            let object = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
            switch object["kind"] as? String {
            case "header":
                XCTAssertEqual(object["generator"] as? Int, SwipeParity.generatorVersion, "the fixture is this generator's")
                count = try XCTUnwrap(object["count"] as? Int)
            case "case":
                let out = try XCTUnwrap(object["out"] as? [[Any]])
                expected[try XCTUnwrap(object["id"] as? Int)] = Expected(
                    inputs: try XCTUnwrap(object["inputs"] as? String),
                    scored: try XCTUnwrap(object["scored"] as? Int),
                    candidates: out.map { "\($0[0]) \($0[1])" }
                )
            default:
                break
            }
        }
        XCTAssertEqual(expected.count, count * SwipeParity.languages.count)

        var differences: [String] = []
        var cases = 0, contexts = 0, layouts = 0
        for language in SwipeParity.languages {
            let setup = try SwipeParity.Setup(language, firstContext: contexts, firstLayout: layouts)
            contexts += setup.contexts.count
            layouts += setup.layouts.count
            let decoders = Dictionary(uniqueKeysWithValues: setup.contexts.map { ($0.id, SwipeParityExportTests.decoder($0, model: setup.model)) })
            let keys = Dictionary(uniqueKeysWithValues: setup.layouts.map { ($0.id, $0.keys) })
            var generator = SwipeParityGenerator(setup, firstID: cases)
            for _ in 0..<count {
                let c = generator.next()
                let want = try XCTUnwrap(expected[c.id], "case \(c.id)")
                XCTAssertEqual(SwipeParityExportTests.inputs(c), "\"\(want.inputs)\"", "case \(c.id) isn't the fixture's swipe")
                let reading = decoders[c.context]!.read(c.points, times: c.times, keys: keys[c.layout]!, previous: c.previous, limit: c.limit)
                let got = reading.candidates.map { "\($0.text) \(String(format: "%016llx", $0.cost.bitPattern))" }
                if got != want.candidates || reading.scored != want.scored {
                    differences.append("case \(c.id): fixture \(want.candidates) of \(want.scored), read \(got) of \(reading.scored)")
                }
            }
            cases += count
        }
        print("swipe fixture: \(cases) cases, \(differences.count) differences")
        XCTAssertEqual(differences, [], "the decoder no longer reads the fixture as recorded")
    }
}
