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

/// Translate what I say's switch where you dictate: "Paste in English" or as spoken, in the
/// full keyboard's Hex key menu and the dictation bar, remembered for the next dictations.
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

    func testTheHexKeyMenuOffersBothAndReportsThePick() {
        let menu = HexStyleMenuView(frame: CGRect(x: 0, y: 0, width: 400, height: 300))
        menu.languageChoices = ["es-ES", "en-US"]
        menu.dictationLanguage = "es-ES"
        XCTAssertFalse(labels(in: menu).contains { $0.hasPrefix("Paste") }, "no rows while translation is off")

        menu.pasteChoice = TranslationPasteChoice(target: "en", source: "es-ES")
        menu.pastesTranslation = false
        var picked: Bool?
        menu.onPasteTranslation = { picked = $0 }
        let rows = controls(in: menu).filter { $0.accessibilityLabel?.hasPrefix("Paste") == true }
        XCTAssertEqual(rows.map(\.accessibilityLabel), ["Paste in English", "Paste in Spanish"])
        XCTAssertEqual(rows.map { $0.accessibilityTraits.contains(.selected) }, [false, true])
        rows[0].sendActions(for: .touchUpInside)
        XCTAssertEqual(picked, true)
    }

    private func controls(in view: UIView) -> [UIControl] {
        view.subviews.flatMap { ($0 as? UIControl).map { [$0] } ?? [] + controls(in: $0) }
    }

    private func labels(in view: UIView) -> [String] {
        controls(in: view).compactMap(\.accessibilityLabel)
    }
}
