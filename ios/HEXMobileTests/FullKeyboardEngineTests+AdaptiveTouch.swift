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
}

// MARK: - Adaptive touch (Settings › Labs)

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
