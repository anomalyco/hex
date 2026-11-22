import CoreGraphics
import Foundation
import XCTest
@testable import HEX

/// Writes the tap decoder's parity fixture (`crates/keyboard/core/tests/fixtures/
/// tap-parity.jsonl`, which `tap_parity.rs` and `TapParityGolden` check): deterministic cases
/// (SplitMix64, a fixed seed) for English, Spanish and German, each with its exact inputs and
/// the shipped decoder's outputs (`TapParityCases`), floating-point values as their bit
/// patterns. Opt-in: it runs only when `HEX_TAP_PARITY_OUT` names the file to write
/// (`TEST_RUNNER_HEX_TAP_PARITY_OUT` for xcodebuild), with `HEX_TAP_PARITY_COUNT` cases per
/// language (400, the committed fixture's count, by default).
///
/// The committed fixture was written by the Swift decoder this replaced (dev fc867484, run
/// in the Rust port's fixed tie order), so it holds that decoder's answers; its `tie`,
/// `swift` and `tieChangesReplacement` fields record where Swift's own dictionary order could
/// have answered otherwise. Writing it again from the shipped decoder is for a deliberate
/// change to the decoder: review the difference first.
///
/// Lines: `setup` (a language's never-suggest lists and the user word sets), `keys` (a
/// letters page as the host passes it, Learn how I type stats, the offsets and `Keys`), then
/// `decide`, `merge`, `replaces` and `model` cases.
@MainActor
final class TapParityExport: XCTestCase {
    /// Cases per language in the committed fixture.
    static let count = 400
    static let seeds: [(KeyboardLanguage, UInt64)] = [(.english, 0x5EED_7A90), (.spanish, 0x5EED_7A91), (.german, 0x5EED_7A92)]

    func testExport() throws {
        let environment = ProcessInfo.processInfo.environment
        guard let out = environment["HEX_TAP_PARITY_OUT"] else {
            throw XCTSkip("set TEST_RUNNER_HEX_TAP_PARITY_OUT to write the tap parity fixture")
        }
        let count = environment["HEX_TAP_PARITY_COUNT"].flatMap(Int.init) ?? TapParityExport.count
        FileManager.default.createFile(atPath: out, contents: nil)
        let handle = try XCTUnwrap(FileHandle(forWritingAtPath: out))
        defer { try? handle.close() }
        var writer = TapParityWriter(handle: handle)
        for (language, seed) in Self.seeds {
            var exporter = try TapParityExporter(language: language, seed: seed)
            try exporter.run(count: count, into: &writer)
            print("TAP-PARITY \(language.rawValue) \(exporter.summary)")
        }
        try writer.flush()
    }
}

/// Buffered JSON Lines.
struct TapParityWriter {
    let handle: FileHandle
    var buffer = Data()

    mutating func write(_ object: [String: Any]) throws {
        buffer.append(try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys, .withoutEscapingSlashes]))
        buffer.append(0x0A)
        if buffer.count > 1 << 20 { try flush() }
    }

    mutating func flush() throws {
        try handle.write(contentsOf: buffer)
        buffer.removeAll(keepingCapacity: true)
    }
}

/// SplitMix64, the tap cases' own (the swipe cases have theirs, with another normal): the
/// generator's only source of variety, so a seed is a fixture.
struct TapParityRandom {
    var state: UInt64

    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }

    /// In 0..<1.
    mutating func uniform() -> Double { Double(next() >> 11) * 0x1p-53 }
    mutating func int(_ n: Int) -> Int { Int(next() % UInt64(n)) }
    mutating func chance(_ p: Double) -> Bool { uniform() < p }
    mutating func pick<T>(_ items: [T]) -> T { items[int(items.count)] }

    /// A standard normal (Box–Muller).
    mutating func gaussian() -> Double {
        let u = max(uniform(), 0x1p-53)
        return (-2 * log(u)).squareRoot() * cos(2 * .pi * uniform())
    }
}

/// The bits of a value, as the fixture writes it.
func bits(_ x: Double) -> String { String(x.bitPattern, radix: 16) }
func bits(_ p: CGPoint) -> [String] { [bits(Double(p.x)), bits(Double(p.y))] }
