import CoreGraphics
import XCTest
@testable import HEX

/// The app's swipe decoder (`SwipeDecoder`, the Rust core through `hk_swipe_*`) reads every
/// case of `SwipeParity` exactly as the Swift decoder it replaced (`SwipeDecoderOracle`), in
/// one process: the same candidates in the same order, the same `scored`, and costs equal bit
/// for bit. 300 cases per language by default (the oracle is slow unoptimised), or
/// `TEST_RUNNER_HEX_SWIPE_PARITY_COUNT`.
@MainActor
final class SwipeOracleParityTests: XCTestCase {
    func testTheCoreReadsEverySwipeAsTheSwiftDecoder() throws {
        let count = Int(ProcessInfo.processInfo.environment["HEX_SWIPE_PARITY_COUNT"] ?? "") ?? 300
        var differences: [String] = []
        var oracleTimes: [UInt64] = [], coreTimes: [UInt64] = []
        var cases = 0, contexts = 0, layouts = 0
        for language in SwipeParity.languages {
            let setup = try SwipeParity.Setup(language, firstContext: contexts, firstLayout: layouts)
            contexts += setup.contexts.count
            layouts += setup.layouts.count
            let oracles = Dictionary(uniqueKeysWithValues: setup.contexts.map { ($0.id, SwipeParityExportTests.oracle($0, model: setup.model)) })
            let decoders = Dictionary(uniqueKeysWithValues: setup.contexts.map { ($0.id, SwipeParityExportTests.decoder($0, model: setup.model)) })
            let keys = Dictionary(uniqueKeysWithValues: setup.layouts.map { ($0.id, $0.keys) })
            var generator = SwipeParityGenerator(setup, firstID: cases)
            for _ in 0..<count {
                let c = generator.next()
                let k = keys[c.layout]!, times = c.times
                var started = DispatchTime.now().uptimeNanoseconds
                let expected = oracles[c.context]!.read(c.points, times: times, keys: k, previous: c.previous, limit: c.limit)
                oracleTimes.append(DispatchTime.now().uptimeNanoseconds - started)
                started = DispatchTime.now().uptimeNanoseconds
                let got = decoders[c.context]!.read(c.points, times: times, keys: k, previous: c.previous, limit: c.limit)
                coreTimes.append(DispatchTime.now().uptimeNanoseconds - started)
                let a = expected.candidates.map { "\($0.text) \(String($0.cost.bitPattern, radix: 16))" }
                let b = got.candidates.map { "\($0.text) \(String($0.cost.bitPattern, radix: 16))" }
                if a != b || expected.scored != got.scored {
                    differences.append("case \(c.id) (\(c.kind.rawValue) \(c.word)): swift \(a) of \(expected.scored), core \(b) of \(got.scored)")
                }
            }
            cases += count
        }
        func timing(_ ns: [UInt64]) -> String {
            let sorted = ns.sorted()
            return "median \(sorted[sorted.count / 2] / 1000) µs, p95 \(sorted[sorted.count * 95 / 100] / 1000) µs"
        }
        print("swipe parity in process: \(cases) cases, \(differences.count) differences; swift \(timing(oracleTimes)); core \(timing(coreTimes))")
        for d in differences.prefix(20) { print("  \(d)") }
        XCTAssertEqual(differences.count, 0, differences.prefix(5).joined(separator: "\n"))
    }
}
