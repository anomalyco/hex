import UIKit
import XCTest
@testable import HEX

final class ShiftStateMachineTests: XCTestCase {
    func testTapTurnsShiftOnForOneCharacter() {
        var shift = ShiftStateMachine()
        shift.shiftDown(now: 0)
        shift.shiftUp()
        XCTAssertEqual(shift.state, .on)
        XCTAssertTrue(shift.isUppercase)
        shift.didType()
        XCTAssertEqual(shift.state, .off)
        XCTAssertFalse(shift.isUppercase)
    }

    func testTappingShiftAgainTurnsItOff() {
        var shift = ShiftStateMachine()
        shift.shiftDown(now: 0)
        shift.shiftUp()
        shift.shiftDown(now: 1)
        shift.shiftUp()
        XCTAssertEqual(shift.state, .off)
    }

    func testChordTypesCapitalsWhileHeldThenTurnsOff() {
        var shift = ShiftStateMachine()
        shift.shiftDown(now: 0)
        XCTAssertTrue(shift.isUppercase)
        shift.didType()  // "A"
        XCTAssertTrue(shift.isUppercase, "still held")
        shift.didType()  // "B"
        XCTAssertTrue(shift.isUppercase)
        shift.shiftUp()
        XCTAssertEqual(shift.state, .off)
    }

    func testChordDoesNotCountTowardDoubleTap() {
        var shift = ShiftStateMachine()
        shift.shiftDown(now: 0)
        shift.didType()
        shift.shiftUp()
        shift.shiftDown(now: 0.1)
        XCTAssertEqual(shift.state, .on, "a fresh tap, not caps lock")
    }

    func testDoubleTapLocksAndTapUnlocks() {
        var shift = ShiftStateMachine()
        shift.shiftDown(now: 0)
        shift.shiftUp()
        shift.shiftDown(now: 0.2)
        shift.shiftUp()
        XCTAssertEqual(shift.state, .locked)
        shift.didType()
        shift.didType()
        XCTAssertEqual(shift.state, .locked)
        shift.refreshAutoCapitalization(false)
        XCTAssertEqual(shift.state, .locked, "auto-caps never clears caps lock")
        shift.shiftDown(now: 5)
        shift.shiftUp()
        XCTAssertEqual(shift.state, .off)
    }

    func testSlowSecondTapTurnsShiftOff() {
        var shift = ShiftStateMachine()
        shift.shiftDown(now: 0)
        shift.shiftUp()
        shift.shiftDown(now: 0.31)
        shift.shiftUp()
        XCTAssertEqual(shift.state, .off)
    }

    func testDoubleTapFromAutoCapsLocks() {
        var shift = ShiftStateMachine()
        shift.refreshAutoCapitalization(true)
        shift.shiftDown(now: 0)
        shift.shiftUp()
        XCTAssertEqual(shift.state, .off, "one tap clears auto-caps")
        shift.shiftDown(now: 0.15)
        shift.shiftUp()
        XCTAssertEqual(shift.state, .locked)
    }

    func testAutoCapsLeavesAHeldShiftAlone() {
        var shift = ShiftStateMachine()
        shift.shiftDown(now: 0)
        shift.refreshAutoCapitalization(false)
        XCTAssertTrue(shift.isUppercase)
    }

    // MARK: - Auto-capitalization

    func testSentenceCapitalization() {
        let cases: [(String?, Bool, Bool)] = [
            (nil, false, true),
            (nil, true, false),  // web view lost the context
            ("", true, true),
            ("Hello", true, false),
            ("Hello ", true, false),
            ("Hello. ", true, true),
            ("Hello.  ", true, true),
            ("Really? ", true, true),
            ("Wow! ", true, true),
            ("He said \"hi.\" ", true, true),
            ("(Done.) ", true, true),
            ("e.g.", true, false),
            ("Line one\n", true, true),
            ("Line one\n  ", true, true),
            ("   ", true, true),
            ("7:30 ", true, false),
        ]
        for (before, hasText, expected) in cases {
            XCTAssertEqual(
                AutoCapitalization.shouldCapitalize(mode: .sentences, before: before, hasText: hasText),
                expected,
                "before: \(String(describing: before)) hasText: \(hasText)"
            )
        }
    }

    func testOtherCapitalizationModes() {
        XCTAssertFalse(AutoCapitalization.shouldCapitalize(mode: .none, before: "", hasText: false))
        XCTAssertTrue(AutoCapitalization.shouldCapitalize(mode: .allCharacters, before: "abc", hasText: true))
        XCTAssertTrue(AutoCapitalization.shouldCapitalize(mode: .words, before: "Jane ", hasText: true))
        XCTAssertFalse(AutoCapitalization.shouldCapitalize(mode: .words, before: "Ja", hasText: true))
        XCTAssertEqual(AutoCapitalization.Mode(UITextAutocapitalizationType.none), .none)
        XCTAssertEqual(AutoCapitalization.Mode(nil), .sentences)
    }

    // MARK: - Double-space period

    func testDoubleSpaceAfterAWordTypesAPeriod() {
        XCTAssertTrue(DoubleSpacePeriod.shouldReplace(before: "late ", lastSpaceAt: 10, now: 10.2))
        XCTAssertTrue(DoubleSpacePeriod.shouldReplace(before: "at 7 ", lastSpaceAt: 10, now: 10.35))
    }

    func testDoubleSpaceEdgeCases() {
        XCTAssertFalse(DoubleSpacePeriod.shouldReplace(before: "late ", lastSpaceAt: 10, now: 10.4), "too slow")
        XCTAssertFalse(DoubleSpacePeriod.shouldReplace(before: "late ", lastSpaceAt: nil, now: 10), "not our space")
        XCTAssertFalse(DoubleSpacePeriod.shouldReplace(before: "late. ", lastSpaceAt: 10, now: 10.1), "already punctuated")
        XCTAssertFalse(DoubleSpacePeriod.shouldReplace(before: "late  ", lastSpaceAt: 10, now: 10.1), "third space")
        XCTAssertFalse(DoubleSpacePeriod.shouldReplace(before: " ", lastSpaceAt: 10, now: 10.1), "start of field")
        XCTAssertFalse(DoubleSpacePeriod.shouldReplace(before: nil, lastSpaceAt: 10, now: 10.1), "no context")
        XCTAssertFalse(DoubleSpacePeriod.shouldReplace(before: "late", lastSpaceAt: 10, now: 10.1), "cursor moved")
    }

    // MARK: - Layers and fields

    func testNumbersGoBackToLettersOnSpaceOrReturn() {
        var layer = KeyboardLayerState()
        layer.switchTo(.numbers)
        layer.didInsert("7")
        XCTAssertEqual(layer.layer, .numbers)
        layer.didInsert(" ")
        XCTAssertEqual(layer.layer, .letters)

        layer.switchTo(.symbols)
        layer.didInsert("#")
        layer.didInsert("\n")
        XCTAssertEqual(layer.layer, .letters)

        layer.switchTo(.numbers)
        layer.reset()
        XCTAssertEqual(layer.layer, .letters)
    }

    func testFieldRouting() {
        XCTAssertEqual(KeyboardFieldRouting.route(for: .default), .keys(.text, initialLayer: .letters))
        XCTAssertEqual(KeyboardFieldRouting.route(for: nil), .keys(.text, initialLayer: .letters))
        XCTAssertEqual(KeyboardFieldRouting.route(for: .URL), .keys(.url, initialLayer: .letters))
        XCTAssertEqual(KeyboardFieldRouting.route(for: .emailAddress), .keys(.email, initialLayer: .letters))
        XCTAssertEqual(KeyboardFieldRouting.route(for: .numbersAndPunctuation), .keys(.text, initialLayer: .numbers))
        XCTAssertEqual(KeyboardFieldRouting.route(for: .numberPad), .handOff)
        XCTAssertEqual(KeyboardFieldRouting.route(for: .decimalPad), .handOff)
        XCTAssertEqual(KeyboardFieldRouting.route(for: .asciiCapableNumberPad), .handOff)
    }
}
