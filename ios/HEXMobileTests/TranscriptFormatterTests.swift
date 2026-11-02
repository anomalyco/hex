import XCTest
@testable import HEX

final class TranscriptFormatterTests: XCTestCase {
    private let recognized = "I know Pi's website has the agent change. Its own website, except in a very ugly way, so maybe there's some... Shared idea there, but this would be funnier and cooler looking, so..."

    func testPolishedTurnsPauseEllipsesIntoSpacesWithoutStartingSentences() {
        XCTAssertEqual(
            TranscriptFormatter.format(recognized, style: .polished),
            "I know Pi's website has the agent change. Its own website, except in a very ugly way, so maybe there's some shared idea there, but this would be funnier and cooler looking, so."
        )
    }

    func testCasualDropsPauseEllipses() {
        XCTAssertEqual(
            TranscriptFormatter.format(recognized, style: .casual),
            "i know pi's website has the agent change. its own website, except in a very ugly way, so maybe there's some shared idea there, but this would be funnier and cooler looking, so"
        )
    }

    func testNotesDropsPauseEllipses() {
        XCTAssertEqual(
            TranscriptFormatter.format(recognized, style: .notes),
            "I know Pi's website has the agent change. Its own website, except in a very ugly way, so maybe there's some shared idea there, but this would be funnier and cooler looking, so"
        )
    }

    func testVerbatimKeepsEllipses() {
        XCTAssertEqual(TranscriptFormatter.format(recognized, style: .verbatim), recognized)
    }

    func testUnicodeEllipsisMidSentence() {
        XCTAssertEqual(TranscriptFormatter.format("Wait… What happened", style: .polished), "Wait what happened.")
    }

    func testPauseKeepsIAndAcronyms() {
        XCTAssertEqual(
            TranscriptFormatter.format("so... I think the... NASA thing works", style: .polished),
            "So I think the NASA thing works."
        )
    }

    func testLeadingEllipsisIsDropped() {
        XCTAssertEqual(TranscriptFormatter.format("...and then we left", style: .polished), "And then we left.")
    }

    func testRealSentenceEndsStayIntact() {
        XCTAssertEqual(
            TranscriptFormatter.format("it works. ship it", style: .polished),
            "It works. Ship it."
        )
    }

    func testFinishPolishedClosesCleanedText() {
        XCTAssertEqual(
            TranscriptFormatter.finishPolished("I know Pi's website has the agent change its own website, so"),
            "I know Pi's website has the agent change its own website, so."
        )
    }
}
