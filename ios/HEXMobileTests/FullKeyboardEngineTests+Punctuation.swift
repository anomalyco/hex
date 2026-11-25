import UIKit
import XCTest
@testable import HEX

extension FullKeyboardEngineTests {
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

    /// Apple's "AZERTY – French" apostrophe key (iOS 27.2 simulator, black box): right after a
    /// letter it shows ’ and types the apostrophe, curled as the field curls quotes; after
    /// anything else it shows and types ´. Held, it offers ' ’ ‘ ` after a letter and ´ ˋ ˆ ¨
    /// elsewhere, each typed as it is.
    func testFrenchApostropheKeyTypesAsApplesDoes() {
        smartField(language: .french)
        engine.autocorrect.isEnabled = false
        let apostrophe = KeyKind.character(FrenchTyping.key)
        let key = engine.layout.key(apostrophe)!
        XCTAssertEqual(engine.keycapLabel(for: key), "´", "nothing before it")
        tap(apostrophe)
        type("l")
        XCTAssertEqual(engine.keycapLabel(for: key), "’")
        XCTAssertEqual(engine.displayLabel(for: key), "’")
        tap(apostrophe)
        XCTAssertEqual(engine.keycapLabel(for: key), "´", "after an apostrophe")
        tap(apostrophe)
        type("ami e ")
        tap(apostrophe)
        type("e")
        XCTAssertEqual(target.text, "´l’´ami e ´e", "never a dead key")

        smartField(quotes: .no, language: .french)
        engine.autocorrect.isEnabled = false
        type("l")
        tap(apostrophe)
        type("homme")
        XCTAssertEqual(target.text, "l'homme")

        smartField(language: .french)
        engine.autocorrect.isEnabled = false
        var id = 700
        func hold(pick: String) {
            let key = engine.layout.key(apostrophe)!
            id += 1
            engine.touchBegan(id: id, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: id), pick)
            guard let menu = engine.activeAccentMenu, let index = menu.options.firstIndex(of: pick) else {
                return XCTFail("no \(pick) in \(engine.activeAccentMenu?.options ?? [])")
            }
            let cell = menu.cellFrames[index]
            engine.touchMoved(id: id, to: CGPoint(x: cell.midX, y: cell.midY))
            engine.touchEnded(id: id, at: CGPoint(x: cell.midX, y: cell.midY))
        }
        hold(pick: "¨")
        type("d")
        hold(pick: "'")
        type("a")
        hold(pick: "‘")
        XCTAssertEqual(target.text, "¨d'a‘", "a held choice goes in as it is")
        type(" ")
        hold(pick: "ˆ")
        XCTAssertEqual(target.text, "¨d'a‘ ˆ")

        // Canada's and Switzerland's own layouts end row 2 in the same key, with the same rules.
        for language in [KeyboardLanguage.frenchCanadian, .frenchSwiss] {
            smartField(language: language)
            engine.autocorrect.isEnabled = false
            let key = engine.layout.key(apostrophe)!
            XCTAssertEqual(key.row, 1, "\(language)")
            XCTAssertEqual(engine.keycapLabel(for: key), "´", "\(language)")
            tap(apostrophe)
            type("qu")
            XCTAssertEqual(engine.keycapLabel(for: key), "’", "\(language)")
            tap(apostrophe)
            type("il")
            XCTAssertEqual(target.text, "´qu’il", "\(language)")
            id += 1
            engine.touchBegan(id: id, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
            XCTAssertTrue(engine.openAccentMenuIfEligible(forTouch: id))
            XCTAssertEqual(Set(engine.activeAccentMenu?.options ?? []), ["'", "’", "‘", "`"], "\(language)")
            engine.touchCancelled(id: id)
        }
    }

    /// Each of Apple's keyboards types its language's quotes (black box, iOS 27.2): „Hallo“ ‚tag‘
    /// in German, where the apostrophe key types ‘ in a word too; « Bonjour » with no-break
    /// spaces inside and ‘salut’ in French; „tak” ‚nie’ in Polish; «да» in Russian; ”hej” ’ok’ in
    /// Swedish, whose keyboard types ” and ’ to open and to close (iOS 26.5 and 27.2).
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
        smartField(language: .swedish)
        keys("\"hej\" 'ok' det's")
        XCTAssertEqual(target.text, "\u{201D}hej\u{201D} \u{2019}ok\u{2019} det\u{2019}s")
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
}
