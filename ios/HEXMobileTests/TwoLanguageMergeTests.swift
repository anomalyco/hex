import Foundation
import XCTest
@testable import HEX

/// Experiments › Two-language dictation's merge: two readings of one recording, the more
/// confident reading for each piece, with a language change costing enough that one doubtful
/// word never flips it.
final class TwoLanguageMergeTests: XCTestCase {
    private func word(
        _ text: String,
        _ start: TimeInterval,
        _ end: TimeInterval,
        _ confidence: Double
    ) -> TwoLanguageTimedWord {
        TwoLanguageTimedWord(text: text, start: start, end: end, confidence: confidence)
    }

    func testEquallyOrLessConfidentSecondaryKeepsThePrimaryReading() {
        let primary = [
            word("hello", 0, 0.5, 0.9),
            word("world", 0.5, 1.0, 0.9),
        ]
        let equal = [
            word("hello", 0, 0.5, 0.9),
            word("world", 0.5, 1.0, 0.9),
        ]
        let quieter = [
            word("hello", 0, 0.5, 0.5),
            word("world", 0.5, 1.0, 0.5),
        ]

        for secondary in [equal, quieter] {
            let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
            XCTAssertEqual(merged.text, "hello world")
            XCTAssertEqual(merged.winner, .primary)
        }
    }

    func testMixedSentenceKeepsTheConfidentReadingPerPiece() {
        // German at the start, English at the end, each read badly by the other language.
        let primary = [
            word("ich", 0, 0.4, 0.9),
            word("bin", 0.4, 0.8, 0.9),
            word("hallo", 0.8, 1.4, 0.2),
            word("welt", 1.4, 2.0, 0.2),
        ]
        let secondary = [
            word("each", 0, 0.4, 0.1),
            word("been", 0.4, 0.8, 0.1),
            word("hello", 0.8, 1.4, 0.9),
            word("world", 1.4, 2.0, 0.9),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "ich bin hello world")
        XCTAssertEqual(merged.winner, .secondary)
    }

    func testSwitchCostKeepsASingleDoubtfulWordInThePrimaryReading() {
        // The middle word is much better in the secondary reading, but two changes cost more
        // than that one word gains.
        let primary = [
            word("a", 0, 0.3, 0.9),
            word("b", 0.3, 0.6, 0.4),
            word("c", 0.6, 0.9, 0.9),
        ]
        let secondary = [
            word("x", 0, 0.3, 0.3),
            word("y", 0.3, 0.6, 0.9),
            word("z", 0.6, 0.9, 0.3),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "a b c")
        XCTAssertEqual(merged.winner, .primary)
    }

    func testTheSeamWordIsKeptOnce() {
        // Primary closes its German piece with "Shared" at 0.8-1.1; the secondary reading
        // starts its English piece with the same word, spelled with a comma, at 1.2-1.7.
        let primary = [
            word("one", 0, 0.4, 0.9),
            word("two", 0.4, 0.8, 0.9),
            word("Shared", 0.8, 1.1, 0.9),
            word("end", 1.1, 1.6, 0.1),
        ]
        let secondary = [
            word("eins", 0, 0.4, 0.1),
            word("zwei", 0.4, 0.8, 0.1),
            word("drei", 0.8, 1.2, 0.1),
            word("shared,", 1.2, 1.7, 0.9),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "one two Shared")
        XCTAssertEqual(merged.winner, .primary)
    }

    func testEmptySecondaryReturnsThePrimaryText() {
        let primary = [
            word("hello", 0, 0.5, 0.9),
            word("world", 0.5, 1.0, 0.9),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: [])
        XCTAssertEqual(merged.text, "hello world")
        XCTAssertEqual(merged.winner, .primary)

        let nothing = TwoLanguageMerger.merge(primary: [], secondary: [])
        XCTAssertEqual(nothing.text, "")
        XCTAssertEqual(nothing.winner, .primary)
    }
}
