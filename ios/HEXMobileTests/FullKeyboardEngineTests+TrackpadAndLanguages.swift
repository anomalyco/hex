import UIKit
import XCTest
@testable import HEX

extension FullKeyboardEngineTests {
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

    /// TestFlight build 147 (iPhone Duo): "Moving up seems to routinely skip a line". At a long
    /// note's first line the host ignores the step past the text's start, and its context stays
    /// as it was. Taken as the text above, it was added again before the known text, over and
    /// over (3 rows grew to 245 in one gesture, iOS 27.2 Duo simulator, and the lift moved the
    /// cursor 26,642 characters).
    func testTheTrackpadTakesAnUnchangedContextAsTheTextsStart() {
        let width: (Character) -> CGFloat = { _ in 10 }
        var state = TrackpadCursor.State(before: "Abc.", after: " ", wrapWidth: 300, lineHeight: 20, width: width)
        XCTAssertEqual(state.move(by: CGVector(dx: 0, dy: -25), at: 0), -5, "just past the known text's start")
        XCTAssertFalse(state.catchUp(before: "Abc.", after: " ", width: width), "the same context: the host ignored the step")
        XCTAssertEqual(String(state.layout.text), "Abc. ", "and nothing was added")
        XCTAssertEqual(state.settle(at: 0.5), 0, "unanswered: the text's start")
        XCTAssertEqual(state.move(by: CGVector(dx: 0, dy: -40), at: 0.6), 0, "the first line keeps it")
        XCTAssertEqual(state.move(by: CGVector(dx: -10, dy: 0), at: 0.7), -1, "and moving along moves it at once")
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
        XCTAssertEqual(view.engine.autocorrect.decoder?.model.language, "pl", "Polish reads its own typing model")
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

    /// On iOS 26 Ukrainian is the system keyboard's eleven-key rows: ї comes from a held і, ґ
    /// from a held г, and ʼ from the 123 page, which goes back to the letters after it, as
    /// Apple's does; Russian ⇄ Ukrainian on the space bar keeps every shared letter in place.
    func testUkrainianOnIOS26TypesHeldLettersAndThePagesApostrophe() {
        let running = KeyboardLanguage.systemMajorVersion
        defer { KeyboardLanguage.systemMajorVersion = running }
        KeyboardLanguage.systemMajorVersion = 26
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.russian, .ukrainian]
        let field = FixtureTextTarget()
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let view = FullKeyboardView(model: KeyboardViewModel(), target: field)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 390, height: 262)
        view.layoutIfNeeded()
        let russian = view.engine.layout
        XCTAssertEqual(view.engine.language, .russian)
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .ukrainian)
        for key in russian.keys where key.character != nil && key.character != "ы" && key.character != "э" {
            XCTAssertEqual(view.engine.layout.key(key.kind)?.frame, key.frame, key.label)
        }
        var id = 300
        func tap(_ kind: KeyKind) {
            let key = view.engine.layout.key(kind)!
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
        tap(.character("п"))
        tap(.layerSwitch(.numbers))
        tap(.character("\u{02BC}"))
        XCTAssertEqual(view.engine.layout.layer, .letters, "back to the letters after ʼ")
        for character in ["я", "т", "ь"] { tap(.character(character)) }
        tap(.space)
        hold("і", pick: "ї")
        hold("г", pick: "ґ")
        XCTAssertEqual(field.text, "пʼять їґ")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .russian)
        XCTAssertEqual(view.engine.layout.keys.map(\.frame), russian.keys.map(\.frame))
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

    /// Settings › Keyboard › Languages with English and Hungarian: the space bar says "Magyar",
    /// the keys are the standard Hungarian QWERTZ (Z ends row 1, Y starts row 3), its nine
    /// accented vowels are all on held keys (á é í ó ö ő ú ü ű, ő and ű the double-acute letters),
    /// English is its bilingual partner, and Switch dictation too dictates hu-HU.
    func testHungarianTypesAndSwitches() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .hungarian]
        settings.dictationFollowsKeyboard = true
        settings.supportedDictationLanguages = ["en-US", "hu-HU"]
        let model = KeyboardViewModel()
        let field = FixtureTextTarget()
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let view = FullKeyboardView(model: model, target: field)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        view.engine.autocorrect.isEnabled = false
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .hungarian)
        XCTAssertEqual(view.plane.spaceLabel, "Magyar")
        XCTAssertNil(view.engine.autocorrect.decoder, "no Hungarian typing model: the dictionary engine")
        XCTAssertEqual(settings.keyboardDictationLanguage(), "hu-HU")
        XCTAssertEqual(KeyboardLanguage.hungarian.letterRows[0], Array("qwertzuiop").map(String.init))
        XCTAssertEqual(KeyboardLanguage.hungarian.letterRows[2].first, "y", "QWERTZ")
        XCTAssertEqual(KeyboardLanguage.hungarian.letterRows(.qwerty)[2].first, "z", "the QWERTY offered too")
        var id = 800
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
            // The Hungarian pill is anchored on the key's own option, so a nine-option key
            // (e's é è ê ë ě ė ę ē) is pushed in from the edge and its cell can already sit
            // under the finger when it opens. Sliding from the highlighted base cell to the
            // picked one is the move that selects it; the cell under the finger is no move.
            let base = menu.cellFrames[menu.selectedIndex]
            view.engine.touchMoved(id: id, to: CGPoint(x: base.midX, y: base.midY))
            let cell = menu.cellFrames[index]
            view.engine.touchMoved(id: id, to: CGPoint(x: cell.midX, y: cell.midY))
            view.engine.touchEnded(id: id, at: CGPoint(x: cell.midX, y: cell.midY))
        }
        tap(.shift)
        type("k")
        hold("o", pick: "ö")
        type("sz")
        hold("o", pick: "ö")
        type("n")
        hold("o", pick: "ö")
        type("m sz")
        hold("e", pick: "é")
        type("pen tal")
        hold("a", pick: "á")
        type("lkozunk holnap")
        XCTAssertEqual(field.text, "Köszönöm szépen találkozunk holnap")
        type(" id")
        hold("o", pick: "ő")
        type(" f")
        hold("u", pick: "ű")
        XCTAssertEqual(field.text, "Köszönöm szépen találkozunk holnap idő fű")
        XCTAssertEqual(field.text.unicodeScalars.count, field.text.count, "precomposed letters, no combining marks")
        tap(.delete)
        XCTAssertEqual(field.text, "Köszönöm szépen találkozunk holnap idő f", "delete takes ű whole")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .english)
        XCTAssertEqual(view.plane.spaceLabel, "English")
        XCTAssertEqual(view.engine.layout.key(.character("y"))?.row, 0, "English is QWERTY again")
    }

    /// Settings › Keyboard › Languages with English and Dutch: the space bar says "Nederlands",
    /// the keys are the English letters (Apple's Dutch keyboard is QWERTY), its accents are on
    /// held keys as Apple's are (ë on E, ï on I, é on E, and j́ on J for a stressed íj́, its
    /// combining acute kept on the j), English is its bilingual partner, and Switch dictation
    /// too dictates nl-NL.
    func testDutchTypesAndSwitches() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .dutch]
        settings.dictationFollowsKeyboard = true
        settings.supportedDictationLanguages = ["en-US", "nl-NL"]
        let model = KeyboardViewModel()
        let field = FixtureTextTarget()
        field.autocapitalizationType = UITextAutocapitalizationType.none
        let view = FullKeyboardView(model: model, target: field)
        view.languageSettings = settings
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        view.engine.autocorrect.isEnabled = false
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .dutch)
        XCTAssertEqual(view.plane.spaceLabel, "Nederlands")
        XCTAssertNil(view.engine.autocorrect.decoder, "no Dutch typing model: the dictionary engine")
        XCTAssertEqual(settings.keyboardDictationLanguage(), "nl-NL")
        XCTAssertEqual(view.engine.layout.key(.character("y"))?.row, 0, "QWERTY")
        var id = 900
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
            // As Hungarian's: slide from the highlighted base cell to the picked one.
            let base = menu.cellFrames[menu.selectedIndex]
            view.engine.touchMoved(id: id, to: CGPoint(x: base.midX, y: base.midY))
            let cell = menu.cellFrames[index]
            view.engine.touchMoved(id: id, to: CGPoint(x: cell.midX, y: cell.midY))
            view.engine.touchEnded(id: id, at: CGPoint(x: cell.midX, y: cell.midY))
        }
        type("idee")
        hold("e", pick: "ë")
        type("n zijn ge")
        hold("i", pick: "ï")
        type("nteresseerd in ")
        tap(.shift)
        type("belgi")
        hold("e", pick: "ë")
        XCTAssertEqual(field.text, "ideeën zijn geïnteresseerd in België")
        type(" v")
        hold("i", pick: "í")
        hold("j", pick: "j\u{0301}")
        type("f")
        XCTAssertEqual(field.text, "ideeën zijn geïnteresseerd in België víj́f")
        view.switchLanguage(by: 1)
        XCTAssertEqual(view.engine.language, .english)
        XCTAssertEqual(view.plane.spaceLabel, "English")
        XCTAssertNil(AccentCatalog.options(for: "j", uppercase: false, language: view.engine.language), "English holds nothing on J")
    }

    /// Settings › Keyboard › Languages with English, French and Russian: the space bar names
    /// the keyboard's language, switching changes the letters (AZERTY, ЙЦУКЕН), the accents and
    /// autocorrect (French with English beside it, Russian alone), and the keyboard comes back
    /// in the language it was left in. Dictation follows on Automatic only with Switch dictation
    /// too on, which also puts a language picked by hand back on Automatic.
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
        XCTAssertNil(settings.keyboardDictationLanguage(osMajorVersion: 27), "Switch dictation too off: Automatic stays")
        settings.dictationFollowsKeyboard = true
        XCTAssertEqual(settings.keyboardDictationLanguage(), "fr-FR", "on: Automatic follows the keyboard")
        settings.dictationFollowsKeyboard = false
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
    /// dictates in (with Switch dictation too on); Spanish first, its ñ key in both; Spanish
    /// alone, its own.
    func testEnglishAndSpanishShareTheFirstLanguagesKeys() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .spanish]
        settings.dictationFollowsKeyboard = true
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
}
