import Foundation
import XCTest
@testable import HEX

/// The oracle copies of the Swift rules and the shipped wrappers over the keyboard core, case
/// for case in one process: the test strings and generated transcripts the exporter makes.
/// `TEST_RUNNER_HEX_DICTATION_INPROCESS_COUNT` sets the generated count (from 100,000 the test
/// strings run through everything too); `_SEED` picks other transcripts.
final class OracleWrapperParityTests: XCTestCase {
    func testTheWrappersGiveWhatTheSwiftRulesGave() {
        let environment = ProcessInfo.processInfo.environment
        let count = environment["HEX_DICTATION_INPROCESS_COUNT"].flatMap(Int.init) ?? 1_500
        let seed = environment["HEX_DICTATION_INPROCESS_SEED"].flatMap(UInt64.init)
        final class Tally {
            var cases = 0
            var differences: [String] = []
        }
        let tally = Tally()
        ParityCases.run(count: count, seed: seed) { record in
            guard let shipped = ShippedDictation.run(record) else { return }
            tally.cases += 1
            if !ShippedDictation.same(shipped, record["out"] as Any), tally.differences.count < 20 {
                tally.differences.append("\(record)\n  wrapper gives \(shipped)")
            }
        }
        print("dictation in-process parity: \(tally.cases) cases, \(tally.differences.count) differences")
        XCTAssertGreaterThan(tally.cases, 1_000)
        XCTAssertTrue(tally.differences.isEmpty, tally.differences.joined(separator: "\n"))
    }
}
