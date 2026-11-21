import UIKit
import XCTest
@testable import HEX

@MainActor
final class FullKeyboardEngineTests: XCTestCase {
    private var target: FixtureTextTarget!
    private var engine: FullKeyboardEngine!
    private var now: TimeInterval = 100
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "FullKeyboardEngineTests-\(UUID().uuidString)"
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

    private func point(_ kind: KeyKind) -> CGPoint {
        let key = engine.layout.key(kind)!
        return CGPoint(x: key.frame.midX, y: key.frame.midY)
    }

    private func down(_ id: Int, _ kind: KeyKind) { engine.touchBegan(id: id, at: point(kind)) }
    private func up(_ id: Int, _ kind: KeyKind) { engine.touchEnded(id: id, at: point(kind)) }

    private func tap(_ kind: KeyKind) {
        let at = point(kind)
        engine.touchBegan(id: 1, at: at)
        engine.touchEnded(id: 1, at: at)
    }

    private func type(_ text: String) {
        for ch in text {
            tap(ch == " " ? .space : .character(String(ch)))
            now += 0.5
        }
    }

    func testRolloverKeepsOrder() {
        down(1, .character("t"))
        down(2, .character("h"))
        up(1, .character("t"))
        up(2, .character("h"))
        XCTAssertEqual(target.text, "th")
    }

    func testRolloverWhenTheSecondKeyLiftsFirst() {
        down(1, .character("t"))
        down(2, .character("h"))
        up(2, .character("h"))
        up(1, .character("t"))
        XCTAssertEqual(target.text, "th")
    }

    /// Space in a rollover keeps its place: a letter held as space goes down goes in first, and
    /// space goes in when the next letter goes down, even if that letter lifts first. It used
    /// to wait for its own lift, so "a b" typed fast came out "ab ".
    func testRolloverWithTheSpaceBarKeepsOrder() {
        down(1, .character("a"))
        down(2, .space)
        up(1, .character("a"))
        down(3, .character("b"))
        up(3, .character("b"))
        up(2, .space)
        XCTAssertEqual(target.text, "a b")

        down(4, .space)
        down(5, .character("c"))
        engine.holdSpace(forTouch: 4)
        XCTAssertFalse(engine.isTrackpadActive, "a space already typed doesn't become the trackpad")
        up(4, .space)
        up(5, .character("c"))
        XCTAssertEqual(target.text, "a b c")
    }

    func testSlidingToAnotherKeyTypesWhereTheFingerLifts() {
        down(1, .character("f"))
        engine.touchMoved(id: 1, to: point(.character("g")))
        XCTAssertEqual(engine.previewKey?.kind, .character("g"))
        up(1, .character("g"))
        XCTAssertEqual(target.text, "g")
    }

    func testShiftChordAndOneShot() {
        down(1, .shift)
        down(2, .character("a"))
        up(2, .character("a"))
        down(3, .character("b"))
        up(3, .character("b"))
        up(1, .shift)
        tap(.character("c"))
        XCTAssertEqual(target.text, "ABc")

        tap(.shift)
        tap(.character("d"))
        tap(.character("e"))
        XCTAssertEqual(target.text, "ABcDe")
    }

    func testDoubleTapShiftLocks() {
        tap(.shift)
        now += 0.1
        tap(.shift)
        type("hi")
        XCTAssertEqual(target.text, "HI")
        XCTAssertEqual(engine.shift.state, .locked)
    }

    func testHoldNumbersSlideToADigitAndLetGo() {
        down(1, .layerSwitch(.numbers))
        XCTAssertEqual(engine.layout.layer, .numbers)
        engine.touchMoved(id: 1, to: point(.character("7")))
        engine.touchEnded(id: 1, at: point(.character("7")))
        XCTAssertEqual(target.text, "7")
        XCTAssertEqual(engine.layout.layer, .letters)
    }

    /// As on Apple's keyboard, the key under the finger shows its pop-up the whole slide.
    func testHoldNumbersPreviewsTheKeyUnderTheFinger() {
        down(1, .layerSwitch(.numbers))
        XCTAssertNil(engine.previewKey, "123 itself has no pop-up")
        engine.touchMoved(id: 1, to: point(.character("5")))
        XCTAssertEqual(engine.previewKey?.kind, .character("5"))
        XCTAssertTrue(engine.pressedKinds.contains(.character("5")), "with Key pop-ups off it darkens")
        engine.touchMoved(id: 1, to: point(.character("6")))
        XCTAssertEqual(engine.previewKey?.kind, .character("6"))
        engine.touchMoved(id: 1, to: point(.space))
        XCTAssertNil(engine.previewKey, "no pop-up over a key that isn't a character")
        engine.touchMoved(id: 1, to: CGPoint(x: point(.character("6")).x, y: engine.layout.touchAreaFrame.minY - 40))
        XCTAssertNil(engine.previewKey, "none off the keys")
        engine.touchMoved(id: 1, to: point(.character("(")))
        XCTAssertEqual(engine.previewKey?.kind, .character("("))
        engine.touchEnded(id: 1, at: point(.character("(")))
        XCTAssertNil(engine.previewKey)
        XCTAssertEqual(target.text, "(")
        XCTAssertEqual(engine.layout.layer, .letters)
    }

    func testLiftingBackOnTheNumbersKeyOrOffTheKeysTypesNothing() {
        down(1, .layerSwitch(.numbers))
        engine.touchMoved(id: 1, to: point(.character("5")))
        let abc = point(.layerSwitch(.letters))
        engine.touchMoved(id: 1, to: abc)
        XCTAssertNil(engine.previewKey)
        engine.touchEnded(id: 1, at: abc)
        XCTAssertEqual(target.text, "")
        XCTAssertEqual(engine.layout.layer, .letters, "a slide taken back goes back to the letters")

        down(2, .layerSwitch(.numbers))
        engine.touchMoved(id: 2, to: point(.character("5")))
        let above = CGPoint(x: point(.character("5")).x, y: engine.layout.touchAreaFrame.minY - 40)
        engine.touchMoved(id: 2, to: above)
        engine.touchEnded(id: 2, at: above)
        XCTAssertEqual(target.text, "")
        XCTAssertEqual(engine.layout.layer, .numbers, "off the keys the numbers stay, as on Apple's keyboard")

        tap(.layerSwitch(.letters))
        tap(.layerSwitch(.numbers))
        XCTAssertEqual(engine.layout.layer, .numbers, "a tap of 123 still stays on the numbers")
        XCTAssertEqual(target.text, "")
    }

    func testHoldSymbolsSlideToASymbolAndLetGo() {
        tap(.layerSwitch(.numbers))
        down(1, .layerSwitch(.symbols))
        XCTAssertEqual(engine.layout.layer, .symbols)
        engine.touchMoved(id: 1, to: point(.character("%")))
        XCTAssertEqual(engine.previewKey?.kind, .character("%"))
        engine.touchEnded(id: 1, at: point(.character("%")))
        XCTAssertEqual(target.text, "%")
        XCTAssertEqual(engine.layout.layer, .numbers, "back to the page it came from")
    }

    func testHoldShiftSlideToALetterTypesACapital() {
        down(1, .shift)
        engine.touchMoved(id: 1, to: point(.character("h")))
        XCTAssertEqual(engine.previewKey?.kind, .character("h"))
        XCTAssertEqual(engine.previewKey.map(engine.displayLabel(for:)), "H")
        engine.touchEnded(id: 1, at: point(.character("h")))
        XCTAssertEqual(target.text, "H")
        XCTAssertEqual(engine.shift.state, .off)
        tap(.character("i"))
        XCTAssertEqual(target.text, "Hi")

        // Slid off and back onto shift: a tap of shift, nothing typed.
        down(2, .shift)
        engine.touchMoved(id: 2, to: point(.character("z")))
        engine.touchMoved(id: 2, to: point(.shift))
        XCTAssertNil(engine.previewKey)
        engine.touchEnded(id: 2, at: point(.shift))
        XCTAssertEqual(target.text, "Hi")
        XCTAssertEqual(engine.shift.state, .on)
    }

    func testNumbersStayUntilSpace() {
        tap(.layerSwitch(.numbers))
        XCTAssertEqual(engine.layout.layer, .numbers)
        tap(.character("4"))
        tap(.character("2"))
        XCTAssertEqual(engine.layout.layer, .numbers)
        tap(.space)
        XCTAssertEqual(engine.layout.layer, .letters)
        XCTAssertEqual(target.text, "42 ")
    }

    func testDoubleSpaceTypesAPeriod() {
        type("ok")
        tap(.space)
        now += 0.2
        tap(.space)
        XCTAssertEqual(target.text, "ok. ")
        now += 0.2
        tap(.space)
        XCTAssertEqual(target.text, "ok.  ", "a third space is just a space")
    }

    func testSlowDoubleSpaceStaysTwoSpaces() {
        type("ok")
        tap(.space)
        now += 0.5
        tap(.space)
        XCTAssertEqual(target.text, "ok  ")
    }

    func testSentenceAutoCapitalization() {
        target.autocapitalizationType = .sentences
        engine.configureForField()
        XCTAssertEqual(engine.shift.state, .on)
        type("hi")
        tap(.space)
        now += 0.2
        tap(.space)
        type("there")
        XCTAssertEqual(target.text, "Hi. There")
    }

    /// Settings › Keyboard › Auto-capitalization off, as Apple's switch: shift never turns
    /// itself on, whatever the field asks, not even after the double-space period; shift and
    /// caps lock still work.
    func testAutoCapitalizationOffNeverTurnsShiftOn() {
        engine.autoCapitalizes = false
        for type in [UITextAutocapitalizationType.sentences, .words, .allCharacters] {
            target.autocapitalizationType = type
            engine.configureForField()
            XCTAssertEqual(engine.shift.state, .off, "autocapitalizationType \(type.rawValue)")
        }
        target.autocapitalizationType = .sentences
        engine.configureForField()
        type("hi")
        tap(.space)
        now += 0.2
        tap(.space)
        XCTAssertEqual(engine.shift.state, .off, "after the double-space period")
        type("there ok")
        tap(.shift)
        type("x")
        tap(.shift)
        tap(.shift)
        type("yz")
        XCTAssertEqual(target.text, "hi. there okXYZ", "shift, then caps lock")

        tap(.shift)
        tap(.space)
        now += 0.2
        tap(.space)
        XCTAssertEqual(target.text, "hi. there okXYZ. ")
        XCTAssertEqual(engine.shift.state, .off)
        engine.autoCapitalizes = true
        XCTAssertEqual(engine.shift.state, .on, "switched back on, the next sentence starts with a capital")
    }

    func testAcceleratingDeleteRampAndWordDelete() {
        type("hello world")
        down(1, .delete)
        XCTAssertEqual(target.text, "hello worl")
        // Initial repeat deletes 1 char.
        engine.repeatDelete(heldFor: FullKeyboardEngine.repeatDelay)
        XCTAssertEqual(target.text, "hello wor")
        // Accelerated repeat deletes 2 chars per tick.
        engine.repeatDelete(heldFor: FullKeyboardEngine.fastRepeatAfter)
        XCTAssertEqual(target.text, "hello w")
        // After 1.4 s, removes whole words backward.
        engine.repeatDelete(heldFor: FullKeyboardEngine.wordDeleteAfter)
        XCTAssertEqual(target.text, "hello ")
        engine.repeatDelete(heldFor: FullKeyboardEngine.wordDeleteAfter)
        XCTAssertEqual(target.text, "")
        up(1, .delete)
    }

    func testLongPressAccentPickerSelectsAndUppercaseWorks() {
        tap(.shift)
        down(1, .character("e"))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 1))
        let menu = try! XCTUnwrap(engine.activeAccentMenu)
        XCTAssertNil(engine.previewKey, "single-key balloon hides while accent picker is open")
        // Apple's order, capitals while shift is on, opening on the key's own letter.
        XCTAssertEqual(menu.options, ["Ë", "É", "E", "È", "Ê", "Ě", "Ẽ", "Ē", "Ė", "Ę"])
        XCTAssertEqual(menu.selectedOption, "E")
        let cell = menu.cellFrames[menu.options.firstIndex(of: "É")!]
        engine.touchMoved(id: 1, to: CGPoint(x: cell.midX, y: cell.midY))
        engine.touchEnded(id: 1, at: CGPoint(x: cell.midX, y: cell.midY))
        XCTAssertNil(engine.activeAccentMenu)
        XCTAssertEqual(target.text, "É")
        XCTAssertEqual(engine.shift.state, .off)
    }

    func testSpacebarTrackpadMovesCaretWithoutInsertingSpace() {
        type("helo")
        let spaceStart = point(.space)
        engine.touchBegan(id: 1, at: spaceStart)
        let enterPoint = CGPoint(x: spaceStart.x - 12, y: spaceStart.y)
        engine.touchMoved(id: 1, to: enterPoint)
        XCTAssertFalse(engine.isTrackpadActive, "a slide alone doesn't start it")
        // Held, then dragged left by 16 pt (-2 chars: before "lo").
        engine.holdSpace(forTouch: 1)
        XCTAssertTrue(engine.isTrackpadActive)
        engine.touchMoved(id: 1, to: CGPoint(x: enterPoint.x - 16, y: spaceStart.y))
        engine.touchEnded(id: 1, at: CGPoint(x: enterPoint.x - 16, y: spaceStart.y))
        XCTAssertFalse(engine.isTrackpadActive)
        XCTAssertEqual(target.cursorIndex, 2)
        tap(.character("l"))
        XCTAssertEqual(target.text, "hello")
    }

    /// X (Brandon): "holding down on the space bar only allows scrubbing horizontally, whereas
    /// Apple's keyboard treats it like a cursor on both x/y axes". Apple's (black box, iOS 27.2)
    /// drags a point through the text at the text's own scale: a line per line height (the
    /// first after half of one, from the middle of the cursor's line), keeping its x across a
    /// short line, and stopping at a line's start, where coming back moves the cursor at once,
    /// and past its end, where coming back takes the travel again. Laid out here with every
    /// character 10 pt wide and 20 pt lines.
    func testTheTrackpadMovesThroughLinesAtTheTextsOwnScale() {
        var state = TrackpadCursor.State(before: "one two three\nfour five\nsix seven", after: " eight nine", wrapWidth: 300, lineHeight: 20, width: { _ in 10 })
        XCTAssertEqual(state.move(by: CGVector(dx: 0, dy: -9), at: 0), 0, "not yet: half a line from the middle")
        XCTAssertEqual(state.move(by: CGVector(dx: 0, dy: -2), at: 0), 23 - 33, "up a line, at its end: \"four five\" is shorter")
        XCTAssertEqual(state.move(by: CGVector(dx: -45, dy: 0), at: 0), -5, "along by the text's width: 4.5 characters, the nearer 5")
        XCTAssertEqual(state.move(by: CGVector(dx: -100, dy: 0), at: 0), -4, "and stops at the line's start")
        XCTAssertEqual(state.move(by: CGVector(dx: 10, dy: 0), at: 0), 1, "coming back moves it at once")
        XCTAssertEqual(state.move(by: CGVector(dx: 0, dy: -20), at: 0), 1 - 15, "up a line, at its x")
        XCTAssertEqual(state.move(by: CGVector(dx: 0, dy: 40), at: 0), 25 - 1, "down two lines, at its x")
        XCTAssertEqual(state.move(by: CGVector(dx: 200, dy: -20), at: 0), 23 - 25, "up to \"four five\", stopped at its end")
        XCTAssertEqual(state.move(by: CGVector(dx: -100, dy: 0), at: 0), 0, "coming back takes that travel again")
        XCTAssertEqual(state.move(by: CGVector(dx: -30, dy: 0), at: 0), -1)
    }

    /// Past the first or last line the host may have more: a host shares only the cursor's
    /// sentence or line, so the cursor steps just past what it showed, in one move the host makes
    /// only if there's text there. Shown it, the cursor goes on to the line there at its x; no
    /// answer means the host never moved, and that line was the text's first.
    func testTheTrackpadStepsPastTheTextItKnowsForTheHostToShowMore() {
        var state = TrackpadCursor.State(before: "Lin", after: "e two is longer", wrapWidth: 300, lineHeight: 20, width: { _ in 10 })
        XCTAssertEqual(state.move(by: CGVector(dx: 0, dy: -25), at: 0), -4, "just past the start: the end of the line before")
        XCTAssertEqual(state.move(by: CGVector(dx: 0, dy: -2), at: 0.01), 0, "waiting for the host")
        XCTAssertTrue(state.catchUp(before: "Line one", after: "", width: { _ in 10 }))
        XCTAssertEqual(state.settle(at: 0.02), 3 - 8, "the line shown, at the x it was at")

        var first = TrackpadCursor.State(before: "Lin", after: "e one", wrapWidth: 300, lineHeight: 20, width: { _ in 10 })
        XCTAssertEqual(first.move(by: CGVector(dx: 0, dy: -25), at: 0), -4)
        XCTAssertEqual(first.settle(at: 0.5), 0, "unanswered: the host never moved, so neither did the cursor")
        XCTAssertEqual(first.move(by: CGVector(dx: 0, dy: -40), at: 0.6), 0, "the first line keeps it")
        XCTAssertEqual(first.move(by: CGVector(dx: 10, dy: 0), at: 0.7), 1, "and coming back moves it at once")
    }

    /// Apple's took 80 pt of finger 1.05 times as far slowly, 1.4 times at 400 pt/s, twice at
    /// 1,000 and 2.6 times at 2,000 (iOS 27.2), from the finger's speed in whatever direction.
    func testTheTrackpadGoesFurtherTheFasterTheFinger() {
        XCTAssertEqual(TrackpadCursor.gain(speed: 0), 1)
        XCTAssertEqual(TrackpadCursor.gain(speed: 400), 1.375, accuracy: 0.01)
        XCTAssertEqual(TrackpadCursor.gain(speed: 1000), 2, accuracy: 0.01)
        XCTAssertEqual(TrackpadCursor.gain(speed: 2000), 2.59, accuracy: 0.01)
        XCTAssertEqual(TrackpadCursor.gain(speed: 100_000), TrackpadCursor.maxGain)
        // Moves 7.7 pt apart at a steady 120 Hz, their times arriving 1 to 11 ms apart.
        let times: [TimeInterval] = [0, 0.0001, 0.0089, 0.0149, 0.0254, 0.0322, 0.0381, 0.0488, 0.0554]
        let path = times.enumerated().map { TrackpadCursor.Sample(time: $1, point: CGPoint(x: CGFloat($0) * 7.7, y: 0)) }
        XCTAssertEqual(TrackpadCursor.speed(along: path), 924, accuracy: 120)
        XCTAssertTrue(TrackpadCursor.isVertical([.init(time: 0, point: .zero), .init(time: 0.01, point: CGPoint(x: 3, y: -5))]))
    }

    /// The proxy moves the cursor in UTF-16 units: a 👍🏽 is four of them, so moving by
    /// characters stalled three steps inside one. Each character's own width counts too.
    func testTheTrackpadMovesOverAnEmojiInOneStep() {
        var state = TrackpadCursor.State(before: "ab👍🏽cd", after: "", wrapWidth: 300, lineHeight: 20, width: { $0 == "👍🏽" ? 20 : 10 })
        XCTAssertEqual(state.move(by: CGVector(dx: -25, dy: 0), at: 0), -2, "back over d and c")
        XCTAssertEqual(state.move(by: CGVector(dx: -10, dy: 0), at: 0), -4, "and the emoji at once")
    }

    /// A line the field wraps: along goes on into the next row (the rows are estimates, and a
    /// single-line field never wraps), and up and down go by rows.
    func testTheTrackpadFollowsRowsTheFieldWraps() {
        let long = String(repeating: "word ", count: 12)
        var state = TrackpadCursor.State(before: long, after: "", wrapWidth: 100, lineHeight: 20, width: { _ in 10 })
        XCTAssertEqual(state.layout.rows.count, 6, "two words a row, the space after each hanging past the edge")
        XCTAssertEqual(state.move(by: CGVector(dx: 0, dy: -20), at: 0), -10, "up a row")
        XCTAssertEqual(state.move(by: CGVector(dx: -10, dy: 0), at: 0), -1, "along it")
        XCTAssertEqual(state.move(by: CGVector(dx: -100, dy: 0), at: 0), -10, "and on into the row before")
    }

    /// Apple's second finger starts a selection while the space bar is the trackpad, which a
    /// keyboard can't make; Hex's typed the blank key under it. Nor does the trackpad tick: Apple's
    /// keyboard asks for no feedback after the space bar's own (iOS 27.2's log).
    func testASecondFingerOnTheTrackpadTypesNothingAndItNeverTicks() {
        type("hello world")
        var ticks = 0
        engine.onSelectionStep = { ticks += 1 }
        let space = point(.space)
        engine.touchBegan(id: 1, at: space)
        engine.holdSpace(forTouch: 1)
        engine.touchMoved(id: 1, to: CGPoint(x: space.x - 30, y: space.y))
        down(2, .character("k"))
        up(2, .character("k"))
        engine.touchMoved(id: 1, to: CGPoint(x: space.x - 60, y: space.y))
        engine.touchEnded(id: 1, at: CGPoint(x: space.x - 60, y: space.y))
        XCTAssertEqual(target.text, "hello world")
        XCTAssertLessThan(target.cursorIndex, 11)
        XCTAssertEqual(ticks, 0)
    }

    /// With two or more languages a quick swipe along the space bar switches language (to the
    /// left the next, to the right the one before) and types nothing; a slip shorter than a
    /// swipe is still a space; a hold, then a slide, moves the cursor. With one language a
    /// quick slide is a space too, as on Apple's (TestFlight build 56: "missing the space
    /// key", where a tap that drifted 10 pt started the trackpad and typed nothing).
    func testASpaceBarSwipeSwitchesLanguageAndAHoldThenSlideMovesTheCursor() {
        var steps: [Int] = []
        engine.onLanguageSwipe = { steps.append($0) }
        engine.switchesLanguage = true
        type("hi")
        let start = point(.space)
        func slide(_ id: Int, by dx: CGFloat) {
            engine.touchBegan(id: id, at: start)
            now += 0.05
            engine.touchMoved(id: id, to: CGPoint(x: start.x + dx / 2, y: start.y))
            engine.touchMoved(id: id, to: CGPoint(x: start.x + dx, y: start.y))
            engine.touchEnded(id: id, at: CGPoint(x: start.x + dx, y: start.y))
            now += 0.5
        }

        slide(1, by: -40)
        XCTAssertEqual(steps, [1])
        slide(2, by: 32)
        XCTAssertEqual(steps, [1, -1])
        XCTAssertEqual(target.text, "hi", "a swipe types nothing")

        slide(3, by: 20)
        XCTAssertEqual(target.text, "hi ", "a short slip is a space")
        XCTAssertEqual(steps, [1, -1])

        // Held (the plane view's `trackpadHoldDelay` timer), then slid: the cursor moves.
        engine.touchBegan(id: 4, at: start)
        engine.holdSpace(forTouch: 4)
        XCTAssertTrue(engine.isTrackpadActive, "held first: the cursor slides")
        // Back over " i" and a little more, at the text's own scale.
        let back = FullKeyboardEngine.trackpadWidth(" ") + FullKeyboardEngine.trackpadWidth("i") + 1
        engine.touchMoved(id: 4, to: CGPoint(x: start.x - back, y: start.y))
        engine.touchEnded(id: 4, at: CGPoint(x: start.x - back, y: start.y))
        XCTAssertEqual(steps, [1, -1])
        XCTAssertEqual(target.cursorIndex, 1)

        // A slow swipe still switches: the hold timer firing mid-swipe leaves it a swipe.
        engine.touchBegan(id: 6, at: start)
        engine.touchMoved(id: 6, to: CGPoint(x: start.x - 15, y: start.y))
        engine.holdSpace(forTouch: 6)
        XCTAssertFalse(engine.isTrackpadActive)
        engine.touchEnded(id: 6, at: CGPoint(x: start.x - 40, y: start.y))
        XCTAssertEqual(steps, [1, -1, 1])

        engine.switchesLanguage = false
        let spaces = target.text.filter { $0 == " " }.count
        for (id, dx) in [(5, CGFloat(12)), (7, -24), (8, 40)] {
            engine.touchBegan(id: id, at: start)
            now += 0.05
            engine.touchMoved(id: id, to: CGPoint(x: start.x + dx, y: start.y))
            XCTAssertFalse(engine.isTrackpadActive, "one language: a quick slide is still a tap")
            engine.touchEnded(id: id, at: CGPoint(x: start.x + dx, y: start.y))
            now += 1
        }
        XCTAssertEqual(target.text.filter { $0 == " " }.count, spaces + 3, "each typed a space")
        XCTAssertEqual(steps, [1, -1, 1])
    }

    /// Settings › Keyboard › Languages with English and Polish: the space bar says "Polski",
    /// the keys are QWERTY, every Polish letter is on a held key (ą ć ę ł ń ó ś ź ż), and
    /// Switch dictation too dictates pl-PL.
    func testPolishTypesAndSwitches() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .polish]
        settings.dictationFollowsKeyboard = true
        settings.supportedDictationLanguages = ["en-US", "pl-PL"]
        let model = KeyboardViewModel()
        var picked: [String?] = []
        model.onLanguageSelected = { picked.append($0) }
        let field = FixtureTextTarget()
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let view = FullKeyboardView(model: model, target: field)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .polish)
        XCTAssertEqual(view.plane.spaceLabel, "Polski")
        XCTAssertNil(view.engine.autocorrect.decoder, "no Polish typing model: the dictionary engine")
        XCTAssertEqual(settings.keyboardDictationLanguage(), "pl-PL")
        XCTAssertEqual(picked, [], "Automatic already follows")
        var id = 400
        func press(_ character: String) {
            let key = view.engine.layout.key(.character(character))!
            let centre = CGPoint(x: key.frame.midX, y: key.frame.midY)
            id += 1
            view.engine.touchBegan(id: id, at: centre)
            view.engine.touchEnded(id: id, at: centre)
        }
        func hold(_ character: String, pick: String) {
            let key = view.engine.layout.key(.character(character))!
            id += 1
            view.engine.touchBegan(id: id, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            XCTAssertTrue(view.engine.openAccentMenuIfEligible(forTouch: id), character)
            guard let menu = view.engine.activeAccentMenu, let index = menu.options.firstIndex(of: pick) else {
                return XCTFail("\(character): no \(pick)")
            }
            let cell = menu.cellFrames[index]
            view.engine.touchMoved(id: id, to: CGPoint(x: cell.midX, y: cell.midY))
            view.engine.touchEnded(id: id, at: CGPoint(x: cell.midX, y: cell.midY))
        }
        press("z")
        press("a")
        for (key, letter) in [("z", "ż"), ("o", "ó"), ("l", "ł"), ("c", "ć"), ("e", "ę"), ("s", "ś"), ("a", "ą"), ("n", "ń"), ("z", "ź")] {
            hold(key, pick: letter)
        }
        XCTAssertEqual(field.text, "zażółćęśąńź")
        for letter in ["u", "i", "y"] {
            let key = view.engine.layout.key(.character(letter))!
            id += 1
            view.engine.touchBegan(id: id, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            XCTAssertFalse(view.engine.openAccentMenuIfEligible(forTouch: id), "\(letter) has no long-press options")
            view.engine.touchEnded(id: id, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
        }
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .english)
        XCTAssertEqual(view.plane.spaceLabel, "English")
    }

    /// Settings › Keyboard › Languages with English and Ukrainian: the space bar says
    /// "Українська", the keys are the twelve-key ЙЦУКЕН, ʼ types the apostrophe Ukrainian
    /// spells with and a held ʼ the other apostrophes, ґ and ї have keys of their own, and
    /// Switch dictation too dictates uk-UA.
    func testUkrainianTypesAndSwitches() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .ukrainian]
        settings.dictationFollowsKeyboard = true
        settings.supportedDictationLanguages = ["en-US", "ru-RU", "uk-UA"]
        let model = KeyboardViewModel()
        var picked: [String?] = []
        model.onLanguageSelected = { picked.append($0) }
        let field = FixtureTextTarget()
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let view = FullKeyboardView(model: model, target: field)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .ukrainian)
        XCTAssertEqual(view.plane.spaceLabel, "Українська")
        XCTAssertNil(view.engine.partnerLanguage)
        XCTAssertNil(view.engine.autocorrect.decoder, "no Ukrainian typing model: the dictionary engine")
        XCTAssertEqual(settings.keyboardDictationLanguage(), "uk-UA")
        XCTAssertEqual(picked, [], "Automatic already follows")
        var id = 300
        func press(_ character: String) {
            let key = view.engine.layout.key(.character(character))!
            let centre = CGPoint(x: key.frame.midX, y: key.frame.midY)
            id += 1
            view.engine.touchBegan(id: id, at: centre)
            view.engine.touchEnded(id: id, at: centre)
        }
        func hold(_ character: String, pick: String) {
            let key = view.engine.layout.key(.character(character))!
            id += 1
            view.engine.touchBegan(id: id, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            XCTAssertTrue(view.engine.openAccentMenuIfEligible(forTouch: id), character)
            guard let menu = view.engine.activeAccentMenu, let index = menu.options.firstIndex(of: pick) else {
                return XCTFail("\(character): no \(pick)")
            }
            let cell = menu.cellFrames[index]
            view.engine.touchMoved(id: id, to: CGPoint(x: cell.midX, y: cell.midY))
            view.engine.touchEnded(id: id, at: CGPoint(x: cell.midX, y: cell.midY))
        }
        for character in ["с", "і", "м", "ʼ", "я", "ґ", "ї"] { press(character) }
        hold("ʼ", pick: "’")
        XCTAssertEqual(field.text, "сімʼяґї’")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .english)
        XCTAssertEqual(view.plane.spaceLabel, "English")
    }

    /// Settings › Keyboard › Languages with English and Greek: the space bar says "Ελληνικά", the
    /// keys are the system Greek keyboard's, and its typing rules are the system keyboard's as
    /// observed: ´ after a vowel accents it (ϊ´ is ΐ) and after anything else types ´, σ types
    /// ς while it ends the word and turns back into σ when a letter follows, and a held σ
    /// offers ς. No partner, no typing model, and Switch dictation too dictates el-GR.
    func testGreekTypesAndSwitches() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .greek]
        settings.dictationFollowsKeyboard = true
        settings.supportedDictationLanguages = ["en-US", "el-GR"]
        let model = KeyboardViewModel()
        var picked: [String?] = []
        model.onLanguageSelected = { picked.append($0) }
        let field = FixtureTextTarget()
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let view = FullKeyboardView(model: model, target: field)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .greek)
        XCTAssertEqual(view.plane.spaceLabel, "Ελληνικά")
        XCTAssertNil(view.engine.partnerLanguage)
        XCTAssertNil(view.engine.autocorrect.decoder, "no Greek typing model: the dictionary engine")
        XCTAssertEqual(settings.keyboardDictationLanguage(), "el-GR")
        XCTAssertEqual(picked, [], "Automatic already follows")
        var id = 500
        func press(_ character: String) {
            let key = view.engine.layout.key(.character(character))!
            let centre = CGPoint(x: key.frame.midX, y: key.frame.midY)
            id += 1
            view.engine.touchBegan(id: id, at: centre)
            view.engine.touchEnded(id: id, at: centre)
        }
        func hold(_ character: String, pick: String) {
            let key = view.engine.layout.key(.character(character))!
            id += 1
            view.engine.touchBegan(id: id, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            XCTAssertTrue(view.engine.openAccentMenuIfEligible(forTouch: id), character)
            guard let menu = view.engine.activeAccentMenu, let index = menu.options.firstIndex(of: pick) else {
                return XCTFail("\(character): no \(pick)")
            }
            let cell = menu.cellFrames[index]
            view.engine.touchMoved(id: id, to: CGPoint(x: cell.midX, y: cell.midY))
            view.engine.touchEnded(id: id, at: CGPoint(x: cell.midX, y: cell.midY))
        }
        for character in ["κ", "α", "λ", "η", "μ", "ε", "´", "ρ", "α"] { press(character) }
        XCTAssertEqual(field.text, "καλημέρα")
        for character in ["λ", "ο", "´", "γ", "ο", "σ"] { press(character) }
        XCTAssertEqual(field.text, "καλημέραλόγος", "final sigma")
        press("ι")
        XCTAssertEqual(field.text, "καλημέραλόγοσι", "σ again once a letter follows")
        hold("ι", pick: "ϊ")
        press("´")
        XCTAssertEqual(field.text, "καλημέραλόγοσιΐ")
        press("´")
        XCTAssertEqual(field.text, "καλημέραλόγοσιΐ´", "an accented vowel takes no second tonos")
        press("ρ")
        press("´")
        hold("σ", pick: "ς")
        XCTAssertEqual(field.text, "καλημέραλόγοσιΐ´ρ´ς")
        XCTAssertEqual(Array(field.text.unicodeScalars.suffix(4)).map(\.value), [0x00B4, 0x03C1, 0x00B4, 0x03C2])
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .english)
        XCTAssertEqual(view.plane.spaceLabel, "English")
        press("s")
        XCTAssertEqual(field.text, "καλημέραλόγοσιΐ´ρ´ςs", "English's s is never a final sigma")
        // Greek's ; ends a sentence: the strip offers sentence starters after it, not clause ones.
        XCTAssertTrue(AutocorrectEngine.isSentenceStart("Τι κάνεις; ", language: .greek))
        XCTAssertFalse(AutocorrectEngine.isSentenceStart("first; "))
    }

    /// TestFlight (build 56): "I use Czech and English." Settings › Keyboard › Languages with
    /// English and Czech: the space bar says "Čeština", the keys are the system Czech keyboard's
    /// (QWERTZ, the ´ and ˇ keys), and they type as observed there with real touches: ´ and ˇ
    /// accent the letter before them (rˇ is ř, uˇ is ů, A´ is Á, but aˇ stays aˇ), and after
    /// anything else type themselves; a held ˇ's ˚ makes ů and å; delete takes the accented letter whole.
    /// English is its bilingual partner, and Switch dictation too dictates cs-CZ.
    func testCzechTypesAndSwitches() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .czech]
        settings.dictationFollowsKeyboard = true
        settings.supportedDictationLanguages = ["en-US", "cs-CZ"]
        let model = KeyboardViewModel()
        var picked: [String?] = []
        model.onLanguageSelected = { picked.append($0) }
        let field = FixtureTextTarget()
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let view = FullKeyboardView(model: model, target: field)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        view.engine.autocorrect.isEnabled = false
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .czech)
        XCTAssertEqual(view.plane.spaceLabel, "Čeština")
        XCTAssertNil(view.engine.autocorrect.decoder, "no Czech typing model: the dictionary engine")
        XCTAssertEqual(settings.keyboardDictationLanguage(), "cs-CZ")
        XCTAssertEqual(picked, [], "Automatic already follows")
        var id = 600
        func tap(_ kind: KeyKind) {
            let key = view.engine.layout.key(kind)!
            let centre = CGPoint(x: key.frame.midX, y: key.frame.midY)
            id += 1
            view.engine.touchBegan(id: id, at: centre)
            view.engine.touchEnded(id: id, at: centre)
        }
        func type(_ keys: String) {
            for character in keys { tap(character == " " ? .space : .character(String(character))) }
        }
        func hold(_ character: String, pick: String) {
            let key = view.engine.layout.key(.character(character))!
            id += 1
            view.engine.touchBegan(id: id, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            XCTAssertTrue(view.engine.openAccentMenuIfEligible(forTouch: id), character)
            guard let menu = view.engine.activeAccentMenu, let index = menu.options.firstIndex(of: pick) else {
                return XCTFail("\(character): no \(pick)")
            }
            let cell = menu.cellFrames[index]
            view.engine.touchMoved(id: id, to: CGPoint(x: cell.midX, y: cell.midY))
            view.engine.touchEnded(id: id, at: CGPoint(x: cell.midX, y: cell.midY))
        }
        tap(.shift)
        type("prˇi´lisˇ zˇlutˇoucˇky´ ku")
        hold("ˇ", pick: "˚")
        type("nˇ")
        XCTAssertEqual(field.text, "Příliš žluťoučký kůň")
        XCTAssertEqual(field.text.unicodeScalars.count, field.text.count, "precomposed letters, no combining marks")
        tap(.delete)
        XCTAssertEqual(field.text, "Příliš žluťoučký ků", "delete takes ň whole")
        while field.hasText { tap(.delete) }
        type("´ x´ aˇ e´´ ")
        XCTAssertEqual(field.text, "´ x´ aˇ é´ ", "a mark with nothing to accent types itself")
        type("a")
        hold("ˇ", pick: "˚")
        type("c")
        hold("ˇ", pick: "ˇ")
        type(" uˇ")
        XCTAssertEqual(field.text, "´ x´ aˇ é´ åč ů")
        tap(.shift)
        type("a´")
        tap(.shift)
        type("uˇ")
        XCTAssertTrue(field.text.hasSuffix("ÁŮ"), field.text)
        // Apple's other Czech layout, "QWERTY – Czech/Slovak", keeps both keys and their rules.
        XCTAssertEqual(KeyboardLanguage.czech.letterRows(.qwerty)[2].last, "ˇ")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .english)
        XCTAssertEqual(view.plane.spaceLabel, "English")
        XCTAssertNil(view.engine.layout.key(.character("ˇ")), "English has no ˇ key")
    }

    /// Settings › Keyboard › Languages with English, French and Russian: the space bar names
    /// the keyboard's language, switching changes the letters (AZERTY, ЙЦУКЕН), the accents and
    /// autocorrect (French with English beside it, Russian alone), and the keyboard comes back
    /// in the language it was left in. Dictation follows on Automatic; Switch dictation too
    /// puts a language picked by hand back on Automatic.
    func testTheSpaceBarSwitchesBetweenTheChosenLanguages() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .french, .russian]
        let model = KeyboardViewModel()
        let view = FullKeyboardView(model: model, target: target)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 393, height: 262)
        view.layoutIfNeeded()
        func firstLetter() -> String? {
            view.engine.layout.keys.filter { $0.row == 0 && $0.character != nil }
                .min { $0.frame.minX < $1.frame.minX }?.character
        }
        XCTAssertEqual(view.engine.language, .english)
        XCTAssertTrue(view.engine.switchesLanguage)
        XCTAssertEqual(view.plane.spaceLabel, "English")
        XCTAssertEqual(view.engine.partnerLanguage, .french)

        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .french)
        XCTAssertEqual(firstLetter(), "a", "AZERTY")
        XCTAssertEqual(view.engine.autocorrect.language, .french)
        XCTAssertEqual(view.engine.partnerLanguage, .english)
        XCTAssertEqual(view.plane.spaceLabel, "Français")
        XCTAssertEqual(settings.currentKeyboardLanguage, .french)

        settings.supportedDictationLanguages = ["en-US", "fr-FR", "ru-RU"]
        XCTAssertEqual(settings.keyboardDictationLanguage(), "fr-FR", "Automatic follows the keyboard")
        settings.dictationLanguage = "de-DE"
        var picked: [String?] = []
        model.onLanguageSelected = { picked.append($0) }
        view.switchLanguage(by: 1)
        XCTAssertEqual(picked, [], "a language picked by hand stays")
        view.switchLanguage(by: -1)
        settings.dictationFollowsKeyboard = true
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .russian)
        XCTAssertEqual(firstLetter(), "й")
        XCTAssertNil(view.engine.partnerLanguage, "Cyrillic has no bilingual partner")
        XCTAssertEqual(view.plane.spaceLabel, "Русский")
        XCTAssertEqual(picked, [nil], "Switch dictation too: back to Automatic, which follows")
        settings.dictationLanguage = nil
        XCTAssertEqual(settings.keyboardDictationLanguage(), "ru-RU")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .english, "round the list")
        view.switchLanguage(by: -1)

        let again = FullKeyboardView(model: KeyboardViewModel(), target: target)
        again.languageSettings = settings
        XCTAssertEqual(again.engine.language, .russian, "back in the language it was left in")

        settings.keyboardLanguages = [.german]
        again.syncSettings()
        XCTAssertEqual(again.engine.language, .german)
        XCTAssertFalse(again.engine.switchesLanguage)
        XCTAssertNil(again.plane.spaceLabel, "one language: the space bar names dictation's, unknown here")
    }

    /// English and Spanish chosen together type on the first one's keys, as Apple's bilingual
    /// English & Spanish keyboard does: English first, QWERTY without the ñ key (ñ on a held n)
    /// in both, the space bar still switching the language it names, autocorrects and
    /// dictates in; Spanish first, its ñ key in both; Spanish alone, its own.
    func testEnglishAndSpanishShareTheFirstLanguagesKeys() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .spanish]
        settings.supportedDictationLanguages = ["en-US", "es-ES"]
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        func row2() -> [String] {
            view.engine.layout.keys.filter { $0.row == 1 && $0.character != nil }
                .sorted { $0.frame.minX < $1.frame.minX }.compactMap(\.character)
        }
        func heldN() -> [String]? {
            guard let key = view.engine.layout.key(.character("n")) else { return nil }
            view.engine.touchBegan(id: 90, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            defer { view.engine.touchCancelled(id: 90) }
            return view.engine.openAccentMenuIfEligible(forTouch: 90) ? view.engine.activeAccentMenu?.options : nil
        }
        XCTAssertEqual(row2(), ["a", "s", "d", "f", "g", "h", "j", "k", "l"])
        XCTAssertEqual(view.plane.spaceLabel, "English")

        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .spanish)
        XCTAssertEqual(view.plane.spaceLabel, "Español")
        XCTAssertEqual(view.engine.autocorrect.language, .spanish)
        XCTAssertEqual(view.engine.partnerLanguage, .english)
        XCTAssertEqual(row2(), ["a", "s", "d", "f", "g", "h", "j", "k", "l"], "no ñ key beside English")
        XCTAssertEqual(Set(heldN() ?? []), ["n", "ñ", "ń", "ņ", "ň"], "ñ on a held n, as English's")
        XCTAssertEqual(settings.keyboardDictationLanguage(), "es-ES")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .english, "and back")
        XCTAssertEqual(settings.keyboardDictationLanguage(), "en-US")

        settings.keyboardLanguages = [.spanish, .english]
        view.syncSettings()
        XCTAssertEqual(view.engine.language, .english, "kept while still chosen")
        XCTAssertEqual(row2().last, "ñ", "Spanish leads: its ñ key for English too")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .spanish)
        XCTAssertEqual(row2().last, "ñ")

        settings.keyboardLanguages = [.spanish]
        view.syncSettings()
        XCTAssertEqual(row2().last, "ñ", "Spanish alone")
        XCTAssertFalse(view.engine.switchesLanguage)
    }

    /// A Spanish variant picked by hand survives the space bar switching away and back:
    /// Switch dictation too puts Automatic back at each switch, and dictation returns to the
    /// Spanish picked last (es-US here), not Spain's.
    func testAPickedSpanishVariantSurvivesTheSpaceBar() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .spanish]
        settings.supportedDictationLanguages = ["en-US", "es-ES", "es-MX", "es-US"]
        settings.automaticDictationLanguage = "en-US"
        settings.dictationFollowsKeyboard = true
        let model = KeyboardViewModel()
        model.onLanguageSelected = { settings.chooseDictationLanguage($0) }
        let view = FullKeyboardView(model: model, target: target)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        XCTAssertEqual(view.engine.language, .english)

        settings.chooseDictationLanguage("es-US")
        XCTAssertEqual(settings.dictationLanguage, "es-US")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .spanish)
        XCTAssertNil(settings.dictationLanguage, "Switch dictation too: back on Automatic")
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["en-US"], region: "ES"), "es-US")

        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .english)
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .spanish)
        XCTAssertEqual(
            settings.keyboardDictationLanguage(preferred: ["en-US"], region: "ES"),
            "es-US",
            "back to the variant picked"
        )
        XCTAssertEqual(settings.effectiveDictationLanguage, "es-US")
    }

    /// With English and Spanish on one keyboard, both type on the first one's layout as chosen
    /// in Settings › Keyboard › Languages › Layout (`KeyboardLanguageList.layout`).
    func testTheSharedKeysTakeTheFirstLanguagesLayout() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .spanish]
        settings.setLetterLayout(.qwertz, for: .english)
        settings.setLetterLayout(.azerty, for: .spanish)
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        XCTAssertEqual(view.engine.layout.letters, .qwertz)
        XCTAssertEqual(view.engine.layout.key(.character("z"))?.row, 0)
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .spanish)
        XCTAssertEqual(view.engine.layout.letters, .qwertz, "Spanish types on English's QWERTZ")
        XCTAssertEqual(view.engine.layout.key(.character("z"))?.row, 0)
        XCTAssertEqual(view.engine.autocorrect.layoutLetters, .qwertz)
        XCTAssertEqual(view.engine.autocorrect.partner?.layoutLetters, .qwertz)

        settings.keyboardLanguages = [.spanish, .english]
        view.syncSettings()
        XCTAssertEqual(view.engine.layout.letters, .azerty, "Spanish leads: its AZERTY for English too")
        XCTAssertEqual(view.engine.layout.key(.character("a"))?.row, 0)
        XCTAssertNil(view.engine.layout.key(.character("ñ")), "a plain layout: no ñ key")

        settings.setLetterLayout(.own, for: .spanish)
        view.syncSettings()
        XCTAssertEqual(view.engine.layout.letters, .own)
        XCTAssertNotNil(view.engine.layout.key(.character("ñ")))
    }

    /// The TestFlight request (build 56): English on QWERTZ. The keys, Spatial autocorrect's
    /// centres, a held key's accents and the touch decoder all read the QWERTZ keys.
    func testEnglishOnQWERTZTypesOnItsOwnKeys() throws {
        engine.width = 402
        engine.letterLayout = .qwertz
        let z = try XCTUnwrap(engine.layout.key(.character("z")))
        let y = try XCTUnwrap(engine.layout.key(.character("y")))
        XCTAssertEqual(z.row, 0)
        XCTAssertEqual(y.row, 2)
        type("zany yak ")
        XCTAssertEqual(target.text, "zany yak ")
        let keys = try XCTUnwrap(engine.autocorrect.decoderKeys)
        XCTAssertEqual(keys.centres["z"], CGPoint(x: z.frame.midX, y: z.frame.midY))
        XCTAssertEqual(keys.centres["y"], CGPoint(x: y.frame.midX, y: y.frame.midY))
        XCTAssertEqual(engine.swipeKeys, keys, "swipes read the same keys")
        XCTAssertEqual(engine.autocorrect.layoutLetters, .qwertz)

        engine.touchBegan(id: 7, at: CGPoint(x: y.frame.midX, y: y.frame.midY))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 7))
        XCTAssertEqual(engine.activeAccentMenu?.selectedOption, "y", "the key's own, where Apple's callout puts it")
        XCTAssertEqual(Set(engine.activeAccentMenu?.options ?? []), ["y", "ý", "ŷ", "ÿ"], "English's holds")
        engine.touchCancelled(id: 7)

        engine.letterLayout = nil
        XCTAssertEqual(engine.layout.letters, .qwerty, "nil: English's own")
        XCTAssertEqual(engine.layout.key(.character("z"))?.row, 2)
    }

    /// Spatial autocorrect's language model was trained on QWERTY text, but its touch part
    /// scores taps against the keys as drawn: a sloppy word on QWERTZ is read as QWERTZ.
    func testSpatialAutocorrectReadsTapsOnTheQWERTZKeys() throws {
        engine.width = 402
        let model = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        engine.letterOdds = model
        engine.autocorrect.decoder = TypingDecoder(model: model)
        engine.letterLayout = .qwertz
        // "yes" and "zoo" with each tap a third of a key off-centre, towards the next key right.
        for word in ["yes", "zoo"] {
            for letter in word {
                let key = try XCTUnwrap(engine.layout.key(.character(String(letter))))
                tapAt(CGPoint(x: key.frame.midX + key.frame.width / 3, y: key.frame.midY))
            }
            tapAt(point(.space))
        }
        XCTAssertEqual(target.text, "yes zoo ")
    }

    func testEnablesReturnKeyAutomaticallyBlocksEmptyField() {
        target.enablesReturnKeyAutomatically = true
        tap(.returnKey)
        XCTAssertEqual(target.text, "")
        type("ok")
        tap(.returnKey)
        XCTAssertEqual(target.text, "ok\n")
    }

    func testRecordingPanelAllocatesLazilyAndTearsDownOnIdle() {
        let model = KeyboardViewModel()
        let view = FullKeyboardView(model: model, target: target)
        view.frame = CGRect(x: 0, y: 0, width: 393, height: 254)
        view.layoutIfNeeded()
        XCTAssertFalse(view.hasAllocatedRecordingPanel, "no Metal scope allocated while typing")

        let t: TimeInterval = 1_000
        model.applyBridgeState(
            hasFullAccess: true,
            snapshot: KeyboardSnapshot(state: .recording, heartbeat: t, expiresAt: t + 60, recordingStartedAt: t - 1, jobID: "j1"),
            isModelInstalled: true,
            style: .polished,
            now: t,
            clearPending: false,
            animated: false
        )
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertTrue(view.hasAllocatedRecordingPanel)

        model.applyBridgeState(
            hasFullAccess: true,
            snapshot: KeyboardSnapshot(state: .ready, heartbeat: t + 1, expiresAt: t + 60),
            isModelInstalled: true,
            style: .polished,
            now: t + 1,
            clearPending: true,
            animated: false
        )
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertFalse(view.hasAllocatedRecordingPanel, "Metal scope torn down once recording ends")
    }

    /// iOS 26 blurs scroll views' edges inside a keyboard; on the emoji panel's 44 pt rows
    /// that blurred every emoji (TestFlight build 32). Every scroll view in the panel and
    /// the search hides all four edge effects.
    func testEmojiPanelScrollViewsHideTheirEdgeEffects() throws {
        guard #available(iOS 26.0, *) else { throw XCTSkip("edge effects arrived in iOS 26") }
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        view.emojiOverride = true
        let defaults = UserDefaults(suiteName: "anomaly.hex.tests.emoji-panel")
        defaults?.removePersistentDomain(forName: "anomaly.hex.tests.emoji-panel")
        view.makeEmojiUsage = { EmojiUsageStore(defaults: defaults) }
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 306)
        view.layoutIfNeeded()
        view.debugOpenEmoji(search: "piz")
        view.layoutIfNeeded()

        func scrollViews(in root: UIView) -> [UIScrollView] {
            root.subviews.flatMap { child -> [UIScrollView] in
                if let scrollView = child as? UIScrollView { return [scrollView] }
                return scrollViews(in: child)
            }
        }
        let found = scrollViews(in: view)
        XCTAssertEqual(found.count, 3, "the grid, the suggestion row and the search results")
        for scrollView in found {
            for edge in [scrollView.topEdgeEffect, scrollView.bottomEdgeEffect, scrollView.leftEdgeEffect, scrollView.rightEdgeEffect] {
                XCTAssertTrue(edge.isHidden, "\(Swift.type(of: scrollView)) at \(scrollView.frame)")
            }
        }
        defaults?.removePersistentDomain(forName: "anomaly.hex.tests.emoji-panel")
    }

    /// Frequently used is a full block, as the system keyboard's: eight columns of the
    /// grid's rows, your emoji first (squidlee on X: Hex showed three columns).
    func testFrequentlyUsedFillsEightColumns() throws {
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        view.emojiOverride = true
        let suite = "anomaly.hex.tests.emoji-frequent"
        let defaults = UserDefaults(suiteName: suite)
        defaults?.removePersistentDomain(forName: suite)
        let usage = EmojiUsageStore(defaults: defaults)
        for _ in 0..<3 { usage.record("🦦") }
        usage.record("🍕")
        view.makeEmojiUsage = { usage }
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 306)
        view.layoutIfNeeded()
        view.debugOpenEmoji()
        view.layoutIfNeeded()
        let panel = try XCTUnwrap(view.emojiPanel)
        let frequent = try XCTUnwrap(panel.sections.first?.entries)
        XCTAssertEqual(panel.frequentCapacity, panel.gridRows * 8)
        XCTAssertEqual(frequent.count, panel.frequentCapacity)
        XCTAssertEqual(Set(frequent).count, frequent.count, "no emoji twice")
        XCTAssertEqual(frequent.prefix(2).map { panel.library.entries[$0].emoji }, ["🦦", "🍕"])
        view.closeEmojiPanel()
        defaults?.removePersistentDomain(forName: suite)
    }

    func testSpaceBarNamesTheDictationLanguage() {
        let model = KeyboardViewModel()
        let view = FullKeyboardView(model: model, target: target)
        view.languageSettings = LanguageSettings(defaults: defaults)
        view.frame = CGRect(x: 0, y: 0, width: 393, height: 262)
        view.layoutIfNeeded()
        XCTAssertNil(view.plane.spaceLabel, "no label until the language is known")

        model.automaticLanguage = "en-US"
        model.languageChoices = ["nb-NO", "en-US"]
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertEqual(view.plane.spaceLabel, "English")

        model.dictationLanguage = "nb-NO"
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertEqual(view.plane.spaceLabel, "Norsk")

        model.dictationLanguage = "ru-RU"
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertEqual(view.plane.spaceLabel, "Русский")

        model.hasFullAccess = false
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertNil(view.plane.spaceLabel, "without Full Access the keyboard can't dictate")
    }

    func testTypesOnTheNorwegianLayout() {
        engine.language = .norwegian
        XCTAssertNotNil(engine.layout.key(.character("å")))
        XCTAssertEqual(engine.autocorrect.language, .norwegian)
        tap(.shift)
        tap(.character("ø"))
        tap(.character("l"))
        XCTAssertEqual(target.text, "Øl")

        let o = engine.layout.key(.character("ø"))!
        engine.touchBegan(id: 7, at: CGPoint(x: o.frame.midX, y: o.frame.midY))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 7))
        XCTAssertEqual(Set(engine.activeAccentMenu?.options ?? []), ["ø", "ö"])
        engine.touchCancelled(id: 7)

        engine.language = .german
        XCTAssertNil(engine.layout.key(.character("å")))
        XCTAssertNotNil(engine.layout.key(.character("ü")))
    }

    /// Spatial autocorrect has English and Spanish models only: Italian falls back to the
    /// dictionary engine in Italian, never to the English model.
    func testSpatialAutocorrectNeverUsesAnotherLanguagesModel() {
        let settings = FullKeyboardSettings()
        let wasOn = settings.spatialAutocorrect
        settings.spatialAutocorrect = true
        defer { settings.spatialAutocorrect = wasOn }
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        view.languageOverride = .spanish
        XCTAssertEqual(view.engine.autocorrect.decoder?.model.language, "es")
        view.languageOverride = .italian
        XCTAssertEqual(view.engine.autocorrect.language, .italian)
        XCTAssertNil(view.engine.autocorrect.decoder)
        // Nor do Russian, Serbian (Latin) and French; nor do English's or Spanish's
        // next-letter odds move French targets (AZERTY's letters aren't theirs).
        for language in [KeyboardLanguage.russian, .ukrainian, .greek, .polish, .romanian, .serbianLatin, .french, .frenchCanadian, .frenchSwiss] {
            view.languageOverride = language
            XCTAssertEqual(view.engine.autocorrect.language, language)
            XCTAssertNil(view.engine.autocorrect.decoder, "\(language)")
            XCTAssertNil(view.engine.letterOdds, "\(language)")
        }
        view.languageOverride = .english
        XCTAssertEqual(view.engine.autocorrect.decoder?.model.language, "en")
    }

    /// The likelier next letter claims a few points of its neighbour's target, as Apple's
    /// keyboard does: on the e/d border, "th" types e and "an" types d. Past the reach, with
    /// autocorrect off, or on a page without letters, every target stays where it is drawn.
    func testTheLikelyNextLetterClaimsAFewPointsOfItsNeighbour() throws {
        engine.letterOdds = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        let e = engine.layout.key(.character("e"))!
        let d = engine.layout.key(.character("d"))!
        let border = e.hitFrame.maxY
        XCTAssertEqual(border, d.hitFrame.minY)
        let x = max(e.frame.minX, d.frame.minX) + 4
        let inD = CGPoint(x: x, y: border + 2)
        let inE = CGPoint(x: x, y: border - 2)
        let pastReach = CGPoint(x: x, y: border + engine.letterReach + 1)

        type("th")
        XCTAssertEqual(engine.key(at: inD)?.kind, .character("e"), "the")
        XCTAssertEqual(engine.key(at: pastReach)?.kind, .character("d"))
        // A finger that rolls a point or two keeps the letter it claimed.
        engine.touchBegan(id: 1, at: inD)
        engine.touchMoved(id: 1, to: CGPoint(x: x, y: border + 3))
        engine.touchEnded(id: 1, at: CGPoint(x: x, y: border + 3))
        XCTAssertEqual(target.text, "the")

        type(" an")
        XCTAssertEqual(engine.key(at: inE)?.kind, .character("d"), "and")
        XCTAssertEqual(engine.key(at: inD)?.kind, .character("d"))

        engine.autocorrect.isEnabled = false
        XCTAssertEqual(engine.key(at: inE)?.kind, .character("e"), "autocorrect off: fixed targets")
        engine.autocorrect.isEnabled = true
        target.autocorrectionType = .no
        XCTAssertEqual(engine.key(at: inE)?.kind, .character("e"), "a field without autocorrect")
    }

    /// A TestFlight tester's sentence (build 25) as tapped on a 402 pt iPhone, from "but": each
    /// letter near the key they hit, drawn from where taps meant for the key they wanted land.
    /// They typed "but i miss here we mire than on the buokt in aoplw kwybaird whichnalso
    /// sixks". Spatial autocorrect with the next-letter odds reads every slip but "we" (way),
    /// a real word two edits away.
    func testTheTestersSentenceComesOutRightWithSpatialAutocorrect() throws {
        engine.width = 402
        let model = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        engine.letterOdds = model
        engine.autocorrect.decoder = TypingDecoder(model: model)
        let taps: [(CGFloat, CGFloat)] = [
            (237.1, 172.9), (267.7, 58.8), (185.2, 75.3), (172.8, 222.0), (301.5, 68.6), (256.1, 230.7),
            (328.1, 175.7), (306.7, 53.4), (84.3, 128.5), (82.1, 119.9), (227.3, 237.6), (243.6, 117.7),
            (97.4, 56.5), (148.7, 51.1), (103.2, 63.1), (150.6, 231.6), (68.5, 77.5), (103.2, 70.6),
            (179.1, 236.2), (323.2, 171.2), (315.6, 60.9), (143.2, 63.5), (94.7, 58.1), (129.1, 227.6),
            (181.7, 61.9), (240.9, 121.0), (41.6, 123.1), (284.4, 173.9), (199.4, 219.6), (334.4, 67.5),
            (271.3, 180.1), (214.8, 237.9), (175.0, 68.1), (251.5, 121.3), (109.3, 68.8), (107.9, 227.9),
            (250.2, 174.8), (254.9, 57.3), (322.1, 67.6), (331.9, 127.7), (175.5, 66.3), (180.5, 240.5),
            (301.3, 62.8), (274.2, 177.8), (191.9, 228.6), (39.5, 120.4), (358.9, 66.9), (372.9, 52.3),
            (360.7, 123.2), (77.0, 59.8), (156.0, 242.4), (319.3, 115.8), (82.3, 60.3), (219.7, 62.6),
            (241.8, 169.2), (45.0, 114.3), (318.2, 73.8), (142.4, 70.3), (122.1, 119.5), (169.6, 233.9),
            (65.8, 58.9), (247.5, 122.4), (294.0, 70.1), (165.6, 185.1), (231.3, 123.3), (297.4, 205.9),
            (44.8, 121.6), (364.4, 124.5), (77.0, 123.4), (332.3, 73.2), (158.3, 236.4), (82.4, 125.4),
            (282.4, 79.9), (137.6, 175.1), (309.7, 126.6), (72.5, 126.2), (108.8, 239.2),
        ]
        // The taps were placed on that build's rows (45 pt keys on a 56 pt pitch from 35 pt
        // down); put each at the same spot on today's row.
        func onTodaysRow(_ y: CGFloat) -> CGFloat {
            let row = min(3, max(0, Int(((y - 35 + 5.5) / 56).rounded(.down))))
            let key = engine.layout.keys.first { $0.row == row }!.frame
            return key.minY + (y - (35 + CGFloat(row) * 56)) / 45 * key.height
        }
        for (i, (x, y)) in taps.enumerated() {
            let point = CGPoint(x: x, y: onTodaysRow(y))
            engine.touchBegan(id: i, at: point)
            engine.touchEnded(id: i, at: point)
            now += 0.2
        }
        XCTAssertEqual(target.text, "but I miss here we more than on the built in apple keyboard which also sucks ")
    }

    /// The Duo unfolded upright splits the keys (`SplitGeometry`): either G types g, both space
    /// bars type a space, the middle between the halves types nothing, and accents and the
    /// delete-word slide work on the right half.
    func testTypesOnTheSplitLayout() throws {
        engine.posture = .unfoldedWide
        engine.windowInset = 75
        engine.width = 801
        let layout = engine.layout
        let split = try XCTUnwrap(layout.split)
        func at(_ label: String, right: Bool) -> CGPoint {
            let key = layout.keys.first { $0.label == label && ($0.frame.midX > split.middle) == right }!
            return CGPoint(x: key.frame.midX, y: key.frame.midY)
        }
        func tapAt(_ point: CGPoint) {
            engine.touchBegan(id: 1, at: point)
            engine.touchEnded(id: 1, at: point)
            now += 0.5
        }
        for (label, right) in [("g", false), ("o", true), ("t", false), ("space", false), ("g", true), ("o", true)] {
            tapAt(at(label, right: right))
        }
        tapAt(CGPoint(x: split.middle, y: at("q", right: false).y))
        tapAt(at("space", right: true))
        XCTAssertEqual(target.text, "got go ")

        engine.touchBegan(id: 2, at: at("u", right: true))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 2))
        let menu = try XCTUnwrap(engine.activeAccentMenu)
        let cell = menu.cellFrames[1]
        engine.touchMoved(id: 2, to: CGPoint(x: cell.midX, y: cell.midY))
        engine.touchEnded(id: 2, at: CGPoint(x: cell.midX, y: cell.midY))
        XCTAssertEqual(target.text, "got go " + menu.options[1])

        let plane = KeyboardKeyPlaneView(engine: engine)
        let delete = at("delete", right: true)
        plane.began(id: 3, sample: TypingLogTouchSample(point: delete, timestamp: 0))
        XCTAssertEqual(target.text, "got go ")
        let slid = CGPoint(x: delete.x - layout.letterWidth * 1.2, y: delete.y)
        plane.moved(id: 3, sample: TypingLogTouchSample(point: slid, timestamp: 0))
        plane.ended(id: 3, sample: TypingLogTouchSample(point: slid, timestamp: 0))
        XCTAssertEqual(target.text, "got ")
    }

    func testEmailFieldShowsAtAndDot() {
        target.keyboardType = .emailAddress
        engine.configureForField()
        tap(.character("@"))
        tap(.character("."))
        XCTAssertEqual(target.text, "@.")
    }

    func testNumberPadOpensOnNumbers() {
        target.keyboardType = .numberPad
        engine.configureForField()
        XCTAssertEqual(engine.layout.layer, .numbers)
    }

    // MARK: - One space per space press

    private func suggestion(_ text: String, _ kind: KeyboardSuggestion.Kind = .secondary) -> KeyboardSuggestion {
        KeyboardSuggestion(displayText: kind == .keepLiteral ? "“\(text)”" : text, replacement: text, kind: kind)
    }

    func testNextWordSuggestionsAppendOnlyOnAPickInTheSameWordGap() throws {
        engine.letterOdds = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        type("thank ")
        let you = try XCTUnwrap(engine.suggestions.first { $0.replacement == "you" })
        XCTAssertEqual(you.kind, .nextWord)
        XCTAssertFalse(you.willAutoReplace)
        XCTAssertEqual(target.text, "thank ")
        engine.acceptSuggestion(you)
        XCTAssertEqual(target.text, "thank you ")
        type("f")
        engine.acceptSuggestion(you)
        XCTAssertEqual(target.text, "thank you f")
    }

    func testNextWordPredictionsUseTheActiveLanguageAndRespectBoundaries() throws {
        engine.language = .spanish
        engine.letterOdds = try XCTUnwrap(TypingDecoderModel.bundled("es"))
        type("por ")
        XCTAssertFalse(engine.suggestions.isEmpty)
        XCTAssertTrue(engine.suggestions.allSatisfy { $0.kind == .nextWord })
        for context in ["@por ", "todo/por ", "por  "] {
            XCTAssertTrue(engine.autocorrect.suggestions(beforeInput: context, field: .text,
                autocorrectionType: .default, predictionModel: engine.letterOdds).isEmpty, context)
        }
        for context in ["por. ", "hola\n"] {
            XCTAssertEqual(engine.autocorrect.suggestions(beforeInput: context, field: .text,
                autocorrectionType: .default, predictionModel: engine.letterOdds).map(\.replacement), ["yo", "el", "la"], context)
        }
        target.autocorrectionType = .no
        engine.refreshSuggestions()
        XCTAssertTrue(engine.suggestions.isEmpty)
    }

    /// An empty field and a new sentence offer three first words, capitalized as shift would
    /// be (not with Auto-capitalization off); after a word the three slots are always filled.
    func testSentenceStartersAndNextWordsFillAllThreeSlots() throws {
        target.autocapitalizationType = .sentences
        engine.configureForField()
        engine.letterOdds = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        engine.refreshSuggestions()
        XCTAssertEqual(engine.suggestions.map(\.replacement), ["I", "The", "I'm"])
        XCTAssertTrue(engine.suggestions.allSatisfy { $0.kind == .nextWord && !$0.willAutoReplace })
        engine.acceptSuggestion(engine.suggestions[1])
        XCTAssertEqual(target.text, "The ")
        type("hi ")
        XCTAssertEqual(engine.suggestions.count, 3, "\(engine.suggestions.map(\.replacement))")
        XCTAssertTrue(engine.suggestions.allSatisfy { $0.kind == .nextWord })
        XCTAssertTrue(engine.suggestions.allSatisfy { $0.replacement.first?.isLowercase == true || $0.replacement == "I" })

        let model = engine.letterOdds
        func words(_ before: String, capitalized: Bool) -> [String] {
            engine.autocorrect.suggestions(beforeInput: before, field: .text, autocorrectionType: .default,
                                           predictionModel: model, capitalizesNext: capitalized).map(\.replacement)
        }
        for context in ["Done. ", "Really? ", "Wow! ", "Hi\n", ""] {
            XCTAssertEqual(words(context, capitalized: true), ["I", "The", "I'm"], context)
        }
        XCTAssertEqual(words("zzqx ", capitalized: false), ["the", "to", "and"], "an unknown word: the commonest words")
        XCTAssertEqual(words("Running late, ", capitalized: false), ["I", "but", "and"], "after a comma, words that start a clause")
        XCTAssertEqual(words("at 7 ", capitalized: false), [], "not after a number")
        XCTAssertEqual(words("Done.", capitalized: true), [], "no space after the period yet")
        engine.autoCapitalizes = false
        XCTAssertEqual(words("Done. ", capitalized: true), ["I", "the", "I'm"], "Auto-capitalization off adds no capitals")
    }

    /// A suggestion types its word and a space, so a space pressed right after it (a pick,
    /// then space out of habit) is swallowed, as on Apple's keyboard. Record typing logs
    /// showed every double space came from this: a word, what was typed, or an emoji picked,
    /// then space.
    func testASpaceRightAfterASuggestionIsSwallowed() {
        type("prett")
        engine.acceptSuggestion(suggestion("pretty"))
        XCTAssertEqual(target.text, "pretty ")
        now += 0.15
        tap(.space)
        XCTAssertEqual(target.text, "pretty ", "one space, not two")
        type("good")
        XCTAssertEqual(target.text, "pretty good")

        type(" is th")
        engine.acceptSuggestion(suggestion("th", .keepLiteral))
        now += 0.15
        tap(.space)
        XCTAssertEqual(target.text, "pretty good is th ", "what you typed, kept from the left slot")

        engine.acceptSuggestion(suggestion("😡", .emoji))
        now += 0.7
        tap(.space)
        XCTAssertEqual(target.text, "pretty good is th 😡 ", "an emoji for the word before")
        now += 0.7
        tap(.space)
        XCTAssertEqual(target.text, "pretty good is th 😡  ", "a second press is a space you asked for")
    }

    func testTheSwallowedSpaceStillCountsTowardsAPeriod() {
        type("prett")
        engine.acceptSuggestion(suggestion("pretty"))
        now += 0.1
        tap(.space)
        now += 0.2
        tap(.space)
        XCTAssertEqual(target.text, "pretty. ")
    }

    func testASpaceAfterTheCursorMovedIsTyped() {
        type("prett")
        engine.acceptSuggestion(suggestion("pretty"))
        target.adjustTextPosition(byCharacterOffset: -7)
        engine.textDidChange()
        tap(.space)
        XCTAssertEqual(target.text, " pretty ")

        let field = FixtureTextTarget()
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let keys = FullKeyboardEngine(target: field, autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults)))
        keys.width = 393
        keys.configureForField()
        let p = keys.layout.key(.character("p"))!.frame
        keys.touchBegan(id: 1, at: CGPoint(x: p.midX, y: p.midY))
        keys.touchEnded(id: 1, at: CGPoint(x: p.midX, y: p.midY))
        keys.acceptSuggestion(suggestion("pretty"))
        keys.textDidChange()
        let space = keys.layout.key(.space)!.frame
        keys.touchBegan(id: 2, at: CGPoint(x: space.midX, y: space.midY))
        keys.touchEnded(id: 2, at: CGPoint(x: space.midX, y: space.midY))
        XCTAssertEqual(field.text, "pretty ", "a host that reports the keys' own edit leaves it swallowed")
    }

    /// "brown" in "the quick brown fox" selected and typed over: the cursor sits between the
    /// space before it and the space after it.
    private func typeOverBrown() {
        type("the quick  fox")
        target.adjustTextPosition(byCharacterOffset: -4)
        engine.textDidChange()
    }

    /// TestFlight: "Tapping on a word to select it, and then quickly typing in a correction
    /// doesn't account for the trailing space and you end up with 2 spaces inserted."
    func testASpaceBeforeASpaceMidSentenceIsHeldForTheNextWord() {
        typeOverBrown()
        type("black ")
        XCTAssertEqual(target.text, "the quick black fox", "one space, not two")
        tap(.space)
        XCTAssertEqual(target.text, "the quick black fox", "nor after a second press")
        type("cat")
        XCTAssertEqual(target.text, "the quick black cat fox", "the next word gets the space")
        tap(.space)
        punctuate(",")
        XCTAssertEqual(target.text, "the quick black cat, fox", "a mark goes against the word")

        target.adjustTextPosition(byCharacterOffset: 4)
        engine.textDidChange()
        type(" jumps")
        target.adjustTextPosition(byCharacterOffset: -5)
        engine.textDidChange()
        tap(.space)
        XCTAssertEqual(target.text, "the quick black cat, fox  jumps", "after a space, a space you asked for")
    }

    /// A held space reads as a typed one for the strip and the keys' page: the strip offers the
    /// next word (never the finished word's completions), a pick goes in after the held space
    /// rather than in the finished word's place, and 123 goes back to ABC.
    func testAHeldSpaceOffersTheNextWordAndGoesBackToLetters() throws {
        typeOverBrown()
        engine.letterOdds = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        type("thank ")
        XCTAssertEqual(target.text, "the quick thank fox")
        XCTAssertTrue(engine.suggestions.allSatisfy { $0.kind == .nextWord }, "\(engine.suggestions.map(\.replacement))")
        let you = try XCTUnwrap(engine.suggestions.first { $0.replacement == "you" })
        engine.acceptSuggestion(you)
        XCTAssertEqual(target.text, "the quick thank you fox")
        XCTAssertTrue(engine.suggestions.allSatisfy { $0.kind == .nextWord }, "\(engine.suggestions.map(\.replacement))")

        tap(.layerSwitch(.numbers))
        tap(.character("7"))
        tap(.space)
        XCTAssertEqual(target.text, "the quick thank you 7 fox")
        XCTAssertEqual(engine.layout.layer, .letters, "a held space goes back to ABC as a typed one does")
    }

    func testAnAutocorrectionMidSentenceHoldsItsSpaceAndDeleteStillOffersTheRevert() {
        type("see  ok")
        target.adjustTextPosition(byCharacterOffset: -3)
        engine.textDidChange()
        type("teh ")
        XCTAssertEqual(target.text, "see the ok")
        XCTAssertNotEqual(engine.suggestions.first?.kind, .revert, "not before delete, as after a typed space")
        tap(.delete)
        XCTAssertEqual(target.text, "see the ok", "delete takes the held space")
        XCTAssertEqual(engine.suggestions.first?.kind, .revert)
        engine.acceptSuggestion(engine.suggestions[0])
        XCTAssertEqual(target.text, "see teh ok", "the strip puts back what was typed")
        tap(.space)
        XCTAssertEqual(target.text, "see teh ok", "space keeps it, with no second space")
    }

    func testAPickedSuggestionMidSentenceTypesNoSecondSpace() {
        typeOverBrown()
        type("blac")
        engine.acceptSuggestion(suggestion("black"))
        XCTAssertEqual(target.text, "the quick black fox")
        now += 0.15
        tap(.space)
        XCTAssertEqual(target.text, "the quick black fox", "the space after the cursor is the one")
        type("cat")
        XCTAssertEqual(target.text, "the quick black cat fox")

        target.adjustTextPosition(byCharacterOffset: 4)
        engine.textDidChange()
        type(" jumps")
        punctuate(".")
        target.adjustTextPosition(byCharacterOffset: -1)
        engine.textDidChange()
        engine.acceptSuggestion(suggestion("jumped"))
        XCTAssertEqual(target.text, "the quick black cat fox jumped.", "no space before a closing mark")
        for mark in [",", "!", "?", ";", ":", ")", "\"", "'", "’", "”"] {
            let field = FixtureTextTarget(text: "word" + mark)
            field.adjustTextPosition(byCharacterOffset: -1)
            let keys = FullKeyboardEngine(target: field, autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults)))
            XCTAssertTrue(keys.textAfterSeparates, mark)
        }
    }

    func testFrenchKeepsTheSpaceBeforeItsSpacedMarks() {
        engine.language = .french
        type("vrai")
        punctuate("?")
        target.adjustTextPosition(byCharacterOffset: -1)
        engine.textDidChange()
        XCTAssertFalse(engine.textAfterSeparates, "French puts a space before ?")
        engine.acceptSuggestion(suggestion("vraie"))
        XCTAssertEqual(target.text, "vraie ?")
    }

    func testADictationMidSentenceAddsNoSecondSpace() {
        XCTAssertEqual(TranscriptFormatter.insertionText(for: "black", beforeContext: "the quick ", afterContext: " fox"), "black")
        XCTAssertEqual(TranscriptFormatter.insertionText(for: "black", beforeContext: "the quick", afterContext: " fox"), " black")
        XCTAssertEqual(TranscriptFormatter.insertionText(for: "black", beforeContext: "the quick ", afterContext: ". Then"), "black")
    }

    /// "th", then the e aimed a few points high 80 ms later, on the strip's lower half under
    /// its left slot. Record typing logs showed that tap picked "th" and a space, so "the"
    /// came out "th" and the space after it doubled. While you type, the keys take it.
    func testATapOnTheStripsLowerHalfWhileTypingTypesTheKeyBelow() {
        type("on t")
        tap(.character("h"))
        let e = engine.layout.key(.character("e"))!
        let strip = engine.layout.stripFrame
        let aimedHigh = CGPoint(x: e.frame.midX, y: strip.maxY - 6)
        XCTAssertLessThan(aimedHigh.y, engine.layout.touchAreaFrame.minY, "on the strip, above the keys' touch area")
        now += 0.08
        XCTAssertTrue(engine.takesStripTouch(at: aimedHigh))
        XCTAssertFalse(engine.takesStripTouch(at: CGPoint(x: aimedHigh.x, y: strip.minY + 6)), "the upper half stays the strip's")
        XCTAssertEqual(engine.key(at: aimedHigh)?.kind, .character("e"))
        engine.touchBegan(id: 9, at: aimedHigh)
        engine.touchEnded(id: 9, at: aimedHigh)
        XCTAssertEqual(target.text, "on the")
        now += 0.1
        tap(.space)
        XCTAssertEqual(target.text, "on the ")

        now += 0.3
        XCTAssertFalse(engine.takesStripTouch(at: aimedHigh), "after a pause a pick is a pick")
        down(1, .character("a"))
        XCTAssertTrue(engine.takesStripTouch(at: aimedHigh), "while a key is down")
        up(1, .character("a"))
    }

    /// With the Hex key on the left the strip runs to the right side, so the keys take its
    /// lower half over p too, and the Hex key, not the keys, keeps the spot over q.
    func testTheStripsLowerHalfRuleFollowsTheHexKeyToTheLeft() {
        engine.hexPlacement = .stripLeading
        type("on t")
        tap(.character("h"))
        let strip = engine.layout.stripFrame
        let p = engine.layout.key(.character("p"))!
        let q = engine.layout.key(.character("q"))!
        let hex = engine.layout.key(.hex)!
        let overP = CGPoint(x: p.frame.midX, y: strip.maxY - 6)
        let overQ = CGPoint(x: q.frame.midX, y: strip.maxY - 6)
        now += 0.08
        XCTAssertTrue(engine.takesStripTouch(at: overP))
        XCTAssertEqual(engine.key(at: overP)?.kind, .character("p"))
        XCTAssertFalse(engine.takesStripTouch(at: overQ), "the Hex key's, not the strip's")
        XCTAssertTrue(hex.hitFrame.contains(overQ))
        now += 0.3
        XCTAssertFalse(engine.takesStripTouch(at: overP), "after a pause a pick is a pick")
    }

    /// Left: ✓ where the Hex key was, ✕ after it, then the Dot Scope well; Right mirrors it.
    func testTheRecordingBarMirrorsWithTheHexKey() throws {
        for onLeft in [false, true] {
            let panel = HexKeyRecordingPanel(loudness: { -62 })
            defer { panel.tearDown() }
            panel.hexOnLeading = onLeft
            panel.showsLoggingMark = true
            panel.frame = CGRect(x: 0, y: 0, width: 393, height: 44)
            panel.layoutIfNeeded()
            let done = try XCTUnwrap(panel.subviews.first { $0.accessibilityLabel == "Done" })
            let cancel = try XCTUnwrap(panel.subviews.first { $0.accessibilityLabel == "Cancel dictation" })
            let well = try XCTUnwrap(panel.subviews.first { $0.subviews.contains(panel.scope) })
            let mark = try XCTUnwrap(panel.subviews.first { $0 is TypingLogMarkView })
            if onLeft {
                XCTAssertEqual(done.frame.minX, KeyboardMetrics.portrait.side, accuracy: 0.5)
                XCTAssertLessThan(done.frame.maxX, cancel.frame.minX)
                XCTAssertLessThan(cancel.frame.maxX, well.frame.minX)
                XCTAssertGreaterThan(mark.frame.minX, well.frame.midX, "the mark at the well's far end")
                XCTAssertLessThanOrEqual(well.frame.minX + panel.scope.frame.maxX, mark.frame.minX)
            } else {
                XCTAssertEqual(done.frame.maxX, 393 - KeyboardMetrics.portrait.side, accuracy: 0.5)
                XCTAssertLessThan(cancel.frame.maxX, done.frame.minX)
                XCTAssertLessThan(well.frame.maxX, cancel.frame.minX)
                XCTAssertLessThan(mark.frame.maxX, well.frame.midX)
            }
        }
    }

    /// The Hex key's menu opens toward the screen's middle, and the Full Access hint's Open Hex
    /// sits beside the key, on whichever side it is.
    func testTheMenuAndFullAccessHintFollowTheHexKey() throws {
        defer { HexKeyPlacement.current = .stripTrailing }
        for onLeft in [false, true] {
            let model = KeyboardViewModel()
            let view = FullKeyboardView(model: model, target: target)
            view.hexPlacementOverride = onLeft ? .stripLeading : .stripTrailing
            view.frame = CGRect(x: 0, y: 0, width: 393, height: 254)
            view.layoutIfNeeded()
            XCTAssertEqual(view.hexKey.frame.midX < view.stripView.frame.minX, onLeft)
            XCTAssertEqual(view.hexKey.frame.midX > view.stripView.frame.maxX, !onLeft)

            view.debugShowStyleMenu()
            view.layoutIfNeeded()
            let menu = try XCTUnwrap(view.subviews.first { $0 is HexStyleMenuView })
            menu.layoutIfNeeded()
            let panel = try XCTUnwrap(menu.subviews.first)
            let hex = view.engine.layout.key(.hex)!.frame
            if onLeft {
                XCTAssertLessThan(panel.frame.minX, hex.maxX, "over the key")
                XCTAssertGreaterThan(panel.frame.maxX, hex.maxX + 100, "and out toward the middle")
            } else {
                XCTAssertGreaterThan(panel.frame.maxX, hex.minX, "over the key")
                XCTAssertLessThan(panel.frame.minX, hex.minX - 100, "and out toward the middle")
            }
            view.dismissStyleMenu()

            model.hasFullAccess = false
            RunLoop.main.run(until: Date().addingTimeInterval(0.02))
            view.showFullAccessHint()
            view.layoutIfNeeded()
            let hint = try XCTUnwrap(view.stripView.subviews.first { $0 is FullAccessHintView })
            hint.layoutIfNeeded()
            let open = try XCTUnwrap(hint.subviews.first { $0 is UIButton })
            let title = try XCTUnwrap(hint.subviews.first { ($0 as? UILabel)?.text == FullAccessHintView.title })
            XCTAssertEqual(open.frame.maxX < title.frame.minX, onLeft, "Open Hex beside the Hex key")
        }
    }

    /// The same through the view: the key plane lies over the strip and claims its lower half
    /// while you type, so the suggestions never see the touch.
    func testTheKeysTakeTheStripsLowerHalfWhileTyping() {
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        var clock: TimeInterval = 50
        view.engine.clock = { clock }
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        let layout = view.engine.layout
        let e = layout.key(.character("e"))!
        let aimedHigh = CGPoint(x: e.frame.midX, y: layout.stripFrame.maxY - 6)
        XCTAssertFalse(view.hitTest(aimedHigh, with: nil) === view.plane, "the strip's, before typing")

        for (id, letter) in ["t", "h"].enumerated() {
            let key = layout.key(.character(letter))!
            let sample = TypingLogTouchSample(point: CGPoint(x: key.frame.midX, y: key.frame.midY), timestamp: 0)
            view.plane.began(id: id + 1, sample: sample)
            view.plane.ended(id: id + 1, sample: sample)
            clock += 0.1
        }
        XCTAssertTrue(view.hitTest(aimedHigh, with: nil) === view.plane)
        let sample = TypingLogTouchSample(point: aimedHigh, timestamp: 0)
        view.plane.began(id: 3, sample: sample)
        view.plane.ended(id: 3, sample: sample)
        XCTAssertEqual(target.text, "the")

        clock += 1
        XCTAssertFalse(view.hitTest(aimedHigh, with: nil) === view.plane, "the strip's again after a pause")
    }

    /// Holding the space bar still starts the trackpad, as Apple's does, so letting go types
    /// nothing: a hold meant to move the cursor typed a space beside the one already there.
    func testHoldingSpaceStillStartsTheTrackpadAndTypesNoSpace() {
        type("hello ")
        let space = point(.space)
        engine.touchBegan(id: 1, at: space)
        engine.holdSpace(forTouch: 1)
        XCTAssertTrue(engine.isTrackpadActive)
        engine.touchEnded(id: 1, at: space)
        XCTAssertEqual(target.text, "hello ")

        engine.touchBegan(id: 2, at: space)
        engine.holdSpace(forTouch: 2)
        let left = CGPoint(x: space.x - FullKeyboardEngine.trackpadWidth(" ") - FullKeyboardEngine.trackpadWidth("o") - 1, y: space.y)
        engine.touchMoved(id: 2, to: left)
        engine.touchEnded(id: 2, at: left)
        XCTAssertEqual(target.text, "hello ")
        XCTAssertEqual(target.cursorIndex, 4, "sliding after the hold moves the cursor")
    }

    func testTheSpaceBarsHoldTimerStartsTheTrackpad() {
        type("hi ")
        let plane = KeyboardKeyPlaneView(engine: engine)
        let sample = TypingLogTouchSample(point: point(.space), timestamp: 0)
        plane.began(id: 1, sample: sample)
        plane.ended(id: 1, sample: sample)
        XCTAssertEqual(target.text, "hi  ", "a tap types a space")

        plane.began(id: 2, sample: sample)
        RunLoop.main.run(until: Date().addingTimeInterval(FullKeyboardEngine.trackpadHoldDelay + 0.15))
        XCTAssertTrue(engine.isTrackpadActive)
        plane.ended(id: 2, sample: sample)
        XCTAssertEqual(target.text, "hi  ", "a hold types none")
    }

    // MARK: - Haptics in every layout

    /// Key haptics come from the same calls whatever the layout (a TestFlight tester felt
    /// none on the Portuguese one): every layout asks for a tap per key going down and a tick
    /// per accent step, the same number of times.
    func testEveryLayoutAsksForTheSameKeyHaptics() {
        var counts: [KeyboardLanguage: Int] = [:]
        for language in KeyboardLanguage.allCases {
            let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget())
            view.languageOverride = language
            view.frame = CGRect(x: 0, y: 0, width: 393, height: 262)
            view.layoutIfNeeded()
            var asked = 0
            view.allowsHaptics = {
                asked += 1
                return true
            }
            // Russian and Ukrainian have no Latin letters: press their own а and hold their own
            // е (Ukrainian's ʼ, its only key with long-press options); Greek its α and ε.
            let first = language == .greek ? "α" : language.isLatin ? "a" : "а"
            for (id, kind) in [KeyKind.character(first), .space, .delete].enumerated() {
                let key = view.engine.layout.key(kind)!
                let sample = TypingLogTouchSample(point: CGPoint(x: key.frame.midX, y: key.frame.midY), timestamp: 0)
                view.plane.began(id: id + 10, sample: sample)
                view.plane.ended(id: id + 10, sample: sample)
            }
            // Serbian's and Romanian's e hold nothing, as on the system keyboard; Serbian's c
            // holds č and ć, Romanian's s ș. Czech's e opens with ě before e, so its r (ř).
            let held: String = switch language {
            case .ukrainian: "ʼ"
            case .russian: "е"
            case .serbianLatin: "c"
            case .romanian: "s"
            case .czech: "r"
            case .greek: "ε"
            default: "e"
            }
            view.debugHoldAccent(held, selectIndex: 1)
            counts[language] = asked
        }
        XCTAssertGreaterThanOrEqual(counts[.english] ?? 0, 4, "three keys and an accent step")
        for language in KeyboardLanguage.allCases {
            XCTAssertEqual(counts[language], counts[.english], "\(language.rawValue) haptics")
        }
    }

    /// TestFlight: "Cannot hear the iOS typing sound when using your keyboard." Every key asks
    /// for the system click as it goes down, delete, shift, 123 and return too, once a press,
    /// whatever Haptics is set to.
    func testEveryKeyClicksOnTouchDown() {
        let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget())
        view.frame = CGRect(x: 0, y: 0, width: 393, height: 262)
        view.layoutIfNeeded()
        var clicks = 0
        view.playInputClick = { clicks += 1 }
        let kinds: [KeyKind] = [.character("a"), .shift, .space, .delete, .returnKey, .layerSwitch(.numbers)]
        for (index, kind) in kinds.enumerated() {
            let key = view.engine.layout.key(kind)!
            let sample = TypingLogTouchSample(point: CGPoint(x: key.frame.midX, y: key.frame.midY), timestamp: 0)
            view.plane.began(id: index + 10, sample: sample)
            XCTAssertEqual(clicks, index + 1, "\(kind) clicks going down")
            view.plane.ended(id: index + 10, sample: sample)
            XCTAssertEqual(clicks, index + 1, "\(kind) doesn't click again coming up")
        }
    }

    // MARK: - Delete swipe

    private func swipeEngine(_ text: String) -> (FixtureTextTarget, FullKeyboardEngine) {
        let field = FixtureTextTarget(text: text)
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let keys = FullKeyboardEngine(
            target: field,
            autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults))
        )
        keys.width = 393
        keys.configureForField()
        return (field, keys)
    }

    private func deleteSample(_ keys: FullKeyboardEngine, dx: CGFloat = 0) -> TypingLogTouchSample {
        let key = keys.layout.key(.delete)!
        return TypingLogTouchSample(point: CGPoint(x: key.frame.midX + dx, y: key.frame.midY), timestamp: 0)
    }

    func testDeleteSwipeDeletesWordByWordRestoresAndCommitsOnLift() {
        let (field, keys) = swipeEngine("one two, three. four")
        let plane = KeyboardKeyPlaneView(engine: keys)
        var ticks = 0
        keys.onSelectionStep = { ticks += 1 }
        let step = keys.layout.letterWidth

        plane.began(id: 1, sample: deleteSample(keys))
        XCTAssertEqual(field.text, "one two, three. fou", "going down deletes a letter, as a tap does")
        plane.moved(id: 1, sample: deleteSample(keys, dx: -step * 0.6))
        XCTAssertEqual(field.text, "one two, three. fou", "less than a key width deletes nothing more")
        XCTAssertEqual(plane.deleteSwipe?.isActive, false)

        plane.moved(id: 1, sample: deleteSample(keys, dx: -step * 1.2))
        XCTAssertEqual(field.text, "one two, three. ")
        plane.moved(id: 1, sample: deleteSample(keys, dx: -step * 2.2))
        XCTAssertEqual(field.text, "one two, ", "punctuation and spaces go with the word before them")
        plane.moved(id: 1, sample: deleteSample(keys, dx: -step * 3.2))
        XCTAssertEqual(field.text, "one ")
        XCTAssertEqual(ticks, 3, "a tick per word")

        plane.moved(id: 1, sample: deleteSample(keys, dx: -step * 1.5))
        XCTAssertEqual(field.text, "one two, three. ", "sliding back types the words back")
        XCTAssertEqual(ticks, 5)

        plane.ended(id: 1, sample: deleteSample(keys, dx: -step * 1.5))
        XCTAssertEqual(field.text, "one two, three. ", "lifting keeps what is deleted")
        XCTAssertNil(plane.deleteSwipe)

        plane.began(id: 2, sample: deleteSample(keys))
        plane.ended(id: 2, sample: deleteSample(keys))
        XCTAssertEqual(field.text, "one two, three.", "a tap after it deletes one character")
    }

    func testDeleteSwipeStopsAtTheStartAndRestoresOnlyItsWords() {
        let (field, keys) = swipeEngine("hi there")
        let plane = KeyboardKeyPlaneView(engine: keys)
        let step = keys.layout.letterWidth
        plane.began(id: 1, sample: deleteSample(keys))
        plane.moved(id: 1, sample: deleteSample(keys, dx: -step * 6))
        XCTAssertEqual(field.text, "")
        plane.moved(id: 1, sample: deleteSample(keys))
        XCTAssertEqual(field.text, "hi ther", "the letter the press deleted stays deleted")
        plane.cancelled(id: 1, sample: deleteSample(keys))
        XCTAssertEqual(field.text, "hi ther")
    }

    /// Types a mark from the 123 page; the next space goes back to the letters.
    private func punctuate(_ mark: String) {
        if engine.layout.layer == .letters { tap(.layerSwitch(.numbers)) }
        tap(.character(mark))
        now += 0.5
    }

    /// Types `text` key by key: letters on the letters page, digits and marks from the 123 page.
    private func keys(_ text: String) {
        for ch in text {
            if ch == " " {
                tap(.space)
            } else if ch.isLetter {
                if engine.layout.layer != .letters { tap(.layerSwitch(.letters)) }
                tap(.character(String(ch)))
            } else {
                if engine.layout.layer == .letters { tap(.layerSwitch(.numbers)) }
                tap(.character(String(ch)))
            }
            now += 0.5
        }
    }

    private func smartField(quotes: UITextSmartQuotesType? = .default, dashes: UITextSmartDashesType? = .default, language: KeyboardLanguage = .english) {
        target = FixtureTextTarget()
        target.autocapitalizationType = UITextAutocapitalizationType.none
        target.smartQuotesType = quotes
        target.smartDashesType = dashes
        engine = FullKeyboardEngine(target: target, autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults)))
        engine.width = 393
        engine.clock = { [unowned self] in self.now }
        engine.language = language
        engine.configureForField()
    }

    /// X (Brandon): "no smart quotes/etc." Apple's keyboard (black box, iOS 27.2, a UITextView
    /// and a text field) curls ' and " by what comes before them, makes a second hyphen an em
    /// dash, and writes autocorrect's apostrophes as ’; a field whose smartQuotesType or
    /// smartDashesType is .no gets straight ones, as there.
    func testSmartQuotesAndDashesFollowTheFieldAsOnApplesKeyboard() {
        smartField()
        keys("'hi' it's \"yes\" (\"ok\") 5'9\" ('a') so '\"ok\"' well--known this -- that one---two yes - no")
        XCTAssertEqual(target.text, "‘hi’ it’s “yes” (“ok”) 5’9” (‘a’) so ‘“ok”’ well—known this — that one—-two yes - no")
        smartField()
        keys("a--")
        tap(.delete)
        XCTAssertEqual(target.text, "a", "delete takes the dash whole")

        smartField()
        type("dont ")
        XCTAssertEqual(target.text, "don’t ", "autocorrect's apostrophe")
        type("im")
        XCTAssertTrue(engine.suggestions.contains { $0.displayText == "I’m" && $0.replacement == "I'm" }, "the strip shows it too")
        type(" ")
        XCTAssertEqual(target.text, "don’t I’m ")
        type("ill")
        tap(.delete)
        keys("'l")
        XCTAssertEqual(target.text, "don’t I’m il’l")
        XCTAssertEqual(AutocorrectEngine.currentToken(in: engine.textBefore), "il'l", "the keys read ’ as an apostrophe")

        smartField(quotes: .no, dashes: .no)
        keys("'hi' it's \"yes\" well--known ")
        type("dont ")
        XCTAssertEqual(target.text, "'hi' it's \"yes\" well--known don't ")

        smartField(quotes: .yes, dashes: .no)
        keys("\"well--known\"")
        XCTAssertEqual(target.text, "“well--known”")
    }

    /// Each of Apple's keyboards types its language's quotes (black box, iOS 27.2): „Hallo“ ‚tag‘
    /// in German, where the apostrophe key types ‘ in a word too; « Bonjour » with no-break
    /// spaces inside and ‘salut’ in French; „tak” ‚nie’ in Polish; «да» in Russian.
    func testSmartQuotesFollowTheKeysLanguage() {
        smartField(language: .german)
        keys("\"hallo\" 'tag' geht's")
        XCTAssertEqual(target.text, "„hallo“ ‚tag‘ geht‘s")
        smartField(language: .french)
        keys("\"bonjour\" 'salut' l'ami")
        XCTAssertEqual(target.text, "«\u{00A0}bonjour\u{00A0}» ‘salut’ l’ami")
        smartField(language: .frenchCanadian)
        keys("\"oui\"")
        XCTAssertEqual(target.text, "«\u{00A0}oui\u{00A0}»")
        smartField(language: .polish)
        keys("\"tak\" 'nie'")
        XCTAssertEqual(target.text, "„tak” ‚nie’")
        smartField(language: .russian)
        keys("\"1\" '2'")
        XCTAssertEqual(target.text, "«1» ‘2’")
    }

    /// Apple's keyboard (black box, iOS 27.2) puts , . ! ? ; : and ) against a picked
    /// suggestion's word, taking the space the suggestion typed; a quote, a bracket or @ keeps
    /// it, and so does a space you typed yourself. A field's smartInsertDeleteType changed
    /// nothing there.
    func testMarksGoAgainstAPickedSuggestion() {
        for (mark, expected) in [(",", "hello,"), (".", "hello."), (")", "hello)"), ("(", "hello ("), ("@", "hello @")] {
            smartField(quotes: .no)
            type("hel")
            engine.acceptSuggestion(KeyboardSuggestion(displayText: "hello", replacement: "hello", kind: .secondary))
            keys(mark)
            XCTAssertEqual(target.text, expected, mark)
        }
        smartField()
        type("hel")
        engine.acceptSuggestion(KeyboardSuggestion(displayText: "hello", replacement: "hello", kind: .secondary))
        keys("\"")
        XCTAssertEqual(target.text, "hello “")
        smartField()
        type("hello ")
        keys(".")
        XCTAssertEqual(target.text, "hello .", "a space you typed stays")
    }

    /// X (Erick): "If I add punctuation after a typo autocorrect does not fix the typo." A
    /// mark that closes a word fixes it as space does and goes in after the fix, as on Apple's
    /// keyboard; delete right after takes the mark, and the strip offers the typo back.
    func testPunctuationAfterATypoFixesItFirst() {
        for mark in [",", ".", "!", "?", ";", ":", ")", "\""] {
            target = FixtureTextTarget()
            target.autocapitalizationType = UITextAutocapitalizationType.none
            engine = FullKeyboardEngine(target: target, autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults)))
            engine.width = 393
            engine.clock = { [unowned self] in self.now }
            engine.configureForField()
            type("see teh")
            punctuate(mark)
            XCTAssertEqual(target.text, "see the" + mark, mark)
            type(" ok")
            XCTAssertEqual(target.text, "see the\(mark) ok", mark)
        }

        type(" recieve")
        punctuate(".")
        XCTAssertTrue(target.text.hasSuffix(" receive."))
        tap(.delete)
        XCTAssertTrue(target.text.hasSuffix(" receive"), "delete takes the mark")
        XCTAssertEqual(engine.suggestions.first?.kind, .revert)
        engine.acceptSuggestion(engine.suggestions[0])
        XCTAssertTrue(target.text.hasSuffix(" recieve"), "the strip puts back what was typed")
        punctuate(".")
        XCTAssertTrue(target.text.hasSuffix(" recieve."), "the word you kept stays")

        type(" approx")
        punctuate(".")
        XCTAssertTrue(target.text.hasSuffix(" approx."), "an abbreviation's period")
        type(" a")
        punctuate(",")
        XCTAssertTrue(target.text.hasSuffix(" a,"))
    }

    /// X (Philippe): French puts a space before ? ! ; and :, but Apple's French keyboards type
    /// none (black box, iOS 27.2 simulator, Français (France) AZERTY: "Salut" then ? gave
    /// "Salut?", and "salut ?" kept the plain space typed before it). Hex types the marks as
    /// tapped too, in every French layout; dictation adds French spacing (`FrenchSpacing`).
    func testFrenchMarksGoInAsTyped() {
        for language in [KeyboardLanguage.french, .frenchCanadian, .frenchSwiss] {
            target = FixtureTextTarget()
            target.autocapitalizationType = UITextAutocapitalizationType.none
            engine = FullKeyboardEngine(target: target, autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults)))
            engine.width = 393
            engine.clock = { [unowned self] in self.now }
            engine.language = language
            engine.configureForField()
            type("salut")
            punctuate("?")
            XCTAssertEqual(target.text, "salut?", language.rawValue)
            type(" oui ")
            punctuate("!")
            XCTAssertEqual(target.text, "salut? oui !", "\(language.rawValue): the space you typed stays as it is")
            type(" non")
            punctuate(":")
            XCTAssertEqual(target.text, "salut? oui ! non:", language.rawValue)
        }
    }

    func testPunctuationNeverChangesASwipedWord() throws {
        let (field, plane) = try swipeKeyboard()
        plane.engine.autocorrect.loadLexiconEntries([("hello", "Hello, friend")])
        swipe("hello", on: plane)
        tap(.layerSwitch(.numbers), on: plane)
        tap(.character(","), on: plane)
        XCTAssertEqual(field.text, "hello,", "a swiped word is a reading, not a typo")
    }

    func testDeleteSwipeWordsMatchOptionDelete() {
        XCTAssertNil(DeleteWordSwipe.wordLength(before: ""))
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "Hello, world. "), 7)
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "it's don't"), 5)
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "Olá, você"), 4)
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "?!"), 2)
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "line\n"), 5)
    }

    // MARK: - Swipe typing

    /// A 390 pt keyboard with Swipe typing on: its key plane, typing into a field holding `text`.
    private func swipeKeyboard(_ text: String = "", language: KeyboardLanguage = .english) throws -> (FixtureTextTarget, KeyboardKeyPlaneView) {
        let field = FixtureTextTarget(text: text)
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let keys = FullKeyboardEngine(
            target: field,
            autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults))
        )
        keys.width = 390
        keys.language = language
        keys.configureForField()
        let plane = KeyboardKeyPlaneView(engine: keys)
        plane.swipe = SwipeTyping(engine: keys, model: try XCTUnwrap(TypingDecoderModel.bundled(language.rawValue)))
        return (field, plane)
    }

    /// Hex's vocabulary list reaches Swipe typing: a word no dictionary or model has
    /// ("kubectl") is swiped once it's in the list.
    func testSwipeTypesYourVocabularyWords() throws {
        let (before, beforePlane) = try swipeKeyboard()
        swipe("kubectl", on: beforePlane)
        XCTAssertNotEqual(before.text, "kubectl", "not a word Swipe typing knows")
        let (field, plane) = try swipeKeyboard()
        plane.engine.autocorrect.setVocabulary(["kubectl"])
        swipe("kubectl", on: plane)
        XCTAssertEqual(field.text, "kubectl")
    }

    private func sample(_ point: CGPoint) -> TypingLogTouchSample {
        TypingLogTouchSample(point: point, timestamp: 0)
    }

    /// Swipes `word` through its keys' centres; `lift` false leaves the finger down.
    private func swipe(_ word: String, on plane: KeyboardKeyPlaneView, id: Int = 50, lift: Bool = true) {
        swipe(points: SwipePath.through(word, keys: plane.engine.swipeKeys), on: plane, id: id, lift: lift)
    }

    private func swipe(points: [CGPoint], on plane: KeyboardKeyPlaneView, id: Int = 50, lift: Bool = true) {
        plane.began(id: id, sample: sample(points[0]))
        for point in points.dropFirst() {
            plane.moved(id: id, sample: sample(point))
        }
        if lift { plane.ended(id: id, sample: sample(points[points.count - 1])) }
    }

    private func tap(_ kind: KeyKind, on plane: KeyboardKeyPlaneView, id: Int = 60) {
        let key = plane.engine.layout.key(kind)!
        let centre = CGPoint(x: key.frame.midX, y: key.frame.midY)
        plane.began(id: id, sample: sample(centre))
        plane.ended(id: id, sample: sample(centre))
    }

    func testKeyFeedbackFiresOnceOnTouchDownBeforeTheKeyTypes() {
        engine.autocorrect.isEnabled = false
        let plane = KeyboardKeyPlaneView(engine: engine)
        var downs: [(kind: KeyKind, text: String)] = []
        plane.onKeyDown = { [unowned self] kind in downs.append((kind, self.target.text)) }

        // Rollover: the second letter ticks as it goes down, before either letter is typed.
        plane.began(id: 1, sample: sample(point(.character("t"))))
        plane.began(id: 2, sample: sample(point(.character("h"))))
        XCTAssertEqual(downs.map(\.kind), [.character("t"), .character("h")])
        XCTAssertEqual(downs.map(\.text), ["", ""], "the tick comes before the rollover types t")
        plane.ended(id: 1, sample: sample(point(.character("t"))))
        plane.ended(id: 2, sample: sample(point(.character("h"))))
        XCTAssertEqual(target.text, "th")
        XCTAssertEqual(downs.count, 2, "lifting a key never ticks")

        // Every other key ticks once on the way down, before it acts.
        downs.removeAll()
        let kinds: [KeyKind] = [.space, .delete, .shift, .returnKey, .layerSwitch(.numbers)]
        for kind in kinds { tap(kind, on: plane) }
        XCTAssertEqual(downs.map(\.kind), kinds)
        XCTAssertEqual(downs.map(\.text), ["th", "th ", "th", "th", "th\n"])
    }

    func testASwipeTypesItsWordAndTheNextOneASpaceLater() throws {
        let (field, plane) = try swipeKeyboard()
        swipe("hello", on: plane)
        XCTAssertEqual(field.text, "hello", "no space before the first word, none after it")
        XCTAssertFalse(plane.engine.isSwiping)
        swipe("world", on: plane)
        XCTAssertEqual(field.text, "hello world")
        tap(.space, on: plane)
        swipe("keyboard", on: plane)
        XCTAssertEqual(field.text, "hello world keyboard", "no second space after one typed")
    }

    /// TestFlight (build 56): "Swipe keyboard doesn't handle next word capitalization
    /// correctly. Also forgets to add a space sometimes. For example try swiping 'this is a
    /// test'. Example: This Isa test." In a field that capitalises sentences, swiping "this"
    /// and "is", then tapping "a" (or sliding on it a little, or a little too far and back),
    /// then swiping "test" types "This is a test"; after a full stop the next swipe starts the
    /// sentence with a capital.
    func testThisIsATestSwipedWithASingleLetterBetween() throws {
        for step in ["tap", "small slide", "slide out and back"] {
            let (field, plane) = try swipeKeyboard()
            field.autocapitalizationType = .sentences
            plane.engine.refreshAutoCapitalization()
            swipe("this", on: plane)
            swipe("is", on: plane)
            let key = try XCTUnwrap(plane.engine.layout.key(.character("a")))
            let a = CGPoint(x: key.frame.midX, y: key.frame.midY)
            switch step {
            case "tap":
                tap(.character("a"), on: plane)
            case "small slide":
                swipe(points: [a, CGPoint(x: a.x + 4, y: a.y), CGPoint(x: a.x + 8, y: a.y + 2)], on: plane)
            default:
                // Past the swipe threshold (a key's pitch), so it is read as a swipe.
                let far = plane.engine.layout.swipePitch * 1.3
                let out = stride(from: 0, through: far, by: 3).map { CGPoint(x: a.x + $0, y: a.y) }
                swipe(points: out + out.reversed().dropFirst(), on: plane)
            }
            swipe("test", on: plane)
            XCTAssertEqual(field.text, "This is a test", step)
        }

        let (field, plane) = try swipeKeyboard()
        field.autocapitalizationType = .sentences
        plane.engine.refreshAutoCapitalization()
        for word in ["this", "is"] { swipe(word, on: plane) }
        tap(.character("a"), on: plane)
        swipe("test", on: plane)
        tap(.layerSwitch(.numbers), on: plane)
        tap(.character("."), on: plane)
        tap(.layerSwitch(.letters), on: plane)
        swipe("it", on: plane)
        swipe("works", on: plane)
        XCTAssertEqual(field.text, "This is a test. It works")

        // A word a swipe back to its first key can mean ("lol" on l) still reads as itself.
        XCTAssertTrue(SwipeTyping.word("lol", startsAndEndsWith: "l"))
        XCTAssertFalse(SwipeTyping.word("as", startsAndEndsWith: "a"))
    }

    /// The plane hands the decoder each touch's time, so a pause on a letter the stroke runs
    /// over counts: "tired" with the finger slowing on r, "tied" on the same path without.
    func testASwipeReadsWhereTheFingerPaused() throws {
        for (pauses, word) in [(Set([2]), "tired"), (Set<Int>(), "tied")] {
            let (field, plane) = try swipeKeyboard()
            let (points, times) = SwipePath.timed("tired", keys: plane.engine.swipeKeys, pausingOn: pauses)
            let start: TimeInterval = 100
            plane.began(id: 50, sample: TypingLogTouchSample(point: points[0], timestamp: start + times[0]))
            for (point, time) in zip(points, times).dropFirst() {
                plane.moved(id: 50, sample: TypingLogTouchSample(point: point, timestamp: start + time))
            }
            plane.ended(id: 50, sample: TypingLogTouchSample(point: points[points.count - 1], timestamp: start + times[times.count - 1]))
            XCTAssertEqual(field.text, word, "pauses \(pauses)")
        }
    }

    /// TestFlight (build 56): "Manual typing right after swipe typing a word should add a space
    /// after for the next word." Apple's keyboard (iOS 27.2, watched in Messages) keeps a space
    /// pending after a swiped word: a letter, digit, opening mark or symbol goes in after it,
    /// closing punctuation goes against the word, and delete takes the letters, then the space.
    func testATapAfterASwipeStartsTheNextWordAsOnApplesKeyboard() throws {
        let (field, plane) = try swipeKeyboard()
        swipe("hello", on: plane)
        for letter in "world" { tap(.character(String(letter)), on: plane) }
        XCTAssertEqual(field.text, "hello world")
        for _ in 0..<5 { tap(.delete, on: plane) }
        XCTAssertEqual(field.text, "hello ", "delete takes the letters, then the space")
        tap(.delete, on: plane)
        XCTAssertEqual(field.text, "hello")

        for (mark, typed) in [(",", "hello,"), (".", "hello."), ("?", "hello?"), ("'", "hello'"), ("-", "hello-"), ("(", "hello ("), ("2", "hello 2"), ("@", "hello @"), ("\"", "hello \"")] {
            let (field, plane) = try swipeKeyboard()
            swipe("hello", on: plane)
            tap(.layerSwitch(.numbers), on: plane)
            tap(.character(mark), on: plane)
            XCTAssertEqual(field.text, typed, mark)
        }

        let (comma, keys) = try swipeKeyboard()
        swipe("hello", on: keys)
        tap(.layerSwitch(.numbers), on: keys)
        tap(.character(","), on: keys)
        tap(.layerSwitch(.letters), on: keys)
        tap(.character("w"), on: keys)
        XCTAssertEqual(comma.text, "hello,w", "the mark took the pending space")
        swipe("world", on: keys)
        XCTAssertEqual(comma.text, "hello,w world")

        let (shifted, shiftKeys) = try swipeKeyboard("Hi")
        shifted.autocapitalizationType = .sentences
        shiftKeys.engine.refreshAutoCapitalization()
        swipe("there", on: shiftKeys)
        tap(.character("y"), on: shiftKeys)
        XCTAssertEqual(shifted.text, "Hi there y", "no capital mid-sentence")
        swipe("you", on: shiftKeys)
        tap(.shift, on: shiftKeys)
        tap(.character("w"), on: shiftKeys)
        XCTAssertEqual(shifted.text, "Hi there y you W", "shift you set stays on")
    }

    func testASwipedWordFollowsSpacingAndPunctuationAsApplesDoes() {
        for (before, after) in [("Hi.", "Hi. "), ("(", "("), ("¿", "¿"), ("say \"", "say \""), ("3", "3 "), ("x\n", "x\n")] {
            XCTAssertEqual(SwipeTyping.needsSpace(before: before), after != before, before)
        }
        XCTAssertFalse(SwipeTyping.needsSpace(before: ""))
        XCTAssertFalse(SwipeTyping.needsSpace(before: nil))
        XCTAssertTrue(SwipeTyping.needsSpace(before: "\"hi\""), "a closing quote")
    }

    func testShiftGivesASwipedWordItsCapital() throws {
        let (field, plane) = try swipeKeyboard()
        tap(.shift, on: plane)
        swipe("hello", on: plane)
        XCTAssertEqual(field.text, "Hello")
        swipe("world", on: plane)
        XCTAssertEqual(field.text, "Hello world", "one-shot shift turned off")

        let (sentence, keys) = try swipeKeyboard("Great.")
        sentence.autocapitalizationType = .sentences
        keys.engine.refreshAutoCapitalization()
        swipe("thanks", on: keys)
        XCTAssertEqual(sentence.text, "Great. Thanks", "the space the swipe adds starts a sentence")

        let (locked, capsKeys) = try swipeKeyboard()
        tap(.shift, on: capsKeys)
        tap(.shift, on: capsKeys)
        swipe("people", on: capsKeys)
        XCTAssertEqual(locked.text, "PEOPLE", "caps lock")
    }

    /// Auto-capitalization off: no sentence-start capital on a swiped word, as on Apple's
    /// keyboard with it off; shift still gives one.
    func testAutoCapitalizationOffLeavesASwipedWordLowercase() throws {
        let (sentence, keys) = try swipeKeyboard("Great.")
        sentence.autocapitalizationType = .sentences
        keys.engine.autoCapitalizes = false
        swipe("thanks", on: keys)
        XCTAssertEqual(sentence.text, "Great. thanks", "the space the swipe adds starts no sentence")
        tap(.shift, on: keys)
        swipe("hello", on: keys)
        XCTAssertEqual(sentence.text, "Great. thanks Hello", "shift still capitalises")
    }

    /// X (Eric Jung): "the swipe keyboard has random auto capitalization of words." Shift and
    /// auto-capitalization around swipes were right (after a period, a return, a comma, a
    /// delete, a pick); the capitals came from readings: the word list's names and
    /// abbreviations, which the dictionary knows only capitalised ("kent", "gs"), and "I'm"
    /// read after a word "I" never follows. A name now needs more evidence than a common word,
    /// your own words don't, and a contraction follows the word before.
    func testASwipeMidSentenceGetsNoStrayCapital() throws {
        let (field, plane) = try swipeKeyboard("Great.")
        field.autocapitalizationType = .sentences
        plane.engine.refreshAutoCapitalization()
        let swipe = try XCTUnwrap(plane.swipe)
        XCTAssertTrue(plane.engine.swipeReadsAsName("kent"))
        XCTAssertTrue(plane.engine.swipeReadsAsName("york"))
        XCTAssertFalse(plane.engine.swipeReadsAsName("many"))
        XCTAssertFalse(plane.engine.swipeReadsAsName("Kent"), "your own spelling")
        let close = [SwipeDecoder.Candidate(text: "kent", cost: 10), SwipeDecoder.Candidate(text: "many", cost: 12)]
        XCTAssertEqual(swipe.ranked(close), ["many", "kent"])
        let clear = [SwipeDecoder.Candidate(text: "kent", cost: 10), SwipeDecoder.Candidate(text: "many", cost: 20)]
        XCTAssertEqual(swipe.ranked(clear), ["kent", "many"])

        for word in ["thanks", "for", "the", "help"] { self.swipe(word, on: plane) }
        tap(.layerSwitch(.numbers), on: plane)
        tap(.character(","), on: plane)
        tap(.space, on: plane)
        for word in ["see", "you", "in", "york"] { self.swipe(word, on: plane) }
        XCTAssertEqual(field.text, "Great. Thanks for the help, see you in York", "a common name keeps its capital")

        let decoder = swipe.decoder
        let path = SwipePath.through("im", keys: plane.engine.swipeKeys)
        let cost = { (previous: String) in
            decoder.decode(path, keys: plane.engine.swipeKeys, previous: previous, limit: 8).first { $0.text == "I'm" }?.cost
        }
        let afterThink = try XCTUnwrap(cost("think")), afterUs = try XCTUnwrap(cost("us"))
        XCTAssertGreaterThan(afterUs - afterThink, 3, "\"I'm\" after \"think\", hardly after \"us\"")
    }

    /// TestFlight (Jason, build 56): "Swipe typing keeps auto capitalising in most cases, even
    /// when it shouldn't. It doesn't work how Apple's keyboard does, where it capitalises only
    /// after a full stop." Shift and auto-capitalisation around swipes are right on dev (after
    /// a comma, a swipe, a pick from the strip, a delete, a tapped word, and a sentence-start
    /// capital you turned off; a capital after "." and for "I'm"). What still gave a capital
    /// mid-sentence was your own words: a Contacts name that is also a common word ("Bell",
    /// "Will") read as the name. Apple's keyboard (black box, iOS 27.2, contact Kate Bell)
    /// swipes "the bell rang" in lowercase.
    func testSwipedWordsTakeACapitalOnlyWhereApplesKeyboardGivesOne() throws {
        let (field, plane) = try swipeKeyboard()
        field.autocapitalizationType = .sentences
        plane.engine.autocorrect.loadLexiconEntries([("Bell", "Bell"), ("Will", "Will"), ("Taylor", "Taylor")])
        plane.engine.refreshAutoCapitalization()
        XCTAssertTrue(plane.engine.shift.isUppercase, "an empty field starts a sentence")
        tap(.shift, on: plane)
        swipe("hello", on: plane)
        swipe("there", on: plane)
        XCTAssertEqual(field.text, "hello there", "the capital you turned off stays off")
        let pick = try XCTUnwrap(plane.engine.suggestions.first)
        plane.engine.acceptSuggestion(pick)
        swipe("the", on: plane)
        swipe("bell", on: plane)
        tap(.delete, on: plane)
        swipe("bell", on: plane)
        swipe("will", on: plane)
        XCTAssertEqual(field.text, "hello \(pick.replacement) the bell will", "after a pick and a delete")
        tap(.layerSwitch(.numbers), on: plane)
        tap(.character(","), on: plane)
        tap(.layerSwitch(.letters), on: plane)
        swipe("ring", on: plane)
        for letter in "now" { tap(.character(String(letter)), on: plane) }
        tap(.space, on: plane)
        swipe("for", on: plane)
        tap(.layerSwitch(.numbers), on: plane)
        tap(.character("."), on: plane)
        tap(.layerSwitch(.letters), on: plane)
        swipe("will", on: plane)
        XCTAssertEqual(field.text, "hello \(pick.replacement) the bell will, ring now for. Will", "a capital only after the full stop")
        XCTAssertEqual(plane.engine.swipeCased("Taylor", capital: false), "Taylor", "a name keeps its capital")
        XCTAssertEqual(plane.engine.swipeCased("Bell", capital: false), "bell")
        XCTAssertEqual(plane.engine.swipeCased("I'm", capital: false), "I'm")
    }

    /// TestFlight (build 56, iPhone 17 Pro, Auto-capitalization on): "Getting random words
    /// capitalized", the field reading "Testing how this World" and the strip "working | Works
    /// | writing". Reproduced on build 56 with "World" and "Works" as `UILexicon` terms (text
    /// replacements whose phrase is the word, as a Contacts name is); #2173 fixed it with
    /// Auto-capitalization on, but with it off a swipe still wrote them capitalised. Apple's
    /// keyboard (black box, iOS 27.2) swipes them lowercase mid-sentence. A word in your
    /// vocabulary keeps the capitals you gave it either way.
    func testSwipedCommonWordsStayLowercaseMidSentenceAndVocabularyKeepsItsCapitals() throws {
        for autoCapitalizes in [true, false] {
            let (field, plane) = try swipeKeyboard()
            field.autocapitalizationType = .sentences
            plane.engine.autoCapitalizes = autoCapitalizes
            plane.engine.autocorrect.loadLexiconEntries([("World", "World"), ("Works", "Works")])
            plane.engine.autocorrect.setVocabulary(["Swift"])
            plane.engine.refreshAutoCapitalization()
            for word in ["testing", "how", "this", "works"] { swipe(word, on: plane) }
            let mode = autoCapitalizes ? "on" : "off"
            XCTAssertEqual(field.text, (autoCapitalizes ? "Testing" : "testing") + " how this works", mode)
            XCTAssertFalse(plane.engine.suggestions.contains { $0.replacement == "World" || $0.replacement == "Works" }, mode)
            XCTAssertEqual(plane.engine.swipeCased("World", capital: false), "world", mode)
            XCTAssertEqual(plane.engine.swipeCased("Swift", capital: false), "Swift", mode)
            XCTAssertEqual(plane.engine.swipeCased("world", capital: true), "World", "a capital shift gives, \(mode)")
        }
    }

    func testDeleteRightAfterASwipeTakesTheWholeWordBack() throws {
        let (field, plane) = try swipeKeyboard()
        swipe("hello", on: plane)
        swipe("world", on: plane)
        tap(.delete, on: plane)
        XCTAssertEqual(field.text, "hello", "the word and the space it brought")
        tap(.delete, on: plane)
        XCTAssertEqual(field.text, "hell", "then one letter at a time")
        swipe("people", on: plane)
        tap(.character("s"), on: plane)
        tap(.delete, on: plane)
        XCTAssertEqual(field.text, "hell people ", "only right after the swipe; the space a tap brought stays")
    }

    func testTheStripOffersTheSwipesOtherReadingsInItsWordsPlace() throws {
        let (field, plane) = try swipeKeyboard("a")
        swipe("of", on: plane)
        XCTAssertEqual(field.text, "a of")
        let readings = plane.engine.suggestions
        XCTAssertTrue(readings.allSatisfy { $0.kind == .swiped })
        XCTAssertEqual(KeyboardStripView.slots(for: readings).count, readings.count)
        let off = try XCTUnwrap(readings.first { $0.replacement == "off" })
        XCTAssertEqual(readings.count >= 2 ? readings[1].replacement : readings[0].replacement, "off", "the likeliest in the centre")
        plane.engine.acceptSuggestion(off)
        XCTAssertEqual(field.text, "a off", "no space after it, as after the swipe")
        XCTAssertTrue(plane.engine.suggestions.contains { $0.replacement == "of" }, "the swiped word joins the others")
        tap(.delete, on: plane)
        XCTAssertEqual(field.text, "a", "delete still takes the swipe back")
    }

    func testSpaceNeverChangesASwipedWord() throws {
        let (field, plane) = try swipeKeyboard()
        plane.engine.autocorrect.loadLexiconEntries([("hello", "Hello, friend")])
        swipe("hello", on: plane)
        tap(.space, on: plane)
        XCTAssertEqual(field.text, "hello ", "a text replacement applies to what you type, not to a swiped word")
    }

    /// Apple's keyboard (black box, iOS 27.2) offers a text replacement in the strip after a
    /// swipe that read as its shortcut, and space leaves the swiped word; a shortcut tapped
    /// after a swiped word goes in on space as anywhere else.
    func testASwipedShortcutOffersItsReplacementAndATappedOneGoesIn() throws {
        let (field, plane) = try swipeKeyboard("I'm")
        plane.engine.autocorrect.loadLexiconEntries([("home", "at my house"), ("myem", "jason@example.com")])
        swipe("home", on: plane)
        XCTAssertEqual(field.text, "I'm home")
        let offer = try XCTUnwrap(plane.engine.suggestions.first { $0.replacement == "at my house" }, "\(plane.engine.suggestions)")
        XCTAssertEqual(plane.engine.suggestions.count >= 2 ? plane.engine.suggestions[1] : plane.engine.suggestions[0], offer, "in the centre")
        plane.engine.acceptSuggestion(offer)
        XCTAssertEqual(field.text, "I'm at my house")
        tap(.delete, on: plane)
        XCTAssertEqual(field.text, "I'm", "delete still takes what the swipe put in back")

        swipe("hello", on: plane)
        for letter in "myem" { tap(.character(String(letter)), on: plane) }
        tap(.space, on: plane)
        XCTAssertEqual(field.text, "I'm hello jason@example.com ")
    }

    func testTapsShortSlidesAndAccentsStillTypeWithSwipeTypingOn() throws {
        let (field, plane) = try swipeKeyboard()
        for letter in "the" { tap(.character(String(letter)), on: plane) }
        XCTAssertEqual(field.text, "the")
        // A slide onto the next key (less than a key's pitch) types where the finger lifts.
        let f = plane.engine.layout.key(.character("f"))!.frame, g = plane.engine.layout.key(.character("g"))!.frame
        let onG = CGPoint(x: g.midX - 4, y: g.midY)
        plane.began(id: 1, sample: sample(CGPoint(x: f.midX, y: f.midY)))
        plane.moved(id: 1, sample: sample(onG))
        XCTAssertFalse(plane.engine.isSwiping)
        plane.ended(id: 1, sample: sample(onG))
        XCTAssertEqual(field.text, "theg")
        // Held for its accents, a letter's drag picks one, however far.
        let e = plane.engine.layout.key(.character("e"))!.frame
        plane.began(id: 2, sample: sample(CGPoint(x: e.midX, y: e.midY)))
        XCTAssertTrue(plane.engine.openAccentMenuIfEligible(forTouch: 2))
        let menu = try XCTUnwrap(plane.engine.activeAccentMenu)
        let cell = menu.cellFrames[2]
        plane.moved(id: 2, sample: sample(CGPoint(x: cell.midX, y: e.midY)))
        XCTAssertFalse(plane.engine.isSwiping)
        plane.ended(id: 2, sample: sample(CGPoint(x: cell.midX, y: e.midY)))
        XCTAssertEqual(field.text, "theg" + menu.options[2])
    }

    /// Holding 123 or shift and sliding across keys is never a swipe, however far it goes.
    func testA123OrShiftSlideNeverStartsASwipe() throws {
        let (field, plane) = try swipeKeyboard()
        func centre(_ kind: KeyKind) -> CGPoint {
            let key = plane.engine.layout.key(kind)!.frame
            return CGPoint(x: key.midX, y: key.midY)
        }
        plane.began(id: 1, sample: sample(centre(.layerSwitch(.numbers))))
        for digit in "1234567" { plane.moved(id: 1, sample: sample(centre(.character(String(digit))))) }
        XCTAssertFalse(plane.engine.isSwiping)
        XCTAssertEqual(plane.engine.previewKey?.kind, .character("7"))
        plane.ended(id: 1, sample: sample(centre(.character("7"))))
        XCTAssertEqual(field.text, "7")
        XCTAssertEqual(plane.engine.layout.layer, .letters)

        plane.began(id: 2, sample: sample(centre(.shift)))
        for letter in "asdfgh" { plane.moved(id: 2, sample: sample(centre(.character(String(letter))))) }
        XCTAssertFalse(plane.engine.isSwiping)
        plane.ended(id: 2, sample: sample(centre(.character("h"))))
        XCTAssertEqual(field.text, "7H")
    }

    func testDeleteAndTheSpaceBarNeverStartASwipe() throws {
        let (field, plane) = try swipeKeyboard("one two three")
        let step = plane.engine.layout.letterWidth
        let delete = plane.engine.layout.key(.delete)!.frame
        plane.began(id: 1, sample: sample(CGPoint(x: delete.midX, y: delete.midY)))
        plane.moved(id: 1, sample: sample(CGPoint(x: delete.midX - step * 2.2, y: delete.midY)))
        plane.ended(id: 1, sample: sample(CGPoint(x: delete.midX - step * 2.2, y: delete.midY)))
        XCTAssertEqual(field.text, "one ", "delete's slide deletes word by word")
        XCTAssertFalse(plane.engine.isSwiping)

        let space = plane.engine.layout.key(.space)!.frame
        plane.began(id: 2, sample: sample(CGPoint(x: space.midX, y: space.midY)))
        plane.moved(id: 2, sample: sample(CGPoint(x: space.midX - 60, y: space.midY)))
        plane.ended(id: 2, sample: sample(CGPoint(x: space.midX - 60, y: space.midY)))
        XCTAssertEqual(field.text, "one  ", "a quick slide along the space bar is a space, as on Apple's")
        XCTAssertFalse(plane.engine.isSwiping)

        // With two languages a quick slide along the space bar switches language, never a swipe.
        var steps: [Int] = []
        plane.engine.onLanguageSwipe = { steps.append($0) }
        plane.engine.switchesLanguage = true
        plane.began(id: 3, sample: sample(CGPoint(x: space.midX, y: space.midY)))
        plane.moved(id: 3, sample: sample(CGPoint(x: space.midX - 25, y: space.midY)))
        plane.moved(id: 3, sample: sample(CGPoint(x: space.midX - 60, y: space.midY)))
        plane.ended(id: 3, sample: sample(CGPoint(x: space.midX - 60, y: space.midY)))
        XCTAssertEqual(steps, [1])
        XCTAssertFalse(plane.engine.isSwiping)
        XCTAssertEqual(field.text, "one  ")
    }

    /// Swipe typing follows the language the space bar switches to: on for English and
    /// Spanish (each with its own model), off for any other.
    func testSwipeTypingFollowsALiveLanguageSwitch() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .german, .spanish]
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        view.languageSettings = settings
        view.swipeOverride = true
        XCTAssertEqual(view.engine.language, .english)
        XCTAssertEqual(view.plane.swipe?.model.language, "en")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .german)
        XCTAssertNil(view.plane.swipe, "German: a drag stays a slide to another key")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .spanish)
        XCTAssertEqual(view.plane.swipe?.model.language, "es")
    }

    /// The input mode's language (`UIInputViewController.primaryLanguage`), which Apple
    /// documents as superseding the Info.plist's en-US: the keyboard reports the language
    /// resolved before the controller attaches, and each space-bar switch.
    func testTheInputModeLanguageFollowsTheSpaceBar() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.italian, .english]
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        view.languageSettings = settings
        XCTAssertEqual(view.reportedInputModeLanguage, "it-IT")

        let controller = RecordingInputController()
        view.hostController = controller
        XCTAssertEqual(controller.primaryLanguage, "it-IT", "reported when the controller attaches")
        view.switchLanguage(by: 1)
        XCTAssertEqual(controller.primaryLanguage, "en-US")
        view.switchLanguage(by: 1)
        XCTAssertEqual(controller.primaryLanguage, "it-IT")
        XCTAssertEqual(controller.reportedLanguages, ["it-IT", "en-US", "it-IT"], "each change sent once")
    }

    func testOnlyTheEnglishAndSpanishLayoutsSwipe() {
        let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget())
        view.swipeOverride = true
        for language in KeyboardLanguage.allCases {
            view.languageOverride = language
            XCTAssertEqual(view.plane.swipe != nil, language == .english || language == .spanish, language.rawValue)
            if let swipe = view.plane.swipe {
                XCTAssertEqual(swipe.model.language, language.rawValue)
            }
        }
        view.languageOverride = .english
        view.swipeOverride = false
        XCTAssertNil(view.plane.swipe, "off when turned off")
        view.swipeOverride = nil
        XCTAssertEqual(view.plane.swipe != nil, FullKeyboardSettings().swipeTyping)
    }

    /// On the Duo's split keyboard (unfolded upright) a swipe reads across the middle between
    /// the halves, as Gboard joins them, and either half's G and V is the one letter.
    func testASwipeReadsAcrossTheSplitKeyboardsMiddle() throws {
        let (field, plane) = try swipeKeyboard()
        let keys = plane.engine
        keys.posture = .unfoldedWide
        keys.windowInset = 75
        keys.width = 801
        let layout = keys.layout
        let split = try XCTUnwrap(layout.split)
        XCTAssertEqual(plane.swipe?.threshold, SplitGeometry.pitch)
        /// Straight strokes through the drawn keys; of a letter's two keys, the one nearer the
        /// key before it (the first letter's: the side `firstOnRight` says, else the left).
        func path(_ word: String, firstOnRight: Bool = false) -> [CGPoint] {
            var centres: [CGPoint] = []
            for letter in word {
                let options = layout.keys.filter { $0.label == String(letter) }.map { CGPoint(x: $0.frame.midX, y: $0.frame.midY) }
                let anchor = centres.last ?? CGPoint(x: firstOnRight ? layout.width : 0, y: options[0].y)
                let key = options.min { hypot($0.x - anchor.x, $0.y - anchor.y) < hypot($1.x - anchor.x, $1.y - anchor.y) }!
                if centres.last != key { centres.append(key) }
            }
            var points = [centres[0]]
            for (a, b) in zip(centres, centres.dropFirst()) {
                let count = max(1, Int((hypot(b.x - a.x, b.y - a.y) / 6).rounded(.up)))
                for i in 1...count {
                    let t = CGFloat(i) / CGFloat(count)
                    points.append(CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t))
                }
            }
            return points
        }
        let hello = path("hello")
        XCTAssertTrue(hello.contains { $0.x < split.leftEdge } && hello.contains { $0.x > split.rightEdge }, "both halves")
        for word in ["hello", "the", "keyboard"] {
            swipe(points: path(word), on: plane)
        }
        XCTAssertEqual(field.text, "hello the keyboard")
        swipe(points: path("give", firstOnRight: true), on: plane)
        XCTAssertEqual(field.text, "hello the keyboard give", "from the right half's G")
    }

    func testASpanishSwipeGetsItsAccentsAndÑ() throws {
        let (field, plane) = try swipeKeyboard(language: .spanish)
        swipe("mañana", on: plane)
        swipe("también", on: plane)
        XCTAssertEqual(field.text, "mañana también")
    }

    func testASecondFingerTypesTheSwipesWordFirst() throws {
        let (field, plane) = try swipeKeyboard()
        swipe("hello", on: plane, lift: false)
        XCTAssertTrue(plane.engine.isSwiping)
        tap(.character("s"), on: plane)
        XCTAssertEqual(field.text, "hello s", "the swipe's word, then the tap's letter a space later")
        XCTAssertFalse(plane.engine.isSwiping)
    }

    func testACancelledSwipeTypesNothing() throws {
        let (field, plane) = try swipeKeyboard("x")
        swipe("hello", on: plane, lift: false)
        plane.cancelled(id: 50, sample: sample(.zero))
        XCTAssertEqual(field.text, "x")
        XCTAssertFalse(plane.engine.isSwiping)
        XCTAssertFalse(plane.engine.suggestions.contains { $0.kind == .swiped })
    }

    func testASwipeNeverTypesASlurAndSwearsOnlyWhenAsked() throws {
        let (field, plane) = try swipeKeyboard()
        let words = OffensiveWords.bundled("en")
        for allow in [false, true] {
            plane.engine.autocorrect.allowsSwearing = allow
            for probe in ["fuck", "shit", "nigger", "faggot"] {
                swipe(probe, on: plane)
                let typed = field.text.split(separator: " ").last.map(String.init) ?? ""
                XCTAssertFalse(words.blocks(typed, allowSwearing: allow), "\(probe) typed \(typed), swear words \(allow)")
                for reading in plane.engine.suggestions {
                    XCTAssertFalse(words.blocks(reading.replacement, allowSwearing: allow), "\(probe) offered \(reading.replacement)")
                }
            }
        }
        swipe("fuck", on: plane)
        XCTAssertTrue(field.text.hasSuffix(" fuck"), "with Suggest swear words on")
    }
}


/// The strip's Paste chip: one read per paste, and where to stop iOS's Allow Paste.
@MainActor
final class KeyboardClipboardTests: XCTestCase {
    private var suiteName = ""
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "hex.tests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    func testTheChipReadsTextOrALinkOnce() {
        let pasteboard = UIPasteboard.withUniqueName()
        defer { UIPasteboard.remove(withName: pasteboard.name) }
        XCTAssertNil(KeyboardClipboardTracker.read(pasteboard))
        pasteboard.url = URL(string: "https://hex.example/a")
        XCTAssertEqual(KeyboardClipboardTracker.read(pasteboard), "https://hex.example/a")
        pasteboard.string = "See you at 7"
        XCTAssertEqual(KeyboardClipboardTracker.read(pasteboard), "See you at 7")
    }

    func testAPasteThatAskedPointsToTheSettingAFewTimes() {
        let tracker = KeyboardClipboardTracker(defaults: defaults)
        XCTAssertFalse(tracker.shouldPointToAllowPaste(readDuration: 0.01, refused: false), "Allow: iOS didn't ask")
        for _ in 0..<KeyboardClipboardTracker.allowPasteHintLimit {
            XCTAssertTrue(tracker.shouldPointToAllowPaste(readDuration: 1.2, refused: false))
        }
        XCTAssertFalse(
            KeyboardClipboardTracker(defaults: defaults).shouldPointToAllowPaste(readDuration: 1.2, refused: false),
            "after a few, Ask is left alone: someone may want it"
        )
        XCTAssertTrue(tracker.shouldPointToAllowPaste(readDuration: 0.01, refused: true), "refused: the chip did nothing, always say why")
    }

    func testCodeFromMessagesNeedsApplesKeyboard() {
        let pasteboard = UIPasteboard.withUniqueName()
        defer { UIPasteboard.remove(withName: pasteboard.name) }
        let tracker = KeyboardClipboardTracker(defaults: defaults)
        func offer(appleKeyboard: Bool) -> KeyboardClipboardOffer? {
            tracker.offer(textContentType: .oneTimeCode, hasFullAccess: true, hasAppleKeyboard: { appleKeyboard }, pasteboard: pasteboard)
        }
        XCTAssertEqual(offer(appleKeyboard: true), .code(canPaste: false))
        XCTAssertNil(offer(appleKeyboard: false), "Hex and Emoji only: the next keyboard gets no code (TestFlight build 56)")
        pasteboard.string = "482913"
        XCTAssertEqual(offer(appleKeyboard: true), .code(canPaste: true))
        XCTAssertEqual(offer(appleKeyboard: false), .paste, "a copied code still pastes")
        XCTAssertEqual(
            tracker.offer(textContentType: nil, hasFullAccess: true, hasAppleKeyboard: { XCTFail("asked outside a code field"); return true }, pasteboard: pasteboard),
            .paste
        )
    }

    func testApplesKeyboardIsNotEmojiOrAnExtension() {
        typealias Mode = KeyboardInputModes.Mode
        let english = Mode(primaryLanguage: "en-US", className: "UIKeyboardInputMode")
        let hex = Mode(primaryLanguage: "en-US", className: "UIKeyboardExtensionInputMode")
        let emoji = Mode(primaryLanguage: "emoji", className: "UIKeyboardInputMode")
        XCTAssertTrue(KeyboardInputModes.hasAppleKeyboard([hex, emoji, english]))
        XCTAssertFalse(KeyboardInputModes.hasAppleKeyboard([hex, emoji]))
        XCTAssertFalse(KeyboardInputModes.hasAppleKeyboard([hex]))
        XCTAssertFalse(KeyboardInputModes.hasAppleKeyboard([hex, hex, emoji]), "another third-party keyboard gets no code either")
    }
}


// MARK: - Adaptive touch (Settings › Experiments)

/// A text field with the traits Adaptive touch must not learn in.
@MainActor
private final class TraitField: KeyboardTextTarget {
    private(set) var text = ""
    var autocapitalizationType: UITextAutocapitalizationType? = UITextAutocapitalizationType.none
    var autocorrectionType: UITextAutocorrectionType? = .default
    var keyboardType: UIKeyboardType? = .default
    var returnKeyType: UIReturnKeyType? = .default
    var keyboardAppearance: UIKeyboardAppearance? { nil }
    var textContentType: UITextContentType?
    var isSecureTextEntry: Bool?
    var documentContextBeforeInput: String? { text }
    var hasText: Bool { !text.isEmpty }
    func insertText(_ inserted: String) { text += inserted }
    func deleteBackward() { if !text.isEmpty { text.removeLast() } }
}

extension FullKeyboardEngineTests {
    private func adaptive(on keys: FullKeyboardEngine? = nil) -> AdaptiveTouch {
        let touch = AdaptiveTouch(storage: .memory, settings: AdaptiveTouchSettings(defaults: defaults))
        (keys ?? engine).adaptiveTouch = touch
        return touch
    }

    private func learned(_ touch: AdaptiveTouch) -> (weak: Int, picked: Int, strong: Int) {
        touch.model.contexts.reduce((0, 0, 0)) { ($0.0 + Int($1.weak), $0.1 + Int($1.picked), $0.2 + Int($1.strong)) }
    }

    /// Admits `taps` labelled taps landing `dx` keycaps right of each key's centre, over
    /// `sessions` keyboard sessions.
    private func teach(_ touch: AdaptiveTouch, dx: CGFloat, taps: Int = 420, sessions: Int = 4) throws {
        let geometry = try XCTUnwrap(touch.geometry)
        for n in 0..<taps {
            if n % max(1, taps / sessions) == 0 { touch.beginSession() }
            let s = n % geometry.letters.count
            let f = geometry.frames[s]
            touch.admit(slot: s, point: CGPoint(x: f.midX + dx * f.width, y: f.midY), quality: AdaptiveTouch.weakQuality)
        }
    }

    private func tapAt(_ point: CGPoint, id: Int = 1) {
        engine.touchBegan(id: id, at: point)
        engine.touchEnded(id: id, at: point)
        now += 0.5
    }

    func testAdaptiveTouchLearnsFromAWordOnlyOnceTheNextOneShowsItStood() {
        let touch = adaptive()
        type("hello ")
        XCTAssertEqual(learned(touch).weak, 0, "a word waits for the next one")
        type("world ")
        XCTAssertEqual(learned(touch).weak, 5)
        target.insertText("x")
        engine.textDidChange()
        type(" again ")
        XCTAssertEqual(learned(touch).weak, 5, "the host changed the text: \"world\" teaches nothing")
    }

    func testAdaptiveTouchLearnsNothingFromAnAutocorrectionButKeepsWhatYouUndidItTo() {
        let touch = adaptive()
        type("teh ")
        XCTAssertEqual(target.text, "the ")
        type("cat ")
        XCTAssertEqual(learned(touch).weak, 0, "an autocorrection left in place can't confirm itself")
        type("teh ")
        engine.deleteBackward()
        XCTAssertEqual(engine.suggestions.first?.kind, .revert)
        engine.acceptSuggestion(engine.suggestions[0])
        type(" dog it ")
        XCTAssertEqual(target.text, "the cat teh dog it ")
        XCTAssertEqual(learned(touch).weak, 3 + 3, "cat and dog, as typed")
        XCTAssertEqual(learned(touch).strong, 3, "\"teh\", kept over its fix")
    }

    func testAdaptiveTouchLabelsAnErasedTapWithTheNeighbourThatReplacedIt() {
        let touch = adaptive()
        type("car")
        engine.deleteBackward()
        type("t is it ")
        XCTAssertEqual(target.text, "cat is it ")
        XCTAssertEqual(learned(touch).strong, 1, "the r tap meant t")
        XCTAssertEqual(learned(touch).weak, 3 + 2)

        // A different word, not a slip: p is no neighbour of t.
        type("cat")
        engine.deleteBackward()
        type("p it is ")
        XCTAssertEqual(learned(touch).strong, 1)
    }

    func testAdaptiveTouchLearnsFromAPickedSuggestionOfTheSameLength() {
        let touch = adaptive()
        type("thw")
        engine.acceptSuggestion(KeyboardSuggestion(displayText: "the", replacement: "the", kind: .secondary))
        type("hel")
        engine.acceptSuggestion(KeyboardSuggestion(displayText: "hello", replacement: "hello", kind: .secondary))
        type("is it ")
        XCTAssertEqual(target.text, "the hello is it ")
        XCTAssertEqual(learned(touch).picked, 3, "the w tap meant e")
        XCTAssertEqual(learned(touch).weak, 2, "a completion teaches nothing; \"is\" as typed")
    }

    func testAdaptiveTouchKeepsClaimedAndStripTouchesOutOfItsLabels() throws {
        engine.letterOdds = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        let touch = adaptive()
        let e = engine.layout.key(.character("e"))!
        let d = engine.layout.key(.character("d"))!
        type("th")
        tapAt(CGPoint(x: max(e.frame.minX, d.frame.minX) + 4, y: e.hitFrame.maxY + 2))
        XCTAssertEqual(target.text, "the", "the next-letter odds claimed a d tap for e")
        type(" w")
        let i = engine.layout.key(.character("i"))!.frame
        tapAt(CGPoint(x: i.midX, y: engine.layout.touchAreaFrame.minY - 4))
        type("n it ")
        XCTAssertEqual(target.text, "the win it ")
        XCTAssertEqual(learned(touch).weak, 2 + 2, "neither the claimed e nor the i from the strip")
    }

    func testAdaptiveTouchNeverLearnsFromSwipesOrSensitiveFields() throws {
        func learnedTyping(_ configure: (TraitField) -> Void) -> Int {
            let field = TraitField()
            configure(field)
            let keys = FullKeyboardEngine(target: field, autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults)))
            keys.width = 393
            keys.configureForField()
            let touch = adaptive(on: keys)
            for ch in "hello world again " {
                let key = keys.layout.key(ch == " " ? .space : .character(String(ch)))!
                keys.touchBegan(id: 1, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
                keys.touchEnded(id: 1, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            }
            return learned(touch).weak
        }
        XCTAssertEqual(learnedTyping { _ in }, 10, "a plain field")
        XCTAssertEqual(learnedTyping { $0.textContentType = .password }, 0)
        XCTAssertEqual(learnedTyping { $0.textContentType = .oneTimeCode }, 0)
        XCTAssertEqual(learnedTyping { $0.textContentType = .emailAddress }, 0)
        XCTAssertEqual(learnedTyping { $0.isSecureTextEntry = true }, 0)
        XCTAssertEqual(learnedTyping { $0.keyboardType = .URL }, 0)
        XCTAssertEqual(learnedTyping { $0.keyboardType = .emailAddress }, 0)
        XCTAssertEqual(learnedTyping { $0.autocorrectionType = .no }, 0)

        let (field, plane) = try swipeKeyboard()
        let touch = adaptive(on: plane.engine)
        for word in ["hello", "world", "again"] {
            swipe(word, on: plane)
        }
        tap(.character("s"), on: plane)
        tap(.space, on: plane)
        swipe("it", on: plane)
        XCTAssertEqual(field.text, "hello world again s it")
        XCTAssertEqual(learned(touch).weak, 0, "never from a swipe")
    }

    func testAdaptiveTouchMovesTheDecodersCentresNeverTheKeys() throws {
        let model = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        engine.letterOdds = model
        engine.autocorrect.decoder = TypingDecoder(model: model)
        let keys = engine.layout.keys
        let area = engine.layout.touchAreaFrame
        let points = stride(from: area.minX + 1, to: area.maxX, by: 7).flatMap { x in
            stride(from: area.minY + 1, to: area.maxY, by: 5).map { CGPoint(x: x, y: $0) }
        }
        let pressed = points.map { engine.key(at: $0)?.kind }
        let stockCentres = try XCTUnwrap(engine.autocorrect.decoderKeys)

        let touch = adaptive()
        XCTAssertEqual(engine.autocorrect.decoderKeys, stockCentres, "nothing learned yet: the stock centres")
        XCTAssertNil(engine.autocorrect.decoderBaselineKeys)
        try teach(touch, dx: 0.2)
        XCTAssertTrue(touch.isActive)
        XCTAssertEqual(engine.layout.keys, keys, "keycaps and hit rectangles stay where they are")
        XCTAssertEqual(points.map { engine.key(at: $0)?.kind }, pressed)
        let e = engine.layout.key(.character("e"))!.frame
        let personal = try XCTUnwrap(engine.autocorrect.decoderKeys?.centres["e"])
        XCTAssertEqual(personal.y, e.midY, accuracy: 0.01)
        XCTAssertGreaterThan(personal.x - e.midX, 0.12 * e.width)
        XCTAssertLessThanOrEqual(personal.x - e.midX, CGFloat(AdaptiveTouchModel.bound) * e.width + 0.01)
        XCTAssertEqual(engine.autocorrect.decoderBaselineKeys, stockCentres, "beside them, the stock centres")
        XCTAssertEqual(engine.swipeKeys.centres["e"]?.x, e.midX, "Swipe typing reads the drawn keys")
    }

    /// With Adaptive touch on, Learn how I type's per-letter offsets no longer move the centres
    /// the decoder reads (no stacking), but stay the baseline a correction must also clear.
    func testAdaptiveTouchReplacesLearnHowITypesOffsetsButKeepsThemAsTheBaseline() throws {
        let e = engine.layout.key(.character("e"))!.frame
        engine.tapOffsets = ["e": CGVector(dx: -4, dy: 2)]
        let journal = try XCTUnwrap(TypingDecoder.Keys(layout: engine.layout, offsets: engine.tapOffsets))
        let touch = adaptive()
        XCTAssertEqual(engine.autocorrect.decoderKeys, TypingDecoder.Keys(layout: engine.layout), "too little evidence: the stock centres")
        XCTAssertEqual(engine.autocorrect.decoderBaselineKeys, journal)
        try teach(touch, dx: 0.2)
        XCTAssertGreaterThan(try XCTUnwrap(engine.autocorrect.decoderKeys?.centres["e"]).x, e.midX)
        XCTAssertEqual(engine.autocorrect.decoderBaselineKeys, journal)
        engine.adaptiveTouch = nil
        XCTAssertEqual(engine.autocorrect.decoderKeys, journal, "off: Learn how I type's offsets, as before")
        XCTAssertNil(engine.autocorrect.decoderBaselineKeys)
    }

    func testAdaptiveTouchKeepsEachKeyboardSizeAndHexKeySideApart() throws {
        let touch = adaptive()
        try teach(touch, dx: 0.2)
        let e = engine.layout.key(.character("e"))!.frame
        let shift = try XCTUnwrap(engine.autocorrect.decoderKeys?.centres["e"]).x - e.midX
        engine.orientation = .landscape
        XCTAssertFalse(touch.isActive, "landscape starts from the stock centres")
        XCTAssertEqual(engine.autocorrect.decoderKeys, TypingDecoder.Keys(layout: engine.layout))
        engine.orientation = .portrait
        XCTAssertTrue(touch.isActive)
        engine.hexPlacement = .stripLeading
        XCTAssertFalse(touch.isActive, "the Hex key on the other side is another grip")
        engine.hexPlacement = .stripTrailing
        engine.width = 440
        XCTAssertFalse(touch.isActive, "another iPhone's width")
        engine.width = 390
        XCTAssertTrue(touch.isActive, "the same size, a slightly different key plane")
        let narrower = engine.layout.key(.character("e"))!.frame
        let moved = try XCTUnwrap(engine.autocorrect.decoderKeys?.centres["e"]).x - narrower.midX
        XCTAssertEqual(moved / narrower.width, shift / e.width, accuracy: 0.001, "offsets are in keycaps")
    }

    /// Another arrangement of the letters is another key plane: what was learned on QWERTY
    /// never moves QWERTZ's centres, and comes back with QWERTY.
    func testAdaptiveTouchKeepsEachLetterLayoutApart() throws {
        let touch = adaptive()
        try teach(touch, dx: 0.2)
        XCTAssertTrue(touch.isActive)
        let qwerty = try XCTUnwrap(AdaptiveTouchGeometry(layout: engine.layout)).key
        engine.letterLayout = .qwertz
        XCTAssertNotEqual(try XCTUnwrap(AdaptiveTouchGeometry(layout: engine.layout)).key, qwerty)
        XCTAssertFalse(touch.isActive, "QWERTZ starts from the stock centres")
        XCTAssertEqual(engine.autocorrect.decoderKeys, TypingDecoder.Keys(layout: engine.layout))
        engine.letterLayout = .azerty
        XCTAssertFalse(touch.isActive)
        engine.letterLayout = .qwerty
        XCTAssertTrue(touch.isActive, "QWERTY's profile is still there")
    }

    func testAdaptiveTouchFallsBackToStockWhenTheGripChangesAndKeepsBothGrips() throws {
        let touch = adaptive()
        try teach(touch, dx: 0.2)
        XCTAssertTrue(touch.isActive)
        let e = engine.layout.key(.character("e"))!.frame
        try teach(touch, dx: -0.2, taps: 24, sessions: 1)
        XCTAssertFalse(touch.isActive, "the profile fits worse than the stock centres: stock at once")
        XCTAssertEqual(engine.autocorrect.decoderKeys, TypingDecoder.Keys(layout: engine.layout))
        XCTAssertNil(engine.autocorrect.decoderBaselineKeys)
        try teach(touch, dx: -0.2, taps: 420, sessions: 4)
        XCTAssertTrue(touch.isActive, "the new grip learned its own profile")
        XCTAssertLessThan(try XCTUnwrap(engine.autocorrect.decoderKeys?.centres["e"]).x, e.midX)
        try teach(touch, dx: 0.2, taps: 40, sessions: 1)
        XCTAssertTrue(touch.isActive, "back to the first grip, whose profile was kept")
        XCTAssertGreaterThan(try XCTUnwrap(engine.autocorrect.decoderKeys?.centres["e"]).x, e.midX)
        XCTAssertEqual(touch.model.contexts.filter { $0.geometry != 0 }.count, 2)
    }

    func testAdaptiveTouchNeedsEvidenceFromSeveralSessionsBeforeTheDecoderUsesIt() throws {
        let touch = adaptive()
        try teach(touch, dx: 0.2, taps: 399, sessions: 4)
        XCTAssertFalse(touch.isActive, "399 taps")
        let other = adaptive()
        try teach(other, dx: 0.2, taps: 800, sessions: 1)
        XCTAssertFalse(other.isActive, "one session")
        let accurate = adaptive()
        try teach(accurate, dx: 0, taps: 800, sessions: 4)
        XCTAssertFalse(accurate.isActive, "taps on the centres: the stock centres already fit")
    }

    func testAdaptiveTouchUntrainedTypesExactlyAsWithItOff() throws {
        let model = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        func typing(adaptive: Bool) -> [String] {
            let field = FixtureTextTarget()
            field.autocapitalizationType = UITextAutocapitalizationType.none
            let keys = FullKeyboardEngine(target: field, autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults)))
            keys.width = 402
            keys.letterOdds = model
            keys.autocorrect.decoder = TypingDecoder(model: model)
            keys.configureForField()
            if adaptive { keys.adaptiveTouch = AdaptiveTouch(storage: .memory, settings: AdaptiveTouchSettings(defaults: defaults)) }
            var seed: UInt64 = 7
            func jitter() -> CGFloat {
                seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
                return CGFloat(Int(seed >> 33) % 1000) / 1000 - 0.5
            }
            var out: [String] = []
            for ch in "teh quick bronw fox jumsp ovr the lazt dog and keeos running " {
                let key = keys.layout.key(ch == " " ? .space : .character(String(ch)))!
                let p = CGPoint(x: key.frame.midX + jitter() * key.frame.width * 0.7, y: key.frame.midY + jitter() * key.frame.height * 0.6)
                keys.touchBegan(id: 1, at: p)
                keys.touchEnded(id: 1, at: p)
                if ch == "x" { keys.deleteBackward() }
                // The decoder reads exactly what it reads with the experiment off. (Suggestions
                // that tie can swap between runs: Swift seeds each dictionary's order.)
                XCTAssertEqual(keys.autocorrect.decoderKeys, TypingDecoder.Keys(layout: keys.layout))
                XCTAssertNil(keys.autocorrect.decoderBaselineKeys)
                out.append(field.text)
            }
            return out
        }
        XCTAssertEqual(typing(adaptive: true), typing(adaptive: false))
    }

    func testAdaptiveTouchFileIsSmallProtectedFromBackupAndReset() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("AdaptiveTouchTests-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent(AdaptiveTouch.fileName)
        let settings = AdaptiveTouchSettings(defaults: defaults)
        let touch = AdaptiveTouch(storage: .file(url), settings: settings)
        engine.adaptiveTouch = touch
        try teach(touch, dx: 0.2)
        touch.save()
        AdaptiveTouch.waitForWrites()
        let data = try Data(contentsOf: url)
        XCTAssertEqual(data.count, AdaptiveTouchModel.byteCount)
        XCTAssertLessThanOrEqual(data.count, 27_712, "averages only, the same size whatever was learned")
        XCTAssertEqual(try url.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup, true)
        XCTAssertEqual(settings.summary?.active, true)
        XCTAssertEqual(AdaptiveTouchStatus.current(settings: settings, fullAccessConfirmed: true), .active)
        XCTAssertEqual(AdaptiveTouchStatus.current(settings: settings, fullAccessConfirmed: false), .sessionOnly)
        XCTAssertEqual(AdaptiveTouch(storage: .file(url), settings: settings).model, touch.model, "the next keyboard process reads it")

        var damaged = data
        damaged[200] ^= 0xFF
        try damaged.write(to: url)
        XCTAssertTrue(AdaptiveTouch(storage: .file(url), settings: settings).model.contexts.allSatisfy { $0.geometry == 0 })
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path), "a damaged file is dropped")

        // Reset touch learning: the running keyboard forgets at once; a file written before
        // can't come back.
        try data.write(to: url)
        settings.requestReset()
        XCTAssertNil(settings.summary)
        XCTAssertEqual(AdaptiveTouchStatus.current(settings: settings, fullAccessConfirmed: true), .learning(taps: 0))
        touch.applyResetIfNeeded()
        XCTAssertFalse(touch.isActive)
        XCTAssertEqual(engine.autocorrect.decoderKeys, TypingDecoder.Keys(layout: engine.layout))
        XCTAssertTrue(touch.model.contexts.allSatisfy { $0.geometry == 0 })
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
        try data.write(to: url)
        XCTAssertTrue(AdaptiveTouch(storage: .file(url), settings: settings).model.contexts.allSatisfy { $0.geometry == 0 })
    }

    func testAdaptiveTouchIsOffByDefaultAndOnlyInMemoryWithoutFullAccess() {
        let settings = AdaptiveTouchSettings(defaults: defaults)
        XCTAssertFalse(settings.isEnabled)
        XCTAssertNil(AdaptiveTouch.sharedIfEnabled(settings: settings, persistent: true))
        settings.isEnabled = true
        XCTAssertEqual(AdaptiveTouch.sharedIfEnabled(settings: settings, persistent: false)?.storage, .memory)
        settings.isEnabled = false
        XCTAssertNil(AdaptiveTouch.sharedIfEnabled(settings: settings, persistent: false))
        settings.summary = (240, false)
        XCTAssertEqual(AdaptiveTouchStatus.current(settings: settings, fullAccessConfirmed: true), .learning(taps: 240))
        settings.summary = (900, false)
        XCTAssertEqual(AdaptiveTouchStatus.current(settings: settings, fullAccessConfirmed: true), .learning(taps: 400))
    }
}

/// Records each language the keyboard reports as its input mode's (`primaryLanguage`), so the
/// test sees both the value and how often it was sent.
@MainActor
private final class RecordingInputController: UIInputViewController {
    private(set) var reportedLanguages: [String] = []

    override var primaryLanguage: String? {
        get { reportedLanguages.last }
        set {
            if let newValue {
                reportedLanguages.append(newValue)
            }
        }
    }
}
