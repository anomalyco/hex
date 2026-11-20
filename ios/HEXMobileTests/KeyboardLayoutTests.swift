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
            let area = layout.touchAreaFrame
            // Row 1 also takes the strip's bottom band, under its pills, as Apple's keys reach
            // above their keycaps; the Hex key in the strip stops where it starts.
            XCTAssertEqual(area.minY, layout.keysAreaFrame.minY - KeyboardLayout.stripTouchBand, describe(layout))
            if let hex = layout.key(.hex), hex.row < 0 {
                XCTAssertLessThanOrEqual(hex.hitFrame.maxY, area.minY, describe(layout))
            }
            for key in layout.keys where key.row == 0 {
                let justAbove = CGPoint(x: key.frame.midX, y: key.frame.minY - 2)
                XCTAssertEqual(layout.hitTest(justAbove)?.kind, key.kind, "\(describe(layout)) \(key.label)")
            }
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
            .italian: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .french: ["azertyuiop", "qsdfghjklm", "wxcvbn"],
            .frenchCanadian: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .frenchSwiss: ["qwertzuiop", "asdfghjkl", "yxcvbnm"],
            .portuguese: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .russian: ["йцукенгшщзх", "фывапролджэ", "ячсмитьбю"],
            .serbianLatin: ["qwertzuiop", "asdfghjkl", "yxcvbnm"],
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

    /// Russian's nine-letter third row keeps its letters as wide as rows 1 and 2; shift and
    /// delete narrow to fit beside them, as on Apple's Russian keyboard.
    func testRussiansNineLetterRowFitsBetweenNarrowerShiftAndDelete() {
        let cases: [(CGFloat, KeyboardOrientation, CGFloat)] = [
            (320, .portrait, 0), (375, .portrait, 0), (402, .portrait, 0), (440, .portrait, 0),
            (667, .landscape, 0), (956, .landscape, 60),
        ]
        for (width, orientation, inset) in cases {
            let layout = KeyboardLayout(width: width, orientation: orientation, horizontalInset: inset, language: .russian)
            let english = KeyboardLayout(width: width, orientation: orientation, horizontalInset: inset)
            let label = "\(width)pt \(orientation)"
            let row3 = layout.keys.filter { $0.row == 2 && $0.character != nil }.sorted { $0.frame.minX < $1.frame.minX }
            XCTAssertEqual(row3.count, 9, label)
            XCTAssertEqual(row3.first!.frame.width, layout.characterWidth, accuracy: 0.001, label)
            let shift = layout.key(.shift)!.frame
            let delete = layout.key(.delete)!.frame
            XCTAssertEqual(row3.first!.frame.minX - shift.maxX, layout.metrics.gap, accuracy: 0.001, label)
            XCTAssertEqual(delete.minX - row3.last!.frame.maxX, layout.metrics.gap, accuracy: 0.001, label)
            XCTAssertEqual(shift.width, delete.width, accuracy: 0.001, label)
            XCTAssertLessThan(shift.width, english.key(.shift)!.frame.width, label)
            XCTAssertGreaterThanOrEqual(shift.width, layout.characterWidth - 0.001, label)
            XCTAssertEqual(
                layout.keys.filter { $0.row == 3 }.map(\.frame),
                english.keys.filter { $0.row == 3 }.map(\.frame),
                label
            )
        }
        // The numbers and symbols pages are English's.
        for layer in [KeyboardLayer.numbers, .symbols] {
            XCTAssertEqual(
                KeyboardLayout(width: 402, layer: layer, language: .russian).keys,
                KeyboardLayout(width: 402, layer: layer).keys
            )
        }
    }

    /// Apple's AZERTY on the tester's iPhone 17 Pro (402 pt; their French and English
    /// keyboard): rows 1 and 2 run edge to edge, W to N sit under E to I, and shift and
    /// delete keep their English size with a gap beside them. A tap in that gap goes to the
    /// nearer key, and every point of the row still types something.
    func testFrenchIsApplesAzerty() {
        for width: CGFloat in [375, 402, 440] {
            let layout = KeyboardLayout(width: width, language: .french)
            let english = KeyboardLayout(width: width)
            let label = "\(width)pt"
            XCTAssertEqual(layout.characterWidth, layout.letterWidth, accuracy: 0.001, label)
            for row in 0..<2 {
                let keys = layout.keys.filter { $0.row == row }.sorted { $0.frame.minX < $1.frame.minX }
                XCTAssertEqual(keys.count, 10, label)
                XCTAssertEqual(keys.first!.frame.minX, layout.metrics.side, accuracy: 0.001, label)
                XCTAssertEqual(keys.last!.frame.maxX, width - layout.metrics.side, accuracy: 0.001, label)
            }
            for (bottom, top) in zip(["w", "n"], ["e", "i"]) {
                XCTAssertEqual(layout.key(.character(bottom))!.frame.minX, layout.key(.character(top))!.frame.minX, accuracy: 0.001, label)
                XCTAssertEqual(layout.key(.character(bottom))!.frame.width, layout.letterWidth, accuracy: 0.001, label)
            }
            let shift = layout.key(.shift)!
            let delete = layout.key(.delete)!
            let w = layout.key(.character("w"))!
            let n = layout.key(.character("n"))!
            XCTAssertEqual(shift.frame, english.key(.shift)!.frame, label)
            XCTAssertEqual(delete.frame, english.key(.delete)!.frame, label)
            XCTAssertGreaterThan(w.frame.minX - shift.frame.maxX, 4 * layout.metrics.gap, "a gap beside shift, \(label)")
            XCTAssertGreaterThan(delete.frame.minX - n.frame.maxX, 4 * layout.metrics.gap, "and beside delete, \(label)")
            XCTAssertEqual(layout.keys.filter { $0.row == 3 }.map(\.frame), english.keys.filter { $0.row == 3 }.map(\.frame), label)
            let y = shift.frame.midY
            for (x, expected) in [
                (shift.frame.maxX + 1, KeyKind.shift), (w.frame.minX - 1, .character("w")),
                (delete.frame.minX - 1, .delete), (n.frame.maxX + 1, .character("n")),
            ] {
                XCTAssertEqual(layout.hitTest(CGPoint(x: x, y: y))?.kind, expected, "x \(x), \(label)")
            }
            XCTAssertEqual(
                layout.hitTest(CGPoint(x: (shift.frame.maxX + w.frame.minX) / 2 - 0.5, y: y))?.kind, .shift, label
            )
        }
        // Apple's measured cells on 402 pt: the A key's 4.7 to 44, shift's 4.7 to 56, W from 63.7.
        let apple = KeyboardLayout(width: 402, language: .french)
        XCTAssertEqual(apple.key(.character("a"))!.frame.midX, (4.7 + 44) / 2, accuracy: 1.5)
        XCTAssertEqual(apple.key(.character("m"))!.frame.midX, (360 + 399.7) / 2, accuracy: 1.5)
        // The numbers and symbols pages are English's, as Apple's are.
        for layer in [KeyboardLayer.numbers, .symbols] {
            XCTAssertEqual(KeyboardLayout(width: 402, layer: layer, language: .french).keys, KeyboardLayout(width: 402, layer: layer).keys)
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
        // Italian's accents come first on each vowel: à è ì ò ù, and é right after è.
        let italian = ["a", "e", "i", "o", "u"].map {
            AccentCatalog.options(for: $0, uppercase: false, language: .italian)!.dropFirst().first
        }
        XCTAssertEqual(italian, ["à", "è", "ì", "ò", "ù"])
        XCTAssertEqual(AccentCatalog.options(for: "e", uppercase: false, language: .italian)?.dropFirst(2).first, "é")
        XCTAssertEqual(AccentCatalog.options(for: "o", uppercase: true, language: .italian)?.prefix(2), ["O", "Ò"])
        XCTAssertEqual(AccentCatalog.options(for: "e", uppercase: false, language: .french)?.dropFirst().first, "é")
        XCTAssertEqual(AccentCatalog.options(for: "c", uppercase: false, language: .french)?.dropFirst().first, "ç")
        // Apple's French order (TextInput_fr): û before ù, and ª and º after the French accents.
        XCTAssertEqual(AccentCatalog.options(for: "u", uppercase: false, language: .french)?.prefix(3), ["u", "û", "ù"])
        XCTAssertEqual(AccentCatalog.options(for: "a", uppercase: true, language: .french)?.prefix(4), ["A", "À", "Â", "ª"])
        XCTAssertEqual(AccentCatalog.options(for: "o", uppercase: false, language: .french)?.prefix(4), ["o", "ô", "œ", "º"])
        for language in [KeyboardLanguage.frenchCanadian, .frenchSwiss] {
            for letter in ["a", "e", "i", "o", "u", "c"] {
                XCTAssertEqual(
                    AccentCatalog.options(for: letter, uppercase: false, language: language),
                    AccentCatalog.options(for: letter, uppercase: false, language: .french),
                    "\(language) \(letter)"
                )
            }
        }
        XCTAssertEqual(AccentCatalog.options(for: "a", uppercase: false, language: .portuguese)?.prefix(3), ["a", "á", "ã"])
        // Russian: ё on е and ъ on ь, as on Apple's; Serbian (Latin): č ć on c, đ on d, š on s, ž on z.
        XCTAssertEqual(AccentCatalog.options(for: "е", uppercase: false, language: .russian)?.prefix(2), ["е", "ё"])
        XCTAssertEqual(AccentCatalog.options(for: "Е", uppercase: true, language: .russian)?.prefix(2), ["Е", "Ё"])
        XCTAssertEqual(AccentCatalog.options(for: "ь", uppercase: false, language: .russian)?.prefix(2), ["ь", "ъ"])
        XCTAssertEqual(AccentCatalog.options(for: "ь", uppercase: true, language: .russian)?.prefix(2), ["Ь", "Ъ"])
        XCTAssertNil(AccentCatalog.options(for: "б", uppercase: false, language: .russian))
        XCTAssertEqual(AccentCatalog.options(for: "c", uppercase: false, language: .serbianLatin)?.prefix(3), ["c", "č", "ć"])
        XCTAssertEqual(AccentCatalog.options(for: "d", uppercase: true, language: .serbianLatin), ["D", "Đ"])
        XCTAssertEqual(AccentCatalog.options(for: "s", uppercase: false, language: .serbianLatin)?.prefix(2), ["s", "š"])
        XCTAssertEqual(AccentCatalog.options(for: "z", uppercase: false, language: .serbianLatin)?.prefix(2), ["z", "ž"])
    }

    func testAccentMenusOnTheNewKeysStayInside() {
        let keys: [KeyboardLanguage: [String]] = [
            .norwegian: ["å", "ø", "æ", "a", "o"],
            .spanish: ["ñ", "a", "o", "n"],
            .german: ["ü", "ö", "ä", "s", "a", "u"],
            .russian: ["й", "е", "х", "ж", "э", "ь", "ю", "я"],
            .serbianLatin: ["c", "d", "s", "z"],
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
        // Italian, French and Portuguese type in their own language, never English.
        XCTAssertEqual(automatic("it-IT", modes: ["it-IT", "en-US"]), .italian)
        XCTAssertEqual(automatic("it-CH"), .italian)
        // French follows Apple's keyboard for the region: AZERTY in France and Belgium,
        // QWERTY in Canada, QWERTZ in Switzerland.
        XCTAssertEqual(automatic("fr-FR"), .french)
        XCTAssertEqual(automatic("fr-BE"), .french)
        XCTAssertEqual(automatic("fr"), .french)
        XCTAssertEqual(automatic("fr-CA"), .frenchCanadian)
        XCTAssertEqual(automatic("fr_CA"), .frenchCanadian)
        XCTAssertEqual(automatic("fr-CH"), .frenchSwiss)
        XCTAssertEqual(automatic("pt-BR"), .portuguese)
        XCTAssertEqual(automatic("en-US", modes: ["it-IT"]), .english, "the dictation language still wins")
        // Russian, and Serbian in Latin script (Hex has no Cyrillic Serbian layout).
        XCTAssertEqual(automatic("ru-RU", modes: ["en-US"]), .russian)
        XCTAssertEqual(automatic("sr-Latn-RS"), .serbianLatin)
        XCTAssertEqual(automatic("sr_Latn"), .serbianLatin)
        XCTAssertEqual(automatic("sr-RS"), .english, "Cyrillic Serbian has no layout")
        XCTAssertEqual(automatic("en-US", modes: ["ru-RU", "sr-Latn"]), .english, "the dictation language still wins")
        // Before Hex knows the dictation language: the first keyboard, then language, with a layout.
        XCTAssertEqual(automatic(nil, modes: ["emoji", "nb-NO", "en-US"]), .norwegian)
        XCTAssertEqual(automatic(nil, preferred: ["nn-NO"]), .norwegian)
        XCTAssertEqual(automatic(nil, modes: ["emoji", "it-IT"], preferred: ["en-US"]), .italian)
        XCTAssertEqual(automatic(nil, preferred: ["it-IT", "en-US"]), .italian)
        XCTAssertEqual(automatic(nil, preferred: ["fr-FR"]), .french)
        XCTAssertEqual(automatic(nil, preferred: ["fr-CA", "en-CA"]), .frenchCanadian)
        XCTAssertEqual(automatic(nil, modes: ["fr-CH"], preferred: ["fr-FR"]), .frenchSwiss)
        XCTAssertEqual(automatic(nil, preferred: ["ru-RU"]), .russian)
        XCTAssertEqual(automatic(nil, modes: ["emoji", "sr-Latn"], preferred: ["en-US"]), .serbianLatin)
        XCTAssertEqual(automatic(nil, modes: ["sr"], preferred: ["en-US"]), .english, "Apple's Cyrillic Serbian keyboard")
        XCTAssertEqual(automatic(nil, preferred: ["el-GR"]), .english)
    }

    /// Settings › Keyboard › Languages: empty is Automatic (one language, as before); the
    /// language last switched to counts only while it is still chosen; the space bar's label
    /// names a French layout's region only beside another French one.
    func testTheChosenLanguagesSwitchInOrder() {
        let automatic = KeyboardLanguageList.resolve(chosen: [], stored: .french, automatic: .norwegian)
        XCTAssertEqual(automatic.languages, [.norwegian])
        XCTAssertEqual(automatic.current, .norwegian)
        XCTAssertFalse(automatic.switches)
        XCTAssertNil(automatic.spaceLabel)

        let list = KeyboardLanguageList.resolve(chosen: [.english, .french, .russian, .english], stored: .russian, automatic: .german)
        XCTAssertEqual(list.languages, [.english, .french, .russian])
        XCTAssertEqual(list.current, .russian)
        XCTAssertEqual(list.switched(by: 1), .english, "round the list")
        XCTAssertEqual(list.switched(by: -1), .french)
        XCTAssertEqual(list.spaceLabel, "Русский")
        XCTAssertEqual(KeyboardLanguageList.resolve(chosen: [.english, .french], stored: .german, automatic: .german).current, .english)
        XCTAssertEqual(KeyboardLanguageList.resolve(chosen: [.french, .english], stored: nil, automatic: .german).spaceLabel, "Français")
        XCTAssertEqual(
            KeyboardLanguageList.resolve(chosen: [.french, .frenchCanadian], stored: .frenchCanadian, automatic: .english).spaceLabel,
            "Français (Canada)"
        )
    }

    /// Settings › Keyboard › Languages offers the iPhone's own keyboards first, then its
    /// languages; Switch dictation too dictates in the iPhone's region where it has one.
    func testLanguagesOfferTheIPhonesKeyboardsAndDictateInItsRegion() {
        XCTAssertEqual(
            KeyboardLanguage.suggested(inputModes: ["en-GB", "emoji", "fr-FR", "en-US"], preferred: ["ru-RU", "fr-CA", "he-IL"]),
            [.english, .french, .russian, .frenchCanadian]
        )
        let supported = ["en-US", "en-GB", "fr-FR", "fr-CA", "ru-RU", "nb-NO", "pt-BR", "pt-PT"]
        XCTAssertEqual(KeyboardLanguage.english.dictationLanguage(supported: supported, preferred: ["en-GB", "fr-FR"]), "en-GB")
        XCTAssertEqual(KeyboardLanguage.english.dictationLanguage(supported: supported, preferred: ["fr-FR"]), "en-US")
        XCTAssertEqual(KeyboardLanguage.french.dictationLanguage(supported: supported, preferred: ["en-GB"]), "fr-FR")
        XCTAssertEqual(KeyboardLanguage.frenchCanadian.dictationLanguage(supported: supported, preferred: ["fr-FR"]), "fr-CA")
        XCTAssertEqual(KeyboardLanguage.portuguese.dictationLanguage(supported: supported, preferred: ["pt-PT"]), "pt-PT")
        XCTAssertEqual(KeyboardLanguage.norwegian.dictationLanguage(supported: supported, preferred: ["nn-NO"]), "nb-NO")
        XCTAssertNil(KeyboardLanguage.serbianLatin.dictationLanguage(supported: supported, preferred: ["sr-Latn-RS"]))
        XCTAssertNil(KeyboardLanguage.german.dictationLanguage(supported: supported, preferred: []), "Apple Speech has no German here")
        XCTAssertEqual(KeyboardLanguage.german.dictationLanguage(supported: [], preferred: []), "de-DE", "not listed yet")
    }

    /// The languages read the one layout chosen before them, and write their first for an
    /// older build.
    func testLanguagesReadTheOneLayoutChosenBefore() {
        let suite = "KeyboardLayoutTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = LanguageSettings(defaults: defaults)
        XCTAssertEqual(settings.keyboardLanguages, [], "Automatic")
        defaults.set("fr-CA", forKey: "settings.keyboardLayout")
        XCTAssertEqual(settings.keyboardLanguages, [.frenchCanadian])
        settings.keyboardLanguages = [.english, .french, .english]
        XCTAssertEqual(settings.keyboardLanguages, [.english, .french])
        XCTAssertEqual(defaults.string(forKey: "settings.keyboardLayout"), "en")
        settings.keyboardLanguages = []
        XCTAssertEqual(settings.keyboardLanguages, [])
        XCTAssertNil(defaults.string(forKey: "settings.keyboardLayout"))
        XCTAssertFalse(settings.dictationFollowsKeyboard, "off by default")
    }

    func testSpellCheckUsesTheLayoutsLanguageOrNone() {
        XCTAssertEqual(KeyboardLanguage.norwegian.spellCheckLanguage(available: ["en_US", "nb"]), "nb")
        XCTAssertNil(KeyboardLanguage.norwegian.spellCheckLanguage(available: ["en_US", "da"]))
        XCTAssertEqual(KeyboardLanguage.german.spellCheckLanguage(available: ["de"]), "de")
        XCTAssertEqual(KeyboardLanguage.italian.spellCheckLanguage(available: ["en_US", "it_IT"]), "it_IT")
        XCTAssertEqual(KeyboardLanguage.french.spellCheckLanguage(available: ["en_US", "fr_FR"]), "fr_FR")
        XCTAssertEqual(KeyboardLanguage.frenchCanadian.spellCheckLanguage(available: ["fr_FR", "fr_CA"]), "fr_CA")
        XCTAssertEqual(KeyboardLanguage.frenchCanadian.spellCheckLanguage(available: ["en_US", "fr_FR"]), "fr_FR")
        XCTAssertEqual(KeyboardLanguage.frenchSwiss.spellCheckLanguage(available: ["en_US", "fr_FR"]), "fr_FR")
        XCTAssertEqual(KeyboardLanguage.portuguese.spellCheckLanguage(available: ["pt_PT", "pt_BR"]), "pt_BR")
        XCTAssertNil(KeyboardLanguage.italian.spellCheckLanguage(available: ["en_US", "es"]), "never English")
        XCTAssertEqual(KeyboardLanguage.russian.spellCheckLanguage(available: ["en_US", "ru_RU"]), "ru_RU")
        XCTAssertNil(KeyboardLanguage.russian.spellCheckLanguage(available: ["en_US", "uk_UA"]), "never English")
        XCTAssertEqual(KeyboardLanguage.serbianLatin.spellCheckLanguage(available: ["en_US", "sr_Latn"]), "sr_Latn")
        XCTAssertNil(KeyboardLanguage.serbianLatin.spellCheckLanguage(available: ["en_US", "sr", "hr_HR"]), "no Cyrillic or Croatian")
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
    func testFullKeyboardAndAutocorrectAreOnByDefault() throws {
        let suite = "FullKeyboardSettingsTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = FullKeyboardSettings(defaults: defaults)

        XCTAssertTrue(settings.isEnabled)
        XCTAssertTrue(settings.autocorrect)

        settings.applyLaunchArguments(["HEX"])
        XCTAssertTrue(settings.isEnabled)
        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.keyboardLaunchArgument, "dictation-bar"])
        XCTAssertFalse(settings.isEnabled)
        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.launchArgument])
        XCTAssertTrue(settings.isEnabled)

        settings.autocorrect = false
        XCTAssertFalse(FullKeyboardSettings(defaults: defaults).autocorrect)
    }

    /// Testers who turned the full keyboard experiment off chose the dictation bar and keep
    /// it; those who never touched it (unset) get the full keyboard.
    func testExplicitlyOffKeepsTheDictationBar() throws {
        let suite = "FullKeyboardSettingsTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        defaults.set(false, forKey: "experiments.fullKeyboard")
        XCTAssertFalse(FullKeyboardSettings(defaults: defaults).isEnabled)
        defaults.set(true, forKey: "experiments.fullKeyboard")
        XCTAssertTrue(FullKeyboardSettings(defaults: defaults).isEnabled)

        FullKeyboardSettings(defaults: defaults).isEnabled = false
        XCTAssertEqual(defaults.object(forKey: "experiments.fullKeyboard") as? Bool, false)
    }

    /// Without Full Access the keyboard reads no App Group: the full keyboard, which types.
    func testNoAppGroupReadsAsFullKeyboard() {
        XCTAssertTrue(FullKeyboardSettings(defaults: nil).isEnabled)
    }
}
