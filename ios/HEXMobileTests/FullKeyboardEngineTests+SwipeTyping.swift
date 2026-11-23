import UIKit
import XCTest
@testable import HEX

extension FullKeyboardEngineTests {
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

    /// Record typing logs: a word of someone's own in capitals took a swiped word's place.
    /// "HELO" from Contacts outranks the model's "hello" (rarer in its books than your own words
    /// cost), and "THE" takes the place of "the"; the swipe types "hello" and "the", the strip
    /// offers "HELO", and caps lock still swipes it.
    func testASwipeTypesTheOrdinaryWordAndOffersYourWordInCapitals() throws {
        let (field, plane) = try swipeKeyboard()
        plane.engine.autocorrect.loadLexiconEntries([("HELO", "HELO"), ("THE", "THE")])
        swipe("hello", on: plane)
        XCTAssertEqual(field.text, "hello")
        XCTAssertTrue(plane.engine.suggestions.contains { $0.replacement == "HELO" }, "offered in the strip")
        swipe("the", on: plane)
        XCTAssertEqual(field.text, "hello the")
        let (locked, capsKeys) = try swipeKeyboard()
        capsKeys.engine.autocorrect.loadLexiconEntries([("HELO", "HELO")])
        tap(.shift, on: capsKeys)
        tap(.shift, on: capsKeys)
        swipe("hello", on: capsKeys)
        XCTAssertEqual(locked.text, "HELO", "caps lock")
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

    // MARK: - Number row (Settings › Keyboard › Number row)

    /// With the number row on, digits type straight from the letters page beside letters, and
    /// nothing corrects a word holding them: "12b", "9pm," and "0199" stay as typed, with
    /// Spatial autocorrect reading the letters around them.
    func testNumberRowTypesDigitsBesideLettersAndNothingCorrectsThem() throws {
        engine.showsNumberRow = true
        let model = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        engine.letterOdds = model
        engine.autocorrect.decoder = TypingDecoder(model: model)
        XCTAssertTrue(engine.layout.hasNumberRow)
        XCTAssertEqual(engine.layout.layer, .letters)
        tap(.shift)
        type("flat 12b at 9pm")
        XCTAssertEqual(engine.layout.layer, .letters, "the digits never leave the letters page")
        tap(.layerSwitch(.numbers))
        XCTAssertTrue(engine.layout.keys.filter(\.isNumberRow).isEmpty, "the 123 page's row 1 is the digits")
        tap(.character(","))
        type(" call 555 0199")
        XCTAssertEqual(target.text, "Flat 12b at 9pm, call 555 0199")
        tap(.space)
        XCTAssertEqual(target.text, "Flat 12b at 9pm, call 555 0199 ")
        let keys = try XCTUnwrap(engine.autocorrect.decoderKeys)
        XCTAssertFalse(keys.centres.keys.contains { $0.isNumber }, "digits aren't in the decoder's letters")
    }

    /// Next-letter touch help never turns a digit into a letter or a letter into a digit: a tap
    /// on a digit's edge right by row 1 types the digit, and a tap on a row-1 letter's top edge
    /// right under the digits types a letter, however likely the letters around them are.
    func testNextLetterHelpLeavesTheNumberRowAlone() throws {
        engine.showsNumberRow = true
        engine.letterOdds = try XCTUnwrap(TypingDecoderModel.bundled("en"))
        type("th")
        let layout = engine.layout
        for digit in layout.keys where digit.isNumberRow {
            let edge = CGPoint(x: digit.frame.midX, y: digit.hitFrame.maxY - 0.5)
            XCTAssertEqual(engine.key(at: edge)?.kind, digit.kind)
        }
        for letter in layout.keys where letter.row == 0 {
            let edge = CGPoint(x: letter.frame.midX, y: letter.hitFrame.minY + 0.5)
            XCTAssertEqual(engine.key(at: edge)?.character?.first?.isLetter, true, letter.label)
        }
    }

    /// A drag from a digit never becomes a swipe, and a swipe across row 1 under the digits
    /// still types its word.
    func testSwipesNeverStartOnTheNumberRowAndStillWorkUnderIt() throws {
        let (field, plane) = try swipeKeyboard()
        plane.engine.showsNumberRow = true
        let one = try XCTUnwrap(plane.engine.layout.key(.character("1")))
        let zero = try XCTUnwrap(plane.engine.layout.key(.character("0")))
        let points = (0...10).map { i in
            CGPoint(x: one.frame.midX + (zero.frame.midX - one.frame.midX) * CGFloat(i) / 10, y: one.frame.midY)
        }
        swipe(points: points, on: plane, lift: false)
        XCTAssertFalse(plane.engine.isSwiping)
        plane.ended(id: 50, sample: sample(points[points.count - 1]))
        XCTAssertFalse(field.text.contains { $0.isLetter }, field.text)

        let (rowOne, rowOnePlane) = try swipeKeyboard()
        rowOnePlane.engine.showsNumberRow = true
        swipe("power", on: rowOnePlane)
        XCTAssertEqual(rowOne.text, "power")
    }

    /// Without the switch, or on an iPad, the keys have no number row.
    func testNumberRowNeedsItsSwitchAndAnIPhone() {
        XCTAssertFalse(engine.layout.hasNumberRow, "off by default")
        engine.showsNumberRow = true
        XCTAssertTrue(engine.layout.hasNumberRow)
        engine.pad = .docked(.mini)
        XCTAssertFalse(engine.hasNumberRow)
        XCTAssertFalse(engine.layout.hasNumberRow)
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

    // MARK: - The host's update after Hex's own insert

    /// A host reports `textDidChange` for the keys' own edits, on the next runloop in
    /// some apps: the echo leaves the swipe's pending space and its word alone.
    func testAHostEchoAfterASwipeKeepsItsPendingSpace() throws {
        let (field, plane) = try swipeKeyboard()
        field.autocapitalizationType = .sentences
        plane.engine.refreshAutoCapitalization()
        swipe("this", on: plane)
        plane.engine.textDidChange()
        swipe("is", on: plane)
        plane.engine.textDidChange()
        tap(.character("a"), on: plane)
        swipe("test", on: plane)
        XCTAssertEqual(field.text, "This is a test")
    }

    /// A host whose proxy lags `insertText` sends `textDidChange` while still
    /// reporting the text from before it, nothing, or an empty string: word boundaries and
    /// casing read what Hex typed, so the space and the lowercase stay right.
    func testALaggingHostKeepsThePendingSpaceAndAddsNoStrayCapital() throws {
        for lag in LaggingField.Lag.allCases {
            let field = LaggingField()
            field.lag = lag
            let (_, plane) = try swipeKeyboard(target: field)
            field.autocapitalizationType = .sentences
            plane.engine.refreshAutoCapitalization()
            swipe("this", on: plane)
            plane.engine.textDidChange()
            swipe("is", on: plane)
            plane.engine.textDidChange()
            tap(.character("a"), on: plane)
            swipe("test", on: plane)
            XCTAssertEqual(field.text, "This is a test", "\(lag)")
        }
    }

    /// A host that stays behind longer than the keys believe one (`typingContextLifetime`)
    /// is read as it reports, so the swipe's pending space is no longer trusted.
    func testAHostStillBehindAfterTheLifetimeIsNoLongerBelieved() throws {
        let field = LaggingField()
        field.lag = .nothing
        let (_, plane) = try swipeKeyboard(target: field)
        plane.engine.clock = { [unowned self] in self.now }
        swipe("hello", on: plane)
        now += FullKeyboardEngine.typingContextLifetime + 1
        plane.engine.textDidChange()
        tap(.character("w"), on: plane)
        XCTAssertEqual(field.text, "hellow", "read as the host reports")
    }

    /// Text changed elsewhere, or a cursor move, is not Hex's echo: it still takes the
    /// swipe's pending space, and casing reads where the cursor now is.
    func testAChangeFromElsewhereOrACursorMoveDropsThePendingSpace() throws {
        for change in ["host edit", "cursor move"] {
            let (field, plane) = try swipeKeyboard()
            field.autocapitalizationType = .sentences
            plane.engine.refreshAutoCapitalization()
            swipe("hello", on: plane)
            XCTAssertEqual(field.text, "Hello")
            if change == "host edit" {
                field.insertText("x")
            } else {
                field.adjustTextPosition(byCharacterOffset: -field.cursorIndex)
            }
            plane.engine.textDidChange()
            XCTAssertEqual(plane.engine.isUppercase, change == "cursor move", "a cursor at the field's start")
            tap(.character("w"), on: plane)
            XCTAssertEqual(field.text, change == "host edit" ? "Helloxw" : "WHello", change)
        }
    }

    /// A delete slide right after a swipe (inside the short window a lagging host is believed)
    /// takes the swiped word's pending space with it: the next tap starts a new word.
    func testADeleteSlideRightAfterASwipeTakesItsPendingSpace() throws {
        let (field, plane) = try swipeKeyboard()
        field.autocapitalizationType = .sentences
        plane.engine.refreshAutoCapitalization()
        swipe("this", on: plane)
        swipe("test", on: plane)
        XCTAssertEqual(field.text, "This test")

        let step = plane.engine.layout.letterWidth
        let slide = DeleteWordSwipe(engine: plane.engine, touchID: 1, startX: 0)
        slide.move(to: -step * 1.2)
        XCTAssertEqual(field.text, "This ")
        tap(.character("a"), on: plane)
        XCTAssertEqual(field.text, "This a", "one space, not two")
    }

    /// An autocorrection on space, with a host that still reports the text from before it: the
    /// next swipe reads the corrected text, so it adds no second space.
    func testAnAutocorrectionOnSpaceBehindAHostThenASwipeAddsNoSecondSpace() throws {
        let field = StaleField()
        let (_, plane) = try swipeKeyboard(target: field)
        for letter in "teh" { tap(.character(String(letter)), on: plane) }
        tap(.space, on: plane)
        XCTAssertEqual(field.text, "the ", "autocorrected on space")
        field.stale = "teh"
        plane.engine.textDidChange()
        swipe("world", on: plane)
        XCTAssertEqual(field.text, "the world", "no second space")
    }

    /// A field cleared from somewhere else (a send) takes the swipe's pending space with it:
    /// the next tap starts an empty field, capital and all.
    func testAFieldClearedAfterASwipeTakesThePendingSpaceWithIt() throws {
        let (field, plane) = try swipeKeyboard()
        field.autocapitalizationType = .sentences
        plane.engine.refreshAutoCapitalization()
        swipe("hello", on: plane)
        XCTAssertEqual(field.text, "Hello")
        while !field.text.isEmpty { field.deleteBackward() }
        plane.engine.textDidChange()
        XCTAssertTrue(plane.engine.isUppercase, "an empty field starts a sentence")
        tap(.character("a"), on: plane)
        XCTAssertEqual(field.text, "A", "no space and a capital")
    }

    /// A swipe whose word carries an apostrophe goes in with a smart quote, while the keys read
    /// the host's straight one: the echo still matches, so the pending space stays.
    func testASmartQuoteSwipeKeepsItsPendingSpaceAcrossAnEcho() throws {
        let (field, plane) = try swipeKeyboard()
        field.smartQuotesType = .yes
        let key = try XCTUnwrap(plane.engine.layout.key(.character("t")))
        plane.engine.touchBegan(id: 90, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
        plane.engine.beginSwipe(touchID: 90)
        plane.engine.endSwipe(typing: ["don't"], points: 12, decodeTime: 0.001)
        XCTAssertEqual(field.text, "don\u{2019}t", "smart quotes curled it")
        plane.engine.textDidChange()
        tap(.character("w"), on: plane)
        XCTAssertEqual(field.text, "don\u{2019}t w")
    }

    /// A delete right after a pick, with a host still reporting the word before it: the held
    /// space goes, not a letter.
    func testADeleteAfterAPickBehindAHostTakesTheHeldSpaceNotALetter() throws {
        let field = StaleField(text: "teh world")
        let (_, plane) = try swipeKeyboard(target: field)
        field.adjustTextPosition(byCharacterOffset: -6)
        plane.engine.textDidChange()
        let pick = try XCTUnwrap(plane.engine.suggestions.first { $0.replacement == "the" })
        plane.engine.acceptSuggestion(pick)
        XCTAssertEqual(field.text, "the world")
        field.stale = ""
        tap(.delete, on: plane)
        XCTAssertEqual(field.text, "the world", "the held space goes, not a letter")
    }

    /// Another of the swipe's readings picked from the strip, with a host still reporting the
    /// old word: the new word keeps its pending space.
    func testAPickedSwipeReadingBehindAHostKeepsItsPendingSpace() throws {
        let field = StaleField()
        let (_, plane) = try swipeKeyboard(target: field)
        let key = try XCTUnwrap(plane.engine.layout.key(.character("h")))
        plane.engine.touchBegan(id: 91, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
        plane.engine.beginSwipe(touchID: 91)
        plane.engine.endSwipe(typing: ["hello", "hullo"], points: 12, decodeTime: 0.001)
        XCTAssertEqual(field.text, "hello")
        field.stale = "hello"
        let reading = try XCTUnwrap(plane.engine.suggestions.first { $0.replacement == "hullo" })
        plane.engine.acceptSuggestion(reading)
        XCTAssertEqual(field.text, "hullo")
        plane.engine.textDidChange()
        tap(.character("a"), on: plane)
        XCTAssertEqual(field.text, "hullo a")
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
        for (before, after) in [("Hi.", "Hi. "), ("(", "("), ("¿", "¿"), ("say \"", "say \""), ("3", "3 "), ("x\n", "x\n"), ("Look \u{FFFC}", "Look \u{FFFC}")] {
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
}

/// A host whose proxy keeps reporting `stale` until it is cleared, as one that has not caught
/// up to the keys' last insert yet.
@MainActor
private final class StaleField: FixtureTextTarget {
    var stale: String?

    override var documentContextBeforeInput: String? {
        stale ?? super.documentContextBeforeInput
    }
}
