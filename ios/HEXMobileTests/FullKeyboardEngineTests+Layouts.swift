import UIKit
import XCTest
@testable import HEX

extension FullKeyboardEngineTests {
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
        // Upright whatever the simulator: turned (an iPad simulator left in landscape) the grid
        // has three rows, and the panel filled under one orientation was read under the other.
        view.orientationOverride = .portrait
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

    /// Spatial autocorrect has English, Spanish and French models only: Italian falls back to
    /// the dictionary engine in Italian, never to the English model, and French reads its own.
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
        // Nor do Russian, Serbian (Latin), and Canada's and Switzerland's French, which have no
        // model of their own yet; nor do English's or Spanish's next-letter odds move their
        // targets. French (France) and Polish read their own models, for the decoder and the letter odds.
        for (language, code) in [(KeyboardLanguage.french, "fr"), (.polish, "pl")] {
            view.languageOverride = language
            XCTAssertEqual(view.engine.autocorrect.decoder?.model.language, code)
            XCTAssertEqual(view.engine.letterOdds?.language, code)
        }
        for language in [KeyboardLanguage.russian, .ukrainian, .greek, .romanian, .serbianLatin, .frenchCanadian, .frenchSwiss] {
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

    /// Settings › Experiments › Join the keyboard when unfolded: on, the Duo unfolded upright
    /// rebuilds as one keyboard rather than the split one, and off splits it again.
    func testJoiningTheUnfoldedKeyboardRebuildsOneLayout() {
        engine.posture = .unfoldedWide
        engine.windowInset = 75
        engine.width = 801
        XCTAssertTrue(engine.layout.isSplit)
        engine.joinsUnfoldedKeyboard = true
        XCTAssertFalse(engine.layout.isSplit)
        XCTAssertEqual(engine.layout.keys.filter { $0.label == "q" }.count, 1)
        XCTAssertEqual(engine.layout.key(.character("q"))?.frame.minX ?? 0, 19, accuracy: 0.001)
        engine.joinsUnfoldedKeyboard = false
        XCTAssertTrue(engine.layout.isSplit)
    }

    func testEmailFieldShowsAtAndDot() {
        target.keyboardType = .emailAddress
        engine.configureForField()
        tap(.character("@"))
        tap(.character("."))
        XCTAssertEqual(target.text, "@.")
    }

    /// Safari's address bar: Apple's `.` beside the space bar types a period on the letters page.
    func testWebSearchFieldShowsApplesDot() {
        target.keyboardType = .webSearch
        engine.configureForField()
        XCTAssertEqual(engine.field, .webSearch)
        XCTAssertEqual(engine.layout.key(.character("."))?.row, 3)
        tap(.character("."))
        XCTAssertEqual(target.text, ".")
        XCTAssertEqual(engine.layout.layer, .letters)
    }

    func testNumberPadOpensOnNumbers() {
        target.keyboardType = .numberPad
        engine.configureForField()
        XCTAssertEqual(engine.layout.layer, .numbers)
    }

    // MARK: - One space per space press

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
            // е (Ukrainian's ʼ, its only key with long-press options); Greek its α and ε; Korean
            // its ㅂ (Hangul holds nothing); Arabic its ض and ا (the hamza forms).
            let first = language == .korean ? "ㅂ" : language == .greek ? "α" : language == .arabic ? "ض" : language.isLatin ? "a" : "а"
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
            case .korean: "ㅂ"
            case .arabic: "ا"
            default: "e"
            }
            view.debugHoldAccent(held, selectIndex: 1)
            counts[language] = asked
        }
        XCTAssertGreaterThanOrEqual(counts[.english] ?? 0, 4, "three keys and an accent step")
        for language in KeyboardLanguage.allCases where language != .korean {
            XCTAssertEqual(counts[language], counts[.english], "\(language.rawValue) haptics")
        }
        XCTAssertEqual(counts[.korean] ?? 0, 3, "three keys, and no held accents")
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
}
