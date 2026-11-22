import XCTest
@testable import HEX

/// The shipped tap decoder (`TypingDecoder` over the Rust core) against the Swift oracle it
/// replaced, in one process: every generated case through both. `HEX_TAP_PARITY_COUNT` cases
/// per language (`TEST_RUNNER_HEX_TAP_PARITY_COUNT` for xcodebuild), 2,000 by default.
@MainActor
final class TapParityInProcess: XCTestCase {
    func testTheShippedDecoderMatchesTheOracle() throws {
        let count = ProcessInfo.processInfo.environment["HEX_TAP_PARITY_COUNT"].flatMap(Int.init) ?? 2_000
        var report: [String] = []
        var failures: [String] = []
        for (n, language) in [KeyboardLanguage.english, .spanish, .german].enumerated() {
            var exporter = try TapParityExporter(language: language, seed: 0x5EED_7A90 &+ UInt64(n))
            let replay = TapParityReplay()
            var cases = 0, exact = 0, tie = 0, swiftDiffers = 0
            var decides = 0, tieDecides = 0, tieReplacement = 0, tieStrip = 0
            var nanos: [UInt64] = []
            try exporter.run(count: count) { line in
                let start = clock_gettime_nsec_np(CLOCK_UPTIME_RAW)
                guard let (ours, expected) = try replay.check(line) else { return }
                if line["kind"] as? String == "decide" {
                    nanos.append(clock_gettime_nsec_np(CLOCK_UPTIME_RAW) - start)
                    decides += 1
                }
                cases += 1
                let isTie = line["tie"] as? Bool ?? false
                if isTie { tie += 1 }
                if line["swift"] != nil { swiftDiffers += 1 }
                if isTie, line["kind"] as? String == "decide" {
                    tieDecides += 1
                    if line["tieChangesReplacement"] as? Bool ?? false { tieReplacement += 1 } else { tieStrip += 1 }
                }
                if TapParityReplay.same(ours, expected) {
                    exact += 1
                } else if failures.count < 20 {
                    failures.append("\(language.rawValue) \(line["kind"] ?? "") ours \(ours) oracle \(expected) typed \(line["typed"] ?? "")")
                }
            }
            nanos.sort()
            let median = nanos.isEmpty ? 0 : Double(nanos[nanos.count / 2]) / 1000
            let p95 = nanos.isEmpty ? 0 : Double(nanos[nanos.count * 95 / 100]) / 1000
            report.append(String(
                format: "TAP-PARITY-INPROCESS %@ cases=%d exact=%d other-diffs=%d tie=%d swift-differs=%d decides=%d tie-decides=%d (%.2f%%) tie-changes-replacement=%d tie-strip-only=%d shipped-decide-us median=%.1f p95=%.1f",
                language.rawValue, cases, exact, cases - exact, tie, swiftDiffers, decides, tieDecides, 100 * Double(tieDecides) / Double(max(1, decides)), tieReplacement, tieStrip, median, p95
            ))
            XCTAssertEqual(exact, cases, "\(language.rawValue): the shipped decoder differs from the oracle")
        }
        print(report.joined(separator: "\n"))
        for failure in failures { print("TAP-PARITY-FAILURE \(failure)") }
    }
}
