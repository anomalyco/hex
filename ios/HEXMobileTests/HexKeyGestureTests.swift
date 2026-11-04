import CoreGraphics
import XCTest
@testable import HEX

final class HexKeyGestureTests: XCTestCase {
    func testTapDoesNothing() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: .zero)
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 3, y: 2)), .ignored, "a jitter is still a tap")
        XCTAssertEqual(gesture.touchUp(), .ignored)
        XCTAssertEqual(gesture.holdElapsed(), .ignored, "a timer left over from the tap")
        XCTAssertEqual(gesture.state, .idle)
    }

    func testStillHoldFiresOnce() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: .zero)
        XCTAssertEqual(gesture.holdElapsed(), .hold)
        XCTAssertEqual(gesture.holdElapsed(), .ignored)
        XCTAssertEqual(gesture.touchUp(), .ignored)
    }

    func testMoveBeforeHoldOpensMenu() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: CGPoint(x: 20, y: 20))
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 20, y: 9)), .openMenu)
        XCTAssertEqual(gesture.holdElapsed(), .ignored, "the menu won, no dictation")
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 0, y: -40)), .dragMenu)
        XCTAssertEqual(gesture.touchUp(), .releaseMenu)
        XCTAssertEqual(gesture.state, .idle)
    }

    func testMoveAfterHoldDoesNothing() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: .zero)
        XCTAssertEqual(gesture.holdElapsed(), .hold)
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 0, y: -80)), .ignored)
        XCTAssertEqual(gesture.touchUp(), .ignored)
    }

    func testCancelResets() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: .zero)
        gesture.cancel()
        XCTAssertEqual(gesture.state, .idle)
        XCTAssertEqual(gesture.holdElapsed(), .ignored)
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 50, y: 0)), .ignored)
    }
}
