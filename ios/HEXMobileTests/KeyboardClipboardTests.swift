import UIKit
import XCTest
@testable import HEX

/// The strip's Paste chip: one read per paste, and where to stop iOS's Allow Paste.
@MainActor
final class KeyboardClipboardTests: XCTestCase {
    private var suiteName = ""
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "hex.tests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    func testTheChipReadsTextOrALinkOnce() {
        let pasteboard = UIPasteboard.withUniqueName()
        defer { UIPasteboard.remove(withName: pasteboard.name) }
        XCTAssertNil(KeyboardClipboardTracker.read(pasteboard))
        pasteboard.url = URL(string: "https://hex.example/a")
        XCTAssertEqual(KeyboardClipboardTracker.read(pasteboard), "https://hex.example/a")
        pasteboard.string = "See you at 7"
        XCTAssertEqual(KeyboardClipboardTracker.read(pasteboard), "See you at 7")
    }

    func testAPasteThatAskedPointsToTheSettingAFewTimes() {
        let tracker = KeyboardClipboardTracker(defaults: defaults)
        XCTAssertFalse(tracker.needsAllowPasteSetting)
        XCTAssertFalse(tracker.shouldPointToAllowPaste(readDuration: 0.01, refused: false), "Allow: iOS didn't ask")
        XCTAssertFalse(tracker.needsAllowPasteSetting)
        for _ in 0..<KeyboardClipboardTracker.allowPasteHintLimit {
            XCTAssertTrue(tracker.shouldPointToAllowPaste(readDuration: 1.2, refused: false))
            XCTAssertTrue(tracker.needsAllowPasteSetting)
        }
        XCTAssertFalse(
            KeyboardClipboardTracker(defaults: defaults).shouldPointToAllowPaste(readDuration: 1.2, refused: false),
            "after a few, Ask is left alone: someone may want it"
        )
        XCTAssertTrue(tracker.needsAllowPasteSetting, "Hex's app page still remembers iOS prompted")
        XCTAssertTrue(tracker.shouldPointToAllowPaste(readDuration: 0.01, refused: true), "refused: the chip did nothing, always say why")
    }

    func testSilentCrossAppPasteClearsAllowPasteSettingButOwnCopyDoesNot() {
        let pasteboard = UIPasteboard.withUniqueName()
        defer { UIPasteboard.remove(withName: pasteboard.name) }
        let tracker = KeyboardClipboardTracker(defaults: defaults)

        XCTAssertTrue(tracker.shouldPointToAllowPaste(readDuration: 1.1, refused: false))
        XCTAssertTrue(tracker.needsAllowPasteSetting)

        // Hex's own copy never prompts iOS, so pasting it fast doesn't prove Allow is on.
        pasteboard.string = "from hex"
        tracker.markOwnedCopy(pasteboard: pasteboard)
        tracker.recordSilentPaste(pasteboard: pasteboard)
        XCTAssertTrue(tracker.needsAllowPasteSetting)

        // A copy from another app pasted without prompting proves Paste from Other Apps is Allow.
        pasteboard.string = "from opencode"
        tracker.recordSilentPaste(pasteboard: pasteboard)
        XCTAssertFalse(tracker.needsAllowPasteSetting)

        // Dismissing hides the app banner until iOS prompts again.
        XCTAssertTrue(tracker.shouldPointToAllowPaste(readDuration: 0.01, refused: true))
        XCTAssertTrue(tracker.needsAllowPasteSetting)
        tracker.dismissAllowPasteSetting()
        XCTAssertFalse(tracker.needsAllowPasteSetting)
    }

    @MainActor
    func testAllowPasteStripNoticeOpensPasteSettings() {
        let model = KeyboardViewModel()
        var opened: [URL] = []
        model.onOpenURL = { opened.append($0) }
        let view = FullKeyboardView(model: model, target: FixtureTextTarget())

        XCTAssertNil(view.allowPasteNotice)
        view.showAllowPasteHint()
        XCTAssertEqual(view.allowPasteNotice, KeyboardClipboardTracker.allowPasteNotice)

        view.stripView.onOpenNotice?()
        XCTAssertNil(view.allowPasteNotice, "tapping Settings clears the strip notice")
        XCTAssertEqual(opened, [KeyboardLaunchRequest.pasteSettings.url])
    }

    func testCodeFromMessagesNeedsApplesKeyboard() {
        let pasteboard = UIPasteboard.withUniqueName()
        defer { UIPasteboard.remove(withName: pasteboard.name) }
        let tracker = KeyboardClipboardTracker(defaults: defaults)
        func offer(appleKeyboard: Bool) -> KeyboardClipboardOffer? {
            tracker.offer(
                textContentType: .oneTimeCode, keyboardType: .numberPad, autocorrectionType: .default, fieldIsEmpty: true,
                hasFullAccess: true, appleKeyboardRoute: { _ in appleKeyboard ? .next : nil }, pasteboard: pasteboard
            )
        }
        XCTAssertEqual(offer(appleKeyboard: true), .code(canPaste: false))
        XCTAssertNil(offer(appleKeyboard: false), "Hex and Emoji only: the next keyboard gets no code (TestFlight build 56)")
        pasteboard.string = "482913"
        XCTAssertEqual(offer(appleKeyboard: true), .code(canPaste: true))
        XCTAssertEqual(offer(appleKeyboard: false), .paste, "a copied code still pastes")
        XCTAssertEqual(
            tracker.offer(
                textContentType: nil, keyboardType: .default, autocorrectionType: .default, fieldIsEmpty: true,
                hasFullAccess: true, appleKeyboardRoute: { _ in XCTFail("asked outside a code field"); return .next }, pasteboard: pasteboard
            ),
            .paste
        )
    }

    /// Where Apple's keyboard is split (the Duo unfolded and upright) Code from Messages opens
    /// iOS's keyboard list instead of switching: the switch to the next keyboard can end the host
    /// app there (TestFlight build 121), and the list's pick doesn't.
    func testCodeFromMessagesOpensTheListWhereApplesKeyboardSplits() {
        let pasteboard = UIPasteboard.withUniqueName()
        defer { UIPasteboard.remove(withName: pasteboard.name) }
        let tracker = KeyboardClipboardTracker(defaults: defaults)
        func offer(canSwitch: Bool) -> KeyboardClipboardOffer? {
            tracker.offer(
                textContentType: .oneTimeCode, keyboardType: .numberPad, autocorrectionType: .default, fieldIsEmpty: true,
                hasFullAccess: true, canSwitchKeyboard: canSwitch, appleKeyboardRoute: { _ in .next }, pasteboard: pasteboard
            )
        }
        XCTAssertEqual(offer(canSwitch: true), .code(canPaste: false))
        XCTAssertEqual(offer(canSwitch: false), .code(canPaste: false, opensList: true))
        pasteboard.string = "482913"
        XCTAssertEqual(offer(canSwitch: true), .code(canPaste: true))
        XCTAssertEqual(offer(canSwitch: false), .code(canPaste: true, opensList: true), "a copied code still pastes")
        XCTAssertEqual(FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget()).currentPosture, .phone, "an iPhone knows at once")
    }

    /// TestFlight build 56: ChatGPT's and Capy's message boxes kept Paste and Code from Messages
    /// in place of suggestions. A `.oneTimeCode` box with letters and default autocorrection is
    /// a chat box; Paste shows only while the field is empty, as Apple's keyboard offers a copy.
    func testChipsGiveWayToSuggestionsInAChatBox() {
        let pasteboard = UIPasteboard.withUniqueName()
        defer { UIPasteboard.remove(withName: pasteboard.name) }
        let tracker = KeyboardClipboardTracker(defaults: defaults)
        func offer(
            _ keyboardType: UIKeyboardType, _ autocorrection: UITextAutocorrectionType, empty: Bool = true
        ) -> KeyboardClipboardOffer? {
            tracker.offer(
                textContentType: .oneTimeCode, keyboardType: keyboardType, autocorrectionType: autocorrection, fieldIsEmpty: empty,
                hasFullAccess: true, appleKeyboardRoute: { _ in .next }, pasteboard: pasteboard
            )
        }
        XCTAssertNil(offer(.default, .default), "a chat box with nothing copied: suggestions")
        XCTAssertEqual(offer(.default, .yes), .code(canPaste: false), "Safari's autocomplete=one-time-code")
        XCTAssertEqual(offer(.default, .no), .code(canPaste: false))
        XCTAssertEqual(offer(.asciiCapableNumberPad, .default), .code(canPaste: false))
        XCTAssertNil(offer(.numberPad, .default, empty: false), "typing a code by hand")
        pasteboard.string = "See you at 7"
        XCTAssertEqual(offer(.default, .default), .paste)
        XCTAssertNil(offer(.default, .default, empty: false), "after a word the strip suggests")
        XCTAssertEqual(offer(.numberPad, .default), .code(canPaste: true))
    }

    func testApplesKeyboardIsNotEmojiOrAnExtension() {
        typealias Mode = KeyboardInputModes.Mode
        let english = Mode(primaryLanguage: "en-US", className: "UIKeyboardInputMode")
        let hex = Mode(primaryLanguage: "en-US", className: "UIKeyboardExtensionInputMode")
        let emoji = Mode(primaryLanguage: "emoji", className: "UIKeyboardInputMode")
        XCTAssertTrue(KeyboardInputModes.hasAppleKeyboard([hex, emoji, english]))
        XCTAssertFalse(KeyboardInputModes.hasAppleKeyboard([hex, emoji]))
        XCTAssertFalse(KeyboardInputModes.hasAppleKeyboard([hex]))
        XCTAssertFalse(KeyboardInputModes.hasAppleKeyboard([hex, hex, emoji]), "another third-party keyboard gets no code either")
    }

    /// TestFlight build 126: in Safari's one-time-code field the switch landed on Emoji or another
    /// third-party keyboard about half the time; on a number pad it always reached Apple's. Where
    /// it could miss, the chip opens iOS's keyboard list instead.
    func testCodeFromMessagesOpensTheListWhereTheSwitchCouldMissApplesKeyboard() {
        typealias Mode = KeyboardInputModes.Mode
        let english = Mode(primaryLanguage: "en-US", className: "UIKeyboardInputMode")
        let french = Mode(primaryLanguage: "fr-FR", className: "UIKeyboardInputMode")
        let hex = Mode(primaryLanguage: "en-US", className: "UIKeyboardExtensionInputMode")
        let other = Mode(primaryLanguage: "en-US", className: "UIKeyboardExtensionInputMode")
        let emoji = Mode(primaryLanguage: "emoji", className: "UIKeyboardInputMode")
        for modes in [[english, hex], [hex, english], [english, french, hex]] {
            XCTAssertEqual(KeyboardInputModes.route(modes, numberPad: false), .next)
            XCTAssertEqual(KeyboardInputModes.route(modes, numberPad: true), .next)
        }
        for modes in [[english, emoji, hex], [hex, emoji, english], [hex, other, english], [english, hex, other]] {
            XCTAssertEqual(KeyboardInputModes.route(modes, numberPad: false), .list)
            XCTAssertEqual(KeyboardInputModes.route(modes, numberPad: true), .next)
        }
        XCTAssertNil(KeyboardInputModes.route([hex, emoji], numberPad: true))

        let pasteboard = UIPasteboard.withUniqueName()
        defer { UIPasteboard.remove(withName: pasteboard.name) }
        let tracker = KeyboardClipboardTracker(defaults: defaults)
        func offer(_ keyboardType: UIKeyboardType, fullAccess: Bool = true) -> KeyboardClipboardOffer? {
            tracker.offer(
                textContentType: .oneTimeCode, keyboardType: keyboardType, autocorrectionType: .yes, fieldIsEmpty: true,
                hasFullAccess: fullAccess, appleKeyboardRoute: { numberPad in
                    KeyboardInputModes.route([english, emoji, hex], numberPad: numberPad)
                }, pasteboard: pasteboard
            )
        }
        XCTAssertEqual(offer(.default), .code(canPaste: false, opensList: true), "Safari's field with Emoji on")
        XCTAssertEqual(offer(.numberPad), .code(canPaste: false))
        XCTAssertEqual(offer(.default, fullAccess: false), .code(canPaste: false, opensList: true), "needs no Full Access")
        pasteboard.string = "482913"
        XCTAssertEqual(offer(.default), .code(canPaste: true, opensList: true))
        XCTAssertEqual(offer(.numberPad, fullAccess: false), .code(canPaste: false), "no Paste without Full Access")
    }
}
