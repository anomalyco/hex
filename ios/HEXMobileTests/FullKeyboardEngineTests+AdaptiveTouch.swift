import UIKit
import XCTest
@testable import HEX

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
        XCTAssertEqual(learnedTyping { $0.keyboardType = .webSearch }, 10, "web search learns as a plain field")
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

    // MARK: Typing configs (Settings › Experiments › Typing configs)

    /// A format-0 file for `layout`'s keyboard: `words` and `keys` as JSON fragments.
    private func configData(words: String = "{}", keys: String = "[]", extra: String = "") -> Data {
        Data("""
        {"format": "hex.typing-config", "version": 0, "id": "cfg-0000000000000000", "basedOn": null,
         "madeFrom": {"exportId": "export-test", "hexVersion": "9.0.0", "hexBuild": "1", "logDays": [], "heldOut": {"tunedOn": "A", "checkedOn": "B"}},
         "madeFor": {"device": "iPhone19,2", "language": "en"}, "note": "test", \(extra)
         "words": \(words), "keys": \(keys)}
        """.utf8)
    }

    private func contextJSON(_ layout: KeyboardLayout, width: Double? = nil, hitModel: Int = KeyboardZone.hitModel) throws -> String {
        var c = TypingConfigFile.Context(layout: layout)
        c.width = width ?? c.width
        c.hitModel = hitModel
        return try XCTUnwrap(String(data: JSONEncoder().encode(c), encoding: .utf8))
    }

    private var zoneProof: String { #""evidence": {"tuned": {"taps": 30, "gained": 14, "lost": 2, "wordsRight": 12.5, "wordsWrong": 1.3}, "checked": {"taps": 24, "gained": 11, "lost": 1, "wordsRight": 9.6, "wordsWrong": 0.7}}"# }
    private var aimProof: String { #""evidence": {"tuned": {"taps": 200, "gained": 20, "lost": 4}, "checked": {"taps": 150, "gained": 18, "lost": 3}}"# }

    private func outcome(_ review: TypingConfigReview, _ path: String) -> TypingConfigReview.Outcome? {
        review.items.first { $0.path == path }?.outcome
    }

    func testTypingConfigReviewNamesWhatItAppliesClampsAndSkips() throws {
        let layout = KeyboardLayout(width: 402)
        let here = try contextJSON(layout), elsewhere = try contextJSON(layout, width: 375)
        let words = """
        {"keep": [{"word": "eepy", "evidence": {"times": 3, "sessions": 3}}, {"word": "zork", "evidence": {"times": 1, "sessions": 1}},
                  {"word": "x", "evidence": {"times": 9, "sessions": 9}}, {"word": "eepy", "evidence": {"times": 3, "sessions": 3}}],
         "neverCorrectTo": [{"word": "DONT", "evidence": {"takenBack": 2}}],
         "fixes": [{"typo": "thus", "word": "this", "mode": "suggest", "evidence": {"times": 2, "sessions": 2}}],
         "dictation": [{"heard": "hacks", "meant": "Hex", "evidence": {"times": 2}}]}
        """
        let keys = """
        [{"context": \(here), "aim": {"o": {"offset": [0.0, 0.5], \(aimProof)}, "1": {"offset": [0, 0], \(aimProof)}},
          "zones": [{"key": "space", "edge": "top", "move": 0.1, \(zoneProof)}, {"key": "space", "edge": "bottom", "move": 0.1, \(zoneProof)},
                    {"key": "n", "edge": "bottom", "move": -0.1, \(zoneProof)}, {"key": "o", "edge": "bottom", "move": 0.4, \(zoneProof)},
                    {"key": "e", "edge": "right", "move": 0.1, "evidence": {"tuned": {"taps": 30, "gained": 14, "lost": 2}, "checked": {"gained": 11, "lost": 1}}}]},
         {"context": \(elsewhere), "aim": {"e": {"offset": [0.1, 0.0], \(aimProof)}}}]
        """
        let review = TypingConfigReview(data: configData(words: words, keys: keys, extra: #""colour": "blue","#), keyboard: layout)
        XCTAssertNil(review.refused)
        XCTAssertEqual(outcome(review, "colour"), .skipped(.unknownField))
        XCTAssertEqual(outcome(review, "words.keep[0]"), .applied)
        XCTAssertEqual(outcome(review, "words.keep[1]"), .skipped(.notEnoughEvidence))
        XCTAssertEqual(outcome(review, "words.keep[2]"), .skipped(.invalidValue))
        let sixth = try XCTUnwrap(review.items.first { $0.path == "words.keep[2]" })
        XCTAssertEqual(sixth.list, .keep)
        XCTAssertEqual(sixth.position, 2, "the sheet can say which entry, from the result alone")
        XCTAssertEqual(review.items.first { $0.path == "words.keep[0]" }?.evidence?.times, 3)
        XCTAssertEqual(review.items.first { $0.path == "keys[0].zones[3]" }?.evidence?.tuned?.taps, 30)
        XCTAssertEqual(review.items.first { $0.path == "colour" }?.list, .file)
        XCTAssertEqual(outcome(review, "words.keep[3]"), .skipped(.duplicate))
        XCTAssertEqual(review.applied.keep, ["eepy"])
        XCTAssertEqual(review.applied.neverCorrectTo, ["DONT"])
        XCTAssertEqual(review.applied.fixes.map(\.mode), [.suggest])
        XCTAssertEqual(review.applied.dictation, [DictationCorrection(heard: "hacks", meant: "Hex")])
        XCTAssertEqual(outcome(review, "keys[0].aim.o"), .clamped(from: [0, 0.5], to: [0, 0.3]))
        XCTAssertEqual(outcome(review, "keys[0].aim.1"), .skipped(.invalidValue))
        XCTAssertEqual(outcome(review, "keys[0].zones[1]"), .skipped(.edgeNotAllowed), "the space bar's bottom is the keyboard's edge")
        // The space bar growing up and n giving its bottom away claim the same strip.
        XCTAssertEqual(outcome(review, "keys[0].zones[0]"), .skipped(.twoZonesClaimPoint))
        XCTAssertEqual(outcome(review, "keys[0].zones[2]"), .skipped(.twoZonesClaimPoint))
        XCTAssertEqual(outcome(review, "keys[0].zones[3]"), .clamped(from: [0.4], to: [0.25]))
        XCTAssertEqual(outcome(review, "keys[0].zones[4]"), .skipped(.notEnoughEvidence), "taps alone don't say what the words gain")
        XCTAssertEqual(outcome(review, "keys[1].aim.e"), .skipped(.forAnotherKeyboard))
        XCTAssertEqual(review.contexts.map(\.differences), [[], ["width"]])
        XCTAssertEqual(review.applied.aim, ["o": CGVector(dx: 0, dy: 0.3)])
        XCTAssertEqual(review.items.map(\.path).filter { $0.hasPrefix("words") }.count, 7, "every entry has an item")

        let newer = TypingConfigReview(data: Data(#"{"format": "hex.typing-config", "version": 1, "colour": 1}"#.utf8), keyboard: layout)
        XCTAssertEqual(outcome(newer, "colour"), .skipped(.newerFormat))
        XCTAssertEqual(TypingConfigReview(data: Data("[]".utf8), keyboard: layout).refused, .notAConfig)
        XCTAssertEqual(TypingConfigReview(data: Data(count: TypingConfigLimits.maxBytes + 1), keyboard: layout).refused, .tooLarge)
    }

    func testTypingConfigZonesMoveWhereATapCountsAndKeepEveryKeycapCentre() throws {
        let layout = KeyboardLayout(width: 402)
        let keys = layout.keys
        let b = try XCTUnwrap(keys.firstIndex { $0.character == "b" }), space = try XCTUnwrap(keys.firstIndex { $0.kind == .space })
        func owner(_ map: KeyboardZoneMap, _ p: CGPoint) -> Int? {
            keys.indices.first { k in (map.regions[k] ?? [keys[k].hitFrame] + (keys[k].gapHitFrame.map { [$0] } ?? [])).contains { $0.contains(p) } }
        }
        let low = CGPoint(x: keys[b].frame.midX, y: keys[b].frame.maxY - 3)
        XCTAssertEqual(owner(KeyboardZoneMap(), low), b)
        let up = layout.zoneMap([KeyboardZone(key: "space", edge: .top, move: 0.15)])
        XCTAssertEqual(up.skipped, [nil])
        XCTAssertEqual(owner(up, low), space, "3 pt above b's keycap bottom is now the space bar's")
        XCTAssertEqual(owner(up, CGPoint(x: keys[b].frame.midX, y: keys[b].frame.midY)), b)
        // Every point still has exactly one key.
        for y in stride(from: layout.touchAreaFrame.minY + 0.25, to: layout.touchAreaFrame.maxY, by: 1.5) {
            for x in stride(from: 0.25, to: layout.width, by: 1.5) {
                let p = CGPoint(x: x, y: y)
                let owners = keys.indices.filter { k in (up.regions[k] ?? [keys[k].hitFrame] + (keys[k].gapHitFrame.map { [$0] } ?? [])).contains { $0.contains(p) } }
                XCTAssertLessThanOrEqual(owners.count, 1, "\(p)")
            }
        }
        XCTAssertEqual(layout.zoneMap([KeyboardZone(key: "space", edge: .top, move: 0.25)]).skipped, [.keyTooSmall], "c, v, b, n and m would lose their centres")
        XCTAssertEqual(layout.zoneMap([KeyboardZone(key: "q", edge: .top, move: 0.1)]).skipped, [.edgeNotAllowed])
        XCTAssertEqual(layout.zoneMap([KeyboardZone(key: "ß", edge: .top, move: 0.1)]).skipped, [.keyNotOnKeyboard])
        let old = try contextJSON(layout, hitModel: KeyboardZone.hitModel - 1)
        let review = TypingConfigReview(data: configData(keys: #"[{"context": \#(old), "zones": [{"key": "space", "edge": "top", "move": 0.1, \#(zoneProof)}]}]"#), keyboard: layout)
        XCTAssertEqual(outcome(review, "keys[0].zones[0]"), .skipped(.needsCheckingAgain))
    }

    func testTypingConfigCompilesTheSwitchedOnPartsAndTheStoreWritesThem() throws {
        let layout = KeyboardLayout(width: 402)
        let here = try contextJSON(layout)
        let data = configData(
            words: #"{"keep": [{"word": "eepy", "evidence": {"times": 3, "sessions": 2}}, {"word": "yoy", "evidence": {"times": 2, "sessions": 2}}]}"#,
            keys: #"[{"context": \#(here), "aim": {"o": {"offset": [0.0, 0.2], \#(aimProof)}}, "zones": [{"key": "space", "edge": "top", "move": 0.15, \#(zoneProof)}]}]"#
        )
        let review = TypingConfigReview(data: data, keyboard: layout)
        XCTAssertEqual(review.switchable.count, 4)
        let compiled = try XCTUnwrap(review.compiled(switchedOn: Set(review.switchable).subtracting([.keep("yoy")])))
        XCTAssertNil(compiled.note)
        let suite = "typing-config-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = TypingConfigStore(settings: TypingConfigSettings(defaults: defaults),
                                      directory: FileManager.default.temporaryDirectory.appendingPathComponent(suite))
        try store.writeActive(compiled)
        XCTAssertEqual(store.settings.generation, 1)
        XCTAssertNil(store.activeData(fullAccess: true), "the experiment is off")
        store.settings.isEnabled = true
        XCTAssertNil(store.activeData(fullAccess: false), "no Full Access, no config")
        let active = try XCTUnwrap(store.activeData(fullAccess: true))
        let read = TypingConfigReview(data: active, keyboard: layout, checksEvidence: false)
        XCTAssertEqual(read.applied.keep, ["eepy"])
        XCTAssertEqual(read.applied.aim, ["o": CGVector(dx: 0, dy: 0.2)])
        XCTAssertEqual(read.applied.zones, [KeyboardZone(key: "space", edge: .top, move: 0.15)])
        store.removeActive()
        XCTAssertEqual(store.settings.generation, 2)
        XCTAssertNil(store.activeData(fullAccess: true))
    }

    func testATypingConfigMovesWhereTapsCountAndItsAimOnlyInTheKeyboard() throws {
        engine.width = 402
        let layout = engine.layout
        let here = try contextJSON(layout)
        let b = try XCTUnwrap(layout.keys.first { $0.character == "b" })
        let low = CGPoint(x: b.frame.midX, y: b.frame.maxY - 3)
        XCTAssertEqual(engine.layout.hitTest(low)?.kind, b.kind)
        let before = try XCTUnwrap(engine.autocorrect.decoderKeys?.centres["o"])
        let data = configData(keys: #"[{"context": \#(here), "aim": {"o": {"offset": [0.0, 0.2]}}, "zones": [{"key": "space", "edge": "top", "move": 0.15}]}]"#)
        engine.typingConfig = TypingConfigActive(data: data)
        XCTAssertEqual(engine.layout.hitTest(low)?.kind, .space, "the zone gives the space bar b's lowest 3 pt")
        let o = try XCTUnwrap(layout.keys.first { $0.character == "o" })
        let after = try XCTUnwrap(engine.autocorrect.decoderKeys?.centres["o"])
        XCTAssertEqual(after.y - before.y, 0.2 * o.frame.height, accuracy: 0.01, "the aim replaces o's centre")
        XCTAssertNotNil(engine.autocorrect.decoderBaselineKeys, "the baseline stays what Hex learned")
        engine.typingConfig = nil
        XCTAssertEqual(engine.layout, layout, "removing it puts every edge back")
        XCTAssertEqual(engine.autocorrect.decoderKeys?.centres["o"], before)
        XCTAssertNil(engine.autocorrect.decoderBaselineKeys)
    }

    func testATypingConfigsWordsAreTheirOwnLayerAndLeaveWithIt() throws {
        func typed(_ word: String) -> String {
            let start = target.text.count
            type(word + " ")
            return String(target.text.dropFirst(start))
        }
        XCTAssertEqual(typed("teh"), "the ", "autocorrect fixes it without a config")
        engine.typingConfig = TypingConfigActive(data: configData(words: #"{"keep": [{"word": "teh"}]}"#))
        XCTAssertTrue(engine.autocorrect.learning.keeps("teh"))
        XCTAssertNil(engine.journal, "nothing was written into what Hex learned")
        XCTAssertEqual(typed("teh"), "teh ", "a word of yours is left alone")
        engine.typingConfig = TypingConfigActive(data: configData(words: #"{"neverCorrectTo": [{"word": "the"}]}"#))
        XCTAssertEqual(typed("hte"), "hte ", "never corrected to a word the config rules out")
        engine.typingConfig = nil
        XCTAssertEqual(engine.autocorrect.learning, TypingLearning(), "removing the config removes exactly its words")
        XCTAssertEqual(typed("teh"), "the ")
    }

    func testTypingConfigReviewChecksEveryKeyboardAndKeepsEachBlockThatApplies() throws {
        let upright = KeyboardLayout(width: 402), sideways = KeyboardLayout(width: 874, orientation: .landscape)
        let french = KeyboardLayout(width: 402, language: .french)
        let keyboards = [TypingConfigKeyboard(layout: upright), TypingConfigKeyboard(layout: sideways)]
        let keys = """
        [{"context": \(try contextJSON(upright)), "aim": {"o": {"offset": [0.0, 0.2], \(aimProof)}}},
         {"context": \(try XCTUnwrap(String(data: JSONEncoder().encode(TypingConfigFile.Context(layout: french)), encoding: .utf8))), "aim": {"e": {"offset": [0.1, 0.0], \(aimProof)}}},
         {"context": \(try XCTUnwrap(String(data: JSONEncoder().encode(TypingConfigFile.Context(layout: sideways)), encoding: .utf8))), "zones": [{"key": "space", "edge": "top", "move": 0.1, \(zoneProof)}]},
         {"context": \(try contextJSON(upright)), "aim": {"p": {"offset": [0.0, 0.2], \(aimProof)}}}]
        """
        let review = TypingConfigReview(data: configData(keys: keys), keyboards: keyboards)
        XCTAssertEqual(review.contexts.map(\.keyboardIndex), [0, nil, 1, nil])
        XCTAssertEqual(review.contexts[1].differences, ["language", "letters"], "against the nearest keyboard")
        XCTAssertEqual(outcome(review, "keys[0].aim.o"), .applied)
        XCTAssertEqual(outcome(review, "keys[1].aim.e"), .skipped(.forAnotherKeyboard))
        XCTAssertEqual(outcome(review, "keys[2].zones[0]"), .applied)
        XCTAssertEqual(outcome(review, "keys[3].aim.p"), .skipped(.duplicate), "a second block for the same keyboard")
        XCTAssertEqual(review.applied.blocks.map(\.block), [0, 2])
        let compiled = try XCTUnwrap(review.compiled(switchedOn: Set(review.switchable)))
        XCTAssertEqual(compiled.keys?.count, 2, "every block for one of the person's keyboards")
        let active = try XCTUnwrap(TypingConfigActive(data: JSONEncoder().encode(compiled)))
        XCTAssertEqual(active.keys(on: upright)?.aim, ["o": CGVector(dx: 0, dy: 0.2)])
        XCTAssertEqual(active.keys(on: sideways)?.zones, [KeyboardZone(key: "space", edge: .top, move: 0.1)])
        XCTAssertNil(active.keys(on: french))

        let suite = "typing-config-keyboards-\(UUID().uuidString)"
        let settings = TypingConfigSettings(defaults: UserDefaults(suiteName: suite))
        defer { UserDefaults().removePersistentDomain(forName: suite) }
        settings.record(keyboards[0])
        settings.record(keyboards[1])
        settings.record(keyboards[0])
        XCTAssertEqual(settings.keyboards.map(\.context.orientation), ["portrait", "landscape"], "most recent first, once each")
    }

    func testTypingConfigReviewCleansTheNoteAndEveryWord() throws {
        let long = String(repeating: "a", count: 300)
        let words = """
        {"keep": [{"word": " eepy ", "evidence": {"times": 2, "sessions": 2}}, {"word": "ad\\u202Emin", "evidence": {"times": 2, "sessions": 2}},
                  {"word": "pass\\u200Bword", "evidence": {"times": 2, "sessions": 2}}, {"word": "ab\\u200Dcd", "evidence": {"times": 2, "sessions": 2}},
                  {"word": "\\u0645\\u06CC\\u200C\\u062E\\u0648\\u0627\\u0647\\u0645", "evidence": {"times": 2, "sessions": 2}},
                  {"word": "\\u0915\\u094D\\u200D\\u0937\\u092E\\u093E", "evidence": {"times": 2, "sessions": 2}},
                  {"word": "cafe\\u0301", "evidence": {"times": 2, "sessions": 2}}, {"word": "\\u200B\\u2066", "evidence": {"times": 2, "sessions": 2}}],
         "fixes": [{"typo": "te\\u2067h", "word": "the", "mode": "apply", "evidence": {"times": 2, "sessions": 2}}]}
        """
        let data = Data(#"{"format": "hex.typing-config", "version": 0, "note": "line one\nline\u202Etwo \#(long)", "words": \#(words)}"#.utf8)
        let review = TypingConfigReview(data: data, keyboard: KeyboardLayout(width: 402))
        XCTAssertNil(review.refused)
        XCTAssertEqual(review.note?.hasPrefix("line one linetwo "), true, "controls and overrides out of the note")
        XCTAssertEqual(review.note?.count, TypingConfigLimits.noteMaxCharacters)
        XCTAssertEqual(outcome(review, "words.keep[0]"), .applied)
        XCTAssertEqual(outcome(review, "words.keep[1]"), .skipped(.hiddenCharacters), "a right-to-left override")
        XCTAssertEqual(outcome(review, "words.keep[2]"), .skipped(.hiddenCharacters), "a zero-width space")
        XCTAssertEqual(outcome(review, "words.keep[3]"), .skipped(.hiddenCharacters), "a joiner between Latin letters")
        XCTAssertEqual(outcome(review, "words.keep[4]"), .applied, "Persian keeps its non-joiner")
        XCTAssertEqual(outcome(review, "words.keep[5]"), .applied, "Devanagari keeps its joiner")
        XCTAssertEqual(outcome(review, "words.keep[7]"), .skipped(.hiddenCharacters))
        XCTAssertEqual(outcome(review, "words.fixes[0]"), .skipped(.hiddenCharacters))
        XCTAssertEqual(review.applied.keep, ["eepy", "\u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645}", "\u{0915}\u{094D}\u{200D}\u{0937}\u{092E}\u{093E}", "caf\u{E9}"])
        XCTAssertEqual(review.items.first { $0.path == "words.keep[6]" }?.text, "caf\u{E9}", "what the sheet shows is what the keyboard loads")
        XCTAssertTrue(review.applied.fixes.isEmpty)
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
