import CoreGraphics
import Translation
import UIKit
import XCTest
@testable import HEX

final class HexKeyGestureTests: XCTestCase {
    func testTapDictates() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: .zero)
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 3, y: 2)), .ignored, "a jitter is still a tap")
        XCTAssertEqual(gesture.touchUp(), .tap)
        XCTAssertEqual(gesture.holdElapsed(), .ignored, "a timer left over from the tap")
        XCTAssertEqual(gesture.state, .idle)
    }

    func testStillHoldFiresOnceAndReleaseFinishes() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: .zero)
        XCTAssertEqual(gesture.holdElapsed(), .hold)
        XCTAssertEqual(gesture.holdElapsed(), .ignored)
        XCTAssertEqual(gesture.touchUp(), .release, "push to talk: lifting finishes")
        XCTAssertEqual(gesture.touchUp(), .ignored)
    }

    func testMoveBeforeHoldOpensMenu() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: CGPoint(x: 20, y: 20))
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 20, y: 9)), .openMenu)
        XCTAssertEqual(gesture.holdElapsed(), .ignored, "the menu won, no dictation")
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 0, y: -40)), .dragMenu)
        XCTAssertEqual(gesture.touchUp(), .releaseMenu)
        XCTAssertEqual(gesture.state, .idle)
    }

    func testMoveAfterHoldDoesNothing() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: .zero)
        XCTAssertEqual(gesture.holdElapsed(), .hold)
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 0, y: -80)), .ignored, "no menu once dictating")
        XCTAssertEqual(gesture.touchUp(), .release)
    }

    func testCancelResets() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: .zero)
        XCTAssertEqual(gesture.cancel(), .abandon, "a press that was neither tap nor hold: drop its capture")
        XCTAssertEqual(gesture.state, .idle)
        XCTAssertEqual(gesture.holdElapsed(), .ignored)
        XCTAssertEqual(gesture.touchMoved(to: CGPoint(x: 50, y: 0)), .ignored)
    }

    func testCancelAfterHoldKeepsDictating() {
        var gesture = HexKeyGesture()
        gesture.touchDown(at: .zero)
        XCTAssertEqual(gesture.holdElapsed(), .hold)
        XCTAssertEqual(gesture.cancel(), .ignored, "the hold already dictates; Done finishes it")
        XCTAssertEqual(gesture.cancel(), .ignored)
    }

    /// In-keyboard dictation listens from touch-down: the key reports the press at once, and
    /// abandons it only when the touch is cancelled before it is a tap or a hold.
    @MainActor
    func testHexKeyReportsThePressAndAbandonsOnlyACancelledOne() {
        let key = HexKeyControl(frame: CGRect(x: 0, y: 0, width: 44, height: 44))
        var presses = 0, abandoned = 0, dictations = 0
        key.onPress = { _ in presses += 1 }
        key.onPressAbandoned = { abandoned += 1 }
        key.onHold = { dictations += 1 }

        _ = key.beginTracking(UITouch(), with: nil)
        XCTAssertEqual(presses, 1, "touch-down, before the lift")
        XCTAssertEqual(dictations, 0)
        key.endTracking(UITouch(), with: nil)
        XCTAssertEqual(dictations, 1, "a tap keeps the capture")
        XCTAssertEqual(abandoned, 0)

        _ = key.beginTracking(UITouch(), with: nil)
        key.cancelTracking(with: nil)
        XCTAssertEqual(presses, 2)
        XCTAssertEqual(abandoned, 1, "the system took the touch: drop it")
        XCTAssertEqual(dictations, 1)
    }
}

/// Translate what I say where you dictate: the Translate to <language> switch and its targets
/// in the full keyboard's Hex key menu and the dictation bar, and what happens when Apple's
/// translation can't run.
@MainActor
final class TranslationPasteSwitchTests: XCTestCase {
    private var suiteName = ""
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "hex.tests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    func testTheChoiceNamesBothLanguagesAndShowsOnlyWhileTranslating() {
        let settings = DictationTranslationSettings(defaults: defaults)
        XCTAssertNil(settings.pasteChoice(source: "es-MX"), "off: no switch")

        settings.isEnabled = true
        settings.targetLanguage = "en"
        let choice = settings.pasteChoice(source: "es-MX")
        XCTAssertEqual(choice?.translatedTitle, "Paste in English")
        XCTAssertEqual(choice?.spokenTitle, "Paste in Spanish")
        XCTAssertNil(settings.pasteChoice(source: "en-GB"), "dictating in the target: nothing to choose")
        XCTAssertEqual(settings.pasteChoice(source: nil)?.spokenTitle, "Paste as spoken")
        settings.targetLanguage = "nb"
        XCTAssertEqual(settings.pasteChoice(source: "de-DE")?.translatedTitle, "Paste in Norwegian")
    }

    /// Build 56's "Paste in Hindi" kept as spoken in its own key while Settings' switch stayed on,
    /// so translation stopped with nothing saying so. That reads as off everywhere now, and the
    /// next switch clears it.
    func testBuild56sAsSpokenReadsAsOffUntilTheSwitch() async {
        defaults.set(true, forKey: "experiments.translateDictation")
        defaults.set(false, forKey: "experiments.translateDictation.pastesTranslation")
        let settings = DictationTranslationSettings(defaults: defaults)
        settings.targetLanguage = "en"
        XCTAssertFalse(settings.isEnabled, "the keyboard's switch and Settings agree: off")
        let stub = StubTranslator()
        let outcome = await DictationTranslator.translate(
            "Nos vemos mañana.",
            source: Locale.Language(identifier: "es-ES"),
            settings: settings,
            engine: stub.engine
        )
        XCTAssertEqual(outcome, .skipped)
        XCTAssertTrue(stub.inputs.isEmpty)

        settings.isEnabled = true
        XCTAssertTrue(settings.isEnabled)
        XCTAssertNil(defaults.object(forKey: "experiments.translateDictation.pastesTranslation"))
    }

    func testTheKeyboardShowsWhereDictationsGo() {
        let model = KeyboardViewModel()
        model.dictationLanguage = "es-ES"
        model.languageChoices = ["es-ES", "en-US"]
        XCTAssertNil(model.pasteChoice)
        XCTAssertEqual(model.languageSummary, "Español")

        model.translationTarget = "en"
        XCTAssertEqual(model.pasteChoice?.translatedTitle, "Paste in English")
        XCTAssertEqual(model.languageSummary, "Español → English")
        model.hasFullAccess = false
        XCTAssertNil(model.pasteChoice, "no Full Access, no translation")
    }

    func testTheHexKeyMenuSwitchesTranslationAndPicksItsTarget() {
        let menu = HexStyleMenuView(frame: CGRect(x: 0, y: 0, width: 400, height: 300))
        menu.languageChoices = ["es-ES", "en-US"]
        menu.dictationLanguage = "es-ES"
        XCTAssertFalse(labels(in: menu).contains { $0.hasPrefix("Translat") }, "no rows without a target (before iOS 26)")

        // Off: the switch names the target, and the second row opens the targets.
        menu.translationTarget = "en"
        menu.translationTargets = ["en", "ja", "es"]
        var toggled: Bool?
        var target: String?
        menu.onTranslationToggle = { toggled = $0 }
        menu.onTranslationTarget = { target = $0 }
        let toggle = controls(in: menu).first { $0.accessibilityLabel == "Translate to English" }
        XCTAssertEqual(toggle?.accessibilityValue, "Off")
        XCTAssertEqual(toggle?.accessibilityTraits.contains(.selected), false)
        toggle?.sendActions(for: .touchUpInside)
        XCTAssertEqual(toggled, true, "a tap turns it on")
        XCTAssertTrue(labels(in: menu).contains("Translation language"))

        menu.isTranslating = true
        let on = controls(in: menu).first { $0.accessibilityLabel == "Translate to English" }
        XCTAssertEqual(on?.accessibilityValue, "On")
        on?.sendActions(for: .touchUpInside)
        XCTAssertEqual(toggled, false, "and off again")

        menu.show(page: .translation)
        XCTAssertEqual(labels(in: menu), ["English", "Japanese", "Spanish"])
        controls(in: menu)[1].sendActions(for: .touchUpInside)
        XCTAssertEqual(target, "ja")

        // One target: nothing to pick, so only the switch.
        menu.show(page: .styles)
        menu.translationTargets = ["en"]
        XCTAssertFalse(labels(in: menu).contains("Translation language"))
    }

    /// The menu's last row is Open Hex, set apart by a line below the translation rows, with no
    /// checkmark or chevron; only a tap on it reports, and without a handler it isn't there.
    func testTheHexKeyMenuEndsWithOpenHex() {
        let menu = HexStyleMenuView(frame: CGRect(x: 0, y: 0, width: 400, height: 300))
        menu.languageChoices = ["es-ES", "en-US"]
        menu.translationTarget = "en"
        menu.translationTargets = ["en", "ja"]
        XCTAssertFalse(labels(in: menu).contains("Open Hex"), "no handler, no row")

        var opened = 0
        menu.onOpenHex = { opened += 1 }
        XCTAssertEqual(labels(in: menu).last, "Open Hex", "below Translation language")
        XCTAssertEqual(opened, 0, "showing it opens nothing")
        let row = controls(in: menu).last
        XCTAssertEqual(row?.accessibilityTraits, .button)
        row?.sendActions(for: .touchUpInside)
        XCTAssertEqual(opened, 1)

        menu.show(page: .languages)
        XCTAssertFalse(labels(in: menu).contains("Open Hex"), "only on the first page")
        menu.show(page: .styles)
        menu.translationTarget = nil
        XCTAssertEqual(labels(in: menu).suffix(2), ["Language, Automatic", "Open Hex"][...], "after the language row without translation")
    }

    func testTheSwitchIsSettingsSwitch() {
        let settings = DictationTranslationSettings(defaults: defaults)
        XCTAssertFalse(settings.isEnabled, "off by default")
        settings.isEnabled = true
        XCTAssertTrue(defaults.bool(forKey: "experiments.translateDictation"), "the same App Group key as Settings' switch")
        settings.isEnabled = false
        XCTAssertFalse(DictationTranslationSettings(defaults: defaults).isEnabled)
    }

    func testTheMenuOffersTargetsAppleTranslatesInto() {
        let choices = DictationTranslationSettings.targetChoices(
            current: "ja",
            dictation: ["zh-CN", "en-US", "nb-NO"],
            preferred: ["zh-Hans-CN", "de-DE", "fr-FR", "it-IT", "ko-KR", "pt-BR"],
            supported: ["en", "ja", "zh", "de", "fr", "it", "ko", "pt"]
        )
        XCTAssertEqual(choices, ["ja", "en", "zh", "de", "fr", "it"], "one per language, no Norwegian, six at most")
        XCTAssertEqual(
            DictationTranslationSettings.targetChoices(current: "nb", dictation: [], preferred: [], supported: ["en", "ja"]),
            ["nb", "en", "ja"],
            "the current target stays even when Apple can't translate into it"
        )
        XCTAssertEqual(
            DictationTranslationSettings.targetChoices(current: "en", dictation: ["en-US"], preferred: ["en-US"], supported: nil),
            ["en", "es", "zh", "fr", "de", "ja"],
            "an English-only iPhone still gets languages to pick, before Apple's list arrives too"
        )
    }

    /// Settings checks and downloads the pair the keyboard translates, from the language the
    /// keyboard speaks. Hex set to English on a Chinese iPhone writes its own Automatic pick
    /// (English), which the keyboard, following the iPhone, doesn't use.
    func testSettingsChecksTheLanguageTheKeyboardSpeaks() {
        let languages = LanguageSettings(defaults: defaults)
        let settings = DictationTranslationSettings(defaults: defaults)
        let chineseIPhone = ["zh-Hans-CN", "en-US"]

        XCTAssertNil(
            settings.keyboardSpokenLanguage(languages: languages, iPhoneLanguages: chineseIPhone),
            "before Hex has listed Apple's languages or the keyboard has dictated: unknown, never a guess"
        )

        languages.supportedDictationLanguages = ["en-US", "ja-JP", "zh-TW", "zh-CN"]
        languages.automaticDictationLanguage = "en-US"
        XCTAssertEqual(
            settings.keyboardSpokenLanguage(languages: languages, iPhoneLanguages: chineseIPhone),
            "zh-CN",
            "before the keyboard's first dictation: the iPhone's first language Hex dictates, as the keyboard's Automatic picks"
        )
        XCTAssertEqual(
            settings.keyboardSpokenLanguage(languages: languages, iPhoneLanguages: ["ga-IE"]),
            "en-US",
            "an iPhone in a language Hex doesn't dictate: US English, the keyboard's last resort"
        )

        settings.noteKeyboardDictation(in: "zh-TW", languages: languages, at: Date(timeIntervalSince1970: 100))
        XCTAssertEqual(settings.keyboardSpokenLanguage(languages: languages, iPhoneLanguages: chineseIPhone), "zh-TW")
        XCTAssertEqual(
            Set(defaults.dictionary(forKey: "experiments.translateDictation.keyboardAutomaticLanguage")?.keys.map { $0 } ?? []),
            ["language", "at"],
            "only the language and when: nothing that says what was said"
        )

        languages.dictationLanguage = "ja-JP"
        settings.noteKeyboardDictation(in: "ja-JP", languages: languages)
        XCTAssertEqual(settings.keyboardSpokenLanguage(languages: languages, iPhoneLanguages: chineseIPhone), "ja-JP")
        XCTAssertEqual(settings.keyboardAutomaticLanguage?.language, "zh-TW", "kept only while the keyboard is on Automatic")
    }

    /// Apple's download sheet comes only after the person turned translation on or picked a
    /// target, and only for a pair whose packs aren't on the iPhone; opening Settings never shows it.
    func testOnlyThePersonsChoiceAsksForTheDownload() {
        XCTAssertTrue(DictationTranslationSettings.asksToDownload(.supported, afterPersonActed: true))
        XCTAssertFalse(DictationTranslationSettings.asksToDownload(.supported, afterPersonActed: false), "the page appearing")
        XCTAssertFalse(DictationTranslationSettings.asksToDownload(.installed, afterPersonActed: true))
        XCTAssertFalse(DictationTranslationSettings.asksToDownload(.unsupported, afterPersonActed: true))
    }

    func testTheHexKeyWearsTheTargetWhileTranslating() {
        let model = KeyboardViewModel()
        model.translationLanguage = "en"
        XCTAssertTrue(model.showsTranslationSwitch)
        XCTAssertNil(model.translatingTo, "off: no badge")
        model.translationTarget = "en"
        XCTAssertEqual(model.translatingTo, "en")
        XCTAssertEqual(TranslationPasteChoice.badge(target: "en"), "EN")
        XCTAssertEqual(TranslationPasteChoice.badge(target: "zh-TW"), "ZH")
        model.hasFullAccess = false
        XCTAssertNil(model.translatingTo)
        XCTAssertFalse(model.showsTranslationSwitch)

        let key = HexKeyControl()
        key.isWarm = true
        key.translatingTo = "en"
        XCTAssertEqual(key.accessibilityLabel, "Dictate with Hex, Translating to English")
    }

    /// No language pack, or a pair Apple can't translate: the transcript goes in as spoken, and
    /// the failure says why and how to fix it, Chinese to English and English to Chinese alike.
    /// It's kept for Settings until a dictation translates.
    func testAMissingPackPastesAsSpokenAndSaysHowToFixIt() async {
        let settings = DictationTranslationSettings(defaults: defaults)
        settings.isEnabled = true
        settings.targetLanguage = "en"
        let stub = StubTranslator()
        stub.status = .supported
        let chinese = Locale.Language(identifier: "zh-CN")
        let outcome = await DictationTranslator.translate("我们明天见。", source: chinese, settings: settings, engine: stub.engine)
        XCTAssertNil(outcome.text)
        XCTAssertEqual(outcome.failure?.reason, .notDownloaded)
        XCTAssertEqual(outcome.failure?.fix, "Download Chinese in Settings")
        XCTAssertEqual(TranslationFailure.note, "Couldn't translate, pasted as spoken")
        XCTAssertTrue(stub.inputs.isEmpty, "no session for a pack that isn't there")
        XCTAssertEqual(settings.lastFailure, outcome.failure)

        settings.targetLanguage = "zh-Hans"
        let english = await DictationTranslator.translate(
            "See you tomorrow.", source: Locale.Language(identifier: "en-US"), settings: settings, engine: stub.engine
        )
        XCTAssertEqual(english.failure?.fix, "Download Chinese in Settings", "the language that isn't English")

        stub.status = .unsupported
        let unsupported = await DictationTranslator.translate("See you.", source: Locale.Language(identifier: "en-US"), settings: settings, engine: stub.engine)
        XCTAssertEqual(unsupported.failure?.fix, "Pick another language in Settings")

        stub.status = .installed
        stub.replies = [.success("我们明天见。")]
        let translated = await DictationTranslator.translate(
            "See you tomorrow.", source: Locale.Language(identifier: "en-US"), settings: settings, engine: stub.engine
        )
        XCTAssertEqual(translated, .translated("我们明天见。"))
        XCTAssertNil(settings.lastFailure, "a translation clears it")
        let same = await DictationTranslator.translate(
            "See you.", source: Locale.Language(identifier: "zh-TW"), settings: settings, engine: stub.engine
        )
        XCTAssertEqual(same, .skipped, "already in the target's language")
    }

    /// A failed translation is tried once more in a new session (`TranslationEngine.apple` makes
    /// one per call), so a session whose service went away (the keyboard or Hex suspended) never
    /// sticks; a missing pack isn't retried, since another session can't download it. Over many
    /// dictations, one failure never stops the ones after it.
    func testAFailedTranslationIsRetriedInANewSessionAndNeverSticks() async {
        let settings = DictationTranslationSettings(defaults: defaults)
        settings.isEnabled = true
        settings.targetLanguage = "en"
        let chinese = Locale.Language(identifier: "zh-CN")
        let stub = StubTranslator()
        stub.replies = [.failure(TranslationError.internalError), .success("See you tomorrow.")]
        var outcome = await DictationTranslator.translate("我们明天见。", source: chinese, settings: settings, engine: stub.engine)
        XCTAssertEqual(outcome, .translated("See you tomorrow."))
        XCTAssertEqual(stub.inputs.count, 2)

        stub.inputs = []
        stub.replies = [.failure(TranslationError.internalError), .failure(TranslationError.internalError)]
        outcome = await DictationTranslator.translate("我们明天见。", source: chinese, settings: settings, engine: stub.engine)
        XCTAssertEqual(outcome.failure?.reason, .failed)
        XCTAssertEqual(outcome.failure?.fix, "Download Chinese again in Settings")
        XCTAssertNotNil(outcome.failure?.detail, "Apple's error code, for diagnosis")
        XCTAssertEqual(stub.inputs.count, 2)

        if #available(iOS 26.0, *) {
            stub.inputs = []
            stub.replies = [.failure(TranslationError.notInstalled)]
            outcome = await DictationTranslator.translate("我们明天见。", source: chinese, settings: settings, engine: stub.engine)
            XCTAssertEqual(outcome.failure?.reason, .notDownloaded)
            XCTAssertEqual(stub.inputs.count, 1, "not retried")
        }

        stub.inputs = []
        var translated = 0
        for round in 0..<30 {
            stub.replies = round % 7 == 3 ? [.failure(TranslationError.internalError), .failure(TranslationError.internalError)] : []
            let result = await DictationTranslator.translate("第\(round)句。", source: chinese, settings: settings, engine: stub.engine)
            if result.text != nil { translated += 1 }
            XCTAssertEqual(result.failure == nil, round % 7 != 3, "round \(round)")
        }
        XCTAssertEqual(translated, 26)
        XCTAssertTrue(settings.isEnabled, "nothing turned it off")
        XCTAssertNil(settings.lastFailure)
    }

    /// The pipeline every path runs: a translation is formatted in the target language, and a
    /// failure inserts the spoken text formatted as spoken, carrying why for the keyboard.
    func testThePipelineTranslatesBothWaysAndPastesAsSpokenOnFailure() async {
        let settings = DictationTranslationSettings()
        let wasEnabled = settings.isEnabled, wasTarget = settings.targetLanguage, wasFailure = settings.lastFailure
        defer {
            settings.isEnabled = wasEnabled
            settings.targetLanguage = wasTarget
            settings.lastFailure = wasFailure
        }
        settings.isEnabled = true
        settings.targetLanguage = "en"
        let stub = StubTranslator()
        stub.replies = [.success("we'll meet at three tomorrow afternoon")]
        var pipeline = DictationPipeline(style: .polished, language: "zh-CN")
        pipeline.translationEngine = stub.engine
        var stages = await pipeline.run("我们明天下午三点见面")
        XCTAssertEqual(stub.inputs, ["我们明天下午三点见面"])
        XCTAssertEqual(stages.translated, "we'll meet at three tomorrow afternoon")
        XCTAssertEqual(stages.formatted, "We'll meet at three tomorrow afternoon.")
        XCTAssertNil(stages.translationFailure)

        settings.targetLanguage = "zh-Hans"
        stub.replies = [.success("明天见")]
        pipeline = DictationPipeline(style: .polished, language: "en-US")
        pipeline.translationEngine = stub.engine
        stages = await pipeline.run("see you tomorrow")
        XCTAssertEqual(stages.translated, "明天见")

        stub.status = .supported
        stages = await pipeline.run("see you tomorrow")
        XCTAssertNil(stages.translated)
        XCTAssertEqual(stages.formatted, "See you tomorrow.", "as spoken")
        XCTAssertEqual(stages.translationFailure?.reason, .notDownloaded)
        XCTAssertEqual(stages.translationFailure?.target, "zh")
    }

    /// The full keyboard's strip says a dictation went in as spoken, with the fix and Open Hex,
    /// in place of Undo dictation, for as long as the note shows.
    func testTheStripShowsTheTranslationNote() {
        let strip = KeyboardStripView(frame: CGRect(x: 0, y: 0, width: 390, height: 44))
        var opened = false
        strip.onOpenNotice = { opened = true }
        let note = TranslationFailure(.notDownloaded, source: Locale.Language(identifier: "zh-CN"), target: Locale.Language(identifier: "en"))
        strip.update(
            suggestions: [],
            showsUndo: true,
            statusText: nil,
            notice: KeyboardStripNotice(title: TranslationFailure.note, detail: note.fix, button: "Open Hex")
        )
        strip.layoutIfNeeded()
        let shown = visibleLabels(in: strip)
        XCTAssertTrue(shown.contains("Couldn't translate, pasted as spoken"))
        XCTAssertTrue(shown.contains("Download Chinese in Settings"))
        XCTAssertFalse(visibleControls(in: strip).contains { $0.configuration?.title == "Undo dictation" }, "the note takes Undo's place")
        visibleControls(in: strip).first { $0.configuration?.title == "Open Hex" }?.sendActions(for: .touchUpInside)
        XCTAssertTrue(opened)

        strip.update(suggestions: [], showsUndo: true, statusText: nil, notice: nil)
        XCTAssertTrue(visibleControls(in: strip).contains { $0.configuration?.title == "Undo dictation" })
        XCTAssertFalse(visibleLabels(in: strip).contains("Couldn't translate, pasted as spoken"))
    }

    /// Paste alone takes a tap anywhere on the bar, top to bottom and up to the Hex key's cell,
    /// which keeps its own; beside Code from Messages, Code alone, and the words, every chip and
    /// word keeps its own touch area.
    @MainActor
    func testPasteTakesTheWholeBar() throws {
        let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget(text: ""))
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        let strip = view.stripView
        let hex = view.hexKey.frame
        XCTAssertTrue(view.hexKey.inStrip)
        func show(_ clipboard: KeyboardClipboardOffer?, _ words: [KeyboardSuggestion] = []) {
            strip.update(suggestions: words, showsUndo: false, statusText: nil, clipboard: clipboard)
            strip.layoutIfNeeded()
        }
        func chip(_ title: String) throws -> UIButton {
            try XCTUnwrap(visibleControls(in: strip).first { $0.configuration?.title == title }, title)
        }
        // Every point of the bar the keys' own reach above row 1 doesn't take.
        let barPoints = stride(from: 0.5, to: hex.minX, by: 6).flatMap { x in
            stride(from: 0.5, to: strip.frame.maxY, by: 4).map { CGPoint(x: x, y: $0) }
        }.filter { !view.plane.point(inside: view.convert($0, to: view.plane), with: nil) }
        XCTAssertGreaterThan(barPoints.count, 200)

        show(.paste)
        let paste = try chip("Paste")
        for point in barPoints { XCTAssertTrue(view.hitTest(point, with: nil) === paste, "\(point)") }
        XCTAssertTrue(view.hitTest(CGPoint(x: hex.minX - 0.5, y: strip.frame.midY), with: nil) === paste)
        XCTAssertTrue(view.hitTest(CGPoint(x: hex.minX + 0.5, y: strip.frame.midY), with: nil) === view.hexKey)
        XCTAssertTrue(view.hitTest(CGPoint(x: hex.midX, y: strip.frame.midY), with: nil) === view.hexKey)
        XCTAssertEqual(visibleControls(in: strip).map(\.configuration?.title), ["Paste"], "one Paste for VoiceOver, at the chip")

        show(.dictation)
        let dictation = try chip("Paste dictation")
        for point in barPoints { XCTAssertTrue(view.hitTest(point, with: nil) === dictation, "\(point)") }

        show(.code(canPaste: true))
        let pasteCode = try chip("Paste"), code = try chip("Code from Messages")
        for point in barPoints {
            let hit = view.hitTest(point, with: nil)
            let inPaste = pasteCode.point(inside: view.convert(point, to: pasteCode), with: nil)
            let inCode = code.bounds.contains(view.convert(point, to: code))
            XCTAssertTrue(inPaste ? hit === pasteCode : inCode ? hit === code : !(hit is UIButton), "two chips keep their own: \(point)")
        }

        show(.code(canPaste: false))
        let codeAlone = try chip("Code from Messages")
        XCTAssertFalse(view.hitTest(CGPoint(x: 2, y: strip.frame.midY), with: nil) === codeAlone, "Code alone keeps its chip's own area")

        show(.paste, ["one", "two", "three"].map { KeyboardSuggestion(displayText: $0, replacement: $0, kind: .secondary) })
        for point in barPoints { XCTAssertFalse(view.hitTest(point, with: nil) is UIButton, "words, not Paste: \(point)") }
    }

    /// Undo dictation still takes the dictation back after the cursor moves within it, before it
    /// or after it, and the cursor stays in the text you moved it to. Where it can't tell the
    /// dictation from other text it goes, and deletes nothing.
    @MainActor
    func testUndoDictationAfterTheCursorMoves() throws {
        func keyboard(_ field: FixtureTextTarget) -> FullKeyboardView {
            let view = FullKeyboardView(model: KeyboardViewModel(), target: field)
            view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
            view.layoutIfNeeded()
            return view
        }
        func undo(_ view: FullKeyboardView) -> UIButton? {
            visibleControls(in: view).first { $0.configuration?.title == "Undo dictation" }
        }
        func dictate(_ text: String, into field: FixtureTextTarget, at cursor: Int, moveBy offset: Int) -> FullKeyboardView {
            let view = keyboard(field)
            field.adjustTextPosition(byCharacterOffset: cursor - field.cursorIndex)
            let before = field.documentContextBeforeInput ?? "", after = field.documentContextAfterInput ?? ""
            field.insertText(text)
            view.didInsertDictation(text, before: before, after: after)
            field.adjustTextPosition(byCharacterOffset: offset)
            view.textDidChange()
            return view
        }

        for (offset, text, cursor) in [(-6, "Hi. Bye.", 3), (-16, "Hi. Bye.", 1), (5, "Hi. Bye.", 8)] {
            let field = FixtureTextTarget(text: "Hi. Bye.")
            let view = dictate(" Meet at noon.", into: field, at: 3, moveBy: offset)
            try XCTUnwrap(undo(view), "moved by \(offset)").sendActions(for: .touchUpInside)
            XCTAssertEqual(field.text, text, "moved by \(offset)")
            XCTAssertEqual(field.cursorIndex, cursor, "moved by \(offset)")
            XCTAssertNil(undo(view))
        }

        // The same words twice: no telling which went in last.
        let twice = FixtureTextTarget(text: "Yes. Yes.")
        var view = dictate(" Yes.", into: twice, at: 9, moveBy: -6)
        XCTAssertNil(undo(view), "gone as the cursor moved")
        XCTAssertEqual(twice.text, "Yes. Yes. Yes.")

        // Changed outside the keys: it can't be found whole any more.
        let edited = FixtureTextTarget(text: "Hi.")
        view = dictate(" Meet at noon.", into: edited, at: 3, moveBy: 0)
        XCTAssertNotNil(undo(view))
        edited.deleteBackward()
        edited.deleteBackward()
        view.textDidChange()
        XCTAssertNil(undo(view))
        XCTAssertEqual(edited.text, "Hi. Meet at noo")

        // Moves are UTF-16 units; a selection, or other text where it went in, finds nothing.
        let undoable = DictationUndo(text: " Ok 👍🏽", before: "Hi", after: "")
        XCTAssertEqual(undoable.plan(before: "H", after: "i Ok 👍🏽", selected: nil), DictationUndo.Plan(toEnd: 9, delete: 5, back: -1))
        XCTAssertEqual(undoable.plan(before: "Hi Ok 👍🏽 and", after: "", selected: nil), DictationUndo.Plan(toEnd: -4, delete: 5, back: 4))
        XCTAssertNil(undoable.plan(before: "H", after: "i Ok 👍🏽", selected: "i"))
        XCTAssertNil(undoable.plan(before: "Bye Ok", after: " 👍🏽", selected: nil))

        // A text view shows only the cursor's sentence, without the space before it (iOS 27.0
        // simulator): the space goes too. Not when the sentence is cut short or follows other text.
        let sentence = DictationUndo(text: " See you at ten.", before: "Hi Sam.", after: "")
        XCTAssertEqual(sentence.plan(before: "See you ", after: "at ten.", selected: nil), DictationUndo.Plan(toEnd: 7, delete: 16, back: 0))
        XCTAssertNil(sentence.plan(before: "you ", after: "at ten.", selected: nil))
        XCTAssertNil(sentence.plan(before: "Ok. See you ", after: "at ten.", selected: nil))
    }

    private func visibleControls(in view: UIView) -> [UIButton] {
        view.subviews.filter { !$0.isHidden && $0.alpha > 0 }.flatMap { ($0 as? UIButton).map { [$0] } ?? [] + visibleControls(in: $0) }
    }

    private func visibleLabels(in view: UIView) -> [String] {
        view.subviews.filter { !$0.isHidden && $0.alpha > 0 }.flatMap { ($0 as? UILabel)?.text.map { [$0] } ?? [] + visibleLabels(in: $0) }
    }

    private func controls(in view: UIView) -> [UIControl] {
        view.subviews.flatMap { ($0 as? UIControl).map { [$0] } ?? [] + controls(in: $0) }
    }

    private func labels(in view: UIView) -> [String] {
        controls(in: view).compactMap(\.accessibilityLabel)
    }
}

/// Apple's translator stood in for: `status` for every pair, and `replies` answered in turn (a
/// translation, or an error thrown), echoing the text once they run out.
@MainActor
final class StubTranslator {
    var status: LanguageAvailability.Status = .installed
    var replies: [Result<String, any Error>] = []
    /// What each translation was asked to translate.
    var inputs: [String] = []

    var engine: TranslationEngine {
        TranslationEngine(
            status: { [unowned self] _, _ in self.status },
            translate: { [unowned self] text, _, _ in
                self.inputs.append(text)
                return try (self.replies.isEmpty ? .success(text) : self.replies.removeFirst()).get()
            }
        )
    }
}
