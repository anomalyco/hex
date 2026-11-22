import UIKit
import XCTest
@testable import HEX

@MainActor
final class FullKeyboardEngineTests: XCTestCase {
    var target: FixtureTextTarget!

    var engine: FullKeyboardEngine!

    var now: TimeInterval = 100

    var suiteName: String!

    var defaults: UserDefaults!

    /// The layouts are Apple's iOS 27 keyboard; CI's newest simulator runtime is iOS 26.5, so the
    /// version these tests describe is pinned (the before-27 Ukrainian test sets 26 itself).
    var runningSystemMajorVersion = 0

    override func setUp() async throws {
        runningSystemMajorVersion = KeyboardLanguage.systemMajorVersion
        KeyboardLanguage.systemMajorVersion = 27
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
        KeyboardLanguage.systemMajorVersion = runningSystemMajorVersion
        defaults.removePersistentDomain(forName: suiteName)
    }

    func point(_ kind: KeyKind) -> CGPoint {
        let key = engine.layout.key(kind)!
        return CGPoint(x: key.frame.midX, y: key.frame.midY)
    }

    func down(_ id: Int, _ kind: KeyKind) { engine.touchBegan(id: id, at: point(kind)) }

    func up(_ id: Int, _ kind: KeyKind) { engine.touchEnded(id: id, at: point(kind)) }

    func tap(_ kind: KeyKind) {
        let at = point(kind)
        engine.touchBegan(id: 1, at: at)
        engine.touchEnded(id: 1, at: at)
    }

    func type(_ text: String) {
        for ch in text {
            tap(ch == " " ? .space : .character(String(ch)))
            now += 0.5
        }
    }

    func tapAt(_ point: CGPoint, id: Int = 1) {
        engine.touchBegan(id: id, at: point)
        engine.touchEnded(id: id, at: point)
        now += 0.5
    }

    func suggestion(_ text: String, _ kind: KeyboardSuggestion.Kind = .secondary) -> KeyboardSuggestion {
        KeyboardSuggestion(displayText: kind == .keepLiteral ? "“\(text)”" : text, replacement: text, kind: kind)
    }

    /// "brown" in "the quick brown fox" selected and typed over: the cursor sits between the
    /// space before it and the space after it.
    func typeOverBrown() {
        type("the quick  fox")
        target.adjustTextPosition(byCharacterOffset: -4)
        engine.textDidChange()
    }

    func swipeEngine(_ text: String) -> (FixtureTextTarget, FullKeyboardEngine) {
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

    func deleteSample(_ keys: FullKeyboardEngine, dx: CGFloat = 0) -> TypingLogTouchSample {
        let key = keys.layout.key(.delete)!
        return TypingLogTouchSample(point: CGPoint(x: key.frame.midX + dx, y: key.frame.midY), timestamp: 0)
    }

    /// Types a mark from the 123 page; the next space goes back to the letters.
    func punctuate(_ mark: String) {
        if engine.layout.layer == .letters { tap(.layerSwitch(.numbers)) }
        tap(.character(mark))
        now += 0.5
    }

    /// Types `text` key by key: letters on the letters page, digits and marks from the 123 page.
    func keys(_ text: String) {
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

    func smartField(quotes: UITextSmartQuotesType? = .default, dashes: UITextSmartDashesType? = .default, language: KeyboardLanguage = .english) {
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

    /// A 390 pt keyboard with Swipe typing on: its key plane, typing into a field holding
    /// `text` (or the given one, for a host that does something particular).
    func swipeKeyboard(
        _ text: String = "", language: KeyboardLanguage = .english, target: FixtureTextTarget? = nil
    ) throws -> (FixtureTextTarget, KeyboardKeyPlaneView) {
        let field = target ?? FixtureTextTarget(text: text)
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

    func sample(_ point: CGPoint) -> TypingLogTouchSample {
        TypingLogTouchSample(point: point, timestamp: 0)
    }

    /// Swipes `word` through its keys' centres; `lift` false leaves the finger down.
    func swipe(_ word: String, on plane: KeyboardKeyPlaneView, id: Int = 50, lift: Bool = true) {
        swipe(points: SwipePath.through(word, keys: plane.engine.swipeKeys), on: plane, id: id, lift: lift)
    }

    func swipe(points: [CGPoint], on plane: KeyboardKeyPlaneView, id: Int = 50, lift: Bool = true) {
        plane.began(id: id, sample: sample(points[0]))
        for point in points.dropFirst() {
            plane.moved(id: id, sample: sample(point))
        }
        if lift { plane.ended(id: id, sample: sample(points[points.count - 1])) }
    }

    func tap(_ kind: KeyKind, on plane: KeyboardKeyPlaneView, id: Int = 60) {
        let key = plane.engine.layout.key(kind)!
        let centre = CGPoint(x: key.frame.midX, y: key.frame.midY)
        plane.began(id: id, sample: sample(centre))
        plane.ended(id: id, sample: sample(centre))
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

    /// Settings › Experiments › Dvorak punctuation keys: on English's Dvorak letters the three
    /// keys type `<`, `>` and `:` while shift is on, their keycaps and pop-ups read so, one-shot
    /// shift turns off after one as it does for a letter, and caps lock keeps them shifted.
    func testDvorakPunctuationTypesShiftedSymbols() throws {
        engine.width = 402
        engine.letterLayout = .dvorak
        XCTAssertNil(engine.layout.key(.character(",")), "off: Apple's seven-letter row 1")
        engine.dvorakPunctuation = true
        XCTAssertEqual(engine.layout.letters, .dvorak)
        let comma = try XCTUnwrap(engine.layout.key(.character(",")))
        let period = try XCTUnwrap(engine.layout.key(.character(".")))
        let semicolon = try XCTUnwrap(engine.layout.key(.character(";")))
        XCTAssertEqual(comma.shifted, "<")
        XCTAssertEqual(period.shifted, ">")
        XCTAssertEqual(semicolon.shifted, ":")
        XCTAssertEqual(comma.row, 0)
        XCTAssertEqual(period.row, 0)
        XCTAssertEqual(semicolon.row, 3)
        XCTAssertEqual(engine.keycapLabel(for: comma), ",")
        XCTAssertEqual(engine.displayLabel(for: comma), ",")
        XCTAssertEqual(engine.keycapLabel(for: semicolon), ";")
        tap(.shift)
        XCTAssertEqual(engine.keycapLabel(for: comma), "<")
        XCTAssertEqual(engine.displayLabel(for: comma), "<")
        XCTAssertEqual(engine.keycapLabel(for: period), ">")
        XCTAssertEqual(engine.displayLabel(for: period), ">")
        XCTAssertEqual(engine.keycapLabel(for: semicolon), ":")
        XCTAssertEqual(engine.displayLabel(for: semicolon), ":")
        tap(.character(","))
        XCTAssertEqual(target.text, "<")
        XCTAssertEqual(engine.shift.state, .off, "one-shot, as after a letter")
        XCTAssertEqual(engine.keycapLabel(for: comma), ",")
        tap(.character("."))
        tap(.character(";"))
        XCTAssertEqual(target.text, "<.;")
        tap(.shift)
        now += 0.1
        tap(.shift)
        XCTAssertEqual(engine.shift.state, .locked)
        tap(.character(","))
        tap(.character("."))
        tap(.character(";"))
        XCTAssertEqual(target.text, "<.;<>:")
        XCTAssertEqual(engine.shift.state, .locked, "caps lock stays on")
        XCTAssertEqual(engine.keycapLabel(for: semicolon), ":")
        // The narrower row 1 is its own key plane for Adaptive touch: what it learns stays there.
        let punctuation = try XCTUnwrap(AdaptiveTouchGeometry(layout: engine.layout)).key
        engine.dvorakPunctuation = false
        XCTAssertNotEqual(try XCTUnwrap(AdaptiveTouchGeometry(layout: engine.layout)).key, punctuation)
    }

    /// Settings › Experiments › Dvorak punctuation keys: a URL or email field already has its own
    /// `.` key on the bottom row, so its Dvorak row 1 stays Apple's rather than have two keys
    /// that type the same character, while the semicolon still joins the space bar.
    func testDvorakPunctuationKeepsFieldsOwnPeriodKey() throws {
        engine.width = 402
        engine.letterLayout = .dvorak
        engine.dvorakPunctuation = true
        for keyboardType in [UIKeyboardType.URL, .emailAddress] {
            target.keyboardType = keyboardType
            engine.configureForField()
            XCTAssertNotEqual(engine.layout.field, .text, "\(keyboardType.rawValue)")
            XCTAssertEqual(engine.layout.keys.filter { $0.kind == .character(".") }.count, 1, "the field's own . only")
            XCTAssertNil(engine.layout.key(.character(",")), "row 1 stays Apple's")
            XCTAssertEqual(engine.layout.key(.character("."))?.row, 3)
            XCTAssertEqual(engine.layout.key(.character(";"))?.shifted, ":")
            XCTAssertEqual(engine.layout.key(.character(";"))?.row, 3)
        }
    }

    /// Settings › Experiments › Dvorak punctuation keys: auto-capitalization's shift types no
    /// symbols — a comma at a sentence's start stays a comma, and its keycap reads so — while a
    /// shift you press, or caps lock, types `<`, and the semicolon `:`.
    func testDvorakPunctuationFollowsAUserShiftNotAutoCapitalization() throws {
        engine.width = 402
        engine.letterLayout = .dvorak
        engine.dvorakPunctuation = true
        target.autocapitalizationType = .sentences
        engine.configureForField()
        let comma = try XCTUnwrap(engine.layout.key(.character(",")))
        XCTAssertEqual(engine.shift.state, .on, "the field's first sentence")
        XCTAssertFalse(engine.shift.isUserShift)
        XCTAssertEqual(engine.keycapLabel(for: comma), ",")
        XCTAssertEqual(engine.displayLabel(for: comma), ",")
        tap(.character(","))
        XCTAssertEqual(target.text, ",")
        XCTAssertEqual(engine.shift.state, .off)

        type("ok. ")
        XCTAssertEqual(target.text, ",ok. ")
        XCTAssertEqual(engine.shift.state, .on, "after a sentence")
        XCTAssertFalse(engine.shift.isUserShift)
        tap(.character(","))
        XCTAssertEqual(target.text, ",ok. ,")

        tap(.shift)
        XCTAssertTrue(engine.shift.isUserShift)
        XCTAssertEqual(engine.keycapLabel(for: comma), "<")
        tap(.character(","))
        XCTAssertEqual(target.text, ",ok. ,<")
        XCTAssertEqual(engine.shift.state, .off, "one-shot")

        tap(.shift)
        now += 0.1
        tap(.shift)
        XCTAssertEqual(engine.shift.state, .locked)
        XCTAssertTrue(engine.shift.isUserShift)
        XCTAssertEqual(engine.keycapLabel(for: comma), "<")
        tap(.character(";"))
        XCTAssertEqual(target.text, ",ok. ,<:")
    }

    /// Settings › Experiments › Comma and period keys: the flag rebuilds the layout with a comma
    /// just left of the space bar and a period just right of it, which type themselves (shift
    /// leaves them as they are), and only on an iPhone's letters page in a text field.
    func testCommaAndPeriodKeysTypeByTheSpaceBar() throws {
        engine.width = 402
        XCTAssertFalse(engine.layout.commaPeriodKeys)
        XCTAssertNil(engine.layout.key(.character(",")))
        XCTAssertNil(engine.layout.key(.character(".")))
        engine.commaPeriodKeys = true
        XCTAssertTrue(engine.layout.commaPeriodKeys)
        let comma = try XCTUnwrap(engine.layout.key(.character(",")))
        let period = try XCTUnwrap(engine.layout.key(.character(".")))
        let space = try XCTUnwrap(engine.layout.key(.space))
        XCTAssertEqual(comma.row, 3)
        XCTAssertEqual(period.row, 3)
        XCTAssertNil(comma.shifted)
        XCTAssertNil(period.shifted)
        let gap = engine.layout.metrics.gap
        XCTAssertEqual(comma.frame.maxX + gap, space.frame.minX, accuracy: 0.001)
        XCTAssertEqual(space.frame.maxX + gap, period.frame.minX, accuracy: 0.001)
        XCTAssertEqual(engine.keycapLabel(for: comma), ",")
        XCTAssertEqual(engine.displayLabel(for: period), ".")
        tap(.character(","))
        XCTAssertEqual(target.text, ",")
        tap(.character("."))
        XCTAssertEqual(target.text, ",.")
        tap(.shift)
        XCTAssertEqual(engine.keycapLabel(for: comma), ",", "no shifted symbols")
        XCTAssertEqual(engine.displayLabel(for: period), ".")
        tap(.character(","))
        tap(.character("."))
        XCTAssertEqual(target.text, ",.,.")
        XCTAssertEqual(engine.shift.state, .off, "one-shot, as after any character")

        // The 123 page keeps its own punctuation row, so neither key sits on its bottom row.
        tap(.layerSwitch(.numbers))
        XCTAssertNil(engine.layout.keys.first { $0.row == 3 && $0.kind == .character(",") })
        XCTAssertNil(engine.layout.keys.first { $0.row == 3 && $0.kind == .character(".") })
        tap(.layerSwitch(.letters))
        for keyboardType in [UIKeyboardType.URL, .emailAddress] {
            target.keyboardType = keyboardType
            engine.configureForField()
            XCTAssertNotEqual(engine.layout.field, .text, "\(keyboardType.rawValue)")
            XCTAssertNil(engine.layout.key(.character(",")), "no comma key")
            XCTAssertEqual(engine.layout.keys.filter { $0.kind == .character(".") }.count, 1, "the field's own .")
            XCTAssertEqual(engine.layout.key(.character("."))?.row, 3)
        }
        target.keyboardType = .default
        engine.configureForField()
        engine.pad = .docked(.mini)
        XCTAssertFalse(engine.layout.commaPeriodKeys)
        XCTAssertNil(engine.layout.keys.first { $0.row == 3 && $0.kind == .character(",") }, "an iPad keeps Apple's bottom row")
        XCTAssertNil(engine.layout.keys.first { $0.row == 3 && $0.kind == .character(".") })
        XCTAssertEqual(engine.layout.key(.character(","))?.row, 2, "the docked iPad's own pair")
        engine.pad = nil
        engine.letterLayout = .dvorak
        engine.dvorakPunctuation = true
        XCTAssertFalse(engine.layout.commaPeriodKeys, "Dvorak punctuation already has both on row 1")
        XCTAssertEqual(engine.layout.key(.character(","))?.row, 0)
        XCTAssertEqual(engine.layout.key(.character(";"))?.row, 3)
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

    /// Settings › Experiments › Quick accents: a held letter's callout opens on its language's
    /// preferred accent (Spanish O's ó, though the callout lists ö first), so letting go where
    /// it opened types it, sliding into another cell still picks that one, and another key
    /// going down commits the preselection too. Letting go far below still cancels. Off, the
    /// key's own stays selected, as before.
    func testQuickAccentsOpensOnTheMainAccentAndTypesItOnRelease() {
        engine.language = .spanish
        XCTAssertFalse(FullKeyboardSettings(defaults: defaults).quickAccents, "off by default")
        engine.quickAccentsEnabled = true

        down(1, .character("o"))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 1))
        XCTAssertEqual(engine.activeAccentMenu?.selectedOption, "ó", "o's preferred accent, not the callout's ö")
        // A finger that hasn't left the cell it opened over leaves the preselection alone.
        engine.touchMoved(id: 1, to: point(.character("o")))
        XCTAssertEqual(engine.activeAccentMenu?.selectedOption, "ó")
        up(1, .character("o"))
        XCTAssertEqual(target.text, "ó")

        // Sliding into another cell still picks it.
        down(2, .character("e"))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 2))
        let menu = try! XCTUnwrap(engine.activeAccentMenu)
        XCTAssertEqual(menu.selectedOption, "é")
        let cell = menu.cellFrames[menu.options.firstIndex(of: "è")!]
        engine.touchMoved(id: 2, to: CGPoint(x: cell.midX, y: cell.midY))
        engine.touchEnded(id: 2, at: CGPoint(x: cell.midX, y: cell.midY))
        XCTAssertEqual(target.text, "óè")

        // Another key going down while the callout is held commits what it preselected.
        down(3, .character("a"))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 3))
        down(4, .character("b"))
        up(4, .character("b"))
        up(3, .character("a"))
        XCTAssertEqual(target.text, "óèáb")

        // Capitals use the callout's own uppercase mapping.
        tap(.shift)
        down(5, .character("a"))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 5))
        XCTAssertEqual(engine.activeAccentMenu?.selectedOption, "Á")
        up(5, .character("a"))
        XCTAssertEqual(target.text, "óèábÁ")

        engine.quickAccentsEnabled = false
        down(6, .character("a"))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 6))
        XCTAssertEqual(engine.activeAccentMenu?.selectedOption, "a", "off: the key's own, as before")
        up(6, .character("a"))
        XCTAssertEqual(target.text, "óèábÁa")

        // English e picks é too, though its callout lists ë first.
        engine.language = .english
        engine.quickAccentsEnabled = true
        down(7, .character("e"))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 7))
        XCTAssertEqual(engine.activeAccentMenu?.selectedOption, "é")
        up(7, .character("e"))
        XCTAssertEqual(target.text, "óèábÁaé")

        // Letting go far below still cancels the preselection, typing nothing.
        engine.language = .spanish
        let o = point(.character("o"))
        down(8, .character("o"))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 8))
        XCTAssertEqual(engine.activeAccentMenu?.selectedOption, "ó")
        engine.touchEnded(id: 8, at: CGPoint(x: o.x, y: o.y + FullKeyboardEngine.accentCancelDrop + 5))
        XCTAssertNil(engine.activeAccentMenu)
        XCTAssertEqual(target.text, "óèábÁaé")

        // The 123 page's held symbols keep the key's own selected.
        tap(.layerSwitch(.numbers))
        down(9, .character("1"))
        XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: 9))
        XCTAssertEqual(engine.activeAccentMenu?.selectedOption, "1")
        up(9, .character("1"))
        XCTAssertEqual(target.text, "óèábÁaé1")
    }
}
