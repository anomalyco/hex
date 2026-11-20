import CoreGraphics
import XCTest
@testable import HEX

final class KeyboardLayoutTests: XCTestCase {
    private let portraitWidths: [CGFloat] = [320, 375, 393, 402, 430, 440]
    private let layers: [KeyboardLayer] = [.letters, .numbers, .symbols]
    private let fields: [KeyboardFieldKind] = [.text, .url, .email]

    private func everyPortraitLayout() -> [KeyboardLayout] {
        var layouts: [KeyboardLayout] = []
        for width in portraitWidths {
            for layer in layers {
                for field in fields {
                    for globe in [false, true] {
                        layouts.append(KeyboardLayout(width: width, layer: layer, field: field, showsGlobe: globe))
                    }
                }
            }
            // Only the letters differ between languages.
            for language in KeyboardLanguage.allCases where language != .english {
                for globe in [false, true] {
                    layouts.append(KeyboardLayout(width: width, showsGlobe: globe, language: language))
                }
                layouts.append(KeyboardLayout(width: width, field: .email, language: language))
            }
        }
        return layouts
    }

    func testPortraitIs262PointsAndLandscapeIs176Points() {
        for width in portraitWidths {
            XCTAssertEqual(KeyboardLayout(width: width).totalHeight, 262, "width \(width)")
        }
        XCTAssertEqual(KeyboardLayout.height(for: .portrait), 262)
        XCTAssertEqual(KeyboardLayout.height(for: .landscape), 176)
    }

    func testPortraitMetricsMatchAppleIOS26() {
        let proMax = KeyboardLayout(width: 440)
        XCTAssertEqual(proMax.letterWidth, (440 - 13 - 54) / 10, accuracy: 0.001)
        let q = proMax.key(.character("q"))!
        XCTAssertEqual(q.frame.minX, 6.5, accuracy: 0.001)
        XCTAssertEqual(q.frame.minY, 44, accuracy: 0.001)
        XCTAssertEqual(q.frame.height, 45)
        let shift = proMax.key(.shift)!
        XCTAssertEqual(shift.frame.width, proMax.letterWidth * 1.34, accuracy: 0.001)
    }

    func testHitTargetsTileTheKeyAreaWithNoDeadZonesOrOverlaps() {
        var generator = SystemRandomNumberGenerator()
        for layout in everyPortraitLayout() + landscapeLayouts() {
            let area = layout.keysAreaFrame
            var rowTop = area.minY
            for row in 0..<4 {
                let targets = layout.keys.filter { $0.row == row }.map(\.hitFrame).sorted { $0.minX < $1.minX }
                XCTAssertEqual(targets.first?.minX, 0, describe(layout))
                XCTAssertEqual(targets.last?.maxX, layout.width, describe(layout))
                for target in targets {
                    XCTAssertEqual(target.minY, rowTop, accuracy: 0.0001, "\(describe(layout)) row \(row)")
                }
                for (a, b) in zip(targets, targets.dropFirst()) {
                    XCTAssertEqual(a.maxX, b.minX, accuracy: 0.0001, "\(describe(layout)) row \(row)")
                }
                rowTop = targets[0].maxY
            }
            XCTAssertEqual(rowTop, area.maxY, accuracy: 0.0001, describe(layout))

            for _ in 0..<400 {
                let point = CGPoint(
                    x: CGFloat.random(in: 0..<layout.width, using: &generator),
                    y: CGFloat.random(in: area.minY..<area.maxY, using: &generator)
                )
                let hits = layout.keys.filter { $0.hitFrame.contains(point) }
                XCTAssertEqual(hits.count, 1, "\(describe(layout)) at \(point): \(hits.map(\.label))")
            }
            XCTAssertNil(layout.hitTest(CGPoint(x: 20, y: layout.stripFrame.midY)), "strip is not a key")
        }
    }

    func testKeycapsSitInsideTheirTargetsAndNeverOverlap() {
        for layout in everyPortraitLayout() + landscapeLayouts() {
            let bounds = CGRect(x: 0, y: 0, width: layout.width, height: layout.totalHeight)
            for key in layout.keys {
                XCTAssertTrue(key.hitFrame.contains(key.frame), "\(describe(layout)) \(key.label)")
                XCTAssertTrue(bounds.contains(key.frame), "\(describe(layout)) \(key.label)")
                XCTAssertGreaterThan(key.frame.width, 20, "\(describe(layout)) \(key.label)")
            }
            for (i, a) in layout.keys.enumerated() {
                for b in layout.keys[(i + 1)...] {
                    XCTAssertFalse(a.frame.intersects(b.frame), "\(describe(layout)) \(a.label) × \(b.label)")
                }
            }
        }
    }

    func testBalloonsStayInsideThe262PointKeyboard() {
        for layout in everyPortraitLayout() {
            let bounds = CGRect(x: 0, y: 0, width: layout.width, height: layout.totalHeight)
            for key in layout.keys where key.character != nil {
                guard let balloon = layout.balloon(for: key) else {
                    return XCTFail("\(describe(layout)) \(key.label) has no balloon")
                }
                XCTAssertTrue(
                    bounds.insetBy(dx: -0.001, dy: -0.001).contains(balloon.bounds),
                    "\(describe(layout)) \(key.label): \(balloon.bubble)"
                )
                XCTAssertGreaterThanOrEqual(balloon.bubble.width, key.frame.width)
            }
        }
    }

    func testAccentMenusClampInwardAcrossEveryWidth() {
        for width in portraitWidths {
            let layout = KeyboardLayout(width: width)
            let bounds = CGRect(x: 0, y: 0, width: layout.width, height: layout.totalHeight)
            for ch in ["e", "a", "i", "o", "u", "n", "c", "s", "z"] {
                let key = layout.key(.character(ch))!
                let options = AccentCatalog.options(for: ch, uppercase: false)!
                let menu = AccentMenu.build(for: key.frame, rawOptions: options, in: layout.width)
                XCTAssertTrue(
                    bounds.insetBy(dx: -0.01, dy: -0.01).contains(menu.bubbleFrame),
                    "width \(width) key \(ch): \(menu.bubbleFrame)"
                )
                XCTAssertEqual(menu.selectedOption, ch)
            }
        }
    }

    func testEdgeBalloonsArePushedInward() {
        let layout = KeyboardLayout(width: 375)
        let q = layout.balloon(for: layout.key(.character("q"))!)!
        let p = layout.balloon(for: layout.key(.character("p"))!)!
        let g = layout.balloon(for: layout.key(.character("g"))!)!
        XCTAssertGreaterThan(q.offsetX, 0)
        XCTAssertLessThan(p.offsetX, 0)
        XCTAssertEqual(g.offsetX, 0, accuracy: 0.001)
        XCTAssertEqual(q.bubble.minY, 0, accuracy: 0.001)
    }

    func testNoBalloonsForModifiersOrInLandscape() {
        let portrait = KeyboardLayout(width: 393)
        XCTAssertNil(portrait.balloon(for: portrait.key(.space)!))
        XCTAssertNil(portrait.balloon(for: portrait.key(.shift)!))
        let landscape = KeyboardLayout(width: 852, orientation: .landscape)
        XCTAssertNil(landscape.balloon(for: landscape.key(.character("a"))!))
    }

    func testBottomRowMatchesIOS26EmailAndURLRightOfSpace() {
        // The Hex key sits at the strip's trailing edge, so the bottom row is Apple's.
        XCTAssertEqual(bottomRow(KeyboardLayout(width: 393)), [.layerSwitch(.numbers), .space, .returnKey])
        XCTAssertEqual(
            bottomRow(KeyboardLayout(width: 375, showsGlobe: true)),
            [.layerSwitch(.numbers), .globe, .space, .returnKey]
        )
        XCTAssertEqual(
            bottomRow(KeyboardLayout(width: 393, layer: .numbers, field: .email)),
            [.layerSwitch(.letters), .space, .character("@"), .character("."), .returnKey]
        )
        XCTAssertEqual(
            bottomRow(KeyboardLayout(width: 393, field: .url)),
            [.layerSwitch(.numbers), .space, .character("."), .character("/"), .returnKey]
        )
    }

    func testHexKeySitsAtTheStripsTrailingEdge() {
        let layout = KeyboardLayout(width: 402)
        let hex = layout.key(.hex)!
        XCTAssertEqual(hex.row, -1)
        XCTAssertEqual(hex.frame.maxX, 402 - layout.metrics.side, accuracy: 0.001)
        XCTAssertEqual(hex.frame.midY, layout.metrics.stripHeight / 2, accuracy: 0.001)
        XCTAssertLessThanOrEqual(layout.stripFrame.maxX, hex.frame.minX)
    }

    func testRowThreeModifiersFollowTheLayer() {
        XCTAssertNotNil(KeyboardLayout(width: 393).key(.shift))
        XCTAssertEqual(KeyboardLayout(width: 393, layer: .numbers).key(.layerSwitch(.symbols))?.row, 2)
        XCTAssertEqual(KeyboardLayout(width: 393, layer: .symbols).key(.layerSwitch(.numbers))?.row, 2)
        let numbers = KeyboardLayout(width: 393, layer: .numbers)
        XCTAssertEqual(numbers.keys.filter { $0.row == 2 }.compactMap(\.character), [".", ",", "?", "!", "'"])
    }

    func testRowTwoIsCentred() {
        let layout = KeyboardLayout(width: 402)
        let a = layout.key(.character("a"))!.frame
        let l = layout.key(.character("l"))!.frame
        XCTAssertEqual(a.minX, layout.width - l.maxX, accuracy: 0.001)
        XCTAssertEqual(layout.key(.character("a"))!.hitFrame.minX, 0)
        XCTAssertEqual(layout.key(.character("l"))!.hitFrame.maxX, layout.width)
    }

    func testLandscapeKeepsInsetsAndHeight() {
        let layout = KeyboardLayout(width: 932, orientation: .landscape, horizontalInset: 60)
        XCTAssertEqual(layout.totalHeight, 176)
        XCTAssertEqual(layout.key(.character("q"))!.frame.minX, 64, accuracy: 0.001)
        XCTAssertEqual(layout.key(.character("q"))!.hitFrame.minX, 0)
    }

    // MARK: - Languages

    func testLetterRowsMatchApplesLayouts() {
        let rows: [KeyboardLanguage: [String]] = [
            .english: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .norwegian: ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"],
            .spanish: ["qwertyuiop", "asdfghjklñ", "zxcvbnm"],
            .german: ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"],
        ]
        for (language, expected) in rows {
            let layout = KeyboardLayout(width: 402, language: language)
            for row in 0..<3 {
                XCTAssertEqual(letters(layout, row: row).joined(), expected[row], "\(language) row \(row + 1)")
            }
        }
    }

    func testElevenKeyRowsFitEveryIPhoneWidthAndLandscape() {
        let cases: [(CGFloat, KeyboardOrientation, CGFloat)] = [
            (375, .portrait, 0), (402, .portrait, 0), (440, .portrait, 0),
            (667, .landscape, 0), (874, .landscape, 0), (956, .landscape, 60),
        ]
        for language in [KeyboardLanguage.norwegian, .german] {
            for (width, orientation, inset) in cases {
                let layout = KeyboardLayout(width: width, orientation: orientation, horizontalInset: inset, language: language)
                let english = KeyboardLayout(width: width, orientation: orientation, horizontalInset: inset)
                let m = layout.metrics
                let label = "\(language) \(width)pt \(orientation)"
                let inner = width - 2 * inset
                XCTAssertEqual(layout.characterWidth, (inner - 2 * m.side - 10 * m.gap) / 11, accuracy: 0.001, label)
                XCTAssertGreaterThan(layout.characterWidth, 26, label)
                for row in 0..<2 {
                    let keys = layout.keys.filter { $0.row == row }.sorted { $0.frame.minX < $1.frame.minX }
                    XCTAssertEqual(keys.count, 11, label)
                    // Rows 1 and 2 run edge to edge, like English row 1.
                    XCTAssertEqual(keys.first!.frame.minX, inset + m.side, accuracy: 0.001, label)
                    XCTAssertEqual(keys.last!.frame.maxX, width - inset - m.side, accuracy: 0.001, label)
                }
                // Row 3's letters are as wide as the others and centred between shift and delete,
                // which keep their English size, as does the whole bottom row.
                let row3 = layout.keys.filter { $0.row == 2 && $0.character != nil }.sorted { $0.frame.minX < $1.frame.minX }
                XCTAssertEqual(row3.first!.frame.width, layout.characterWidth, accuracy: 0.001, label)
                XCTAssertEqual(row3.first!.frame.minX - inset, width - inset - row3.last!.frame.maxX, accuracy: 0.001, label)
                XCTAssertEqual(layout.key(.shift)!.frame, english.key(.shift)!.frame, label)
                XCTAssertEqual(layout.key(.delete)!.frame, english.key(.delete)!.frame, label)
                XCTAssertLessThan(layout.key(.shift)!.frame.maxX, row3.first!.frame.minX, label)
                XCTAssertEqual(
                    layout.keys.filter { $0.row == 3 }.map(\.frame),
                    english.keys.filter { $0.row == 3 }.map(\.frame),
                    label
                )
            }
        }
    }

    func testSpanishKeepsTenKeysWithNOnRowTwo() {
        for width: CGFloat in [375, 402, 440] {
            let layout = KeyboardLayout(width: width, language: .spanish)
            XCTAssertEqual(layout.characterWidth, layout.letterWidth, accuracy: 0.001)
            let n = layout.key(.character("ñ"))!
            XCTAssertEqual(n.frame.maxX, width - layout.metrics.side, accuracy: 0.001)
            XCTAssertEqual(n.hitFrame.maxX, width)
            XCTAssertEqual(layout.key(.character("a"))!.frame.minX, layout.metrics.side, accuracy: 0.001)
        }
    }

    func testAccentsMatchEachLanguage() {
        XCTAssertEqual(AccentCatalog.options(for: "ø", uppercase: false, language: .norwegian), ["ø", "ö"])
        XCTAssertEqual(AccentCatalog.options(for: "æ", uppercase: true, language: .norwegian), ["Æ", "Ä"])
        XCTAssertNil(AccentCatalog.options(for: "å", uppercase: false, language: .norwegian))
        let norwegianA = AccentCatalog.options(for: "a", uppercase: false, language: .norwegian)!
        XCTAssertFalse(norwegianA.contains("å") || norwegianA.contains("æ"), "they have their own keys")
        XCTAssertTrue(AccentCatalog.options(for: "s", uppercase: false, language: .german)!.contains("ß"))
        XCTAssertFalse(AccentCatalog.options(for: "u", uppercase: false, language: .german)!.contains("ü"))
        XCTAssertEqual(AccentCatalog.options(for: "e", uppercase: false, language: .spanish)?.dropFirst().first, "é")
        XCTAssertEqual(AccentCatalog.options(for: "n", uppercase: true, language: .spanish), ["N", "Ñ", "Ń"])
        XCTAssertEqual(
            AccentCatalog.options(for: "a", uppercase: false),
            AccentCatalog.options(for: "a", uppercase: false, language: .english),
            "English is unchanged"
        )
    }

    func testAccentMenusOnTheNewKeysStayInside() {
        let keys: [KeyboardLanguage: [String]] = [
            .norwegian: ["å", "ø", "æ", "a", "o"],
            .spanish: ["ñ", "a", "o", "n"],
            .german: ["ü", "ö", "ä", "s", "a", "u"],
        ]
        for width in portraitWidths {
            for (language, characters) in keys {
                let layout = KeyboardLayout(width: width, language: language)
                let bounds = CGRect(x: 0, y: 0, width: layout.width, height: layout.totalHeight)
                for ch in characters {
                    let key = layout.key(.character(ch))!
                    guard let options = AccentCatalog.options(for: ch, uppercase: false, language: language) else { continue }
                    let menu = AccentMenu.build(for: key.frame, rawOptions: options, in: layout.width)
                    XCTAssertTrue(
                        bounds.insetBy(dx: -0.01, dy: -0.01).contains(menu.bubbleFrame),
                        "\(language) \(width) \(ch): \(menu.bubbleFrame)"
                    )
                    XCTAssertEqual(menu.selectedOption, ch)
                }
            }
        }
    }

    func testTheLayoutFollowsTheDictationLanguage() {
        func automatic(_ dictation: String?, modes: [String] = [], preferred: [String] = []) -> KeyboardLanguage {
            KeyboardLanguage.automatic(dictationLanguage: dictation, inputModes: modes, preferredLanguages: preferred)
        }
        XCTAssertEqual(automatic("nb-NO"), .norwegian)
        XCTAssertEqual(automatic("es-MX"), .spanish)
        XCTAssertEqual(automatic("de-AT"), .german)
        XCTAssertEqual(automatic("en-GB", modes: ["nb-NO"]), .english, "the dictation language wins")
        XCTAssertEqual(automatic("he-IL", preferred: ["he-IL", "de-DE"]), .english, "Hebrew has no layout yet")
        // Before Hex knows the dictation language: the first keyboard, then language, with a layout.
        XCTAssertEqual(automatic(nil, modes: ["emoji", "nb-NO", "en-US"]), .norwegian)
        XCTAssertEqual(automatic(nil, preferred: ["nn-NO"]), .norwegian)
        XCTAssertEqual(automatic(nil, preferred: ["fr-FR"]), .english)
    }

    func testSpellCheckUsesTheLayoutsLanguageOrNone() {
        XCTAssertEqual(KeyboardLanguage.norwegian.spellCheckLanguage(available: ["en_US", "nb"]), "nb")
        XCTAssertNil(KeyboardLanguage.norwegian.spellCheckLanguage(available: ["en_US", "da"]))
        XCTAssertEqual(KeyboardLanguage.german.spellCheckLanguage(available: ["de"]), "de")
    }

    // MARK: - Helpers

    private func landscapeLayouts() -> [KeyboardLayout] {
        [
            KeyboardLayout(width: 667, orientation: .landscape),
            KeyboardLayout(width: 852, orientation: .landscape, layer: .numbers),
            KeyboardLayout(width: 956, orientation: .landscape, field: .email, horizontalInset: 60),
        ] + KeyboardLanguage.allCases.flatMap { language in
            [
                KeyboardLayout(width: 667, orientation: .landscape, language: language),
                KeyboardLayout(width: 956, orientation: .landscape, showsGlobe: true, horizontalInset: 60, language: language),
            ]
        }
    }

    private func letters(_ layout: KeyboardLayout, row: Int) -> [String] {
        layout.keys.filter { $0.row == row && $0.character != nil }
            .sorted { $0.frame.minX < $1.frame.minX }
            .compactMap(\.character)
    }

    private func bottomRow(_ layout: KeyboardLayout) -> [KeyKind] {
        layout.keys.filter { $0.row == 3 }.sorted { $0.frame.minX < $1.frame.minX }.map(\.kind)
    }

    private func describe(_ layout: KeyboardLayout) -> String {
        "\(layout.width)pt \(layout.orientation) \(layout.layer) \(layout.field) globe=\(layout.showsGlobe) \(layout.language)"
    }
}

@MainActor
final class FullKeyboardSettingsTests: XCTestCase {
    func testFullKeyboardIsOffAndAutocorrectOnByDefault() throws {
        let suite = "FullKeyboardSettingsTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = FullKeyboardSettings(defaults: defaults)

        XCTAssertFalse(settings.isEnabled)
        XCTAssertTrue(settings.autocorrect)

        settings.applyLaunchArguments(["HEX"])
        XCTAssertFalse(settings.isEnabled)
        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.launchArgument])
        XCTAssertTrue(settings.isEnabled)

        settings.autocorrect = false
        XCTAssertFalse(FullKeyboardSettings(defaults: defaults).autocorrect)
    }

    func testNoAppGroupReadsAsOff() {
        XCTAssertFalse(FullKeyboardSettings(defaults: nil).isEnabled)
    }
}
