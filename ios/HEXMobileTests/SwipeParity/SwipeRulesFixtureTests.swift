import CoreGraphics
import XCTest
@testable import HEX

/// The keyboard core types the committed fixture's swipe-rules cases as recorded
/// (`crates/keyboard/core/tests/fixtures/swipe-rules.jsonl`, bundled with the tests; the Rust
/// test `swipe_rules.rs` reads it too): each profile's words and dictionary, and each case's
/// inputs, come from the fixture, and every outcome must be the recorded one. The fixture's
/// outcomes are the Swift rules' (`SwipeRules`' cases, proven alike in one process before the
/// Swift rules went); export it again only to record an intended change in the core:
///
///     xcodebuild test … -only-testing:HEXMobileTests/SwipeRulesFixtureTests/testExportSwipeRulesFixture \
///       TEST_RUNNER_HEX_SWIPE_RULES_OUT=/tmp/swipe-rules.jsonl TEST_RUNNER_HEX_SWIPE_RULES_COUNT=12000
@MainActor
final class SwipeRulesFixtureTests: XCTestCase {
    func testExportSwipeRulesFixture() throws {
        let env = ProcessInfo.processInfo.environment
        guard let path = env["HEX_SWIPE_RULES_OUT"], !path.isEmpty else {
            throw XCTSkip("set TEST_RUNNER_HEX_SWIPE_RULES_OUT to write the fixture")
        }
        let count = Int(env["HEX_SWIPE_RULES_COUNT"] ?? "") ?? 12_000
        let suite = "SwipeRulesFixtureTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let runner = try SwipeRulesRunner(defaults: defaults)
        var lines: [[String: Any]] = [["kind": "header", "generator": SwipeRules.generatorVersion, "count": count]]
        lines += SwipeRules.profiles.map { runner.record(profile: $0) }
        var generator = SwipeRules.Generator()
        for _ in 0..<count { lines.append(runner.record(generator.next())) }
        var data = Data()
        for line in lines {
            data.append(try JSONSerialization.data(withJSONObject: line, options: [.sortedKeys, .withoutEscapingSlashes]))
            data.append(0x0A)
        }
        try data.write(to: URL(fileURLWithPath: path))
        print("swipe rules: wrote \(count) cases to \(path)")
    }

    func testTheCoreTypesTheFixtureAsRecorded() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "swipe-rules", withExtension: "jsonl"))
        let text = try String(contentsOf: url, encoding: .utf8)
        var words: [Int: SwipeDecoder.Words] = [:], checkers: [Int: SwipeRules.Checker] = [:]
        var decoders: [String: SwipeDecoder] = [:]
        var cases = 0
        var differences: [String] = []
        for line in text.split(separator: "\n") {
            let o = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
            let kind = try XCTUnwrap(o["kind"] as? String)
            func check(_ profile: Int) -> (String) -> Bool { { [checker = checkers[profile]!] in checker.isWordValid($0) } }
            func expect(_ got: Any, _ want: Any?) {
                if "\(got)" != "\(want ?? "nil")" { differences.append("\(line.prefix(160)): read \(got)") }
            }
            switch kind {
            case "header":
                XCTAssertEqual(o["generator"] as? Int, SwipeRules.generatorVersion)
                continue
            case "profile":
                let id = try XCTUnwrap(o["id"] as? Int)
                let replacements = try XCTUnwrap(o["replacements"] as? [[String]])
                words[id] = SwipeDecoder.Words(
                    language: try XCTUnwrap(o["language"] as? String), hasDictionary: try XCTUnwrap(o["hasDictionary"] as? Bool),
                    replacements: Dictionary(uniqueKeysWithValues: replacements.map { ($0[0], $0[1]) }),
                    own: try XCTUnwrap(o["own"] as? [String]), vocabulary: try XCTUnwrap(o["vocabulary"] as? [String])
                )
                checkers[id] = SwipeRules.Checker(try XCTUnwrap(o["valid"] as? [String]))
                continue
            case "typed":
                let profile = try XCTUnwrap(o["profile"] as? Int)
                let s = try XCTUnwrap(o["state"] as? [String: Any])
                let state = SwipeDecoder.TypingState(
                    before: s["before"] as? String,
                    shift: [ShiftState.off, .on, .locked][try XCTUnwrap(s["shift"] as? Int)],
                    shiftHeld: try XCTUnwrap(s["shiftHeld"] as? Bool),
                    autoCapitalization: [AutoCapitalization.Mode.none, .words, .sentences, .allCharacters][try XCTUnwrap(s["autoCapitalization"] as? Int)],
                    autoCapitalizes: try XCTUnwrap(s["autoCapitalizes"] as? Bool), autocorrection: try XCTUnwrap(s["autocorrection"] as? Bool)
                )
                let typed = try XCTUnwrap(SwipeDecoder.typed(try XCTUnwrap(o["readings"] as? [String]), in: state, words: try XCTUnwrap(words[profile]), isWordValid: check(profile)))
                let out = try XCTUnwrap(o["out"] as? [String: Any])
                expect([typed.space, typed.word, typed.alternatives] as [Any], [try XCTUnwrap(out["space"] as? Bool), try XCTUnwrap(out["word"] as? String), try XCTUnwrap(out["alternatives"] as? [String])] as [Any])
            case "name":
                let profile = try XCTUnwrap(o["profile"] as? Int)
                expect(SwipeDecoder.readsAsName(try XCTUnwrap(o["word"] as? String), words: try XCTUnwrap(words[profile]), isWordValid: check(profile)), o["out"] as? Bool)
            case "space":
                expect(SwipeDecoder.needsSpace(before: o["before"] as? String), o["out"] as? Bool)
            case "rank":
                let profile = try XCTUnwrap(o["profile"] as? Int), language = try XCTUnwrap(o["language"] as? String)
                if decoders[language] == nil {
                    decoders[language] = SwipeDecoder(model: try XCTUnwrap(TypingDecoderModel.bundled(language)))
                }
                let candidates = try XCTUnwrap(o["candidates"] as? [[String]]).map {
                    SwipeDecoder.Candidate(text: $0[0], cost: Double(bitPattern: UInt64($0[1], radix: 16)!))
                }
                expect(decoders[language]!.ranked(candidates, words: try XCTUnwrap(words[profile]), isWordValid: check(profile)), o["out"] as? [Int])
            case "slid":
                let points = try XCTUnwrap(o["points"] as? [String]).map { pair -> CGPoint in
                    let xy = pair.split(separator: ",").map { Double($0)! }
                    return CGPoint(x: xy[0], y: xy[1])
                }
                let pitch = try XCTUnwrap(Double(try XCTUnwrap(o["pitch"] as? String)))
                expect(SwipeDecoder.isSlidTap(points, pitch: pitch, key: o["key"] as? String, firstReading: o["first"] as? String), o["out"] as? Bool)
            case "ends":
                expect(SwipeDecoder.word(try XCTUnwrap(o["word"] as? String), startsAndEndsWith: try XCTUnwrap(o["letter"] as? String)), o["out"] as? Bool)
            default:
                XCTFail("an unknown line in the fixture: \(line.prefix(40))")
            }
            cases += 1
        }
        print("swipe rules fixture: \(cases) cases, \(differences.count) differences")
        for difference in differences.prefix(10) { print("  \(difference)") }
        XCTAssertGreaterThan(cases, 0)
        XCTAssertEqual(differences, [], "the core no longer types the fixture as recorded")
    }
}
