import CoreGraphics
import Foundation
import XCTest
@testable import HEX

/// Writes the tap decoder's parity fixture for the Rust port (`crates/keyboard/core/tests/
/// tap_parity.rs`): deterministic cases (SplitMix64, a fixed seed) for English, Spanish and
/// German, each with its exact inputs and the oracle's outputs (`TypingDecoderOracle.swift`),
/// floating-point values as their bit patterns. Opt-in: it runs only when
/// `HEX_TAP_PARITY_OUT` names the file to write (`TEST_RUNNER_HEX_TAP_PARITY_OUT` for
/// xcodebuild), with `HEX_TAP_PARITY_COUNT` cases per language (300 by default).
///
/// Every case that reaches a dictionary walk whose order decides exact cost ties runs three
/// times: in the Rust port's fixed order (`fixed`, what Rust must match), the reverse of it,
/// and Swift's own dictionary order (`swift`, written only where it differs from `fixed`).
/// `tie` marks a case whose result depends on that order: Swift's is then one of several.
///
/// Lines: `setup` (a language's never-suggest lists and the user word sets), `keys` (a
/// letters page as the host passes it, Learn how I type stats, and the oracle's offsets and
/// `Keys`), then `decide`, `merge`, `replaces` and `model` cases.
@MainActor
final class TapParityExport: XCTestCase {
    func testExport() throws {
        let environment = ProcessInfo.processInfo.environment
        guard let out = environment["HEX_TAP_PARITY_OUT"] else {
            throw XCTSkip("set TEST_RUNNER_HEX_TAP_PARITY_OUT to write the tap parity fixture")
        }
        let count = environment["HEX_TAP_PARITY_COUNT"].flatMap(Int.init) ?? 300
        FileManager.default.createFile(atPath: out, contents: nil)
        let handle = try XCTUnwrap(FileHandle(forWritingAtPath: out))
        defer { try? handle.close() }
        var writer = TapParityWriter(handle: handle)
        for (n, language) in [KeyboardLanguage.english, .spanish, .german].enumerated() {
            var exporter = try TapParityExporter(language: language, seed: 0x5EED_7A90 &+ UInt64(n))
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
