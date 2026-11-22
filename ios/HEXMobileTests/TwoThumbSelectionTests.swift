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

    private func point(_ kind: KeyKind) -> CGPoint {
        let key = engine.layout.key(kind)!
        return CGPoint(x: key.frame.midX, y: key.frame.midY)
    }

    /// The characters a thumb passes over, measured as the trackpad measures them.
    private func width(of characters: String) -> CGFloat {
        characters.reduce(0) { $0 + FullKeyboardEngine.trackpadWidth($1) }
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

    /// Moves the thumb on id over characters (backwards, toward the text's start, for the
    /// end before it), earning no gain (one sample, no time passed), so the distance is
    /// exactly those characters' widths.
    private func move(_ id: Int, over characters: String, from start: CGPoint, backwards: Bool = false) -> CGPoint {
        let dx = backwards ? -width(of: characters) : width(of: characters)
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
        _ = move(1, over: "llo", from: space, backwards: true)
        _ = move(2, over: " wor", from: letterA)
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
        _ = move(2, over: "👍🏽cd", from: letterA, backwards: true)
        liftBoth()
        XCTAssertEqual(engine.selectionPresentation?.selected, "👍🏽cd")
        engine.performTrackpadSelectionAction(.delete) { _ in }
        XCTAssertEqual(target.text, "ab")
        XCTAssertFalse(engine.isSelectionActive)
    }

    /// Copy leaves the text alone and moves the cursor to the range's end (before it here, so
    /// the move is real).
    func testCopyLeavesTheTextAndMovesTheCursorToTheEnd() {
        type("hello world")
        target.adjustTextPosition(byCharacterOffset: -6)
        var copied: String?
        beginSelection(second: letterA)
        _ = move(2, over: " w", from: letterA)
        liftBoth()
        XCTAssertEqual(engine.selectionPresentation?.selected, " w")
        engine.performTrackpadSelectionAction(.copy) { copied = $0 }
        XCTAssertEqual(copied, " w")
        XCTAssertEqual(target.text, "hello world")
        XCTAssertEqual(target.cursorIndex, 7, "the cursor went to the range's end")
        XCTAssertFalse(engine.isSelectionActive)
    }

    /// Cut copies the range and takes it out.
    func testCutCopiesAndDeletesTheRange() {
        type("hello world")
        var copied: String?
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        liftBoth()
        engine.performTrackpadSelectionAction(.cut) { copied = $0 }
        XCTAssertEqual(copied, "ld")
        XCTAssertEqual(target.text, "hello wor")
    }

    /// Cancel leaves the host exactly as it was.
    func testCancelChangesNothing() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
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

    /// Any other key while the actions show only dismisses them: it types nothing.
    func testAnotherKeyDismissesTheActionsWithoutActing() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        liftBoth()
        XCTAssertTrue(engine.selectionActionsShown)
        engine.touchBegan(id: 3, at: space)
        engine.touchEnded(id: 3, at: space)
        XCTAssertFalse(engine.isSelectionActive)
        XCTAssertEqual(target.text, "hello world")
        XCTAssertEqual(target.cursorIndex, 11)
    }

    /// The host editing the text during a selection dismisses it: the offsets no longer hold.
    func testAHostEditDismissesTheSelection() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        target.insertText("x")
        engine.selectionTextChanged()
        XCTAssertFalse(engine.isSelectionActive)
        XCTAssertNil(engine.selectionPresentation)
        XCTAssertEqual(target.text, "hello worldx")
    }

    /// Dictate leaves the range in place until its words arrive, then they replace it.
    func testDictateLeavesTheRangeUntilItsWordsArrive() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        liftBoth()
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        XCTAssertEqual(target.text, "hello world", "nothing is taken out up front")
        XCTAssertEqual(target.cursorIndex, 11)
        XCTAssertTrue(engine.hasPendingSelectionDictation)
        XCTAssertTrue(engine.applyPendingSelectionDictation())
        XCTAssertEqual(target.text, "hello wor")
        XCTAssertEqual(target.cursorIndex, 9)
        target.insertText("there")
        XCTAssertEqual(target.text, "hello worthere")
        XCTAssertFalse(engine.hasPendingSelectionDictation)
    }

    /// A cancelled or empty dictation leaves the range and cursor exactly as they were; a
    /// later dictation inserts normally.
    func testACancelledDictateLeavesTheTextAlone() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        liftBoth()
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        engine.clearPendingSelectionDictation()
        XCTAssertEqual(target.text, "hello world")
        XCTAssertEqual(target.cursorIndex, 11)
        XCTAssertFalse(engine.applyPendingSelectionDictation())
        XCTAssertEqual(target.text, "hello world")
        target.insertText("there")
        XCTAssertEqual(target.text, "hello worldthere")
    }

    /// Changed text keeps the range and inserts the result at the cursor.
    func testChangedTextInsertsTheDictationAtTheCursor() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        liftBoth()
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        target.insertText("x")
        engine.selectionTextChanged()
        XCTAssertFalse(engine.hasPendingSelectionDictation)
        XCTAssertFalse(engine.applyPendingSelectionDictation())
        XCTAssertEqual(target.text, "hello worldx")
        target.insertText("there")
        XCTAssertEqual(target.text, "hello worldxthere")
    }

    /// Another field never gets the range replaced.
    func testAnotherFieldInsertsTheDictationAtTheCursor() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        liftBoth()
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        target.documentIdentifier = UUID()
        XCTAssertFalse(engine.applyPendingSelectionDictation())
        XCTAssertEqual(target.text, "hello world")
        target.insertText("there")
        XCTAssertEqual(target.text, "hello worldthere")
        XCTAssertFalse(engine.hasPendingSelectionDictation)
    }

    /// A field change clears the pending replacement.
    func testAFieldChangeClearsThePendingDictate() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        liftBoth()
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        target.documentIdentifier = UUID()
        engine.configureForField()
        XCTAssertFalse(engine.hasPendingSelectionDictation)
    }

    /// A key typed while waiting clears the pending replacement; the range stays.
    func testTypingAKeyClearsThePendingDictate() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        liftBoth()
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        let at = point(.character("k"))
        engine.touchBegan(id: 4, at: at)
        engine.touchEnded(id: 4, at: at)
        XCTAssertEqual(target.text, "hello worldk", "the range stays; the key types at the cursor")
        XCTAssertFalse(engine.hasPendingSelectionDictation)
        XCTAssertFalse(engine.applyPendingSelectionDictation())
        target.insertText("there")
        XCTAssertEqual(target.text, "hello worldkthere")
    }

    /// A range covering the whole shared text is replaced only when the words arrive; a
    /// cancelled dictation leaves every character.
    func testDictateLeavesAWholeSharedRangeUntilItsWordsArrive() {
        type("abc")
        target.adjustTextPosition(byCharacterOffset: -3)
        beginSelection(second: letterA)
        _ = move(1, over: "abc", from: space)
        liftBoth()
        XCTAssertEqual(engine.selectionPresentation?.selected, "abc")
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        XCTAssertEqual(target.text, "abc")
        XCTAssertTrue(engine.applyPendingSelectionDictation())
        XCTAssertEqual(target.text, "")
        target.insertText("new")
        XCTAssertEqual(target.text, "new")
    }

    /// The experiment is off until its switch is turned on, and it keeps its App Group key.
    func testTwoThumbSelectionIsOffByDefaultAndKeepsItsAppGroupKey() {
        let settings = FullKeyboardSettings(defaults: defaults)
        XCTAssertFalse(settings.twoThumbSelection)
        settings.twoThumbSelection = true
        XCTAssertTrue(FullKeyboardSettings(defaults: defaults).twoThumbSelection)
        XCTAssertEqual(defaults.object(forKey: "experiments.twoThumbSelection") as? Bool, true)
    }

    /// Refreshing the strip with no selection leaves its visibility exactly as it was: the
    /// emoji panel and the recording bar own it then.
    func testRefreshStripLeavesTheStripsVisibilityAloneWithoutASelection() {
        let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget())
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        view.stripView.isHidden = true
        view.refreshStrip()
        XCTAssertTrue(view.stripView.isHidden)
        view.stripView.isHidden = false
        view.refreshStrip()
        XCTAssertFalse(view.stripView.isHidden)
    }

    /// The emoji panel hides the strip; a refresh while it is open must not bring it back
    /// behind the panel.
    func testRefreshStripKeepsTheStripHiddenBehindTheEmojiPanel() {
        let model = KeyboardViewModel()
        let view = FullKeyboardView(model: model, target: FixtureTextTarget())
        view.emojiOverride = true
        view.makeEmojiUsage = { EmojiUsageStore(defaults: self.defaults) }
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        view.debugOpenEmoji()
        XCTAssertTrue(view.stripView.isHidden, "the emoji panel hides the strip")
        view.refreshStrip()
        XCTAssertTrue(view.stripView.isHidden, "and a refresh leaves it hidden")
    }

    /// A selection takes the strip and puts it back exactly as hidden or visible as it was.
    func testASelectionRestoresTheStripsPriorVisibility() {
        let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget())
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        let spaceKey = view.engine.layout.key(.space)!
        let letterKey = view.engine.layout.key(.character("a"))!
        let space = CGPoint(x: spaceKey.frame.midX, y: spaceKey.frame.midY)
        let letter = CGPoint(x: letterKey.frame.midX, y: letterKey.frame.midY)
        view.engine.trackpadSelectionState.enabled = true

        func selectAndCancel(_ first: Int) {
            view.engine.touchBegan(id: first, at: space)
            view.engine.holdSpace(forTouch: first)
            view.engine.touchBegan(id: first + 1, at: letter)
            XCTAssertTrue(view.engine.isSelectionActive)
            view.engine.touchCancelled(id: first + 1)
            XCTAssertFalse(view.engine.isSelectionActive)
        }

        view.stripView.isHidden = true
        selectAndCancel(10)
        XCTAssertTrue(view.stripView.isHidden, "hidden before the selection, hidden after")
        view.stripView.isHidden = false
        selectAndCancel(20)
        XCTAssertFalse(view.stripView.isHidden, "visible before the selection, visible after")
    }

    /// The space bar's finger can lift first: its trackpad is over, and the other thumb keeps
    /// moving its end until it lifts too.
    func testTheSpaceBarFingerCanLiftFirst() {
        type("hello world")
        target.adjustTextPosition(byCharacterOffset: -6)
        beginSelection(second: letterA)
        var left = move(2, over: "llo", from: letterA, backwards: true)
        engine.touchEnded(id: 1, at: space)
        XCTAssertTrue(engine.isSelectionActive)
        XCTAssertEqual(engine.selectionPresentation?.phase, .selecting)
        XCTAssertFalse(engine.isTrackpadActive, "the space bar's trackpad is over")
        left = CGPoint(x: left.x - width(of: "e"), y: left.y)
        engine.touchMoved(id: 2, to: left)
        XCTAssertEqual(engine.trackpadSelectionState.session?.range.lowerBound, 1)
        engine.touchEnded(id: 2, at: left)
        XCTAssertEqual(engine.selectionPresentation?.phase, .actions)
        XCTAssertEqual(engine.selectionPresentation?.selected, "ello")
        XCTAssertEqual(target.text, "hello world")
    }

    /// A touch the system takes away ends the selection, changing nothing.
    func testACancelledTouchEndsTheSelection() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        engine.touchCancelled(id: 2)
        XCTAssertFalse(engine.isSelectionActive)
        XCTAssertNil(engine.selectionPresentation)
        XCTAssertEqual(target.text, "hello world")
        XCTAssertEqual(target.cursorIndex, 11)
    }

    /// A third finger while both thumbs are down is ignored: it types nothing and moves nothing.
    func testAThirdFingerIsIgnored() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        engine.touchBegan(id: 3, at: point(.character("k")))
        engine.touchEnded(id: 3, at: point(.character("k")))
        XCTAssertEqual(engine.selectionPresentation?.phase, .selecting)
        XCTAssertEqual(target.text, "hello world")
        XCTAssertNil(engine.touches[3])
    }

    /// A host edit while the actions show makes the range stale: an action dismisses them and
    /// changes nothing.
    func testAGuardDismissesTheActionsWhenTheTextNoLongerMatches() {
        type("hello world")
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        liftBoth()
        target.insertText("x")
        engine.performTrackpadSelectionAction(.delete) { _ in }
        XCTAssertFalse(engine.isSelectionActive)
        XCTAssertEqual(target.text, "hello worldx")
        XCTAssertEqual(target.cursorIndex, 12)
    }

    /// One thumb can lift while the other keeps moving its end.
    func testOneThumbCanLiftWhileTheOtherKeepsMoving() {
        type("hello world")
        target.adjustTextPosition(byCharacterOffset: -6)
        beginSelection(second: letterA)
        _ = move(2, over: "ld", from: letterA, backwards: true)
        let lowered = engine.trackpadSelectionState.session!.range
        engine.touchEnded(id: 2, at: letterA)
        XCTAssertEqual(engine.selectionPresentation?.phase, .selecting, "one thumb is still down")
        _ = move(1, over: " w", from: space)
        let grown = engine.trackpadSelectionState.session!.range
        XCTAssertEqual(grown.lowerBound, lowered.lowerBound, "the lifted thumb's end stays put")
        XCTAssertGreaterThan(grown.upperBound, lowered.upperBound, "the other end keeps moving")
        engine.touchEnded(id: 1, at: space)
        XCTAssertEqual(engine.selectionPresentation?.phase, .actions)
        XCTAssertEqual(engine.selectionPresentation?.selected, "lo w", "from the lifted thumb's end to the grown one")
    }
}

/// A field whose cursor counts UTF-16 units, as `UITextDocumentProxy` moves it, and whose
/// `deleteBackward` takes one Character (a grapheme), as a host's does.
@MainActor
private final class UTF16TextTarget: KeyboardTextTarget {
    private(set) var text: String
    private(set) var cursor: Int
    var documentIdentifier: UUID?
    /// How much around the cursor the host shares, in UTF-16 units; nil shares it all.
    var beforeWindow: Int?
    var afterWindow: Int?
    var autocapitalizationType: UITextAutocapitalizationType? = UITextAutocapitalizationType.none
    var autocorrectionType: UITextAutocorrectionType? = .default
    var keyboardType: UIKeyboardType? = .default
    var returnKeyType: UIReturnKeyType? = .default
    var keyboardAppearance: UIKeyboardAppearance? = .default
    var smartQuotesType: UITextSmartQuotesType?
    var smartDashesType: UITextSmartDashesType?

    init(text: String, cursor: Int? = nil, document: UUID? = nil) {
        self.text = text
        self.cursor = cursor ?? text.utf16.count
        documentIdentifier = document
    }

    private func index(_ offset: Int) -> String.Index {
        let clamped = max(0, min(text.utf16.count, offset))
        let utf16 = text.utf16.index(text.utf16.startIndex, offsetBy: clamped)
        return String.Index(utf16, within: text) ?? text.endIndex
    }

    var documentContextBeforeInput: String? {
        let all = String(text[..<index(cursor)])
        guard let beforeWindow else { return all }
        return String(all.suffix(beforeWindow))
    }
    var documentContextAfterInput: String? {
        let all = String(text[index(cursor)...])
        guard let afterWindow else { return all }
        return String(all.prefix(afterWindow))
    }
    var hasText: Bool { !text.isEmpty }

    func insertText(_ inserted: String) {
        text.insert(contentsOf: inserted, at: index(cursor))
        cursor += inserted.utf16.count
    }

    func deleteBackward() {
        let end = index(cursor)
        guard end > text.startIndex else { return }
        let start = text.index(before: end)
        cursor -= text[start..<end].utf16.count
        text.removeSubrange(start..<end)
    }

    func adjustTextPosition(byCharacterOffset offset: Int) {
        cursor = max(0, min(text.utf16.count, cursor + offset))
    }
}

/// The same selection on a field whose cursor and text positions count UTF-16 units, as
/// `UITextDocumentProxy.adjustTextPosition` does: an emoji is several of them.
@MainActor
final class TwoThumbSelectionUTF16Tests: XCTestCase {
    private var target: UTF16TextTarget!
    private var engine: FullKeyboardEngine!
    private var now: TimeInterval = 100
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "TwoThumbSelectionUTF16Tests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
        target = UTF16TextTarget(text: "a👍🏽bc")
        engine = FullKeyboardEngine(target: target, autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults)))
        engine.width = 393
        engine.clock = { [unowned self] in self.now }
        engine.configureForField()
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    private var space: CGPoint {
        let key = engine.layout.key(.space)!
        return CGPoint(x: key.frame.midX, y: key.frame.midY)
    }

    private var letterA: CGPoint {
        let key = engine.layout.key(.character("a"))!
        return CGPoint(x: key.frame.midX, y: key.frame.midY)
    }

    private func width(of characters: String) -> CGFloat {
        characters.reduce(0) { $0 + FullKeyboardEngine.trackpadWidth($1) }
    }

    private func beginSelection(second: CGPoint) {
        engine.trackpadSelectionState.enabled = true
        engine.touchBegan(id: 1, at: space)
        engine.holdSpace(forTouch: 1)
        engine.touchBegan(id: 2, at: second)
        XCTAssertTrue(engine.isSelectionActive)
    }

    private func liftBoth() {
        engine.touchEnded(id: 2, at: .zero)
        engine.touchEnded(id: 1, at: space)
    }

    private func move(_ id: Int, over characters: String, from start: CGPoint, backwards: Bool = false) -> CGPoint {
        let dx = backwards ? -width(of: characters) : width(of: characters)
        let next = CGPoint(x: start.x + dx, y: start.y)
        engine.touchMoved(id: id, to: next)
        return next
    }

    /// The range ends before the cursor, four UTF-16 units back: Copy must move by UTF-16
    /// units, which a Character-counting cursor would get wrong.
    func testCopyMovesTheCursorByUTF16UnitsPastAnEmoji() {
        beginSelection(second: letterA)
        _ = move(1, over: "bc", from: space, backwards: true)
        _ = move(2, over: "👍🏽bc", from: letterA, backwards: true)
        liftBoth()
        XCTAssertEqual(engine.selectionPresentation?.selected, "👍🏽")
        var copied: String?
        engine.performTrackpadSelectionAction(.copy) { copied = $0 }
        XCTAssertEqual(copied, "👍🏽")
        XCTAssertEqual(target.text, "a👍🏽bc")
        XCTAssertEqual(target.cursor, 5, "the range's end, in UTF-16 units")
    }

    /// The words replace the emoji when they arrive, rather than taking it out up front.
    func testDictateReplacesAnEmojiRangeOnResult() {
        beginSelection(second: letterA)
        _ = move(1, over: "bc", from: space, backwards: true)
        _ = move(2, over: "👍🏽bc", from: letterA, backwards: true)
        liftBoth()
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        XCTAssertEqual(target.text, "a👍🏽bc", "nothing is taken out up front")
        XCTAssertTrue(engine.applyPendingSelectionDictation())
        XCTAssertEqual(target.text, "abc")
        XCTAssertEqual(target.cursor, 1)
        target.insertText("ok")
        XCTAssertEqual(target.text, "aokbc")
    }

    /// A session over two lines, with `sharedBefore` UTF-16 units around the cursor the host
    /// shares.
    private func multiLineSession(sharedBefore: Int) -> TrackpadSelectionSession {
        let text = "first line\nsecond line"
        let state = TrackpadCursor.State(before: text, after: "", wrapWidth: 300, lineHeight: 20, width: { _ in 8 })
        let session = TrackpadSelectionSession(state: state, sharedBefore: sharedBefore, sharedAfter: 0)
        session.add(touch: 1, role: .right, point: .zero, time: 0)
        session.add(touch: 2, role: .left, point: .zero, time: 0)
        return session
    }

    private func typewriterEngine(_ target: KeyboardTextTarget) -> FullKeyboardEngine {
        let engine = FullKeyboardEngine(target: target, autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults)))
        engine.width = 393
        engine.configureForField()
        return engine
    }

    private func selectSecondLine(_ session: TrackpadSelectionSession) {
        session.move(touch: 2, to: CGPoint(x: -11 * 8, y: 0), time: 0)
    }

    /// The host sharing only the current line still lets an action inside it run.
    func testAnActionWithinTheSharedLineWorks() {
        let text = "first line\nsecond line"
        let target = UTF16TextTarget(text: text)
        target.beforeWindow = 11
        let engine = typewriterEngine(target)
        let session = multiLineSession(sharedBefore: text.utf16.count)
        selectSecondLine(session)
        XCTAssertEqual(session.range, 11..<22)
        _ = session.lift(touch: 2)
        _ = session.lift(touch: 1)
        XCTAssertEqual(target.documentContextBeforeInput, "second line")
        XCTAssertEqual(session.before, text)
        XCTAssertEqual(session.anchor, 22)
        XCTAssertEqual(session.phase, .actions)
        XCTAssertNil(engine.selectionStaleReason(session), "stale: \(engine.selectionStaleReason(session) ?? "")")
        engine.trackpadSelectionState.session = session
        // Only Dictate asks for a dictation; the other actions return false and do their work.
        _ = engine.performTrackpadSelectionAction(.delete, copy: { _ in })
        XCTAssertEqual(target.text, "first line\n")
    }

    /// A range reaching past the line the host shares is refused without changing the text.
    func testARangeBeyondTheSharedLineIsRefused() {
        let text = "first line\nsecond line"
        let target = UTF16TextTarget(text: text)
        target.beforeWindow = 11
        let engine = typewriterEngine(target)
        let session = multiLineSession(sharedBefore: text.utf16.count)
        selectSecondLine(session)
        session.move(touch: 2, to: CGPoint(x: -11 * 8, y: -20), time: 0)
        XCTAssertEqual(session.range, 0..<22)
        _ = session.lift(touch: 2)
        _ = session.lift(touch: 1)
        engine.trackpadSelectionState.session = session
        XCTAssertFalse(engine.performTrackpadSelectionAction(.delete, copy: { _ in }))
        XCTAssertEqual(target.text, text)
        XCTAssertFalse(engine.isSelectionActive)
    }

    /// A session whose range is "econ" inside the second line, with only that line shared.
    private func windowedSession() -> TrackpadSelectionSession {
        let session = multiLineSession(sharedBefore: 11)
        // From the end (22): the right end back over "d line" to 16, the left over
        // "econd line" to 12: text[12..<16] is "econ".
        session.move(touch: 1, to: CGPoint(x: -6 * 8, y: 0), time: 0)
        session.move(touch: 2, to: CGPoint(x: -10 * 8, y: 0), time: 0)
        return session
    }

    /// The host sharing only the current line replaces the range when the words arrive.
    func testDictateReplacesInWindowedText() {
        let text = "first line\nsecond line"
        let target = UTF16TextTarget(text: text)
        target.beforeWindow = 11
        let engine = typewriterEngine(target)
        let session = windowedSession()
        XCTAssertEqual(session.range, 12..<16)
        _ = session.lift(touch: 2)
        _ = session.lift(touch: 1)
        engine.trackpadSelectionState.session = session
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        XCTAssertEqual(target.text, text, "nothing is taken out up front")
        XCTAssertTrue(engine.applyPendingSelectionDictation())
        XCTAssertEqual(target.text, "first line\nsd line")
        XCTAssertEqual(target.cursor, 12)
        target.insertText("X")
        XCTAssertEqual(target.text, "first line\nsXd line")
    }

    /// Changed text in the shared line inserts the result at the cursor and keeps the range.
    func testWindowedDictateInsertsAtTheCursorWhenTextChanged() {
        let text = "first line\nsecond line"
        let target = UTF16TextTarget(text: text)
        target.beforeWindow = 11
        let engine = typewriterEngine(target)
        let session = windowedSession()
        _ = session.lift(touch: 2)
        _ = session.lift(touch: 1)
        engine.trackpadSelectionState.session = session
        XCTAssertTrue(engine.performTrackpadSelectionAction(.dictate, copy: { _ in }))
        target.insertText("x")
        XCTAssertFalse(engine.applyPendingSelectionDictation())
        XCTAssertEqual(target.text, "first line\nsecond linex")
        target.insertText("Y")
        XCTAssertEqual(target.text, "first line\nsecond linexY")
    }

    /// The range and the strip are clamped to the text the host shared when the selection
    /// began, even when the trackpad knows more.
    func testTheRangeAndStripStayInsideTheSharedText() {
        let session = multiLineSession(sharedBefore: 11)
        session.move(touch: 2, to: CGPoint(x: -11 * 8, y: 0), time: 0)
        session.move(touch: 2, to: CGPoint(x: -11 * 8, y: -20), time: 0)
        XCTAssertEqual(session.range, 11..<22, "the line the host shared")
        XCTAssertEqual(session.presentation?.selected, "second line")
        XCTAssertEqual(session.presentation?.before, "")
    }
}

