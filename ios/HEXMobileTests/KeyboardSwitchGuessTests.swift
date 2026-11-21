import XCTest
@testable import HEX

@MainActor
final class KeyboardSwitchGuessTests: XCTestCase {
    private let field = UUID()

    func testSameFieldAfterHidingIsASwitchBack() {
        let guess = KeyboardSwitchGuess(hiddenDocument: field, hiddenAt: 100, hostLeftAt: nil)
        XCTAssertTrue(guess.isSwitchBack(to: field))
    }

    func testAnotherFieldOrAnUnnamedOneIsNot() {
        let guess = KeyboardSwitchGuess(hiddenDocument: field, hiddenAt: 100, hostLeftAt: nil)
        XCTAssertFalse(guess.isSwitchBack(to: UUID()))
        XCTAssertFalse(guess.isSwitchBack(to: nil))
        XCTAssertFalse(KeyboardSwitchGuess().isSwitchBack(to: field))
    }

    func testTheAppLeavingAroundOrAfterTheHideIsNot() {
        let leftAfter = KeyboardSwitchGuess(hiddenDocument: field, hiddenAt: 100, hostLeftAt: 130)
        let leftJustBefore = KeyboardSwitchGuess(hiddenDocument: field, hiddenAt: 100, hostLeftAt: 99)
        let leftLongBefore = KeyboardSwitchGuess(hiddenDocument: field, hiddenAt: 100, hostLeftAt: 50)
        XCTAssertFalse(leftAfter.isSwitchBack(to: field))
        XCTAssertFalse(leftJustBefore.isSwitchBack(to: field))
        XCTAssertTrue(leftLongBefore.isSwitchBack(to: field))
    }

    func testStoreRoundTrips() throws {
        let suite = "KeyboardSwitchGuessTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = KeyboardSwitchGuessStore(defaults: defaults)
        store.keyboardHid(in: field, at: 100)
        XCTAssertTrue(store.guess.isSwitchBack(to: field))
        store.hostLeft(at: 101)
        XCTAssertFalse(store.guess.isSwitchBack(to: field))
    }

    func testAutoStartDefaultsToWhenSwitching() throws {
        let suite = "KeyboardSwitchGuessTests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = FullKeyboardSettings(defaults: defaults)
        XCTAssertEqual(settings.dictationBarAutoStart, .whenSwitching)
        settings.dictationBarAutoStart = .always
        XCTAssertEqual(settings.dictationBarAutoStart, .always)
    }
}
