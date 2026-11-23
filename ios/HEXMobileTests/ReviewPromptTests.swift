import XCTest
@testable import HEX

/// When Hex asks for a review: only after a good moment, once per version, 120 days apart,
/// and at most three times a year.
final class ReviewPromptTests: XCTestCase {
    private var suiteName = ""
    private var defaults: UserDefaults!
    private var now = Date(timeIntervalSince1970: 1_800_000_000)
    private let day: TimeInterval = 24 * 60 * 60

    override func setUp() {
        suiteName = "hex.tests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() {
        defaults.removePersistentDomain(forName: suiteName)
    }

    private func prompt(_ version: String = "1.0") -> ReviewPrompt {
        ReviewPrompt(defaults: defaults, version: version, now: { [unowned self] in now })
    }

    func testHexIsReadyAsksOnlyAfterThePracticeDictated() {
        XCTAssertFalse(prompt().shouldAsk(at: .practiceDictated), "typed, not dictated")
        prompt().notePracticeDictated()
        XCTAssertTrue(prompt().shouldAsk(at: .practiceDictated))
    }

    func testTheNoteIsUsedOnce() {
        prompt().notePracticeDictated()
        XCTAssertFalse(prompt("1.0").shouldAsk(at: .opened), "first open, no keyboard dictation")
        XCTAssertTrue(prompt().shouldAsk(at: .practiceDictated))
        now += 200 * day
        XCTAssertFalse(prompt("1.1").shouldAsk(at: .practiceDictated), "Try again doesn't ask twice")
    }

    func testAnOpenAsksAfterTheKeyboardDictated() {
        XCTAssertTrue(prompt().shouldAsk(at: .opened, keyboardResultID: "a"))
        now += 200 * day
        XCTAssertFalse(prompt("1.1").shouldAsk(at: .opened, keyboardResultID: "a"), "nothing new since")
        XCTAssertTrue(prompt("1.1").shouldAsk(at: .opened, keyboardResultID: "b"))
    }

    func testTheThirdOpenAsksWithoutAKeyboardDictation() {
        XCTAssertFalse(prompt().shouldAsk(at: .opened))
        XCTAssertFalse(prompt().shouldAsk(at: .opened))
        XCTAssertTrue(prompt().shouldAsk(at: .opened))
    }

    func testOncePerVersion() {
        XCTAssertTrue(prompt().shouldAsk(at: .settingChanged))
        now += 365 * day
        XCTAssertFalse(prompt().shouldAsk(at: .settingChanged))
        XCTAssertTrue(prompt("1.1").shouldAsk(at: .settingChanged))
    }

    func testAsksAre120DaysApart() {
        XCTAssertTrue(prompt("1.0").shouldAsk(at: .settingChanged))
        now += 119 * day
        XCTAssertFalse(prompt("1.1").shouldAsk(at: .settingChanged))
        now += 1 * day
        XCTAssertTrue(prompt("1.1").shouldAsk(at: .settingChanged))
    }

    func testNeverMoreThanThreeInAYear() {
        // Asks on days 0, 120 and 240; day 360 is far enough from the last but inside the
        // first's year, so the fourth waits for day 365.
        for (i, version) in ["1.0", "1.1", "1.2"].enumerated() {
            if i > 0 { now += ReviewPrompt.spacing }
            XCTAssertTrue(prompt(version).shouldAsk(at: .settingChanged))
        }
        now += ReviewPrompt.spacing
        XCTAssertFalse(prompt("1.3").shouldAsk(at: .settingChanged), "fourth inside the year")
        now += 5 * day
        XCTAssertTrue(prompt("1.3").shouldAsk(at: .settingChanged))
    }
}
