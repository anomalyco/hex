import XCTest
@testable import HEX

/// The shipped tap decoder (`TypingDecoder` and `TypingDecoderModel` over the Rust core)
/// against the committed fixture (`crates/keyboard/core/tests/fixtures/tap-parity.jsonl`,
/// bundled with the tests): the cases are generated again with the fixture's seeds and every
/// line, inputs and answers, must be the fixture's. The Rust core checks the same file
/// (`tap_parity.rs`); this checks the path iOS ships, through the C interface and the Swift
/// wrappers.
@MainActor
final class TapParityGolden: XCTestCase {
    func testTheShippedDecoderAnswersAsTheFixture() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "tap-parity", withExtension: "jsonl"), "the fixture is bundled")
        let fixture = try String(contentsOf: url, encoding: .utf8).split(separator: "\n").map {
            try XCTUnwrap(JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any])
        }
        var expected = fixture.makeIterator()
        var lines = 0, failures: [String] = []
        for (language, seed) in TapParityExport.seeds {
            var cases = try TapParityExporter(language: language, seed: seed)
            try cases.run(count: TapParityExport.count) { line in
                lines += 1
                guard let want = expected.next() else { failures.append("the fixture ends early"); return }
                if !Self.same(line, want), failures.count < 10 {
                    failures.append("line \(lines): shipped \(Self.text(line))\nfixture \(Self.text(want))")
                }
            }
        }
        XCTAssertEqual(lines, fixture.count, "every fixture line generated")
        XCTAssert(failures.isEmpty, failures.joined(separator: "\n"))
    }

    /// The fixture's notes on Swift's own tie order, which the shipped decoder doesn't have.
    private static let notes: Set<String> = ["tie", "swift", "tieChangesReplacement"]

    private static func same(_ line: [String: Any], _ want: [String: Any]) -> Bool {
        text(line) == text(want.filter { !notes.contains($0.key) })
    }

    private static func text(_ line: [String: Any]) -> String {
        let data = try? JSONSerialization.data(withJSONObject: line, options: [.sortedKeys, .withoutEscapingSlashes])
        return data.map { String(decoding: $0, as: UTF8.self) } ?? ""
    }
}
