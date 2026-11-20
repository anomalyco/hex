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
        XCTAssertEqual(menu.options.first, "E")
        // Slide to index 2 ("É").
        let cell = menu.cellFrames[2]
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
        // Enter trackpad mode (-12 pt) then drag left by 16 pt (-2 chars: before "lo").
        let enterPoint = CGPoint(x: spaceStart.x - 12, y: spaceStart.y)
        engine.touchMoved(id: 1, to: enterPoint)
        XCTAssertTrue(engine.isTrackpadActive)
        engine.touchMoved(id: 1, to: CGPoint(x: enterPoint.x - 16, y: spaceStart.y))
        engine.touchEnded(id: 1, at: CGPoint(x: enterPoint.x - 16, y: spaceStart.y))
        XCTAssertFalse(engine.isTrackpadActive)
        XCTAssertEqual(target.cursorIndex, 2)
        tap(.character("l"))
        XCTAssertEqual(target.text, "hello")
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

    func testSpaceBarNamesTheDictationLanguage() {
        let model = KeyboardViewModel()
        let view = FullKeyboardView(model: model, target: target)
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
        for language in [KeyboardLanguage.russian, .serbianLatin, .french, .frenchCanadian, .frenchSwiss] {
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
        for (i, (x, y)) in taps.enumerated() {
            engine.touchBegan(id: i, at: CGPoint(x: x, y: y))
            engine.touchEnded(id: i, at: CGPoint(x: x, y: y))
            now += 0.2
        }
        XCTAssertEqual(target.text, "but I miss here we more than on the built in apple keyboard which also sucks ")
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
            // Russian has no Latin letters: press its own а and hold its own е.
            let cyrillic = language == .russian
            for (id, kind) in [KeyKind.character(cyrillic ? "а" : "a"), .space, .delete].enumerated() {
                let key = view.engine.layout.key(kind)!
                let sample = TypingLogTouchSample(point: CGPoint(x: key.frame.midX, y: key.frame.midY), timestamp: 0)
                view.plane.began(id: id + 10, sample: sample)
                view.plane.ended(id: id + 10, sample: sample)
            }
            view.debugHoldAccent(cyrillic ? "е" : "e", selectIndex: 1)
            counts[language] = asked
        }
        XCTAssertGreaterThanOrEqual(counts[.english] ?? 0, 4, "three keys and an accent step")
        for language in KeyboardLanguage.allCases {
            XCTAssertEqual(counts[language], counts[.english], "\(language.rawValue) haptics")
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

    func testDeleteSwipeWordsMatchOptionDelete() {
        XCTAssertNil(DeleteWordSwipe.wordLength(before: ""))
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "Hello, world. "), 7)
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "it's don't"), 5)
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "Olá, você"), 4)
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "?!"), 2)
        XCTAssertEqual(DeleteWordSwipe.wordLength(before: "line\n"), 5)
    }
}