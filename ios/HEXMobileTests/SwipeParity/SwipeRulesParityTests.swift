import XCTest
@testable import HEX

/// Swipe typing's rules in the keyboard core type every `SwipeRules` case as the Swift rules
/// they replaced (`SwipeRulesOracle`) did, in one process: the same space, word and
/// alternatives, the same order of readings, the same names, spaces and slid taps. 20,000
/// cases by default, or `TEST_RUNNER_HEX_SWIPE_RULES_COUNT`.
@MainActor
final class SwipeRulesParityTests: XCTestCase {
    func testTheCoreTypesSwipesAsTheSwiftRules() throws {
        let count = Int(ProcessInfo.processInfo.environment["HEX_SWIPE_RULES_COUNT"] ?? "") ?? 20_000
        let suite = "SwipeRulesParityTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let runner = try SwipeRulesRunner(defaults: defaults)
        var generator = SwipeRules.Generator()
        var kinds: [String: Int] = [:]
        var differences: [String] = []
        for i in 0..<count {
            let c = generator.next()
            let kind = "\(c)".prefix { $0 != "(" }
            kinds[String(kind), default: 0] += 1
            let swift = runner.swift(c), core = runner.core(c)
            if swift != core { differences.append("case \(i) \(c): swift \(swift.debugDescription), core \(core.debugDescription)") }
        }
        print("swipe rules in process: \(count) cases \(kinds.sorted { $0.key < $1.key }), \(differences.count) differences")
        for difference in differences.prefix(20) { print("  \(difference)") }
        XCTAssertEqual(differences.count, 0, differences.prefix(5).joined(separator: "\n"))
    }
}
