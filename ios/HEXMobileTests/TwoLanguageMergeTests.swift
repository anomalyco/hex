import CoreMedia
import Foundation
import Speech
import XCTest
@testable import HEX

/// Experiments › Two-language dictation's merge: two readings of one recording, the more
/// confident reading for each piece, with a language change costing enough that one doubtful
/// word never flips it, and the primary's own text whenever the second reading adds nothing.
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
            word(" hello", 0, 0.5, 0.9),
            word(" world", 0.5, 1.0, 0.9),
        ]
        let equal = [
            word(" hello", 0, 0.5, 0.9),
            word(" world", 0.5, 1.0, 0.9),
        ]
        let quieter = [
            word(" hello", 0, 0.5, 0.5),
            word(" world", 0.5, 1.0, 0.5),
        ]

        for secondary in [equal, quieter] {
            let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
            XCTAssertEqual(merged.text, "hello world")
            XCTAssertEqual(merged.winner, .primary)
            XCTAssertFalse(merged.usedSecondary)
        }
    }

    func testMixedSentenceKeepsTheConfidentReadingPerPiece() {
        // German at the start, English at the end, each read badly by the other language.
        let primary = [
            word(" ich", 0, 0.4, 0.9),
            word(" bin", 0.4, 0.8, 0.9),
            word(" hallo", 0.8, 1.4, 0.2),
            word(" welt", 1.4, 2.0, 0.2),
        ]
        let secondary = [
            word(" each", 0, 0.4, 0.1),
            word(" been", 0.4, 0.8, 0.1),
            word(" hello", 0.8, 1.4, 0.9),
            word(" world", 1.4, 2.0, 0.9),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "ich bin hello world")
        XCTAssertEqual(merged.winner, .secondary)
        XCTAssertTrue(merged.usedSecondary)
    }

    func testLanguageChangeWhereTheReadingsNeverEndAWordTogetherKeepsBothStretches() {
        // English, then Chinese, in one recording (times and confidences as Apple Speech gave
        // them for a Samantha-then-Tingting clip, shortened). The Chinese reading hears the
        // English as one long garbled word that ends after the English does, and the English
        // reading hears the Chinese as garbled words, so the readings never end a word
        // together until the very end.
        let primary = [
            word(" Thanks", 0.0, 0.3, 0.98),
            word(" again", 0.3, 0.6, 1.0),
            word(" for", 0.6, 0.9, 1.0),
            word(" your", 0.9, 1.1, 1.0),
            word(" help", 1.1, 1.4, 1.0),
            word(" last", 1.4, 1.7, 1.0),
            word(" week.", 1.7, 2.3, 0.72),
            word(" Zen", 2.7, 3.2, 0.2),
            word(" wash", 3.2, 3.9, 0.15),
            word(" so", 3.9, 4.6, 0.1),
        ]
        let secondary = [
            word("fromiauthch", 0.0, 2.5, 0.3),
            word("现在", 2.7, 3.0, 0.95),
            word("我", 3.0, 3.25, 0.95),
            word("说", 3.25, 3.45, 0.95),
            word("中文。", 3.45, 4.6, 0.95),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "Thanks again for your help last week. 现在我说中文。")
        XCTAssertTrue(merged.usedSecondary)
    }

    func testSwitchCostKeepsASingleDoubtfulWordInThePrimaryReading() {
        // The middle word is much better in the secondary reading, but two changes cost more
        // than that one word gains.
        let primary = [
            word(" a", 0, 0.3, 0.9),
            word(" b", 0.3, 0.6, 0.4),
            word(" c", 0.6, 0.9, 0.9),
        ]
        let secondary = [
            word(" x", 0, 0.3, 0.3),
            word(" y", 0.3, 0.6, 0.9),
            word(" z", 0.6, 0.9, 0.3),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "a b c")
        XCTAssertEqual(merged.winner, .primary)
        XCTAssertFalse(merged.usedSecondary)
    }

    func testSeamWordReadTwiceIsKeptOnce() {
        // The primary closes its German piece with "Shared" at 0.7-0.9; the secondary starts
        // its English piece with the same spoken word at 0.8-1.0, overlapping by half the
        // shorter one.
        let primary = [
            word(" eins", 0, 0.3, 0.9),
            word(" zwei", 0.3, 0.5, 0.9),
            word(" Shared", 0.7, 0.9, 0.9),
        ]
        let secondary = [
            word(" one", 0, 0.3, 0.1),
            word(" two", 0.3, 0.5, 0.1),
            word(" drei", 0.5, 0.8, 0.1),
            word(" Shared", 0.8, 1.0, 0.9),
            word(" weiter", 1.0, 1.4, 0.9),
            word(" end", 1.4, 1.7, 0.9),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "eins zwei Shared weiter end")
        XCTAssertEqual(merged.winner, .primary)
        XCTAssertTrue(merged.usedSecondary)
    }

    func testSameTextApartInTimeIsKeptTwice() {
        // The same word said twice: the readings' spans do not overlap, so it is not a seam
        // duplicate and both stay.
        let primary = [
            word(" eins", 0, 0.3, 0.9),
            word(" zwei", 0.3, 0.5, 0.9),
            word(" Shared", 0.7, 0.9, 0.9),
        ]
        let secondary = [
            word(" one", 0, 0.3, 0.1),
            word(" two", 0.3, 0.5, 0.1),
            word(" drei", 0.5, 0.9, 0.1),
            word(" Shared", 1.0, 1.2, 0.9),
            word(" end", 1.2, 1.5, 0.9),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "eins zwei Shared Shared end")
        XCTAssertEqual(merged.winner, .primary)
        XCTAssertTrue(merged.usedSecondary)
    }

    func testEmptySecondaryReturnsThePrimaryText() {
        let primary = [
            word(" hello", 0, 0.5, 0.9),
            word(" world", 0.5, 1.0, 0.9),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: [])
        XCTAssertEqual(merged.text, "hello world")
        XCTAssertEqual(merged.winner, .primary)
        XCTAssertFalse(merged.usedSecondary)

        let nothing = TwoLanguageMerger.merge(primary: [], secondary: [])
        XCTAssertEqual(nothing.text, "")
        XCTAssertEqual(nothing.winner, .primary)
        XCTAssertFalse(nothing.usedSecondary)
    }

    func testAllPrimaryUnspacedTextLeavesSecondaryUnused() {
        // Chinese runs arrive without spaces; the merge need not reconstruct them because a
        // primary-only result keeps Apple's own text.
        let primary = [
            word("你好", 0, 0.5, 0.9),
            word("世界", 0.5, 1.0, 0.9),
        ]
        let secondary = [
            word("nǐ hǎo", 0, 0.5, 0.2),
            word("shì jiè", 0.5, 1.0, 0.2),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "你好世界")
        XCTAssertEqual(merged.winner, .primary)
        XCTAssertFalse(merged.usedSecondary)
    }

    func testMixedOutputKeepsTheWinningRunsPunctuation() {
        let primary = [
            word(" hallo", 0, 0.5, 0.9),
            word(" welt", 0.5, 1.0, 0.2),
        ]
        let secondary = [
            word(" hello", 0, 0.5, 0.2),
            word(" world!", 0.5, 1.0, 0.95),
        ]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "hallo world!")
        XCTAssertTrue(merged.usedSecondary)
    }

    func testReadingCoveringLessAudioDoesNotWinOnConfidenceAlone() {
        // The primary is more confident but covers a tenth of the segment; the secondary
        // covers all of it less confidently and so has more confident audio to offer.
        let primary = [word(" hi", 0, 0.1, 0.9)]
        let secondary = [word(" hello there", 0, 1.0, 0.8)]

        let merged = TwoLanguageMerger.merge(primary: primary, secondary: secondary)
        XCTAssertEqual(merged.text, "hello there")
        XCTAssertEqual(merged.winner, .secondary)
        XCTAssertTrue(merged.usedSecondary)
    }

    @available(iOS 26.0, *)
    func testWordsKeepRunsSpacingAndAttachPunctuation() {
        var text = AttributedString("“")
        text += timed(" Hello", 0, 0.5, 0.9)
        text += timed(" world", 0.5, 1.0, 0.8)
        text += timed(".", 0.95, 1.05, 0.3)
        text += AttributedString("!")
        text += timed(" again", 1.0, 1.5, 0.7)

        let words = AppleSpeechTwoLanguage.words(in: text)
        XCTAssertEqual(words.count, 3)
        XCTAssertEqual(words.map(\.text), ["“ Hello", " world.!", " again"])
        XCTAssertEqual(words[0].start, 0, accuracy: 0.0001)
        XCTAssertEqual(words[1].end, 1.05, accuracy: 0.0001)
        XCTAssertEqual(words[1].confidence, 0.8, accuracy: 0.0001)
        XCTAssertEqual(words[2].start, 1.0, accuracy: 0.0001)
    }

    @available(iOS 26.0, *)
    private func timed(
        _ text: String,
        _ start: Double,
        _ end: Double,
        _ confidence: Double
    ) -> AttributedString {
        var attributes = AttributeContainer()
        attributes[AttributeScopes.SpeechAttributes.TimeRangeAttribute.self] = CMTimeRange(
            start: CMTime(seconds: start, preferredTimescale: 600),
            duration: CMTime(seconds: end - start, preferredTimescale: 600)
        )
        attributes[AttributeScopes.SpeechAttributes.ConfidenceAttribute.self] = confidence
        var piece = AttributedString(text)
        piece.mergeAttributes(attributes)
        return piece
    }
}
