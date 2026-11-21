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
