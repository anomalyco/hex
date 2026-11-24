import UIKit
import XCTest
@testable import HEX

/// Settings › Labs › ABC under the emoji key: the emoji panel's ABC takes the emoji
/// key's x and width, in the bar's row, so switching back to the letters needs no thumb move.
/// Off, and with no emoji key behind the panel, ABC stays at the bar's leading end.
@MainActor
final class EmojiPanelLettersKeyTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "EmojiPanelLettersKeyTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    /// Off by default, like every experiment.
    func testTheExperimentDefaultsOff() {
        let settings = FullKeyboardSettings(defaults: defaults)
        XCTAssertFalse(settings.emojiLettersUnderEmojiKey, "off by default")
        settings.emojiLettersUnderEmojiKey = true
        XCTAssertTrue(settings.emojiLettersUnderEmojiKey)
        settings.emojiLettersUnderEmojiKey = false
        XCTAssertFalse(settings.emojiLettersUnderEmojiKey)
    }

    /// On, ABC sits exactly under the emoji key that opened the panel (same x, width and
    /// centre), and a tap there goes to ABC, as does one left of it. Off, ABC keeps the bar's
    /// old leading-end place. The emoji key moves with the globe and the phone's width, so ABC
    /// follows both.
    func testLettersKeySitsUnderTheEmojiKey() throws {
        for width in [375, 402, 440] as [CGFloat] {
            for needsGlobe in [false, true] {
                for experiment in [true, false] {
                    let model = KeyboardViewModel()
                    model.needsGlobeKey = needsGlobe
                    let view = FullKeyboardView(model: model, target: FixtureTextTarget())
                    view.emojiOverride = true
                    view.emojiLettersUnderEmojiKeyOverride = experiment
                    view.makeEmojiUsage = { EmojiUsageStore(defaults: self.defaults) }
                    view.frame = CGRect(x: 0, y: 0, width: width, height: 306)
                    view.layoutIfNeeded()
                    view.debugOpenEmoji()
                    view.layoutIfNeeded()

                    let panel = try XCTUnwrap(view.emojiPanel)
                    panel.layoutIfNeeded()
                    let letters = try XCTUnwrap(panel.subviews.first { $0.accessibilityLabel == "Letters" })
                    let key = try XCTUnwrap(view.engine.layout.key(.emoji), "an emoji key on a \(width) pt keyboard")
                    let bar = letters.frame
                    let frame = letters.convert(letters.bounds, to: view)
                    let context = "\(width) pt, globe \(needsGlobe), experiment \(experiment)"

                    if experiment {
                        XCTAssertEqual(frame.midX, key.frame.midX, accuracy: 0.5, "ABC's centre is the emoji key's (\(context))")
                        XCTAssertEqual(frame.width, key.frame.width, accuracy: 0.5, "ABC is as wide as the emoji key (\(context))")
                    } else {
                        let keyWidth = min(44, KeyboardLayout.letterWidth(
                            innerWidth: width - 2 * panel.horizontalInset,
                            metrics: panel.metrics
                        ) * KeyboardLayout.bottomSwitchWidthRatio)
                        XCTAssertEqual(frame.minX, panel.horizontalInset + panel.metrics.side, accuracy: 0.5, "ABC stays at the bar's leading end (\(context))")
                        XCTAssertEqual(frame.width, keyWidth, accuracy: 0.5, "ABC keeps its old width (\(context))")
                    }

                    // The bar's row, under the emoji key: ABC, not a category.
                    if experiment {
                        let keyX = panel.convert(CGPoint(x: key.frame.midX, y: bar.midY), from: view).x
                        XCTAssertTrue(
                            panel.hitTest(CGPoint(x: keyX, y: bar.midY), with: nil) === letters,
                            "a tap under the emoji key hits ABC (\(context))"
                        )
                        XCTAssertTrue(
                            panel.hitTest(CGPoint(x: bar.minX - 6, y: bar.midY), with: nil) === letters,
                            "a tap left of ABC still hits ABC (\(context))"
                        )
                        // ABC's width comes out of the categories' room, and they still end
                        // before delete.
                        let categories = try XCTUnwrap(
                            panel.subviews.compactMap { $0 as? EmojiCategoryBar }.first,
                            "a category bar on a \(width) pt keyboard"
                        )
                        let delete = try XCTUnwrap(panel.subviews.first { $0.accessibilityLabel == "Delete" })
                        XCTAssertGreaterThan(categories.frame.width, 0, "the categories keep their room (\(context))")
                        XCTAssertLessThan(categories.frame.maxX, delete.frame.minX, "the categories end before delete (\(context))")
                    }
                }
            }
        }
    }

    /// TestFlight build 121 (zh): on the dictation bar the panel's ABC went back to the bar,
    /// not to letters. There it is a back key; the full keyboard's panel keeps ABC.
    func testDictationBarPanelShowsBackNotABC() throws {
        let library = try XCTUnwrap(EmojiLibrary.load(language: "en", bundle: Bundle(for: FullKeyboardView.self)))
        let panel = EmojiPanelView(library: library, usage: EmojiUsageStore(defaults: defaults))
        XCTAssertNotNil(panel.subviews.first { $0.accessibilityLabel == "Letters" })
        panel.returnsToBar = true
        let back = try XCTUnwrap(panel.subviews.compactMap { $0 as? PanelKeyButton }.first { $0.accessibilityLabel == String(localized: "Back") })
        XCTAssertNil(back.title)
        XCTAssertEqual(back.symbol, "chevron.left")
        XCTAssertNil(panel.subviews.first { $0.accessibilityLabel == "Letters" })
    }

    /// The panel a fixture or the dictation bar shows alone has no emoji key behind it
    /// (`emojiKeyFrame` stays nil), so ABC stays at the leading end.
    func testStandalonePanelKeepsTheLeadingEnd() throws {
        let library = try XCTUnwrap(EmojiLibrary.load(language: "en", bundle: Bundle(for: FullKeyboardView.self)))
        let panel = EmojiPanelView(library: library, usage: EmojiUsageStore(defaults: defaults))
        panel.frame = CGRect(x: 0, y: 0, width: 402, height: 306)
        panel.fill(before: "pizza ")
        panel.layoutIfNeeded()
        let letters = try XCTUnwrap(panel.subviews.first { $0.accessibilityLabel == "Letters" })
        let keyWidth = min(44, KeyboardLayout.letterWidth(
            innerWidth: panel.bounds.width - 2 * panel.horizontalInset,
            metrics: panel.metrics
        ) * KeyboardLayout.bottomSwitchWidthRatio)
        XCTAssertEqual(letters.frame.minX, panel.horizontalInset + panel.metrics.side, accuracy: 0.5)
        XCTAssertEqual(letters.frame.width, keyWidth, accuracy: 0.5)
    }

    /// iOS hands a keyboard extension only the touches that land on something its layers draw, so
    /// a touch between two emoji, between two rows or beside a category icon used to reach nothing:
    /// a swipe started there did nothing and a glide couldn't be caught (TestFlight build 147).
    /// Every point of the open panel, and of the search's results row, is drawn on, if invisibly.
    func testEveryPointOfThePanelAndTheSearchResultsTakesTouches() throws {
        let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget())
        view.emojiOverride = true
        view.makeEmojiUsage = { EmojiUsageStore(defaults: self.defaults) }
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 306)
        view.layoutIfNeeded()
        view.debugOpenEmoji()
        view.layoutIfNeeded()
        let panel = try XCTUnwrap(view.emojiPanel)
        panel.layoutIfNeeded()
        assertDrawnEverywhere(panel, "the panel")

        view.debugOpenEmoji(search: "cat")
        view.layoutIfNeeded()
        let results = try XCTUnwrap(view.emojiResults)
        results.layoutIfNeeded()
        assertDrawnEverywhere(results, "the search's results")
    }

    /// Every point 4 pt apart in `view` lies on a layer that draws (a fill or contents), as iOS
    /// needs before it routes a touch there to the keyboard.
    private func assertDrawnEverywhere(_ view: UIView, _ what: String, file: StaticString = #filePath, line: UInt = #line) {
        func draws(_ layer: CALayer, at point: CGPoint) -> Bool {
            guard !layer.isHidden, layer.opacity > 0, layer.bounds.contains(point) else { return false }
            if (layer.backgroundColor?.alpha ?? 0) > 0 || layer.contents != nil { return true }
            return (layer.sublayers ?? []).contains { draws($0, at: $0.convert(point, from: layer)) }
        }
        var missed: [CGPoint] = []
        for x in stride(from: CGFloat(1), to: view.bounds.width, by: 4) {
            for y in stride(from: CGFloat(1), to: view.bounds.height, by: 4) where !draws(view.layer, at: CGPoint(x: x, y: y)) {
                missed.append(CGPoint(x: x, y: y))
            }
        }
        XCTAssertTrue(missed.isEmpty, "\(what): \(missed.count) points draw nothing, as \(missed.prefix(3))", file: file, line: line)
    }
}
