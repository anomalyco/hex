import CoreGraphics
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
/// in the full keyboard's Hex key menu and the dictation bar, and the paste switch Settings keeps.
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

    func testTheLastChoiceIsRememberedAndAsSpokenSkipsTranslation() async {
        let settings = DictationTranslationSettings(defaults: defaults)
        settings.isEnabled = true
        settings.targetLanguage = "en"
        XCTAssertTrue(settings.pastesTranslation, "translated until you pick otherwise")
        settings.pastesTranslation = false
        XCTAssertFalse(DictationTranslationSettings(defaults: defaults).pastesTranslation)
        let translated = await DictationTranslator.translateIfEnabled(
            "Nos vemos mañana.",
            source: Locale.Language(identifier: "es-ES"),
            settings: settings
        )
        XCTAssertNil(translated, "as spoken pastes the transcript itself")
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
        model.pastesTranslation = false
        XCTAssertEqual(model.languageSummary, "Español", "as spoken")
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

    func testTheSwitchIsSettingsSwitchAndTurningItOnTranslates() {
        let settings = DictationTranslationSettings(defaults: defaults)
        XCTAssertFalse(settings.isTranslating)
        settings.pastesTranslation = false
        settings.isTranslating = true
        XCTAssertTrue(settings.isEnabled, "the same App Group key as Settings' switch")
        XCTAssertTrue(settings.pastesTranslation, "on means translated, whatever the paste switch said")
        settings.pastesTranslation = false
        XCTAssertFalse(settings.isTranslating, "Settings' Paste in English off: not translating")
        settings.isTranslating = true
        settings.isTranslating = false
        XCTAssertFalse(settings.isEnabled)
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
        model.pastesTranslation = false
        XCTAssertNil(model.translatingTo, "pasting as spoken: no badge")
        model.pastesTranslation = true
        model.hasFullAccess = false
        XCTAssertNil(model.translatingTo)
        XCTAssertFalse(model.showsTranslationSwitch)

        let key = HexKeyControl()
        key.isWarm = true
        key.translatingTo = "en"
        XCTAssertEqual(key.accessibilityLabel, "Dictate with Hex, Translating to English")
    }

    private func controls(in view: UIView) -> [UIControl] {
        view.subviews.flatMap { ($0 as? UIControl).map { [$0] } ?? [] + controls(in: $0) }
    }

    private func labels(in view: UIView) -> [String] {
        controls(in: view).compactMap(\.accessibilityLabel)
    }
}
