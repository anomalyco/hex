import UIKit
import XCTest
@testable import HEX

/// Settings › Experiments › Two-thumb selection: while the space bar is the trackpad, a second
/// finger picks a range the strip shows, and Delete, Cut, Copy, Dictate or Cancel act on it.
/// Off, a second finger is ignored as it always was.
@MainActor
final class TwoThumbSelectionTests: XCTestCase {
    private var target: FixtureTextTarget!
    private var engine: FullKeyboardEngine!
    private var now: TimeInterval = 100
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "TwoThumbSelectionTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
        target = FixtureTextTarget()
        target.autocapitalizationType = UITextAutocapitalizationType.none
        let ac = AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults))
        engine = FullKeyboardEngine(target: target, autocorrect: ac)
        engine.width = 393
        engine.clock = { [unowned self] in self.now }
        engine.configureForField()
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    // MARK: - Helpers

    private var space: CGPoint {
        let key = engine.layout.key(.space)!
        return CGPoint(x: key.frame.midX, y: key.frame.midY)
    }

    private var letterA: CGPoint {
        let key = engine.layout.key(.character("a"))!
        return CGPoint(x: key.frame.midX, y: key.frame.midY)
    }

    /// text's last or first characters, measured as the trackpad measures them.
    private func width(of characters: Int, in text: String) -> CGFloat {
        var total: CGFloat = 0
        guard characters != 0 else { return 0 }
        for index in 0..<abs(characters) {
            let character = characters < 0
                ? text[text.index(text.endIndex, offsetBy: -index - 1)]
                : text[text.index(text.startIndex, offsetBy: index)]
            total += FullKeyboardEngine.trackpadWidth(character)
        }
        return total
    }

    /// Holds the space bar into the trackpad, then lands second to start the selection.
    private func beginSelection(second: CGPoint) {
        engine.trackpadSelectionState.enabled = true
        engine.touchBegan(id: 1, at: space)
        engine.holdSpace(forTouch: 1)
        XCTAssertTrue(engine.isTrackpadActive)
        engine.touchBegan(id: 2, at: second)
        XCTAssertTrue(engine.isSelectionActive)
    }

    private func liftBoth() {
        engine.touchEnded(id: 2, at: .zero)
        engine.touchEnded(id: 1, at: space)
    }

    /// Moves the thumb on id characters left (negative) or right, earning no gain (one sample,
    /// no time passed), so the distance is exactly the characters' widths.
    private func move(_ id: Int, characters: Int, in text: String, from start: CGPoint) -> CGPoint {
        let dx: CGFloat = characters < 0 ? -width(of: characters, in: text) : width(of: characters, in: text)
        let next = CGPoint(x: start.x + dx, y: start.y)
        engine.touchMoved(id: id, to: next)
        return next
    }

    /// The field already holds text when the keyboard opens on it, as when you come back to it.
    private func type(_ text: String) {
        target.insertText(text)
        engine.textDidChange()
    }

    // MARK: - Off

    /// Off (the default), a second finger is ignored exactly as before: it types nothing, moves
    /// nothing, and the trackpad ends normally.
    func testOffASecondFingerIsIgnored() {
        type("hello world")
        let start = space
        engine.touchBegan(id: 1, at: start)
        engine.holdSpace(forTouch: 1)
        engine.touchMoved(id: 1, to: CGPoint(x: start.x - 30, y: start.y))
        engine.touchBegan(id: 2, at: letterA)
        engine.touchMoved(id: 2, to: CGPoint(x: letterA.x + 20, y: letterA.y))
        engine.touchEnded(id: 2, at: letterA)
        XCTAssertFalse(engine.isSelectionActive, "off, the second finger starts no selection")
        engine.touchMoved(id: 1, to: CGPoint(x: start.x - 60, y: start.y))
        engine.touchEnded(id: 1, at: CGPoint(x: start.x - 60, y: start.y))
        XCTAssertEqual(target.text, "hello world")
        XCTAssertLessThan(target.cursorIndex, 11)
    }

    // MARK: - On

    /// On, the finger further left moves the start and the other the end, each independently,
    /// and the range is what lies between them however they cross.
    func testTwoThumbsMoveOneEndEachAndTheRangeIsNormalized() {
        type("hello world")
        target.adjustTextPosition(byCharacterOffset: -6)
        let anchor = target.cursorIndex
        beginSelection(second: letterA)
        _ = move(1, characters: -3, in: "hello", from: space)
        _ = move(2, characters: 4, in: "hello world", from: letterA)
        XCTAssertEqual(engine.selectionPresentation?.phase, .selecting)
        XCTAssertEqual(engine.selectionPresentation?.selected, "llo wor")
        liftBoth()
        XCTAssertEqual(engine.selectionPresentation?.phase, .actions)
        XCTAssertEqual(engine.selectionPresentation?.selected, "llo wor")
        XCTAssertEqual(target.text, "hello world", "selecting changes nothing in the host")
        XCTAssertEqual(target.cursorIndex, anchor, "and the host's cursor stays where it was")
    }

    /// Delete takes exactly the range, an emoji included, one host character at a time.
    func testDeleteRemovesTheRangeIncludingAnEmoji() {
        type("ab👍🏽cd")
        beginSelection(second: letterA)
        _ = move(2, characters: -3, in: "ab👍🏽cd", from: letterA)
        liftBoth()
        XCTAssertEqual(engine.selectionPresentation?.selected, "👍🏽cd")
        engine.performTrackpadSelectionAction(.delete) { _ in }
        XCTAssertEqual(target.text, "ab")
        XCTAssertFalse(engine.isSelectionActive)
    }

    /// Copy leaves the text alone and puts the cursor at the range's end.
    func testCopyLeavesTheTextAndMovesTheCursorToTheEnd() {
        type("hello world")
        var copied: String?
        beginSelection(second: letterA)
        _ = move(2, characters: -2, in: "hello world", from: letterA)
        liftBoth()
        XCTAssertEqual(engine.selectionPresentation?.selected, "ld")
        engine.performTrackpadSelectionAction(.copy) { copied = $0 }
        XCTAssertEqual(copied, "ld")
        XCTAssertEqual(target.text, "hello world")
        XCTAssertEqual(target.cursorIndex, 11)
        XCTAssertFalse(engine.isSelectionActive)
    }

    /// Cut copies the range and takes it out.
    func testCutCopiesAndDeletesTheRange() {
        type("hello world")
        var copied: String?
        beginSelection(second: letterA)
        _ = move(2, characters: -2, in: "hello world", from: letterA)
        liftBoth()
        engine.performTrackpadSelectionAction(.cut) { copied = $0 }
        XCTAssertEqual(copied, "ld")
        XCTAssertEqual(target.text, "hello wor")
    }

    /// Cancel leaves the host exactly as it was.
    func testCancelChangesNothing() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, characters: -2, in: "hello world", from: letterA)
        liftBoth()
        engine.performTrackpadSelectionAction(.cancel) { _ in XCTFail("cancel copies nothing") }
        XCTAssertEqual(target.text, "hello world")
        XCTAssertEqual(target.cursorIndex, 11)
        XCTAssertFalse(engine.isSelectionActive)
    }

    /// An empty range ends like a normal trackpad lift: no actions, nothing changed.
    func testAnEmptyRangeJustEnds() {
        type("hello world")
        beginSelection(second: letterA)
        liftBoth()
        XCTAssertFalse(engine.isSelectionActive)
        XCTAssertNil(engine.selectionPresentation)
        XCTAssertEqual(target.text, "hello world")
    }

    /// Any other key while the actions show dismisses them without acting, and types.
    func testAnotherKeyDismissesTheActionsWithoutActing() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, characters: -2, in: "hello world", from: letterA)
        liftBoth()
        XCTAssertTrue(engine.selectionActionsShown)
        engine.touchBegan(id: 3, at: space)
        engine.touchEnded(id: 3, at: space)
        XCTAssertFalse(engine.isSelectionActive)
        XCTAssertEqual(target.text, "hello world ")
        XCTAssertEqual(target.cursorIndex, 12)
    }

    /// Dictate takes the range out for the keyboard's dictation; a cancelled or empty one puts
    /// it back.
    func testDictateRestoresTheRangeWhenTheDictationProducesNothing() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, characters: -2, in: "hello world", from: letterA)
        liftBoth()
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        XCTAssertEqual(target.text, "hello wor")
        XCTAssertTrue(engine.hasPendingSelectionDictation)

        engine.restoreSelectionDictation()
        XCTAssertEqual(target.text, "hello world")
        XCTAssertEqual(target.cursorIndex, 11)
        XCTAssertFalse(engine.hasPendingSelectionDictation)
    }

    /// A dictation that did put text in its place keeps it: nothing is put back over it.
    func testDictateDoesNotRestoreOverADictationsText() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, characters: -2, in: "hello world", from: letterA)
        liftBoth()
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        target.insertText("there")
        engine.restoreSelectionDictation()
        XCTAssertEqual(target.text, "hello worthere")
        XCTAssertFalse(engine.hasPendingSelectionDictation)
    }

    /// One thumb can lift while the other keeps moving its end.
    func testOneThumbCanLiftWhileTheOtherKeepsMoving() {
        type("hello world")
        target.adjustTextPosition(byCharacterOffset: -6)
        beginSelection(second: letterA)
        _ = move(2, characters: -2, in: "hello world", from: letterA)
        let lowered = engine.trackpadSelectionState.session!.range
        engine.touchEnded(id: 2, at: letterA)
        XCTAssertEqual(engine.selectionPresentation?.phase, .selecting, "one thumb is still down")
        _ = move(1, characters: 2, in: " world", from: space)
        let grown = engine.trackpadSelectionState.session!.range
        XCTAssertEqual(grown.lowerBound, lowered.lowerBound, "the lifted thumb's end stays put")
        XCTAssertGreaterThan(grown.upperBound, lowered.upperBound, "the other end keeps moving")
        engine.touchEnded(id: 1, at: space)
        XCTAssertEqual(engine.selectionPresentation?.phase, .actions)
        XCTAssertEqual(engine.selectionPresentation?.selected, "lo w", "from the lifted thumb's end to the grown one")
    }
}
