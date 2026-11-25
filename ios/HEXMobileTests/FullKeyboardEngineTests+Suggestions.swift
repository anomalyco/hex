import UIKit
import XCTest
@testable import HEX

extension FullKeyboardEngineTests {
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

    /// X and TestFlight: "when you type the word of an emoji it suggests and replaces that word
    /// (smile → 😊), similar to iOS". The strip's emoji tapped while the word is typed takes the
    /// word's place, with a space as any suggestion; after the space it goes after the word, as
    /// before. Delete then takes the space and the strip offers the word back, as after an
    /// autocorrection. Learn how I type learns nothing from it.
    func testAnEmojiTappedForTheWordBeingTypedTakesItsPlaceAndDeleteOffersTheWordBack() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let settings = TypingJournalSettings(defaults: defaults)
        settings.isEnabled = true
        let journal = TypingJournal(directory: directory, settings: settings)
        engine.journal = journal
        let smile = KeyboardSuggestion(displayText: "😊", replacement: "😊", kind: .emoji)
        let pizza = KeyboardSuggestion(displayText: "🍕", replacement: "🍕", kind: .emoji)

        type("we smile")
        engine.acceptSuggestion(smile)
        XCTAssertEqual(target.text, "we 😊 ", "in the word's place, with its space")
        XCTAssertNotEqual(engine.suggestions.first?.kind, .revert, "not before delete, as after an autocorrection")
        tap(.delete)
        XCTAssertEqual(target.text, "we 😊", "delete takes the space")
        XCTAssertEqual(engine.suggestions.first, suggestion("smile", .revert), "and the strip offers the word back")
        engine.acceptSuggestion(engine.suggestions[0])
        XCTAssertEqual(target.text, "we smile")

        type(" pizza ")
        engine.acceptSuggestion(pizza)
        XCTAssertEqual(target.text, "we smile pizza 🍕 ", "after the space, after the word")
        XCTAssertTrue(journal.current.corrections.isEmpty, "no typing fix: \(journal.current.corrections)")
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
            panel.frame = CGRect(x: 0, y: 0, width: 393, height: 44)
            panel.layoutIfNeeded()
            let done = try XCTUnwrap(panel.subviews.first { $0.accessibilityLabel == "Done" })
            let cancel = try XCTUnwrap(panel.subviews.first { $0.accessibilityLabel == "Cancel dictation" })
            let well = try XCTUnwrap(panel.subviews.first { $0.subviews.contains(panel.scope) })
            if onLeft {
                XCTAssertEqual(done.frame.minX, KeyboardMetrics.portrait.side, accuracy: 0.5)
                XCTAssertLessThan(done.frame.maxX, cancel.frame.minX)
                XCTAssertLessThan(cancel.frame.maxX, well.frame.minX)
            } else {
                XCTAssertEqual(done.frame.maxX, 393 - KeyboardMetrics.portrait.side, accuracy: 0.5)
                XCTAssertLessThan(cancel.frame.maxX, done.frame.minX)
                XCTAssertLessThan(well.frame.maxX, cancel.frame.minX)
            }
        }
    }

    /// iOS hands the field's traits to the keyboard as it brings it on screen, after its first
    /// words were chosen without them, so they change then ("the" to "The" once the field says it
    /// capitalizes, "I'm" to "I’m"). Until the keyboard has appeared those changes just show;
    /// once it is on screen they roll, and the next appearance (another field) holds still again.
    func testTheStripsWordsJustShowUntilTheKeyboardIsOnScreen() throws {
        let window = try XCTUnwrap(UIApplication.shared.connectedScenes.compactMap { ($0 as? UIWindowScene)?.windows.first }.first)
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        view.frame = CGRect(x: 0, y: 0, width: 393, height: 254)
        window.addSubview(view)
        defer { view.removeFromSuperview() }
        view.layoutIfNeeded()
        let strip = view.stripView
        func show(_ words: [String]) -> Double? {
            strip.update(suggestions: words.map { KeyboardSuggestion(displayText: $0, replacement: $0, kind: .nextWord) }, showsUndo: false, statusText: nil)
            return strip.debugMotionSample()["slot1"]
        }
        XCTAssertEqual(show(["I", "the", "I'm"]), 1)
        XCTAssertEqual(show(["I", "The", "I’m"]), 1, "appearing: the recased word is just there")
        view.keyboardDidAppear()
        XCTAssertEqual(show(["I", "They", "I’m"]), 0, "on screen: a change rolls")
        view.prepareForField()
        XCTAssertEqual(show(["I", "There", "I’m"]), 1, "appearing again, for another field")
    }

    /// The Hex key's menu ends with Open Hex: opening the menu opens nothing (#2707: only a tap
    /// that says Open Hex opens Hex), and a tap on the row closes the menu and opens Hex at Home.
    func testTheHexKeyMenusOpenHexOpensHexAtHomeOnlyWhenTapped() throws {
        let model = KeyboardViewModel()
        var opened: [URL] = []
        model.onOpenURL = { opened.append($0) }
        let view = FullKeyboardView(model: model, target: target)
        view.frame = CGRect(x: 0, y: 0, width: 393, height: 254)
        view.layoutIfNeeded()
        view.debugShowStyleMenu()
        view.layoutIfNeeded()
        let menu = try XCTUnwrap(view.subviews.first { $0 is HexStyleMenuView })
        menu.layoutIfNeeded()
        func rows(in view: UIView) -> [UIControl] {
            view.subviews.flatMap { subview -> [UIControl] in
                if let control = subview as? UIControl, control.accessibilityLabel != nil { return [control] }
                return rows(in: subview)
            }
        }
        let all = rows(in: menu)
        XCTAssertEqual(all.last?.accessibilityLabel, "Open Hex", "the last row")
        XCTAssertTrue(opened.isEmpty, "opening the menu opens nothing")
        all.last?.sendActions(for: .touchUpInside)
        XCTAssertEqual(opened, [KeyboardLaunchRequest.home.url])
        XCTAssertFalse(view.subviews.contains { $0 is HexStyleMenuView }, "the menu closes")
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
}
