import UIKit
import XCTest
@testable import HEX

extension FullKeyboardEngineTests {
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
        swipe("rang", on: plane)
        tap(.delete, on: plane)
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

    /// Record typing logs: "two" swiped three times running for "yep", each deleted, while the
    /// strip offered "yep". A word taken back right after its swipe isn't the next swipe's word
    /// at that place: its next reading goes in, and the strip may still offer the word. Every
    /// word taken back there counts; a letter typed in between forgets them; a swipe that reads
    /// only as that word still types it; deleting it letter by letter takes it back too.
    func testASwipeRightAfterItsWordWasDeletedTypesTheNextReading() throws {
        func swipe(_ readings: [String], on plane: KeyboardKeyPlaneView) {
            let key = plane.engine.layout.key(.character("t"))!
            plane.engine.touchBegan(id: 93, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            plane.engine.beginSwipe(touchID: 93)
            plane.engine.endSwipe(typing: readings, points: 12, decodeTime: 0.001)
        }
        let readings = ["two", "yep", "tip"]
        let (field, plane) = try swipeKeyboard("so")
        swipe(readings, on: plane)
        XCTAssertEqual(field.text, "so two")
        tap(.delete, on: plane)
        swipe(readings, on: plane)
        XCTAssertEqual(field.text, "so yep", "the next reading")
        XCTAssertTrue(plane.engine.suggestions.contains { $0.replacement == "two" }, "still offered in the strip")
        tap(.delete, on: plane)
        swipe(readings, on: plane)
        XCTAssertEqual(field.text, "so tip", "neither word taken back here")
        tap(.delete, on: plane)
        tap(.character("a"), on: plane)
        tap(.delete, on: plane)
        swipe(readings, on: plane)
        XCTAssertEqual(field.text, "so two", "a letter typed in between forgets them")
        tap(.delete, on: plane)
        swipe(["two"], on: plane)
        XCTAssertEqual(field.text, "so two", "the only reading still goes in")

        // On an iPhone (TestFlight), at the start of an empty field the re-swipe still typed "two". A
        // real iPhone reports nothing before the cursor there (nil), where these fields report "".
        @MainActor final class ReportsNothingWhenEmpty: FixtureTextTarget {
            override var documentContextBeforeInput: String? { super.documentContextBeforeInput.flatMap { $0.isEmpty ? nil : $0 } }
        }
        let (empty, blank) = try swipeKeyboard(target: ReportsNothingWhenEmpty())
        swipe(readings, on: blank)
        tap(.delete, on: blank)
        XCTAssertNil(empty.documentContextBeforeInput)
        swipe(readings, on: blank)
        XCTAssertEqual(empty.text, "yep", "at the start of an empty field too")

        let (letters, keys) = try swipeKeyboard("so")
        keys.engine.deletesSwipedWordsLetterByLetter = true
        swipe(readings, on: keys)
        for _ in 0..<4 { tap(.delete, on: keys) }
        XCTAssertEqual(letters.text, "so")
        swipe(readings, on: keys)
        XCTAssertEqual(letters.text, "so yep", "every character the swipe typed taken out")
        for _ in 0..<3 { tap(.delete, on: keys) }
        XCTAssertEqual(letters.text, "so ")
        swipe(readings, on: keys)
        XCTAssertEqual(letters.text, "so tip", "or just its letters")
    }

    /// The same through the decoder: the same path swiped again after its word was deleted
    /// types the reading the strip offered beside it.
    func testTheSamePathSwipedAgainAfterADeleteTypesTheStripsReading() throws {
        let (field, plane) = try swipeKeyboard()
        swipe("hello", on: plane)
        XCTAssertEqual(field.text, "hello")
        let next = try XCTUnwrap(plane.engine.swipedWord?.alternatives.first)
        tap(.delete, on: plane)
        swipe("hello", on: plane)
        XCTAssertEqual(field.text, next)
    }

    /// Settings › Experiments › Delete swiped words letter by letter (off by default): right
    /// after a swipe, delete acts as anywhere else and takes one character, instead of the
    /// whole word and the space the swipe added before it (#2321).
    func testDeleteSwipedWordsLetterByLetterTakesOneCharacterAtATime() throws {
        let (field, plane) = try swipeKeyboard()
        plane.engine.deletesSwipedWordsLetterByLetter = true
        swipe("hello", on: plane)
        swipe("world", on: plane)
        tap(.delete, on: plane)
        XCTAssertEqual(field.text, "hello worl", "one letter, not the word and the space before it")
        XCTAssertNil(plane.engine.swipedWord, "the swipe's record goes with the deleted letter")
        tap(.delete, on: plane)
        XCTAssertEqual(field.text, "hello wor", "and one at a time after it")
    }

    /// The experiment's switch reaches the keyboard from the App Group, and the test puts it
    /// back so no other test sees it on.
    func testTheDeleteSwipedWordsLetterByLetterSettingReachesTheKeyboard() {
        let settings = FullKeyboardSettings()
        let wasOn = settings.deletesSwipedWordsLetterByLetter
        settings.deletesSwipedWordsLetterByLetter = true
        defer { settings.deletesSwipedWordsLetterByLetter = wasOn }
        let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget())
        XCTAssertTrue(view.engine.deletesSwipedWordsLetterByLetter)
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

    /// Settings › Experiments › Swipe typing in more languages (off by default): on, a swipe
    /// works in every language with a typing model, not only English and Spanish; off, French
    /// keeps a drag as a slide to another key, as before.
    func testSwipeTypingInMoreLanguagesFollowsTheLanguageSwitch() {
        let settings = FullKeyboardSettings()
        let wasMore = settings.swipeMoreLanguages
        defer { settings.swipeMoreLanguages = wasMore }
        let languages = LanguageSettings(defaults: defaults)
        languages.keyboardLanguages = [.english, .french, .polish]

        settings.swipeMoreLanguages = false
        let off = FullKeyboardView(model: KeyboardViewModel(), target: target)
        off.swipeOverride = true
        off.languageOverride = .french
        XCTAssertNil(off.plane.swipe, "off, French keeps a drag as a slide to another key")

        settings.swipeMoreLanguages = true
        let on = FullKeyboardView(model: KeyboardViewModel(), target: target)
        on.languageSettings = languages
        on.swipeOverride = true
        XCTAssertEqual(on.engine.language, .english)
        XCTAssertEqual(on.plane.swipe?.model.language, "en")
        on.switchLanguage(by: 1)
        XCTAssertEqual(on.engine.language, .french)
        XCTAssertEqual(on.plane.swipe?.model.language, "fr")
        on.switchLanguage(by: 1)
        XCTAssertEqual(on.engine.language, .polish)
        XCTAssertEqual(on.plane.swipe?.model.language, "pl")
    }

    /// German's swipe still waits on its own model: Swipe typing in more languages adds it only
    /// with Spatial autocorrect and German spatial autocorrect on, as Spatial autocorrect does.
    func testSwipeTypingInMoreLanguagesKeepsGermansModelGate() {
        let settings = FullKeyboardSettings()
        let wasMore = settings.swipeMoreLanguages
        let wasSpatial = settings.spatialAutocorrect
        let wasGerman = settings.germanSpatialAutocorrect
        defer {
            settings.swipeMoreLanguages = wasMore
            settings.spatialAutocorrect = wasSpatial
            settings.germanSpatialAutocorrect = wasGerman
        }
        settings.swipeMoreLanguages = true
        settings.spatialAutocorrect = true
        settings.germanSpatialAutocorrect = false
        let withoutGerman = FullKeyboardView(model: KeyboardViewModel(), target: target)
        withoutGerman.swipeOverride = true
        withoutGerman.languageOverride = .german
        XCTAssertNil(withoutGerman.plane.swipe, "the German experiment is off")

        settings.germanSpatialAutocorrect = true
        let german = FullKeyboardView(model: KeyboardViewModel(), target: target)
        german.swipeOverride = true
        german.languageOverride = .german
        XCTAssertEqual(german.plane.swipe?.model.language, "de")

        settings.spatialAutocorrect = false
        let withoutSpatial = FullKeyboardView(model: KeyboardViewModel(), target: target)
        withoutSpatial.swipeOverride = true
        withoutSpatial.languageOverride = .german
        XCTAssertNil(withoutSpatial.plane.swipe, "Spatial autocorrect is off")
    }

    /// The input mode's language (`UIInputViewController.primaryLanguage`), which Apple
    /// documents as superseding the Info.plist's `mul`: the keyboard reports the language
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

    /// The globe hands its own touches to UIKit's `handleInputModeList(from:with:)` (Apple's
    /// documented wiring). Where Apple's keyboard is split (the Duo unfolded), a tap's lift isn't
    /// passed on, so the list opens instead of UIKit's switch to the next keyboard, which can end
    /// the app being typed in there (TestFlight build 121).
    func testTheGlobePassesItsTouchesToUIKit() {
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target)
        view.hostController = RecordingInputController()
        for event: UIControl.Event in [.touchDown, .touchDragInside, .touchDragOutside, .touchUpInside, .touchUpOutside, .touchCancel] {
            XCTAssertEqual(view.globeButton.actions(forTarget: view, forControlEvent: event), ["globeTouch:with:"])
        }
        for phase: UITouch.Phase in [.began, .moved, .ended, .cancelled] {
            XCTAssertTrue(InputModeListTouch.passes(phase: phase, onKey: true, leftKey: false, appleKeyboardSplits: false))
        }
        XCTAssertFalse(InputModeListTouch.passes(phase: .ended, onKey: true, leftKey: false, appleKeyboardSplits: true), "a tap")
        XCTAssertTrue(InputModeListTouch.passes(phase: .began, onKey: true, leftKey: false, appleKeyboardSplits: true))
        XCTAssertTrue(InputModeListTouch.passes(phase: .moved, onKey: false, leftKey: true, appleKeyboardSplits: true))
        XCTAssertTrue(InputModeListTouch.passes(phase: .ended, onKey: false, leftKey: true, appleKeyboardSplits: true), "lifting over the list picks")
        XCTAssertTrue(InputModeListTouch.passes(phase: .ended, onKey: true, leftKey: true, appleKeyboardSplits: true))
        XCTAssertTrue(InputModeListTouch.passes(phase: .cancelled, onKey: true, leftKey: false, appleKeyboardSplits: true))
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
