import Foundation
import XCTest
@testable import HEX

/// A parity case through the shipped Swift API (`TranscriptFormatter`, `SpokenLists`, …, now
/// wrappers over the keyboard core): what it gives, or nil for a case of Swift's own
/// `Character` and `folding` (the core's Unicode tests check those).
enum ShippedDictation {
    static func run(_ record: [String: Any]) -> Any? {
        let text = record["text"] as? String ?? ""
        let language = record["lang"] as? String
        switch record["op"] as? String {
        case "format":
            guard let style = (record["style"] as? String).flatMap(DictationStyle.init(rawValue:)) else { return nil }
            return TranscriptFormatter.format(
                text, style: style, language: language, lists: record["lists"] as? Bool ?? false,
                punctuation: record["punct"] as? Bool ?? false, endsWithPeriod: record["period"] as? Bool ?? true
            )
        case "spoken_punctuation": return SpokenPunctuation.format(text, language: language)
        case "spoken_lists": return SpokenLists.format(text, language: language)
        case "cleanup_rules": return SpokenCleanupRules.clean(text, language: language)
        case "spoken_symbols": return SpokenCleanupRules.spokenSymbols(in: text, language: language)
        case "spoken_counts": return SpokenCleanupRules.spokenCounts(in: text, language: language)
        case "cleanup_chain":
            let symbols = SpokenCleanupRules.spokenSymbols(in: text, language: language)
            let counted = SpokenCleanupRules.spokenCounts(in: symbols, language: language)
            return SpokenCleanupRules.clean(counted, language: language)
        case "french_spacing": return FrenchSpacing.apply(text, language: language)
        case "reconstruct":
            return CleanupReconstruction.reconstruct(record["words"] as? [String] ?? [],
                                                     deletes: record["deletes"] as? [Bool] ?? [])
        case "insertion":
            let words = (record["words"] as? [String]).map(Set.init)
            return TranscriptFormatter.insertionText(
                for: text, beforeContext: record["before"] as? String, afterContext: record["after"] as? String,
                isWord: words.map { words in { words.contains($0) } }
            )
        case "starts_sentence": return TranscriptFormatter.startsSentence(after: text)
        case "dropping_final_full_stop": return TranscriptFormatter.droppingFinalFullStop(text)
        case "continuing_sentence":
            let words = Set(record["words"] as? [String] ?? [])
            return TranscriptFormatter.continuingSentence(text) { words.contains($0) }
        case "words": return CleanupTagger.words(in: text)
        case "lowercases_continuations":
            guard let language else { return NSNull() }
            return TranscriptFormatter.lowercasesContinuations(in: language)
        case "supports": return SpokenCleanupRules.supports(language)
        case "filler_pattern": return TranscriptFormatter.fillerPattern(for: language) ?? NSNull()
        default: return nil
        }
    }

    /// Whether two outputs are the same, scalar for scalar (not Swift's canonical `==`).
    static func same(_ a: Any, _ b: Any) -> Bool {
        switch (a, b) {
        case let (a as String, b as String): return a.unicodeScalars.elementsEqual(b.unicodeScalars)
        case let (a as [String], b as [String]): return a.count == b.count && zip(a, b).allSatisfy { same($0, $1) }
        case let (a as Bool, b as Bool): return a == b
        case (is NSNull, is NSNull): return true
        default: return false
        }
    }

    /// The committed fixture the Rust core's tests also read.
    static let fixture = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("../crates/keyboard/core/tests/fixtures/dictation-parity.jsonl").standardizedFileURL
}

/// The shipped dictation rules give the committed fixture's outputs, case for case: the
/// golden check of what iOS runs (the Rust core, through `DictationCore`).
final class DictationGoldenTests: XCTestCase {
    func testTheShippedRulesGiveTheFixture() throws {
        let lines = try String(contentsOf: ShippedDictation.fixture, encoding: .utf8).split(separator: "\n")
        var cases = 0
        var differences: [String] = []
        for line in lines {
            let record = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
            guard let got = ShippedDictation.run(record) else { continue }
            cases += 1
            if !ShippedDictation.same(got, record["out"] as Any) {
                differences.append("\(line)\n  gives \(got)")
            }
        }
        XCTAssertGreaterThan(cases, 3_000)
        XCTAssertTrue(differences.isEmpty, "\(differences.count) of \(cases) differ:\n"
            + differences.prefix(10).joined(separator: "\n"))
    }
}
