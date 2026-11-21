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

    func testPortraitIs270PointsAndLandscapeIs189Points() {
        for width in portraitWidths {
            XCTAssertEqual(KeyboardLayout(width: width).totalHeight, 270, "width \(width)")
        }
        XCTAssertEqual(KeyboardLayout.height(for: .portrait), 270)
        XCTAssertEqual(KeyboardLayout.height(for: .landscape), 189)
    }

    /// The strip is Apple's 44 pt predictive bar on every iPhone, either way up, and on the Duo;
    /// row 1 starts 52 pt down (Apple's keycaps start 53 pt down), or right under the strip on
    /// the Duo, where iOS draws its own band above the keyboard.
    func testStripIsPinnedInEveryOrientationAndPosture() {
        let postures: [KeyboardPosture] = [.phone, .cover, .unfoldedWide, .unfoldedTall]
        for posture in postures {
            for orientation in [KeyboardOrientation.portrait, .landscape] {
                let m = KeyboardMetrics.for(orientation, posture: posture)
                XCTAssertEqual(m.stripTop, 0, "\(posture) \(orientation)")
                XCTAssertEqual(m.stripHeight, 44, "\(posture) \(orientation)")
                XCTAssertEqual(m.stripArea, posture == .phone ? 52 : 44, "\(posture) \(orientation)")
            }
        }
        for layout in everyPortraitLayout() + landscapeLayouts() + duoLayouts() {
            XCTAssertEqual(layout.stripFrame.height, 44, describe(layout))
            let rowOneTop = layout.keys.filter { $0.row == 0 }.map(\.frame.minY).min()
            XCTAssertEqual(rowOneTop ?? -1, layout.metrics.stripArea, accuracy: 0.001, describe(layout))
        }
    }

    /// The iPhone Duo (iOS 27.2 simulator): Apple's row pitch for each screen, keycaps starting
    /// as far in from the screen's side as Apple's, and keys filling the width.
    func testDuoKeysMatchApplesRowsAndEdges() {
        let cases: [(KeyboardLayout, windowInset: CGFloat, pitch: CGFloat, appleEdge: CGFloat)] = [
            (KeyboardLayout(width: 452, posture: .cover, windowInset: 7), 7, 56, 8),
            (KeyboardLayout(width: 655, posture: .unfoldedTall, windowInset: 7), 7, 59.7, 19),
            (KeyboardLayout(width: 801, posture: .unfoldedWide, windowInset: 75), 75, 51.7, 86),
        ]
        for (layout, windowInset, pitch, appleEdge) in cases {
            let q = layout.key(.character("q"))!.frame
            let a = layout.key(.character("a"))!.frame
            let p = layout.key(.character("p"))!.frame
            XCTAssertEqual(a.minY - q.minY, pitch, accuracy: 0.001, describe(layout))
            XCTAssertEqual(windowInset + q.minX, appleEdge, accuracy: 0.5, describe(layout))
            XCTAssertEqual(p.maxX, layout.width - layout.metrics.side, accuracy: 0.001, describe(layout))
            XCTAssertEqual(layout.totalHeight, KeyboardLayout.height(for: .portrait, posture: layout.posture), describe(layout))
        }
        // Folded in landscape the Duo keeps the iPhone's landscape keys.
        let land = KeyboardLayout(width: 528, orientation: .landscape, posture: .cover, windowInset: 75)
        XCTAssertEqual(land.metrics.keyHeight, KeyboardMetrics.landscape.keyHeight)
        XCTAssertEqual(land.totalHeight, 181)
    }

    /// Apple's split keyboard on the Duo unfolded upright (iOS 27.2 simulator, keyboard at
    /// x = 75 pt on the 951 pt screen): each half's keys, G and V on both, and the keys'
    /// positions and widths.
    func testSplitMatchesApplesOnTheUnfoldedDuo() {
        let layout = KeyboardLayout(width: 801, posture: .unfoldedWide, windowInset: 75)
        XCTAssertTrue(layout.isSplit)
        func half(_ row: Int, right: Bool) -> [String] {
            layout.keys.filter { $0.row == row && ($0.frame.midX > layout.split!.middle) == right }
                .sorted { $0.frame.minX < $1.frame.minX }.map(\.label)
        }
        XCTAssertEqual(half(0, right: false), ["q", "w", "e", "r", "t"])
        XCTAssertEqual(half(0, right: true), ["y", "u", "i", "o", "p"])
        XCTAssertEqual(half(1, right: false), ["a", "s", "d", "f", "g"])
        XCTAssertEqual(half(1, right: true), ["g", "h", "j", "k", "l"])
        XCTAssertEqual(half(2, right: false), ["shift", "z", "x", "c", "v"])
        XCTAssertEqual(half(2, right: true), ["v", "b", "n", "m", "delete"])
        XCTAssertEqual(half(3, right: false), ["123", "space"])
        XCTAssertEqual(half(3, right: true), ["space", "return"])
        // Apple's keycaps, in screen points (keyboard at 75): q 85.3, t ends 359.3, y 591.3,
        // p ends 865.3, a 114, the right G 563, z 171, shift ends 148.3, delete 802.3.
        func x(_ label: String, right: Bool = false) -> CGRect {
            layout.keys.first { $0.label == label && ($0.frame.midX > layout.split!.middle) == right }!.frame.offsetBy(dx: 75, dy: 0)
        }
        XCTAssertEqual(x("q").minX, 85.3, accuracy: 1)
        XCTAssertEqual(x("t").maxX, 359.3, accuracy: 1)
        XCTAssertEqual(x("y", right: true).minX, 591.3, accuracy: 1)
        XCTAssertEqual(x("p", right: true).maxX, 865.3, accuracy: 1)
        XCTAssertEqual(x("a").minX, 114, accuracy: 1.5)
        XCTAssertEqual(x("g", right: true).minX, 563, accuracy: 1)
        XCTAssertEqual(x("z").minX, 171, accuracy: 1)
        XCTAssertEqual(x("shift").maxX, 148.3, accuracy: 1)
        XCTAssertEqual(x("delete", right: true).minX, 802.3, accuracy: 1)
        XCTAssertEqual(x("q").width, 46.3, accuracy: 0.01)
        // The strip sits over the right half, where Apple's predictions are.
        XCTAssertEqual(layout.stripFrame.minX + 75, 552.7, accuracy: 1)
    }

    /// Each half's targets tile it; the middle maps to no key; both of a doubled letter's keys
    /// read as one for Spatial autocorrect.
    func testSplitTargetsLeaveTheMiddleToNoKey() {
        var layouts: [KeyboardLayout] = []
        for layer in layers {
            for field in fields {
                layouts.append(KeyboardLayout(width: 801, layer: layer, field: field, showsGlobe: true, showsEmoji: true, posture: .unfoldedWide, windowInset: 75))
            }
        }
        for language in KeyboardLanguage.allCases {
            layouts.append(KeyboardLayout(width: 801, language: language, posture: .unfoldedWide, windowInset: 75))
        }
        for layout in layouts {
            let split = layout.split!
            let area = layout.touchAreaFrame
            for row in 0..<4 {
                let keys = layout.keys.filter { $0.row == row }
                let y = keys[0].hitFrame.midY
                XCTAssertNil(layout.hitTest(CGPoint(x: split.middle, y: y)), "\(describe(layout)) row \(row) middle")
                for key in keys {
                    XCTAssertTrue(key.hitFrame.contains(key.frame), "\(describe(layout)) \(key.label)")
                    XCTAssertLessThan(key.frame.maxX, layout.width + 0.001, describe(layout))
                }
                // No point of a half is without a key: its edge to just past its inner key.
                for x in stride(from: 0.5, to: layout.width, by: 3) {
                    let point = CGPoint(x: x, y: y)
                    let hits = keys.filter { $0.hitFrame.contains(point) }
                    XCTAssertLessThanOrEqual(hits.count, 1, "\(describe(layout)) \(point)")
                    let inLeft = x <= keys.filter { $0.frame.midX < split.middle }.map(\.hitFrame.maxX).max()!
                    let inRight = x >= keys.filter { $0.frame.midX > split.middle }.map(\.hitFrame.minX).min()!
                    if inLeft || inRight {
                        XCTAssertEqual(hits.count, 1, "\(describe(layout)) \(point)")
                    }
                }
            }
            XCTAssertEqual(area.maxY, layout.totalHeight, accuracy: 0.001)
            XCTAssertLessThan(layout.unsplitX(split.rightEdge), split.middle)
        }
        let english = KeyboardLayout(width: 801, posture: .unfoldedWide, windowInset: 75)
        let gs = english.keys.filter { $0.label == "g" }
        XCTAssertEqual(gs.count, 2)
        XCTAssertEqual(english.unsplitX(gs[0].frame.midX), english.unsplitX(gs[1].frame.midX), accuracy: 0.5)
        let keys = TypingDecoder.Keys(layout: english)!
        let g = keys.centres["g"]!
        XCTAssertEqual(g.x, english.unsplitX(gs[0].frame.midX), accuracy: 0.5)
    }

    func testSplitHalvesFollowEachLayout() {
        XCTAssertEqual(SplitGeometry.halves(Array("qwertyuiop").map(String.init)).left.joined(), "qwert")
        XCTAssertEqual(SplitGeometry.halves(Array("asdfghjkl").map(String.init)).right.joined(), "ghjkl")
        XCTAssertEqual(SplitGeometry.halves(Array("zxcvbnm").map(String.init)).left.joined(), "zxcv")
        // Odd rows of other layouts share their middle key too (Russian's 11-key first row).
        let russian = SplitGeometry.halves(KeyboardLanguage.russian.letterRows[0])
        XCTAssertEqual(russian.left.last, russian.right.first)
        // Only the Duo unfolded upright splits.
        XCTAssertFalse(KeyboardLayout(width: 655, posture: .unfoldedTall, windowInset: 7).isSplit)
        XCTAssertFalse(KeyboardLayout(width: 452, posture: .cover, windowInset: 7).isSplit)
        XCTAssertFalse(KeyboardLayout(width: 724, orientation: .landscape).isSplit)
    }

    func testPostureFollowsTheScreen() {
        XCTAssertEqual(KeyboardPosture.current(screenSize: CGSize(width: 402, height: 874), isFoldable: false), .phone)
        XCTAssertEqual(KeyboardPosture.current(screenSize: CGSize(width: 874, height: 402), isFoldable: false), .phone)
        XCTAssertEqual(KeyboardPosture.current(screenSize: CGSize(width: 466, height: 678), isFoldable: true), .cover)
        XCTAssertEqual(KeyboardPosture.current(screenSize: CGSize(width: 678, height: 466), isFoldable: true), .cover)
        XCTAssertEqual(KeyboardPosture.current(screenSize: CGSize(width: 951, height: 669), isFoldable: true), .unfoldedWide)
        XCTAssertEqual(KeyboardPosture.current(screenSize: CGSize(width: 669, height: 951), isFoldable: true), .unfoldedTall)
    }

    func testPortraitMetricsMatchAppleIOS26() {
        let proMax = KeyboardLayout(width: 440)
        XCTAssertEqual(proMax.letterWidth, (440 - 13 - 54) / 10, accuracy: 0.001)
        let q = proMax.key(.character("q"))!
        XCTAssertEqual(q.frame.minX, 6.5, accuracy: 0.001)
        XCTAssertEqual(q.frame.minY, 52, accuracy: 0.001)
        XCTAssertEqual(q.frame.height, 45)
        let shift = proMax.key(.shift)!
        XCTAssertEqual(shift.frame.width, proMax.letterWidth * 1.34, accuracy: 0.001)
    }

    func testHitTargetsTileTheKeyAreaWithNoDeadZonesOrOverlaps() {
        var generator = SystemRandomNumberGenerator()
        for layout in everyPortraitLayout() + landscapeLayouts() + duoLayouts() where !layout.isSplit {
            let area = layout.touchAreaFrame
            // Row 1 also takes the gap under the strip (or, on the Duo, the strip's bottom band,
            // under its pills), as Apple's keys reach above their keycaps; the Hex key in the
            // strip stops where it starts.
            XCTAssertEqual(area.minY, layout.keysAreaFrame.minY - layout.rowOneReach, describe(layout))
            XCTAssertGreaterThanOrEqual(area.minY, layout.stripFrame.maxY - KeyboardLayout.stripTouchBand, describe(layout))
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
        for layout in everyPortraitLayout() + landscapeLayouts() + duoLayouts() {
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

    func testBalloonsStayInsideTheKeyboard() {
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

    /// Settings › Keyboard › Hex key › Left: the key at the strip's leading edge, the strip
    /// after it to the keyboard's side, and every other key where Right puts it.
    func testHexKeyOnTheLeftSitsAtTheStripsLeadingEdge() {
        let right = KeyboardLayout(width: 402, hexPlacement: .stripTrailing)
        let layout = KeyboardLayout(width: 402, hexPlacement: .stripLeading)
        let hex = layout.key(.hex)!
        XCTAssertTrue(layout.hexOnLeading)
        XCTAssertFalse(right.hexOnLeading)
        XCTAssertEqual(hex.row, -1)
        XCTAssertEqual(hex.frame.minX, layout.metrics.side, accuracy: 0.001)
        XCTAssertEqual(hex.frame.size, right.key(.hex)!.frame.size)
        XCTAssertGreaterThanOrEqual(layout.stripFrame.minX, hex.frame.maxX)
        XCTAssertEqual(layout.stripFrame.maxX, 402, accuracy: 0.001)
        XCTAssertEqual(layout.stripFrame.width, right.stripFrame.width, accuracy: 0.001)
        XCTAssertEqual(layout.keys.filter { $0.row >= 0 }, right.keys.filter { $0.row >= 0 })
        XCTAssertNil(layout.hitTest(CGPoint(x: layout.stripFrame.maxX - 20, y: layout.stripFrame.midY)), "strip is not a key")
    }

    /// On the unfolded Duo's split keyboard, Left puts the strip and the Hex key over the left
    /// half, mirroring Right's right half.
    func testHexKeyOnTheLeftPutsTheSplitStripOverTheLeftHalf() {
        let right = KeyboardLayout(width: 801, posture: .unfoldedWide, windowInset: 75, hexPlacement: .stripTrailing)
        let layout = KeyboardLayout(width: 801, posture: .unfoldedWide, windowInset: 75, hexPlacement: .stripLeading)
        let split = layout.split!
        let hex = layout.key(.hex)!
        XCTAssertEqual(hex.frame.minX, layout.metrics.side, accuracy: 0.001)
        XCTAssertLessThan(layout.stripFrame.maxX, split.middle)
        XCTAssertEqual(layout.stripFrame.maxX, split.leftEdge + SplitGeometry.stripLead, accuracy: 0.001)
        XCTAssertGreaterThanOrEqual(layout.stripFrame.minX, hex.frame.maxX)
        XCTAssertGreaterThan(right.stripFrame.minX, split.middle)
        XCTAssertEqual(layout.stripFrame.width, right.stripFrame.width, accuracy: 0.5)
        XCTAssertEqual(layout.keys.filter { $0.row >= 0 }, right.keys.filter { $0.row >= 0 })
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
        XCTAssertEqual(layout.totalHeight, 189)
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

    /// Bilingual autocorrect pairs the current language with the first other Latin-script one
    /// that has a dictionary: none for Russian, nor for Serbian (no iOS dictionary), nor
    /// between two French layouts (one dictionary).
    func testBilingualAutocorrectPairsTwoLatinLanguagesWithDictionaries() {
        let available = ["en_US", "fr_FR", "fr_CA", "ru_RU", "de_DE"]
        func partner(_ chosen: [KeyboardLanguage], _ current: KeyboardLanguage) -> KeyboardLanguage? {
            KeyboardLanguageList.resolve(chosen: chosen, stored: current, automatic: .english).partner(available: available)
        }
        XCTAssertEqual(partner([.english, .french, .russian], .french), .english)
        XCTAssertEqual(partner([.english, .french, .russian], .english), .french)
        XCTAssertNil(partner([.english, .french, .russian], .russian))
        XCTAssertEqual(partner([.russian, .german, .english], .english), .german, "the first other Latin one")
        XCTAssertNil(partner([.french, .frenchCanadian], .frenchCanadian), "one language, two layouts")
        XCTAssertNil(partner([.serbianLatin, .english], .serbianLatin), "Serbian has no dictionary to check")
        XCTAssertNil(partner([.english, .serbianLatin], .english))
        XCTAssertNil(partner([.english], .english))
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

    /// The iPhone Duo's keyboard views (iOS 27.2 simulator): 452 pt of the 466 pt cover screen
    /// folded, 655 of 669 pt unfolded and turned, 801 of 951 pt unfolded upright, and 528 of
    /// 678 pt folded in landscape.
    private func duoLayouts() -> [KeyboardLayout] {
        var layouts: [KeyboardLayout] = []
        for (width, posture, inset) in [(452.0, KeyboardPosture.cover, 7.0), (655, .unfoldedTall, 7), (801, .unfoldedWide, 75)] {
            for layer in layers {
                for field in fields {
                    layouts.append(KeyboardLayout(width: width, layer: layer, field: field, showsGlobe: true, posture: posture, windowInset: inset))
                }
            }
            for language in KeyboardLanguage.allCases {
                layouts.append(KeyboardLayout(width: width, language: language, posture: posture, windowInset: inset))
            }
        }
        layouts.append(KeyboardLayout(width: 528, orientation: .landscape, posture: .cover, windowInset: 75))
        layouts.append(KeyboardLayout(width: 528, orientation: .landscape, showsGlobe: true, posture: .cover, windowInset: 75))
        return layouts
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
        "\(layout.width)pt \(layout.orientation) \(layout.layer) \(layout.field) globe=\(layout.showsGlobe) \(layout.language) \(layout.posture)"
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

    func testHexKeyIsOnTheRightByDefault() throws {
        let suite = "FullKeyboardSettingsTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = FullKeyboardSettings(defaults: defaults)

        XCTAssertFalse(settings.hexKeyOnLeft)
        XCTAssertEqual(settings.hexKeyPlacement, .stripTrailing)
        XCTAssertEqual(FullKeyboardSettings(defaults: nil).hexKeyPlacement, .stripTrailing, "without the App Group")

        settings.hexKeyOnLeft = true
        XCTAssertEqual(FullKeyboardSettings(defaults: defaults).hexKeyPlacement, .stripLeading)
        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.hexKeyLaunchArgument, "right"])
        XCTAssertFalse(settings.hexKeyOnLeft)
        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.hexKeyLaunchArgument, "left"])
        XCTAssertTrue(settings.hexKeyOnLeft)
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
