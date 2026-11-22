import CoreGraphics
import XCTest
@testable import HEX

final class KeyboardLayoutTests: XCTestCase {
    private let portraitWidths: [CGFloat] = [320, 375, 393, 402, 430, 440]
    private let layers: [KeyboardLayer] = [.letters, .numbers, .symbols]
    private let fields: [KeyboardFieldKind] = [.text, .url, .email]
    /// Every layout here is Apple's iOS 27 keyboard, read on 27.0 and 27.2 simulators, while CI's
    /// newest simulator runtime is iOS 26.5. Pin the version the expectations describe
    /// (`KeyboardLanguage.systemMajorVersion` exists for this; the before-27 Ukrainian test sets
    /// 26 itself) so the suite does not depend on the runner's runtime.
    private var runningSystemMajorVersion = 0

    override func setUp() {
        super.setUp()
        runningSystemMajorVersion = KeyboardLanguage.systemMajorVersion
        KeyboardLanguage.systemMajorVersion = 27
    }

    override func tearDown() {
        KeyboardLanguage.systemMajorVersion = runningSystemMajorVersion
        super.tearDown()
    }

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
            // Every other arrangement a language offers (Settings › Keyboard › Languages › Layout).
            for language in KeyboardLanguage.allCases {
                for letters in language.letterLayouts.dropFirst() {
                    for globe in [false, true] {
                        layouts.append(KeyboardLayout(width: width, showsGlobe: globe, language: language, letters: letters))
                    }
                }
            }
        }
        return layouts
    }

    func testPortraitHeightFollowsApplesKeyPlaneAndLandscapeIs204Points() {
        for width in portraitWidths {
            XCTAssertEqual(KeyboardLayout(width: width).totalHeight, 247, "width \(width)")
        }
        // 46 pt of strip, then 4 rows: 43 pt on a 54 pt pitch with row 4 4 pt short (17, 17 Pro),
        // 42 pt on 54 pt and 4 pt under (SE), 45 pt on 56 pt and 7 pt under (Air, Pro Max).
        XCTAssertEqual(KeyboardLayout.height(for: .portrait), 247)
        XCTAssertEqual(KeyboardLayout.height(for: .portrait, phoneSize: .homeButton), 254)
        XCTAssertEqual(KeyboardLayout.height(for: .portrait, phoneSize: .large), 266)
        XCTAssertEqual(KeyboardLayout.height(for: .landscape), 204)
        XCTAssertEqual(KeyboardPhoneSize.current(screenSize: CGSize(width: 375, height: 667)), .homeButton)
        XCTAssertEqual(KeyboardPhoneSize.current(screenSize: CGSize(width: 402, height: 874)), .regular)
        XCTAssertEqual(KeyboardPhoneSize.current(screenSize: CGSize(width: 390, height: 844)), .regular)
        XCTAssertEqual(KeyboardPhoneSize.current(screenSize: CGSize(width: 420, height: 912)), .large)
        XCTAssertEqual(KeyboardPhoneSize.current(screenSize: CGSize(width: 956, height: 440)), .large)
    }

    /// Where iOS gives the keyboard a shorter view than it asked for (Spotlight, iOS 27.3), every
    /// row fits inside it under the same strip; at full height nothing moves.
    func testRowsFitAViewShorterThanAskedFor() {
        for size in [KeyboardPhoneSize.regular, .large, .homeButton] {
            let full = KeyboardLayout(width: 375, showsEmoji: true, phoneSize: size)
            XCTAssertEqual(KeyboardLayout(width: 375, showsEmoji: true, phoneSize: size, availableHeight: full.totalHeight), full)
            XCTAssertEqual(KeyboardLayout(width: 375, showsEmoji: true, phoneSize: size, availableHeight: 0), full)
            let height = full.totalHeight - 20
            let short = KeyboardLayout(width: 375, showsEmoji: true, phoneSize: size, availableHeight: height)
            XCTAssertEqual(short.totalHeight, height, accuracy: 0.01, "\(size)")
            XCTAssertEqual(short.stripFrame, full.stripFrame)
            XCTAssertEqual(short.key(.character("q"))?.frame.minY, full.key(.character("q"))?.frame.minY)
            XCTAssertEqual(short.keys.count, full.keys.count)
            for key in short.keys where key.row >= 0 {
                XCTAssertLessThanOrEqual(key.frame.maxY, height + 0.01, "\(size) \(key.label)")
                XCTAssertLessThanOrEqual(key.hitFrame.maxY, height + 0.01, "\(size) \(key.label)")
                XCTAssertGreaterThan(key.frame.height, 30, "\(size) \(key.label)")
            }
            let space = short.key(.space)
            XCTAssertEqual(space?.hitFrame.maxY ?? 0, height, accuracy: 0.01)
        }
    }

    /// The strip is Apple's predictive bar from its suggestion pill's top down on every iPhone
    /// held upright: 38 pt (Apple's 44 pt bar less the 6 pt above its pill), then Apple's 8 pt to
    /// row 1, which starts 46 pt down. Its pills are 28 pt, on the words' line. Turned: 42 pt,
    /// row 1 at 50 pt, as Apple's. On the Duo folded and
    /// upright it is 32 pt and the same gap (row 1 at 40 pt, as Apple's); elsewhere on the Duo,
    /// Apple's whole 44 pt bar right on the keys.
    func testStripIsPinnedInEveryOrientationAndPosture() {
        let postures: [KeyboardPosture] = [.phone, .cover, .unfoldedWide, .unfoldedTall]
        for posture in postures {
            for orientation in [KeyboardOrientation.portrait, .landscape] {
                let m = KeyboardMetrics.for(orientation, posture: posture)
                XCTAssertEqual(m.stripTop, 0, "\(posture) \(orientation)")
                let cover = posture == .cover && orientation == .portrait
                let phone: (CGFloat, CGFloat, CGFloat) = orientation == .portrait ? (38, 46, 28) : (42, 50, 36)
                let expected = posture == .phone ? phone : cover ? (32, 40, 31) : (44, 44, 36)
                XCTAssertEqual(m.stripHeight, expected.0, "\(posture) \(orientation)")
                XCTAssertEqual(m.stripArea, expected.1, "\(posture) \(orientation)")
                XCTAssertEqual(m.stripPillHeight, expected.2, "\(posture) \(orientation)")
            }
        }
        for layout in everyPortraitLayout() + landscapeLayouts() + duoLayouts() {
            XCTAssertEqual(layout.stripFrame.height, layout.metrics.stripHeight, describe(layout))
            XCTAssertTrue([38, 32, 42, 44].contains(layout.stripFrame.height), describe(layout))
            let rowOneTop = layout.keys.filter { $0.row == 0 }.map(\.frame.minY).min()
            XCTAssertEqual(rowOneTop ?? -1, layout.metrics.stripArea, accuracy: 0.001, describe(layout))
        }
    }

    /// On an iPhone held upright nothing in the strip touches the keyboard's top edge, which is
    /// the panel's edge whenever iOS draws nothing above the keyboard (an iPhone 17 Pro's Safari
    /// form, iOS 27.2): the words are on Apple's line, 26 pt above row 1 and 20 pt down, the
    /// small chip and the pills are even around that line, and the pills start 6 pt down, as
    /// Apple's pill does below its panel's edge.
    func testPhoneStripKeepsItsContentOffTheTopEdgeAndOnApplesLine() {
        for size in [KeyboardPhoneSize.regular, .homeButton, .large] {
            let m = KeyboardMetrics.phonePortrait(size)
            XCTAssertEqual(m.stripHeight, KeyboardMetrics.stripBarHeight, "\(size)")
            XCTAssertEqual(m.stripArea - m.stripTextCenter, 26, "\(size): the words' line above row 1")
            XCTAssertEqual(m.stripTextCenter, 20, "\(size): the words' line below the top edge")
            XCTAssertEqual(m.stripChipHeight, 18, "\(size)")
            XCTAssertEqual(m.stripTextCenter - m.stripChipHeight / 2, 11, "\(size): above the chip")
            XCTAssertEqual(m.stripInset, 6, "\(size): above a pill")
            XCTAssertEqual(m.stripInset + m.stripPillHeight / 2, m.stripTextCenter, "\(size): pills on the line")
            XCTAssertLessThanOrEqual(m.stripInset + m.stripPillHeight, m.stripHeight, "\(size): pills inside the strip")
            let band = m.stripLineBand
            XCTAssertEqual(band.top + band.height / 2, m.stripTextCenter, "\(size)")
            XCTAssertGreaterThanOrEqual(band.top, 0, "\(size)")
            XCTAssertLessThanOrEqual(band.top + band.height, m.stripHeight, "\(size)")
        }
        let layout = KeyboardLayout(width: 402)
        let hex = layout.key(.hex)!
        XCTAssertEqual(hex.frame.minY, 6, accuracy: 0.001)
        XCTAssertEqual(hex.hitFrame.minY, 0, accuracy: 0.001, "the room above the mark still presses the key")
        // Where the words centre in the strip, the band is the whole strip and nothing moved.
        for m in [KeyboardMetrics.landscape, KeyboardMetrics.for(.portrait, posture: .cover), KeyboardMetrics.for(.portrait, posture: .unfoldedTall)] {
            XCTAssertEqual(m.stripTextCenter, m.stripHeight / 2)
            XCTAssertEqual(m.stripLineBand.top, 0)
            XCTAssertEqual(m.stripLineBand.height, m.stripHeight)
            XCTAssertEqual(m.stripInset, (m.stripHeight - m.stripPillHeight) / 2)
        }
    }

    /// The iPhone Duo: Apple's row pitch for each screen, keycaps starting as far in from the
    /// screen's side as Apple's, and keys filling the width (folded measured on a device, the
    /// iOS 27.2 simulator for the unfolded screens).
    func testDuoKeysMatchApplesRowsAndEdges() {
        let cases: [(KeyboardLayout, windowInset: CGFloat, pitch: CGFloat, appleEdge: CGFloat)] = [
            // Folded, Apple's keys start 18.5 pt in from the 466 pt cover screen: Hex's start so
            // far in too, whether iOS gives the view the whole screen or an inset platter.
            (KeyboardLayout(width: 452, posture: .cover, windowInset: 7), 7, 56, 18.5),
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
        XCTAssertEqual(land.totalHeight, 198)
    }

    /// Settings › Experiments › Join the keyboard when unfolded: off, the unfolded Duo splits as
    /// before; on, one joined keyboard with unfoldedTall's side inset and unfoldedWide's rows.
    func testJoiningTheUnfoldedDuoDrawsOneKeyboard() {
        let split = KeyboardLayout(width: 801, posture: .unfoldedWide, windowInset: 75)
        XCTAssertTrue(split.isSplit)
        XCTAssertEqual(split.metrics.side, 11, accuracy: 0.001)
        for layer in layers {
            for field in fields {
                let joined = KeyboardLayout(
                    width: 801, layer: layer, field: field, showsGlobe: true, showsEmoji: true,
                    posture: .unfoldedWide, windowInset: 75, joinsUnfoldedKeyboard: true
                )
                XCTAssertFalse(joined.isSplit, describe(joined))
                XCTAssertEqual(joined.metrics.side, 19, accuracy: 0.001, describe(joined))
                XCTAssertEqual(joined.metrics.keyHeight, 39, accuracy: 0.001, describe(joined))
                XCTAssertEqual(joined.metrics.rowGap, 12.7, accuracy: 0.001, describe(joined))
                XCTAssertEqual(joined.totalHeight, split.totalHeight, accuracy: 0.001, describe(joined))
                for row in 0..<4 {
                    let keys = joined.keys.filter { $0.row == row }.sorted { $0.hitFrame.minX < $1.hitFrame.minX }
                    XCTAssertEqual(keys.first?.hitFrame.minX ?? -1, 0, describe(joined))
                    XCTAssertEqual(keys.last?.hitFrame.maxX ?? -1, 801, describe(joined))
                    for (a, b) in zip(keys, keys.dropFirst()) {
                        XCTAssertEqual(a.hitFrame.maxX, b.hitFrame.minX, accuracy: 0.0001, describe(joined))
                    }
                }
                guard layer == .letters else { continue }
                XCTAssertEqual(joined.key(.character("q"))!.frame.minX, 19, accuracy: 0.001, describe(joined))
                XCTAssertEqual(joined.key(.character("p"))!.frame.maxX, 782, accuracy: 0.001, describe(joined))
                XCTAssertEqual(letters(joined, row: 0).joined(), "qwertyuiop", describe(joined))
            }
        }
        XCTAssertFalse(
            KeyboardLayout(width: 655, posture: .unfoldedTall, windowInset: 7, joinsUnfoldedKeyboard: true).isSplit,
            "only the unfolded wide posture ever splits"
        )
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

    /// Apple's keys beside Hex's in the same field (iOS 27.2 simulators), every edge within a
    /// device pixel: the numbers below are Apple's, read off screenshots.
    func testPortraitMetricsMatchApple() {
        let proMax = KeyboardLayout(width: 440, phoneSize: .large)
        XCTAssertEqual(proMax.letterWidth, (440 - 13 - 54) / 10, accuracy: 0.001)
        let q = proMax.key(.character("q"))!
        XCTAssertEqual(q.frame.minX, 6.5, accuracy: 0.001)
        XCTAssertEqual(q.frame.minY, 46, accuracy: 0.001)
        XCTAssertEqual(q.frame.height, 45)
        XCTAssertEqual(proMax.key(.character("a"))!.frame.minY - q.frame.minY, 56, accuracy: 0.001)
        XCTAssertEqual(proMax.totalHeight - proMax.key(.space)!.frame.maxY, 7, accuracy: 0.001)
        // Shift 6.67 to 56.67 pt and delete 383.33 to 433.67 pt on Apple's Pro Max.
        XCTAssertEqual(proMax.key(.shift)!.frame.maxX, 56.67, accuracy: 0.34)
        XCTAssertEqual(proMax.key(.delete)!.frame.minX, 383.33, accuracy: 0.34)

        // The 17 Pro: 43 pt keys on a 54 pt pitch, row 4 ending on the view's bottom edge, 4 pt
        // short of Apple's (a third-party keyboard's view ends there); shift to 52, delete from 350.
        let pro = KeyboardLayout(width: 402)
        let proQ = pro.key(.character("q"))!
        XCTAssertEqual(proQ.frame.height, 43)
        XCTAssertEqual(pro.key(.character("a"))!.frame.minY - proQ.frame.minY, 54, accuracy: 0.001)
        XCTAssertEqual(pro.key(.space)!.frame.minY - proQ.frame.minY, 3 * 54, accuracy: 0.001)
        XCTAssertEqual(pro.key(.space)!.frame.height, 39)
        XCTAssertEqual(pro.key(.space)!.frame.maxY, pro.totalHeight, accuracy: 0.001)
        XCTAssertEqual(pro.key(.shift)!.frame.maxX, 52, accuracy: 0.34)
        XCTAssertEqual(pro.key(.delete)!.frame.minX, 350, accuracy: 0.34)

        // The SE: 6.5 pt in from each side, 42 pt keys on a 54 pt pitch, 4 pt under row 4.
        let se = KeyboardLayout(width: 375, showsGlobe: true, phoneSize: .homeButton)
        XCTAssertEqual(se.key(.character("q"))!.frame.minX, 6.5, accuracy: 0.001)
        XCTAssertEqual(se.key(.character("p"))!.frame.maxX, 368.5, accuracy: 0.001)
        XCTAssertEqual(se.key(.character("q"))!.frame.height, 42)
        XCTAssertEqual(se.totalHeight - se.key(.space)!.frame.maxY, 4, accuracy: 0.001)
        XCTAssertEqual(se.key(.shift)!.frame.maxX, 48.5, accuracy: 0.26)
    }

    /// Apple's emoji key (iPhone 17 Pro, mapped by tapping): its keycap runs 56 to 99.3 pt, but
    /// only 62 to 94 pt types it, from 7.5 pt below its top; 54 to 62 and 94 to 102 type
    /// nothing, so a tap meant for 123 or space that lands near it doesn't switch keyboards.
    func testEmojiKeyTakesOnlyTheMiddleOfItsKeycapAsApplesDoes() {
        let layout = KeyboardLayout(width: 402, showsEmoji: true)
        let emoji = layout.key(.emoji)!
        XCTAssertEqual(emoji.frame.minX, 56, accuracy: 0.34)
        XCTAssertEqual(emoji.frame.maxX, 99.33, accuracy: 0.34)
        XCTAssertEqual(emoji.hitFrame.minX, emoji.frame.minX + 6, accuracy: 0.001)
        XCTAssertEqual(emoji.hitFrame.maxX, emoji.frame.maxX - 6, accuracy: 0.001)
        XCTAssertEqual(emoji.hitFrame.minY, emoji.frame.minY + 7.5, accuracy: 0.001)
        let y = emoji.frame.midY + 5
        XCTAssertEqual(layout.hitTest(CGPoint(x: emoji.frame.midX, y: y))?.kind, .emoji)
        XCTAssertNil(layout.hitTest(CGPoint(x: emoji.frame.minX + 3, y: y)), "the emoji key's left margin types nothing")
        XCTAssertNil(layout.hitTest(CGPoint(x: emoji.frame.maxX - 3, y: y)), "nor its right margin")
        XCTAssertNil(layout.hitTest(CGPoint(x: emoji.frame.midX, y: emoji.frame.minY + 3)), "nor its top")
        XCTAssertEqual(layout.hitTest(CGPoint(x: emoji.frame.minX - 4, y: y))?.kind, .layerSwitch(.numbers))
        XCTAssertEqual(layout.hitTest(CGPoint(x: emoji.frame.maxX + 4, y: y))?.kind, .space)
    }

    /// Apple's portrait bottom row stands on row 3's columns: 123 (and emoji or the globe) end
    /// at z's right edge, space runs from x to n, return from m to the edge. Without an emoji key
    /// Apple's 123 fills the first block, which is Hex's usual row.
    func testBottomRowStandsOnRowThreesColumns() {
        for width in portraitWidths {
            for (globe, emoji) in [(false, false), (true, false), (false, true)] {
                for layer in [KeyboardLayer.letters, .numbers, .symbols] {
                    let layout = KeyboardLayout(width: width, layer: layer, showsGlobe: globe, showsEmoji: emoji)
                    let letters = KeyboardLayout(width: width)
                    let label = "\(width) globe \(globe) emoji \(emoji) \(layer)"
                    let z = letters.key(.character("z"))!.frame, x = letters.key(.character("x"))!.frame
                    let n = letters.key(.character("n"))!.frame, m = letters.key(.character("m"))!.frame
                    let space = layout.key(.space)!.frame, ret = layout.key(.returnKey)!.frame
                    XCTAssertEqual(space.minX, x.minX, accuracy: 0.001, label)
                    XCTAssertEqual(space.maxX, n.maxX, accuracy: 0.001, label)
                    XCTAssertEqual(ret.minX, m.minX, accuracy: 0.001, label)
                    XCTAssertEqual(ret.maxX, width - layout.metrics.side, accuracy: 0.001, label)
                    let page = layout.key(.layerSwitch(layer == .letters ? .numbers : .letters))!.frame
                    XCTAssertEqual(page.minX, layout.metrics.side, accuracy: 0.001, label)
                    let blockEnd = [KeyKind.globe, .emoji].compactMap { layout.key($0)?.frame.maxX }.max() ?? page.maxX
                    XCTAssertEqual(blockEnd, z.maxX, accuracy: 0.001, label)
                }
            }
        }
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
            // Apple's emoji key on row 4 takes only the middle of its keycap; the rest of its
            // slot types nothing (`testEmojiKeyTakesOnlyTheMiddleOfItsKeycapAsApplesDoes`).
            let emoji = layout.key(.emoji).flatMap { $0.row == 3 && layout.metrics.keyHeight >= 40 ? $0 : nil }
            var emojiSlot: CGRect?
            var rowTop = area.minY
            for row in 0..<4 {
                var keys = layout.keys.filter { $0.row == row }.sorted { $0.hitFrame.minX < $1.hitFrame.minX }
                if row == 3, let emoji, let i = keys.firstIndex(where: { $0.kind == .emoji }) {
                    let slot = CGRect(x: keys[i - 1].hitFrame.maxX, y: rowTop, width: keys[i + 1].hitFrame.minX - keys[i - 1].hitFrame.maxX, height: area.maxY - rowTop)
                    XCTAssertTrue(slot.contains(emoji.hitFrame), describe(layout))
                    emojiSlot = slot
                    keys.remove(at: i)
                    var targets = keys.map(\.hitFrame)
                    targets.insert(slot, at: i)
                    for target in targets { XCTAssertEqual(target.minY, rowTop, accuracy: 0.0001, describe(layout)) }
                    for (a, b) in zip(targets, targets.dropFirst()) { XCTAssertEqual(a.maxX, b.minX, accuracy: 0.0001, describe(layout)) }
                    rowTop = targets[0].maxY
                    continue
                }
                let targets = keys.map(\.hitFrame)
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
                let inEmojiMargin = emojiSlot?.contains(point) == true && emoji?.hitFrame.contains(point) == false
                XCTAssertEqual(hits.count, inEmojiMargin ? 0 : 1, "\(describe(layout)) at \(point): \(hits.map(\.label))")
            }
            XCTAssertNil(layout.hitTest(CGPoint(x: 20, y: layout.stripFrame.midY)), "strip is not a key")
        }
    }

    func testKeycapsSitInsideTheirTargetsAndNeverOverlap() {
        for layout in everyPortraitLayout() + landscapeLayouts() + duoLayouts() {
            let bounds = CGRect(x: 0, y: 0, width: layout.width, height: layout.totalHeight).insetBy(dx: -0.001, dy: -0.001)
            for key in layout.keys {
                XCTAssertTrue(key.hitFrame.insetBy(dx: -0.001, dy: -0.001).contains(key.frame), "\(describe(layout)) \(key.label)")
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
        XCTAssertEqual(hex.frame.midY, layout.metrics.stripTextCenter, accuracy: 0.001)
        XCTAssertLessThanOrEqual(layout.stripFrame.maxX, hex.frame.minX)
    }

    /// Apple's slots are thirds of the keyboard (dividers end at 134 and 268 pt on a 402 pt
    /// iPhone); the Hex key takes its room from the slot beside it, on either side.
    func testSuggestionSlotsAreThirdsOfTheBarLessTheHexKey() {
        let right = KeyboardLayout(width: 402)
        let slots = right.stripSlotFrames
        for (x, expected) in zip(slots.map(\.minX), [0, 134, 268] as [CGFloat]) { XCTAssertEqual(x, expected, accuracy: 0.001) }
        XCTAssertEqual(slots[0].maxX, 134, accuracy: 0.001)
        XCTAssertEqual(slots[1].maxX, 268, accuracy: 0.001)
        XCTAssertEqual(slots[2].maxX, right.stripFrame.maxX, accuracy: 0.001)
        XCTAssertTrue(slots.allSatisfy { $0.minY == right.stripFrame.minY && $0.height == right.stripFrame.height })

        let left = KeyboardLayout(width: 402, hexPlacement: .stripLeading)
        let mirrored = left.stripSlotFrames
        XCTAssertEqual(mirrored[0].minX, left.stripFrame.minX, accuracy: 0.001)
        for (x, expected) in zip(mirrored.map(\.maxX), [134, 268, 402] as [CGFloat]) { XCTAssertEqual(x, expected, accuracy: 0.001) }

        for layout in everyPortraitLayout() + landscapeLayouts() + duoLayouts() {
            let frames = layout.stripSlotFrames
            XCTAssertEqual(frames.first!.minX, layout.stripFrame.minX, accuracy: 0.001, describe(layout))
            XCTAssertEqual(frames.last!.maxX, layout.stripFrame.maxX, accuracy: 0.001, describe(layout))
            XCTAssertTrue(frames.allSatisfy { $0.width > 40 }, describe(layout))
        }
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
        XCTAssertEqual(layout.totalHeight, 204)
        XCTAssertEqual(layout.key(.character("q"))!.frame.minX, 64, accuracy: 0.001)
        XCTAssertEqual(layout.key(.character("q"))!.hitFrame.minX, 0)
    }

    /// Apple's keys turned (iPhone 17 Pro, 874 pt, keys from 79 to 795 pt; iOS 27.2): 32 pt on a
    /// 40 pt pitch, 2 pt under row 4, and row 4 on row 1's columns: 123 on q, emoji on w, space
    /// from e to i, return from o. On the SE they start 72 pt in, 47 pt wide.
    func testLandscapeMatchesApplesKeys() {
        let pro = KeyboardLayout(width: 874, orientation: .landscape, showsEmoji: true, horizontalInset: 75)
        let col = { (c: String) in pro.key(.character(c))!.frame }
        XCTAssertEqual(col("q").minX, 79, accuracy: 0.34)
        XCTAssertEqual(col("p").maxX, 795, accuracy: 0.34)
        XCTAssertEqual(col("q").height, 32)
        XCTAssertEqual(col("a").minY - col("q").minY, 40, accuracy: 0.001)
        XCTAssertEqual(pro.totalHeight - pro.key(.space)!.frame.maxY, 2, accuracy: 0.001)
        XCTAssertEqual(pro.key(.layerSwitch(.numbers))!.frame.minX, col("q").minX, accuracy: 0.001)
        XCTAssertEqual(pro.key(.layerSwitch(.numbers))!.frame.maxX, col("q").maxX, accuracy: 0.001)
        XCTAssertEqual(pro.key(.emoji)!.frame.minX, col("w").minX, accuracy: 0.001)
        XCTAssertEqual(pro.key(.space)!.frame.minX, col("e").minX, accuracy: 0.001)
        XCTAssertEqual(pro.key(.space)!.frame.maxX, col("i").maxX, accuracy: 0.001)
        XCTAssertEqual(pro.key(.returnKey)!.frame.minX, col("o").minX, accuracy: 0.001)
        XCTAssertEqual(pro.key(.returnKey)!.frame.maxX, col("p").maxX, accuracy: 0.001)

        let se = KeyboardLayout(width: 667, orientation: .landscape, phoneSize: .homeButton)
        XCTAssertEqual(se.key(.character("q"))!.frame.minX, 72, accuracy: 0.5)
        XCTAssertEqual(se.key(.character("p"))!.frame.maxX, 595, accuracy: 0.5)
        XCTAssertEqual(se.key(.character("q"))!.frame.width, 47, accuracy: 0.2)
    }

    // MARK: - Languages

    func testLetterRowsMatchApplesLayouts() {
        let rows: [KeyboardLanguage: [String]] = [
            .english: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .norwegian: ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"],
            .spanish: ["qwertyuiop", "asdfghjklñ", "zxcvbnm"],
            .german: ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"],
            .italian: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .french: ["azertyuiop", "qsdfghjklm", "wxcvbn´"],
            .frenchCanadian: ["qwertyuiop", "asdfghjkl´", "zxcvbnm"],
            .frenchSwiss: ["qwertzuiop", "asdfghjkl´", "yxcvbnm"],
            .portuguese: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .polish: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .romanian: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .russian: ["йцукенгшщзх", "фывапролджэ", "ячсмитьбю"],
            .ukrainian: ["йцукенгшщзхї", "фівапролджєʼ", "ячсмитьбюґ"],
            .greek: ["ερτυθιοπ´", "ασδφγηξκλ", "ζχψωβνμ"],
            .serbianLatin: ["qwertzuiop", "asdfghjkl", "yxcvbnm"],
            .czech: ["qwertzuiop", "asdfghjkl´", "yxcvbnmˇ"],
            .hungarian: ["qwertzuiop", "asdfghjkl", "yxcvbnm"],
            .korean: ["ㅂㅈㄷㄱㅅㅛㅕㅑㅐㅔ", "ㅁㄴㅇㄹㅎㅗㅓㅏㅣ", "ㅋㅌㅊㅍㅠㅜㅡ"],
        ]
        for (language, expected) in rows {
            let layout = KeyboardLayout(width: 402, language: language)
            for row in 0..<3 {
                XCTAssertEqual(letters(layout, row: row).joined(), expected[row], "\(language) row \(row + 1)")
            }
        }
    }

    /// Settings › General › Keyboard › Keyboards › Add New Keyboard's Layout list for each
    /// language (iOS 27.0 simulator, black box), Hex's own layout first: French's own AZERTY is
    /// Apple's "AZERTY – French" (`testFrenchIsApplesAzerty`), its other layouts the plain ones,
    /// and Norwegian, Russian and Ukrainian keep one layout here.
    func testEachLanguageOffersApplesLayoutsWithItsOwnFirst() {
        let offered: [KeyboardLanguage: [KeyboardLetterLayout]] = [
            .english: [.qwerty, .azerty, .qwertz, .dvorak],
            .german: [.own, .qwertz, .qwerty, .azerty],
            .spanish: [.own, .qwerty, .azerty, .qwertz],
            .french: [.azerty, .qwerty, .qwertz],
            .frenchCanadian: [.qwerty, .azerty, .qwertz],
            .frenchSwiss: [.qwertz, .azerty, .qwerty],
            .italian: [.qwerty, .azerty, .qwertz],
            .portuguese: [.qwerty, .azerty, .qwertz],
            .polish: [.qwerty, .qwertz, .azerty],
            .romanian: [.qwerty, .qwertz, .azerty],
            .serbianLatin: [.qwertz, .qwerty, .azerty],
            .norwegian: [.own],
            .russian: [.own],
            .ukrainian: [.own],
            .greek: [.own],
            .czech: [.own, .qwerty],
            .hungarian: [.own, .qwerty],
            .korean: [.own],
        ]
        for language in KeyboardLanguage.allCases {
            XCTAssertEqual(language.letterLayouts, offered[language], "\(language)")
            XCTAssertEqual(language.letterRows(language.defaultLetterLayout), language.letterRows, "\(language): today's keys by default")
            XCTAssertEqual(KeyboardLayout(width: 402, language: language).letters, language.defaultLetterLayout)
        }
        XCTAssertEqual(KeyboardLayout(width: 402, language: .norwegian, letters: .qwertz).letters, .own, "not offered: its own")
        XCTAssertEqual(KeyboardLanguage.german.layoutName(.own), "QWERTZ – Deutsch")
        XCTAssertEqual(KeyboardLanguage.spanish.layoutName(.own), "QWERTY – Español")
        XCTAssertEqual(KeyboardLanguage.english.layoutName(.dvorak), "Dvorak")
        XCTAssertEqual(KeyboardLanguage.czech.layoutName(.own), "QWERTZ – Čeština")
        XCTAssertEqual(KeyboardLanguage.czech.layoutName(.qwerty), "QWERTY – Čeština")
        XCTAssertEqual(KeyboardLanguage.hungarian.layoutName(.own), "QWERTZ – Magyar")
        XCTAssertEqual(KeyboardLanguage.hungarian.layoutName(.qwerty), "QWERTY – Magyar")
        XCTAssertEqual(KeyboardLanguage.polish.layoutName(.qwerty), "QWERTY")
    }

    /// The plain layouts are the same 26 letters in every language (iOS 27.0 simulator: English,
    /// German, French, Spanish, Italian, Portuguese, Polish, Serbian and Romanian alike), but
    /// Czech, whose QWERTY is its own with Z and Y swapped (`testCzechsRowsAndPagesMatchApples`).
    func testPlainLayoutsAreApplesInEveryLanguage() {
        let plain: [KeyboardLetterLayout: [String]] = [
            .qwerty: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            .qwertz: ["qwertzuiop", "asdfghjkl", "yxcvbnm"],
            .azerty: ["azertyuiop", "qsdfghjklm", "wxcvbn"],
        ]
        for language in KeyboardLanguage.allCases where language != .czech {
            // Each French keyboard's own layout is Apple's "– French" one, with the apostrophe key.
            for letters in language.letterLayouts where letters != .own && letters != .dvorak && !(language.isFrench && letters == language.defaultLetterLayout) {
                let layout = KeyboardLayout(width: 402, language: language, letters: letters)
                for row in 0..<3 {
                    XCTAssertEqual(self.letters(layout, row: row).joined(), plain[letters]?[row], "\(language) \(letters) row \(row + 1)")
                }
            }
        }
    }

    /// The TestFlight request: English on QWERTZ. Y and Z swap places; nothing else moves, and
    /// the keys keep English's holds (no ä ö ü keys; ü only on a held u, as on English QWERTY).
    @MainActor
    func testEnglishOnQWERTZSwapsYAndZAndKeepsEnglishsHolds() throws {
        let qwerty = KeyboardLayout(width: 402, language: .english)
        let qwertz = KeyboardLayout(width: 402, language: .english, letters: .qwertz)
        let z = try XCTUnwrap(qwertz.key(.character("z")))
        let y = try XCTUnwrap(qwertz.key(.character("y")))
        XCTAssertEqual(z.frame, qwerty.key(.character("y"))?.frame)
        XCTAssertEqual(y.frame, qwerty.key(.character("z"))?.frame)
        XCTAssertEqual(z.row, 0)
        XCTAssertEqual(y.row, 2)
        for letter in "abcdefghijklmnopqrstuvwx" {
            XCTAssertEqual(qwertz.key(.character(String(letter)))?.frame, qwerty.key(.character(String(letter)))?.frame, "\(letter)")
        }
        XCTAssertEqual(qwertz.keys.filter { $0.character == nil }.map(\.frame), qwerty.keys.filter { $0.character == nil }.map(\.frame))
        for letter in "aeiouyzsnc" {
            XCTAssertEqual(
                AccentCatalog.options(for: String(letter), uppercase: false, language: .english, letters: .qwertz),
                AccentCatalog.options(for: String(letter), uppercase: false, language: .english),
                "\(letter)"
            )
        }
        XCTAssertNil(qwertz.key(.character("ü")))
        // Its neighbours price a slip: T and Z touch on QWERTZ, not on QWERTY.
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("t", "z", language: .english, letters: .qwertz))
        XCTAssertFalse(AutocorrectEngine.areAdjacentKeys("t", "z", language: .english))
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("y", "x", language: .english, letters: .qwertz))
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("t", "y", language: .english))
        XCTAssertFalse(AutocorrectEngine.areAdjacentKeys("t", "y", language: .english, letters: .qwertz))
    }

    /// A plain layout has no keys for German's ä ö ü or Spanish's ñ; Apple's keyboard then holds
    /// each right after its base letter (iOS 27.0 simulator, German QWERTZ and QWERTY, Spanish
    /// QWERTY), every other hold staying the language's.
    func testPlainLayoutsHoldTheLanguagesOwnLetters() {
        func held(_ letter: String, _ language: KeyboardLanguage, _ letters: KeyboardLetterLayout) -> [String]? {
            AccentCatalog.options(for: letter, uppercase: false, language: language, letters: letters)
        }
        for letters in [KeyboardLetterLayout.qwertz, .qwerty, .azerty] {
            XCTAssertEqual(held("a", .german, letters), ["a", "ä", "à", "á", "â", "æ", "ã", "å", "ā"], "\(letters)")
            XCTAssertEqual(held("o", .german, letters), ["o", "ö", "ô", "ò", "ó", "õ", "œ", "ø", "ō"])
            XCTAssertEqual(held("u", .german, letters), ["u", "ü", "û", "ù", "ú", "ū"])
            XCTAssertEqual(held("s", .german, letters), ["s", "ß", "ś", "š"])
            XCTAssertEqual(held("y", .german, letters), ["y", "ÿ"])
            XCTAssertNil(KeyboardLayout(width: 402, language: .german, letters: letters).key(.character("ä")))
        }
        XCTAssertEqual(held("A", .german, .qwertz), ["a", "ä", "à", "á", "â", "æ", "ã", "å", "ā"])
        XCTAssertEqual(AccentCatalog.options(for: "u", uppercase: true, language: .german, letters: .qwertz)?.prefix(2), ["U", "Ü"])
        XCTAssertEqual(held("u", .german, .own), ["u", "û", "ù", "ú", "ū"], "its own layout has a ü key")
        XCTAssertEqual(held("n", .spanish, .qwerty), ["n", "ñ", "ń"])
        XCTAssertEqual(held("n", .spanish, .own), ["n", "ń"])
        XCTAssertEqual(held("e", .french, .qwertz), held("e", .french, .azerty), "French's letters all have keys")
    }

    /// Apple's iPhone Dvorak (iOS 27.0 simulator, 402 pt): row 1 is seven letters and delete
    /// (a pitch and a half wide) at the right, row 2 ten letters from the left edge, row 3 shift
    /// and nine letters edge to edge, on a 38.3 pt pitch; the bottom row is unchanged.
    func testDvorakMatchesApples() throws {
        let layout = KeyboardLayout(width: 402, language: .english, letters: .dvorak)
        let qwerty = KeyboardLayout(width: 402, language: .english)
        XCTAssertEqual(letters(layout, row: 0).joined(), "pyfgcrl")
        XCTAssertEqual(letters(layout, row: 1).joined(), "aoeuidhtns")
        XCTAssertEqual(letters(layout, row: 2).joined(), "qjkxbmwvz")
        let delete = try XCTUnwrap(layout.key(.delete))
        let shift = try XCTUnwrap(layout.key(.shift))
        XCTAssertEqual(delete.row, 0)
        XCTAssertEqual(shift.row, 2)
        let m = layout.metrics
        let a = try XCTUnwrap(layout.key(.character("a"))).frame
        let o = try XCTUnwrap(layout.key(.character("o"))).frame
        let p = try XCTUnwrap(layout.key(.character("p"))).frame
        let l = try XCTUnwrap(layout.key(.character("l"))).frame
        let q = try XCTUnwrap(layout.key(.character("q"))).frame
        let z = try XCTUnwrap(layout.key(.character("z"))).frame
        let pitch = o.minX - a.minX
        XCTAssertEqual(pitch, 38.3, accuracy: 0.3)
        XCTAssertEqual(a.minX, m.side, accuracy: 0.01)
        XCTAssertEqual(z.maxX, 402 - m.side, accuracy: 0.01)
        XCTAssertEqual(delete.frame.maxX, 402 - m.side, accuracy: 0.01)
        XCTAssertEqual(delete.frame.minX - l.maxX, m.gap, accuracy: 0.01)
        XCTAssertEqual(delete.frame.width, 1.5 * pitch - m.gap, accuracy: 0.01)
        XCTAssertEqual(p.minX, 77, accuracy: 2, "P where Apple's starts")
        XCTAssertEqual(q.minX, shift.frame.maxX + m.gap, accuracy: 0.01)
        XCTAssertEqual(layout.characterWidth, a.width, accuracy: 0.01)
        XCTAssertEqual(layout.keys.filter { $0.row == 3 }.map(\.frame), qwerty.keys.filter { $0.row == 3 }.map(\.frame))
        XCTAssertEqual(layout.hitTest(CGPoint(x: 10, y: p.midY))?.kind, .character("p"), "the space left of P is P's")
    }

    /// Settings › Keyboard › Languages › Layout is kept per language in the App Group; a language
    /// never set, or set to one it doesn't offer, types on its own, as without the App Group.
    func testTheLayoutIsKeptPerLanguageWithItsOwnAsTheFallback() {
        let suite = "KeyboardLayoutTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = LanguageSettings(defaults: defaults)
        XCTAssertEqual(settings.letterLayout(for: .english), .qwerty)
        XCTAssertEqual(settings.letterLayout(for: .german), .own)
        settings.setLetterLayout(.qwertz, for: .english)
        settings.setLetterLayout(.qwertz, for: .german)
        XCTAssertEqual(settings.letterLayout(for: .english), .qwertz)
        XCTAssertEqual(settings.letterLayout(for: .german), .qwertz)
        XCTAssertEqual(settings.letterLayout(for: .french), .azerty, "each language its own")
        settings.setLetterLayout(.dvorak, for: .german)
        XCTAssertEqual(settings.letterLayout(for: .german), .own, "not offered: its own")
        defaults.set(["en": "colemak"], forKey: "settings.keyboardLanguages.layouts")
        XCTAssertEqual(settings.letterLayout(for: .english), .qwerty, "unknown: its own")
        XCTAssertEqual(LanguageSettings(defaults: nil).letterLayout(for: .english), .qwerty, "no App Group")
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

    /// The standard Ukrainian ЙЦУКЕН in the rows the system Ukrainian keyboard shows on an
    /// iPhone 17 Pro (iOS 27.2 simulator, 402 pt, read through accessibility): rows 1 and 2
    /// are twelve keys edge to edge (Й at 5 pt, Ї at 367 pt, a 33 pt pitch), row 3's ten letters
    /// sit between a shift and delete one letter wide, and the bottom row and the numbers and
    /// symbols pages are English's.
    func testUkrainiansTwelveKeyRowsMatchApples() {
        let cases: [(CGFloat, KeyboardOrientation, CGFloat)] = [
            (320, .portrait, 0), (375, .portrait, 0), (402, .portrait, 0), (440, .portrait, 0),
            (667, .landscape, 0), (956, .landscape, 60),
        ]
        for (width, orientation, inset) in cases {
            let layout = KeyboardLayout(width: width, orientation: orientation, horizontalInset: inset, language: .ukrainian)
            let english = KeyboardLayout(width: width, orientation: orientation, horizontalInset: inset)
            let m = layout.metrics
            let label = "\(width)pt \(orientation)"
            XCTAssertEqual(layout.characterWidth, (width - 2 * inset - 2 * m.side - 11 * m.gap) / 12, accuracy: 0.001, label)
            for row in 0..<2 {
                let keys = layout.keys.filter { $0.row == row }.sorted { $0.frame.minX < $1.frame.minX }
                XCTAssertEqual(keys.count, 12, label)
                XCTAssertEqual(keys.first!.frame.minX, inset + m.side, accuracy: 0.001, label)
                XCTAssertEqual(keys.last!.frame.maxX, width - inset - m.side, accuracy: 0.001, label)
            }
            let row3 = layout.keys.filter { $0.row == 2 && $0.character != nil }.sorted { $0.frame.minX < $1.frame.minX }
            XCTAssertEqual(row3.count, 10, label)
            let shift = layout.key(.shift)!.frame
            let delete = layout.key(.delete)!.frame
            XCTAssertEqual(shift.width, layout.characterWidth, accuracy: 0.001, label)
            XCTAssertEqual(delete.width, layout.characterWidth, accuracy: 0.001, label)
            XCTAssertEqual(row3.first!.frame.minX - shift.maxX, m.gap, accuracy: 0.001, label)
            XCTAssertEqual(delete.minX - row3.last!.frame.maxX, m.gap, accuracy: 0.001, label)
            XCTAssertEqual(
                layout.keys.filter { $0.row == 3 }.map(\.frame),
                english.keys.filter { $0.row == 3 }.map(\.frame),
                label
            )
        }
        let pro = KeyboardLayout(width: 402, language: .ukrainian)
        XCTAssertEqual(pro.key(.character("й"))!.frame.minX, 5, accuracy: 2)
        XCTAssertEqual(pro.key(.character("ї"))!.frame.minX, 367, accuracy: 2)
        for layer in [KeyboardLayer.numbers, .symbols] {
            let ukrainian = KeyboardLayout(width: 402, layer: layer, language: .ukrainian).keys
            let english = KeyboardLayout(width: 402, layer: layer).keys
            XCTAssertEqual(ukrainian.map(\.frame), english.map(\.frame))
            XCTAssertEqual(ukrainian.map(\.character), english.map { $0.character == "'" ? "\u{02BC}" : $0.character }, "ʼ where English has '")
        }
    }

    /// Before iOS 27 the system Ukrainian keyboard is Russian's rows with і for ы and є for э
    /// (iOS 26.2, 26.4 and 26.5 simulators, iPhone 12 and 17 Pro, read through accessibility):
    /// 11, 11 and 9 keys, every letter the two share on Russian's key, shift and delete one
    /// letter wide, ї on a held і, ґ on a held г and ʼ only on the 123 and #+= pages. From iOS 27
    /// it is the twelve-key rows (`testUkrainiansTwelveKeyRowsMatchApples`).
    /// Before iOS 27 Apple's iPhone keyboard draws other rows (iOS 26.5 simulators, beside Hex in
    /// the same field, black box): upright on an 844 to 874 pt iPhone every row is 7 pt higher
    /// with row 4 whole (254 pt), on a 900 pt or taller one 2 pt lower (264 pt), and turned its
    /// keycaps are 27.33 pt on a 35.33 pt pitch (187.33 pt). The keys' columns, the strip, the SE,
    /// the Duo and the iPad are the same as from iOS 27. iOS 18 (unmeasured) keeps the iOS 27 rows,
    /// so any version but 26 draws the same keys.
    func testIOS26RowsAreApplesIOS26Rows() {
        let running = KeyboardLanguage.systemMajorVersion
        defer { KeyboardLanguage.systemMajorVersion = running }
        func layouts() -> [String: KeyboardLayout] {
            [
                "17 Pro": KeyboardLayout(width: 402),
                "12": KeyboardLayout(width: 390),
                "Pro Max": KeyboardLayout(width: 440, phoneSize: .large),
                "SE": KeyboardLayout(width: 375, phoneSize: .homeButton),
                "17 Pro turned": KeyboardLayout(width: 874, orientation: .landscape, horizontalInset: 62),
                "Pro Max turned": KeyboardLayout(width: 956, orientation: .landscape, horizontalInset: 62, phoneSize: .large),
                "SE turned": KeyboardLayout(width: 667, orientation: .landscape, phoneSize: .homeButton),
                "cover": KeyboardLayout(width: 452, posture: .cover, windowInset: 7),
                "cover turned": KeyboardLayout(width: 528, orientation: .landscape, posture: .cover, windowInset: 75),
                "iPad mini": KeyboardLayout(width: 744, pad: .docked(.mini)),
            ]
        }
        KeyboardLanguage.systemMajorVersion = 27
        let today = layouts()
        KeyboardLanguage.systemMajorVersion = 25
        let older = layouts()
        KeyboardLanguage.systemMajorVersion = 18
        let ios18 = layouts()
        KeyboardLanguage.systemMajorVersion = 26
        let ios26 = layouts()
        for (name, layout) in today {
            XCTAssertEqual(older[name]!.keys, layout.keys, "\(name): 25 draws 27's keys")
            XCTAssertEqual(ios18[name]!.keys, layout.keys, "\(name): 18 draws 27's keys")
            XCTAssertEqual(older[name]!.totalHeight, layout.totalHeight, name)
        }
        for name in ["17 Pro", "Pro Max", "17 Pro turned"] {
            XCTAssertNotEqual(ios26[name]!.keys, today[name]!.keys, "\(name): 26 differs")
        }
        let heights: [String: (CGFloat, CGFloat)] = [
            "17 Pro": (247, 254), "12": (247, 254), "Pro Max": (266, 264), "SE": (254, 254),
            "17 Pro turned": (204, 187.33), "Pro Max turned": (204, 187.33), "SE turned": (206, 206),
        ]
        for (name, (before, after)) in heights {
            XCTAssertEqual(today[name]!.totalHeight, before, accuracy: 0.01, name)
            XCTAssertEqual(ios26[name]!.totalHeight, after, accuracy: 0.01, name)
            XCTAssertEqual(ios26[name]!.stripFrame, today[name]!.stripFrame, "\(name): the strip stays")
            XCTAssertEqual(ios26[name]!.keys.map(\.frame.minX), today[name]!.keys.map(\.frame.minX), "\(name): the same columns")
            XCTAssertEqual(ios26[name]!.keys.map(\.frame.width), today[name]!.keys.map(\.frame.width), "\(name): the same widths")
        }
        for name in ["cover", "cover turned", "iPad mini", "SE", "SE turned"] {
            XCTAssertEqual(ios26[name]!.keys, today[name]!.keys, "\(name) keeps today's keys")
            XCTAssertEqual(ios26[name]!.totalHeight, today[name]!.totalHeight, name)
        }
        // Rows from the view's bottom, as Apple's sit above where iOS ends a third-party keyboard.
        func rows(_ layout: KeyboardLayout) -> [(CGFloat, CGFloat)] {
            (0..<4).map { row in
                let frames = layout.keys.filter { $0.row == row }.map(\.frame)
                return (layout.totalHeight - frames.map(\.minY).min()!, layout.totalHeight - frames.map(\.maxY).max()!)
            }
        }
        let pro = rows(ios26["17 Pro"]!)
        XCTAssertEqual(pro.map(\.0), [208, 154, 100, 46], "7 pt higher than today's 201, 147, 93, 39")
        XCTAssertEqual(pro.map(\.1), [165, 111, 57, 3], "row 4 whole, 3 pt above the bottom")
        XCTAssertEqual(rows(today["17 Pro"]!).map(\.0), [201, 147, 93, 39])
        XCTAssertEqual(rows(ios26["Pro Max"]!).map(\.0), [218, 162, 106, 50], "2 pt lower than today's 220")
        let turned = rows(ios26["17 Pro turned"]!)
        for (row, expected) in [(0, 137.33), (1, 102.0), (2, 66.67), (3, 31.33)] {
            XCTAssertEqual(turned[row].0, expected, accuracy: 0.01, "turned row \(row + 1)")
        }
        XCTAssertEqual(turned[3].1, 4, accuracy: 0.01)
        XCTAssertEqual(KeyboardLayout.height(for: .landscape), 82.0 / 3 * 4 + 24 + 4 + 50, accuracy: 0.01)
        XCTAssertEqual(KeyboardLayout.height(for: .portrait, numberRow: true) - KeyboardLayout.height(for: .portrait), 54 * KeyboardMetrics.numberRowPitchRatio, accuracy: 0.01, "the number row sits on top")
        // A view iOS makes 20 pt shorter than asked (Spotlight, #2280) still holds every row.
        for (width, orientation, size) in [(402.0, KeyboardOrientation.portrait, KeyboardPhoneSize.regular), (440, .portrait, .large), (874, .landscape, .regular)] {
            let full = KeyboardLayout(width: width, orientation: orientation, phoneSize: size)
            let short = KeyboardLayout(width: width, orientation: orientation, phoneSize: size, availableHeight: full.totalHeight - 20)
            XCTAssertLessThanOrEqual(short.keys.map(\.frame.maxY).max()!, full.totalHeight - 20 + 0.01, "\(width)")
            XCTAssertEqual(Set(short.keys.map(\.row)), Set(full.keys.map(\.row)), "\(width)")
        }
    }

    func testUkrainianBeforeIOS27IsRussiansRows() {
        let running = KeyboardLanguage.systemMajorVersion
        defer { KeyboardLanguage.systemMajorVersion = running }
        KeyboardLanguage.systemMajorVersion = 26
        XCTAssertEqual(KeyboardLanguage.ukrainian.letterRows.map { $0.joined() }, ["йцукенгшщзх", "фівапролджє", "ячсмитьбю"])
        let cases: [(CGFloat, KeyboardOrientation, CGFloat)] = [
            (375, .portrait, 0), (390, .portrait, 0), (402, .portrait, 0), (440, .portrait, 0),
            (844, .landscape, 47), (956, .landscape, 60),
        ]
        for (width, orientation, inset) in cases {
            let label = "\(width)pt \(orientation)"
            let ukrainian = KeyboardLayout(width: width, orientation: orientation, horizontalInset: inset, language: .ukrainian)
            let russian = KeyboardLayout(width: width, orientation: orientation, horizontalInset: inset, language: .russian)
            XCTAssertEqual(ukrainian.keys.map(\.frame), russian.keys.map(\.frame), label)
            for key in russian.keys {
                guard let character = key.character else { continue }
                let own = ["ы": "і", "э": "є"][character] ?? character
                XCTAssertEqual(ukrainian.key(.character(own))?.frame, key.frame, "\(label) \(own)")
            }
            XCTAssertEqual(ukrainian.key(.shift)?.frame, russian.key(.shift)?.frame, label)
            XCTAssertEqual(ukrainian.key(.delete)?.frame, russian.key(.delete)?.frame, label)
            for letter in ["ї", "ґ", "ʼ"] { XCTAssertNil(ukrainian.key(.character(letter)), "\(label) \(letter) is held") }
        }
        // Apple's 390 pt iPhone 12 (iOS 26.5): row 1 runs from 4.7 to 387.7 pt, 11 keys.
        let twelve = KeyboardLayout(width: 390, language: .ukrainian)
        XCTAssertEqual(twelve.key(.character("й"))!.frame.minX, 6.67, accuracy: 1)
        XCTAssertEqual(twelve.key(.character("х"))!.frame.maxX, 383.67, accuracy: 1)
        XCTAssertEqual(AccentCatalog.options(for: "і", uppercase: false, language: .ukrainian), ["і", "ї"])
        XCTAssertEqual(AccentCatalog.options(for: "І", uppercase: true, language: .ukrainian), ["І", "Ї"])
        XCTAssertEqual(AccentCatalog.options(for: "г", uppercase: false, language: .ukrainian), ["г", "ґ"])
        for letter in KeyboardLanguage.ukrainian.letterRows.joined() where letter != "і" && letter != "г" {
            XCTAssertNil(AccentCatalog.options(for: letter, uppercase: false, language: .ukrainian), letter)
        }
        XCTAssertEqual(AccentCatalog.options(for: "ʼ", uppercase: false, language: .ukrainian), ["ʼ", "'", "’", "‘", "`"], "the 123 page's ʼ")
        let coordinates = KeyboardLanguage.ukrainian.keyCoordinates()
        XCTAssertEqual(coordinates["є"]?.0, 1)
        XCTAssertEqual(coordinates["є"]?.1, 10, "under х")
        XCTAssertEqual(coordinates["я"]?.1, 1, "row 3 starts under ц")
        XCTAssertNil(coordinates["ї"])
        for layer in [KeyboardLayer.numbers, .symbols] {
            XCTAssertEqual(KeyboardLayout.characterRows(for: layer, language: .ukrainian)[2].last, "\u{02BC}")
        }
        KeyboardLanguage.systemMajorVersion = 27
        XCTAssertEqual(KeyboardLanguage.ukrainian.letterRows.map { $0.joined() }, ["йцукенгшщзхї", "фівапролджєʼ", "ячсмитьбюґ"])
        XCTAssertNil(AccentCatalog.options(for: "і", uppercase: false, language: .ukrainian))
    }

    /// The standard Greek layout in the rows the system Greek keyboard shows on an iPhone 17
    /// Pro (iOS 27.2 simulator, 402 pt, read through accessibility): rows 1 and 2 are nine
    /// letter-wide keys at the same places (ε over α where English has A, the tonos key ´ over λ),
    /// row 3 starts under σ, and the bottom row is English's. Its 123 page has the ano teleia ·
    /// where English has ;, and ; (the Greek question mark) where English has ?; its #+= page
    /// has ? where English has |.
    func testGreeksRowsAndPagesMatchApples() throws {
        for width in portraitWidths {
            let layout = KeyboardLayout(width: width, language: .greek)
            let english = KeyboardLayout(width: width)
            XCTAssertEqual(layout.characterWidth, english.characterWidth, accuracy: 0.001)
            let row1 = layout.keys.filter { $0.row == 0 }.map(\.frame)
            let row2 = layout.keys.filter { $0.row == 1 }.map(\.frame)
            XCTAssertEqual(row1.map(\.minX), row2.map(\.minX), "\(width): row 1 over row 2")
            XCTAssertEqual(layout.key(.character("ζ"))!.frame.minX, layout.key(.character("σ"))!.frame.minX, accuracy: 0.001)
            XCTAssertEqual(layout.key(.shift)!.frame, english.key(.shift)!.frame)
            XCTAssertEqual(layout.keys.filter { $0.row == 3 }.map(\.frame), english.keys.filter { $0.row == 3 }.map(\.frame))
        }
        let pro = KeyboardLayout(width: 402, language: .greek)
        let qwerty = KeyboardLayout(width: 402)
        XCTAssertEqual(try XCTUnwrap(pro.key(.character("ε"))).frame.minX, try XCTUnwrap(qwerty.key(.character("a"))).frame.minX, accuracy: 0.001)
        XCTAssertEqual(try XCTUnwrap(pro.key(.character("´"))).frame.minX, try XCTUnwrap(qwerty.key(.character("l"))).frame.minX, accuracy: 0.001)
        XCTAssertEqual(Array("´".unicodeScalars).map(\.value), [0x00B4])
        XCTAssertNil(pro.key(.character("ς")), "ς is held on σ")
        XCTAssertNil(pro.key(.character(";")))
        let numbers = KeyboardLayout.characterRows(for: .numbers, language: .greek)
        XCTAssertEqual(numbers[1], ["-", "/", ":", "·", "(", ")", "$", "&", "@", "\""])
        XCTAssertEqual(numbers[2], [".", ",", ";", "!", "'"])
        let symbols = KeyboardLayout.characterRows(for: .symbols, language: .greek)
        XCTAssertEqual(symbols[1], ["_", "\\", "?", "~", "<", ">", "€", "£", "¥", "•"])
        XCTAssertEqual(symbols[2], [".", ",", ";", "!", "'"])
        for layer in [KeyboardLayer.numbers, .symbols] {
            XCTAssertEqual(
                KeyboardLayout(width: 402, layer: layer, language: .greek).keys.map(\.frame),
                KeyboardLayout(width: 402, layer: layer).keys.map(\.frame),
                "the same keys, a few characters changed"
            )
            XCTAssertEqual(KeyboardLayout(width: 402, layer: layer, language: .ukrainian).keys.map(\.frame), KeyboardLayout(width: 402, layer: layer).keys.map(\.frame))
        }
    }

    /// The standard Czech layout in the rows the system Czech keyboard ("QWERTZ – Czech/Slovak")
    /// shows on an iPhone 17 Pro (iOS 27.2 simulator, 402 pt, read through accessibility): rows
    /// 1 and 2 are ten letter-wide keys edge to edge, the ´ key ending row 2 under P; row 3 is
    /// shift, Y to M and the ˇ key, and delete, each a letter wide, Y under S. Its 123 and #+=
    /// pages are English's. "QWERTY – Czech/Slovak" is the same with Z and Y swapped.
    @MainActor
    func testCzechsRowsAndPagesMatchApples() throws {
        for width in portraitWidths {
            let layout = KeyboardLayout(width: width, language: .czech)
            let english = KeyboardLayout(width: width)
            XCTAssertEqual(layout.characterWidth, english.characterWidth, accuracy: 0.001)
            for row in 0..<2 {
                let keys = layout.keys.filter { $0.row == row }.sorted { $0.frame.minX < $1.frame.minX }
                XCTAssertEqual(keys.count, 10, "\(width)")
                XCTAssertEqual(keys.first!.frame.minX, layout.metrics.side, accuracy: 0.001)
                XCTAssertEqual(keys.last!.frame.maxX, width - layout.metrics.side, accuracy: 0.001)
            }
            XCTAssertEqual(layout.key(.character("´"))!.frame.minX, layout.key(.character("p"))!.frame.minX, accuracy: 0.001)
            XCTAssertEqual(layout.key(.character("y"))!.frame.minX, layout.key(.character("s"))!.frame.minX, accuracy: 0.001)
            XCTAssertEqual(layout.key(.character("ˇ"))!.frame.minX, layout.key(.character("o"))!.frame.minX, accuracy: 0.001)
            XCTAssertEqual(layout.key(.shift)!.frame.width, layout.characterWidth, accuracy: 0.001)
            XCTAssertEqual(layout.key(.delete)!.frame.maxX, width - layout.metrics.side, accuracy: 0.001)
            XCTAssertEqual(layout.keys.filter { $0.row == 3 }.map(\.frame), english.keys.filter { $0.row == 3 }.map(\.frame))
        }
        XCTAssertEqual(Array("´ˇ˚".unicodeScalars).map(\.value), [0x00B4, 0x02C7, 0x02DA])
        let qwerty = KeyboardLayout(width: 402, language: .czech, letters: .qwerty)
        XCTAssertEqual((0..<3).map { letters(qwerty, row: $0).joined() }, ["qwertyuiop", "asdfghjkl´", "zxcvbnmˇ"])
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("z", "t", language: .czech))
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("y", "s", language: .czech))
        XCTAssertFalse(AutocorrectEngine.areAdjacentKeys("y", "t", language: .czech))
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("y", "t", language: .czech, letters: .qwerty))
        for layer in [KeyboardLayer.numbers, .symbols] {
            XCTAssertEqual(KeyboardLayout(width: 402, layer: layer, language: .czech).keys, KeyboardLayout(width: 402, layer: layer).keys)
        }
    }

    /// The standard Hungarian QWERTZ, its own layout: `qwertzuiop`, `asdfghjkl`, `yxcvbnm`, so
    /// Z ends row 1 and Y starts row 3, with QWERTY offered as its second layout (Y and Z
    /// swapped). Its accented vowels are all held on a e i o u, so no key of its own, and shift,
    /// delete and the 123/#+= pages stay English's.
    @MainActor
    func testHungariansRowsAndHoldMatchItsLayout() throws {
        for width in portraitWidths {
            let layout = KeyboardLayout(width: width, language: .hungarian)
            let english = KeyboardLayout(width: width)
            XCTAssertEqual(layout.characterWidth, english.characterWidth, accuracy: 0.001)
            XCTAssertEqual(layout.keys.filter { $0.row == 3 }.map(\.frame), english.keys.filter { $0.row == 3 }.map(\.frame), "\(width)")
            XCTAssertEqual(layout.key(.shift)!.frame, english.key(.shift)!.frame, "\(width)")
            XCTAssertEqual(layout.key(.delete)!.frame, english.key(.delete)!.frame, "\(width)")
        }
        let own = KeyboardLayout(width: 402, language: .hungarian)
        XCTAssertEqual((0..<3).map { letters(own, row: $0).joined() }, ["qwertzuiop", "asdfghjkl", "yxcvbnm"])
        XCTAssertEqual(own.key(.character("z"))?.row, 0)
        XCTAssertEqual(own.key(.character("y"))?.row, 2)
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("z", "t", language: .hungarian))
        XCTAssertFalse(AutocorrectEngine.areAdjacentKeys("y", "t", language: .hungarian))
        let qwerty = KeyboardLayout(width: 402, language: .hungarian, letters: .qwerty)
        XCTAssertEqual((0..<3).map { letters(qwerty, row: $0).joined() }, ["qwertyuiop", "asdfghjkl", "zxcvbnm"])
        XCTAssertTrue(AutocorrectEngine.areAdjacentKeys("y", "t", language: .hungarian, letters: .qwerty))
        // All nine accented vowels on their base letter, base first, and ű/ő with the double acute.
        XCTAssertEqual(Array("áéíóöőúüű".unicodeScalars).map(\.value), [0xE1, 0xE9, 0xED, 0xF3, 0xF6, 0x151, 0xFA, 0xFC, 0x171])
        for (base, accented) in [("a", ["á"]), ("e", ["é"]), ("i", ["í"]), ("o", ["ó", "ö", "ő"]), ("u", ["ú", "ü", "ű"])] {
            for form in accented {
                XCTAssertTrue(AccentCatalog.options(for: base, uppercase: false, language: .hungarian)?.contains(form) == true, form)
                XCTAssertTrue(AccentCatalog.options(for: base, uppercase: true, language: .hungarian)?.contains(form.uppercased()) == true, form)
            }
            XCTAssertEqual(AccentCatalog.options(for: base, uppercase: false, language: .hungarian)?.first, base, base)
        }
        for letter in KeyboardLanguage.hungarian.letterRows.joined() where !["a", "e", "i", "o", "u"].contains(letter) {
            XCTAssertNil(AccentCatalog.options(for: letter, uppercase: false, language: .hungarian), letter)
        }
        XCTAssertEqual(shownHolds("o", .hungarian), ["ō", "ø", "œ", "õ", "ò", "ô", "ő", "ö", "ó"], "ó nearest the key, then ö, then ő")
        for layer in [KeyboardLayer.numbers, .symbols] {
            XCTAssertEqual(KeyboardLayout(width: 402, layer: layer, language: .hungarian).keys, KeyboardLayout(width: 402, layer: layer).keys)
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
            // Apple's "AZERTY – French": W to N and the apostrophe key, a letter wide, centred
            // between shift and delete at their English size.
            let row3 = layout.keys.filter { $0.row == 2 && $0.character != nil }.sorted { $0.frame.minX < $1.frame.minX }
            XCTAssertEqual(row3.compactMap(\.character), ["w", "x", "c", "v", "b", "n", FrenchTyping.key], label)
            for key in row3 { XCTAssertEqual(key.frame.width, layout.letterWidth, accuracy: 0.001, label) }
            XCTAssertEqual((row3.first!.frame.minX + row3.last!.frame.maxX) / 2, width / 2, accuracy: 0.001, label)
            let shift = layout.key(.shift)!
            let delete = layout.key(.delete)!
            let w = row3.first!
            let apostrophe = row3.last!
            XCTAssertEqual(shift.frame, english.key(.shift)!.frame, label)
            XCTAssertEqual(delete.frame, english.key(.delete)!.frame, label)
            XCTAssertGreaterThan(w.frame.minX - shift.frame.maxX, 2 * layout.metrics.gap, "a gap beside shift, \(label)")
            XCTAssertGreaterThan(delete.frame.minX - apostrophe.frame.maxX, 2 * layout.metrics.gap, "and beside delete, \(label)")
            XCTAssertEqual(layout.keys.filter { $0.row == 3 }.map(\.frame), english.keys.filter { $0.row == 3 }.map(\.frame), label)
            let y = shift.frame.midY
            for (x, expected) in [
                (shift.frame.maxX + 1, KeyKind.shift), (w.frame.minX - 1, .character("w")),
                (delete.frame.minX - 1, .delete), (apostrophe.frame.maxX + 1, .character(FrenchTyping.key)),
            ] {
                XCTAssertEqual(layout.hitTest(CGPoint(x: x, y: y))?.kind, expected, "x \(x), \(label)")
            }
        }
        // Apple's keycaps on 402 pt (iPhone 17 Pro simulator, iOS 27.2): A 6.67 to 40, M 362 to
        // 395.67; row 3 shift 6.67 to 52, W 65.67 to 99.33, N 263.33 to 296.67, the apostrophe
        // key 302.67 to 336.33, delete 350 to 395.67.
        let apple = KeyboardLayout(width: 402, language: .french)
        for (kind, minX, maxX) in [
            (KeyKind.character("a"), 6.67, 40.0), (.character("m"), 362, 395.67), (.shift, 6.67, 52),
            (.character("w"), 65.67, 99.33), (.character("n"), 263.33, 296.67),
            (.character(FrenchTyping.key), 302.67, 336.33), (.delete, 350, 395.67),
        ] {
            XCTAssertEqual(apple.key(kind)!.frame.minX, minX, accuracy: 0.5, "\(kind)")
            XCTAssertEqual(apple.key(kind)!.frame.maxX, maxX, accuracy: 0.5, "\(kind)")
        }
        // The numbers and symbols pages are English's, as Apple's are.
        for layer in [KeyboardLayer.numbers, .symbols] {
            XCTAssertEqual(KeyboardLayout(width: 402, layer: layer, language: .french).keys, KeyboardLayout(width: 402, layer: layer).keys)
        }
        // Only France's AZERTY has it after N: French on QWERTY or QWERTZ, Canadian and Swiss
        // French on AZERTY, and every other language's AZERTY keep six letters on row 3.
        for (language, letters) in [
            (KeyboardLanguage.french, KeyboardLetterLayout.qwerty), (.french, .qwertz), (.frenchCanadian, .azerty),
            (.frenchSwiss, .azerty), (.english, .azerty), (.german, .azerty),
        ] {
            XCTAssertNil(KeyboardLayout(width: 402, language: language, letters: letters).key(.character(FrenchTyping.key)), "\(language) \(letters)")
        }
        // Turned, on the Duo folded and unfolded (split upright, joined by the experiment, and
        // turned) and with the number row: the same seven keys between shift and delete, which
        // keep English's frames there, and every other row as English's.
        let shapes: [(String, (KeyboardLanguage) -> KeyboardLayout)] = [
            ("landscape", { KeyboardLayout(width: 874, orientation: .landscape, language: $0) }),
            ("number row", { KeyboardLayout(width: 402, language: $0, numberRow: true) }),
            ("cover", { KeyboardLayout(width: 452, language: $0, posture: .cover, windowInset: 7) }),
            ("unfolded tall", { KeyboardLayout(width: 655, language: $0, posture: .unfoldedTall, windowInset: 7) }),
            ("unfolded split", { KeyboardLayout(width: 801, language: $0, posture: .unfoldedWide, windowInset: 75) }),
            ("unfolded joined", { KeyboardLayout(width: 801, language: $0, posture: .unfoldedWide, windowInset: 75, joinsUnfoldedKeyboard: true) }),
        ]
        for (name, make) in shapes {
            let layout = make(.french)
            let english = make(.english)
            let row3 = layout.keys.filter { $0.row == 2 && $0.character != nil }.sorted { $0.frame.minX < $1.frame.minX }
            XCTAssertEqual(Set(row3.compactMap(\.character)), ["w", "x", "c", "v", "b", "n", FrenchTyping.key], name)
            XCTAssertEqual(row3.last?.character, FrenchTyping.key, name)
            let shift = layout.key(.shift)!.frame
            let delete = layout.key(.delete)!.frame
            XCTAssertEqual(shift, english.key(.shift)!.frame, name)
            XCTAssertEqual(delete, english.key(.delete)!.frame, name)
            XCTAssertGreaterThanOrEqual(row3.first!.frame.minX, shift.maxX, name)
            XCTAssertLessThanOrEqual(row3.last!.frame.maxX, delete.minX, name)
            for (a, b) in zip(row3, row3.dropFirst()) {
                XCTAssertLessThanOrEqual(a.frame.maxX, b.frame.minX, "\(name): \(a.label) clear of \(b.label)")
            }
            XCTAssertEqual(layout.keys.filter { $0.row == 3 }.map(\.frame), english.keys.filter { $0.row == 3 }.map(\.frame), name)
            XCTAssertEqual(layout.keys.filter(\.isNumberRow), english.keys.filter(\.isNumberRow), name)
        }
        // Holding it: ´ ˋ ˆ ¨, Apple's order read from the key.
        XCTAssertEqual(AccentCatalog.options(for: FrenchTyping.key, uppercase: false, language: .french), ["´", "ˋ", "ˆ", "¨"])
        XCTAssertEqual(shownHolds(FrenchTyping.key, .french), ["¨", "ˆ", "ˋ"])
    }

    /// Apple's Français (Canada) and (Suisse) own layouts, "QWERTY – French" and "QWERTZ –
    /// French" (iOS 27.2 simulator, iPhone 17 Pro and SE, upright and turned): row 2 is A to L and
    /// the apostrophe key, ten keys a letter wide edge to edge under row 1's, and every other row
    /// is the plain layout's. The iPad keeps its own ' and : key instead.
    func testCanadianAndSwissFrenchEndRowTwoInTheApostropheKey() {
        let shapes: [(String, (KeyboardLanguage, KeyboardLetterLayout?) -> KeyboardLayout)] = [
            ("402", { KeyboardLayout(width: 402, language: $0, letters: $1) }),
            ("375", { KeyboardLayout(width: 375, language: $0, letters: $1, phoneSize: .homeButton) }),
            ("landscape", { KeyboardLayout(width: 874, orientation: .landscape, language: $0, letters: $1) }),
            ("number row", { KeyboardLayout(width: 402, language: $0, letters: $1, numberRow: true) }),
            ("cover", { KeyboardLayout(width: 452, language: $0, letters: $1, posture: .cover, windowInset: 7) }),
            ("unfolded tall", { KeyboardLayout(width: 655, language: $0, letters: $1, posture: .unfoldedTall, windowInset: 7) }),
            ("unfolded split", { KeyboardLayout(width: 801, language: $0, letters: $1, posture: .unfoldedWide, windowInset: 75) }),
            ("unfolded joined", { KeyboardLayout(width: 801, language: $0, letters: $1, posture: .unfoldedWide, windowInset: 75, joinsUnfoldedKeyboard: true) }),
        ]
        for (language, plain) in [(KeyboardLanguage.frenchCanadian, KeyboardLetterLayout.qwerty), (.frenchSwiss, .qwertz)] {
            XCTAssertEqual(language.defaultLetterLayout, plain)
            for (name, make) in shapes {
                let layout = make(language, nil)
                let france = make(.french, plain)
                let label = "\(language) \(name)"
                XCTAssertNil(france.key(.character(FrenchTyping.key)), "France's \(plain) stays plain: \(label)")
                let row = { (l: KeyboardLayout, r: Int) in l.keys.filter { $0.row == r && $0.character != nil }.sorted { $0.frame.minX < $1.frame.minX } }
                let row2 = row(layout, 1)
                XCTAssertEqual(row2.compactMap(\.character), "asdfghjkl".map(String.init) + [FrenchTyping.key], label)
                for (a, b) in zip(row2, row2.dropFirst()) { XCTAssertLessThanOrEqual(a.frame.maxX, b.frame.minX, label) }
                for r in [0, 2] { XCTAssertEqual(row(layout, r).map(\.frame), row(france, r).map(\.frame), "row \(r + 1): \(label)") }
                XCTAssertEqual(layout.key(.shift)?.frame, france.key(.shift)?.frame, label)
                XCTAssertEqual(layout.key(.delete)?.frame, france.key(.delete)?.frame, label)
                XCTAssertEqual(layout.keys.filter { $0.row == 3 }.map(\.frame), france.keys.filter { $0.row == 3 }.map(\.frame), label)
                XCTAssertEqual(layout.keys.filter(\.isNumberRow), france.keys.filter(\.isNumberRow), label)
                if !layout.isSplit {
                    XCTAssertEqual(row2.map(\.frame.minX), row(layout, 0).map(\.frame.minX), "edge to edge under row 1: \(label)")
                    XCTAssertEqual(row2.map(\.frame.width), row(layout, 0).map(\.frame.width), label)
                }
            }
            // Apple's keycaps on 402 pt: row 2's A 6.67 to 40, its apostrophe key 362 to 395.67.
            let apple = KeyboardLayout(width: 402, language: language)
            for (kind, minX, maxX) in [(KeyKind.character("a"), 6.67, 40.0), (.character(FrenchTyping.key), 362, 395.67)] {
                XCTAssertEqual(apple.key(kind)!.frame.minX, minX, accuracy: 0.5, "\(language) \(kind)")
                XCTAssertEqual(apple.key(kind)!.frame.maxX, maxX, accuracy: 0.5, "\(language) \(kind)")
            }
            let pad = KeyboardLayout(width: 744, language: language, pad: .docked(.mini))
            XCTAssertNil(pad.key(.character(FrenchTyping.key)), "\(language) iPad")
            XCTAssertEqual(pad.keys.filter { $0.row == 1 }.compactMap(\.character), "asdfghjkl".map(String.init), "\(language) iPad")
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

    /// A held key's callout as Hex shows it, left to right, without the base.
    private func shownHolds(_ character: String, _ language: KeyboardLanguage, width: CGFloat = 402) -> [String] {
        let layout = KeyboardLayout(width: width, language: language)
        guard let key = layout.key(.character(character)),
              let options = AccentCatalog.options(for: character, uppercase: false, language: language) else { return [] }
        return AccentMenu.build(for: key.frame, rawOptions: options, in: layout.width).options.filter { $0 != character }
    }

    /// Each language's held keys match the system keyboard's long-press options as observed
    /// (iOS 27.2 simulator): a key it holds nothing on holds nothing, and Hex's callout shows
    /// the others in the system's left-to-right order, the base under the thumb.
    func testAccentsMatchEachLanguage() {
        XCTAssertEqual(AccentCatalog.options(for: "ø", uppercase: false, language: .norwegian), ["ø", "ö"])
        XCTAssertEqual(AccentCatalog.options(for: "æ", uppercase: true, language: .norwegian), ["Æ", "Ä"])
        XCTAssertNil(AccentCatalog.options(for: "å", uppercase: false, language: .norwegian))
        let norwegianA = AccentCatalog.options(for: "a", uppercase: false, language: .norwegian)!
        XCTAssertFalse(norwegianA.contains("å") || norwegianA.contains("æ"), "they have their own keys")
        XCTAssertTrue(AccentCatalog.options(for: "s", uppercase: false, language: .german)!.contains("ß"))
        XCTAssertFalse(AccentCatalog.options(for: "u", uppercase: false, language: .german)!.contains("ü"))
        XCTAssertNil(AccentCatalog.options(for: "ü", uppercase: false, language: .german))
        XCTAssertNil(AccentCatalog.options(for: "z", uppercase: false, language: .german))
        XCTAssertEqual(AccentCatalog.options(for: "e", uppercase: false, language: .spanish)?.dropFirst().first, "é")
        XCTAssertEqual(AccentCatalog.options(for: "n", uppercase: true, language: .spanish), ["N", "Ń"], "ñ has its own key")
        XCTAssertNil(AccentCatalog.options(for: "ñ", uppercase: false, language: .spanish))
        XCTAssertEqual(AccentCatalog.options(for: "l", uppercase: false, language: .spanish), ["l", "l·l"])
        XCTAssertNil(AccentCatalog.options(for: "z", uppercase: false, language: .spanish))
        XCTAssertEqual(AccentCatalog.options(for: "z", uppercase: false), ["z", "ź", "ž", "ż"])
        XCTAssertNil(AccentCatalog.options(for: "q", uppercase: false))
        // English's held I offers the dotless ı, İ in capitals; never a second I.
        let capitalI = AccentCatalog.options(for: "i", uppercase: true)!
        XCTAssertTrue(capitalI.contains("İ"))
        XCTAssertEqual(capitalI.filter { $0 == "I" }.count, 1)
        XCTAssertEqual(shownHolds("u", .english), ["ų", "ů", "ű", "ũ", "ü", "ú", "ù", "û", "ǔ", "ū"])
        XCTAssertEqual(shownHolds("a", .english), ["à", "á", "â", "ä", "ǎ", "æ", "ã", "å", "ā", "ă", "ą"])
        XCTAssertEqual(AccentCatalog.options(for: "e", uppercase: false, language: .italian)?.prefix(3), ["e", "é", "è"])
        XCTAssertEqual(AccentCatalog.options(for: "a", uppercase: false, language: .italian)?.dropFirst().first, "à")
        XCTAssertTrue(AccentCatalog.options(for: "o", uppercase: false, language: .italian)!.contains("ò"))
        XCTAssertEqual(AccentCatalog.options(for: "e", uppercase: false, language: .french)?.dropFirst().first, "é")
        XCTAssertEqual(AccentCatalog.options(for: "c", uppercase: false, language: .french)?.dropFirst().first, "ç")
        XCTAssertEqual(AccentCatalog.options(for: "u", uppercase: false, language: .french)?.prefix(3), ["u", "û", "ù"])
        XCTAssertEqual(AccentCatalog.options(for: "a", uppercase: true, language: .french)?.prefix(4), ["A", "À", "Â", "Æ"])
        for language in [KeyboardLanguage.frenchCanadian, .frenchSwiss] {
            for letter in ["a", "e", "i", "o", "u", "c", "y", "n"] {
                XCTAssertEqual(
                    AccentCatalog.options(for: letter, uppercase: false, language: language),
                    AccentCatalog.options(for: letter, uppercase: false, language: .french),
                    "\(language) \(letter)"
                )
            }
        }
        XCTAssertEqual(AccentCatalog.options(for: "a", uppercase: false, language: .portuguese)?.prefix(3), ["a", "á", "ã"])
        // Russian: ё on е and ъ on ь, with the letters of other Cyrillic languages.
        XCTAssertEqual(AccentCatalog.options(for: "е", uppercase: false, language: .russian)?.prefix(2), ["е", "ё"])
        XCTAssertEqual(AccentCatalog.options(for: "Е", uppercase: true, language: .russian)?.prefix(2), ["Е", "Ё"])
        XCTAssertEqual(AccentCatalog.options(for: "ь", uppercase: false, language: .russian)?.prefix(2), ["ь", "ъ"])
        XCTAssertEqual(AccentCatalog.options(for: "ь", uppercase: true, language: .russian)?.prefix(2), ["Ь", "Ъ"])
        XCTAssertNil(AccentCatalog.options(for: "б", uppercase: false, language: .russian))
        XCTAssertEqual(shownHolds("ж", .russian), ["ѥ", "ӂ", "ӝ", "җ"])
        // Ukrainian: only ʼ offers anything among the letters (the other apostrophes); ₴ on $.
        XCTAssertEqual(AccentCatalog.options(for: "ʼ", uppercase: false, language: .ukrainian), ["ʼ", "'", "’", "‘", "`"])
        for letter in KeyboardLanguage.ukrainian.letterRows.joined() where letter != "ʼ" {
            XCTAssertNil(AccentCatalog.options(for: letter, uppercase: false, language: .ukrainian), letter)
        }
        XCTAssertEqual(AccentCatalog.options(for: "$", uppercase: false, language: .ukrainian)?.last, "₴")
        XCTAssertFalse(AccentCatalog.options(for: "$", uppercase: false)?.contains("₴") ?? true, "the hryvnia is Ukrainian's")
        XCTAssertFalse(KeyboardLanguage.ukrainian.letterRows.joined().contains { ["ы", "э", "ъ", "ё"].contains($0) })
        // Serbian (Latin): č ć on c, đ on d, š on s, ž on z, and nothing else.
        XCTAssertEqual(AccentCatalog.options(for: "c", uppercase: false, language: .serbianLatin)?.prefix(3), ["c", "č", "ć"])
        XCTAssertEqual(AccentCatalog.options(for: "d", uppercase: true, language: .serbianLatin), ["D", "Đ"])
        XCTAssertEqual(AccentCatalog.options(for: "s", uppercase: false, language: .serbianLatin)?.prefix(2), ["s", "š"])
        XCTAssertEqual(shownHolds("z", .serbianLatin), ["ž", "ź", "ż"])
        XCTAssertNil(AccentCatalog.options(for: "e", uppercase: false, language: .serbianLatin))
        // The 123 and #+= pages: English's, with each language's own where its keyboard differs.
        XCTAssertEqual(AccentCatalog.options(for: "&", uppercase: false), ["&", "§"])
        XCTAssertNil(AccentCatalog.options(for: "#", uppercase: false))
        XCTAssertEqual(AccentCatalog.options(for: "#", uppercase: false, language: .russian), ["#", "№"])
        XCTAssertEqual(AccentCatalog.options(for: "1", uppercase: false, language: .spanish), ["1", "1º", "1ª"])
        XCTAssertNil(AccentCatalog.options(for: "1", uppercase: false))
        XCTAssertEqual(AccentCatalog.options(for: "\"", uppercase: false, language: .german)?.dropFirst().first, "„")
        XCTAssertEqual(AccentCatalog.options(for: ".", uppercase: false, language: .french), [".", "…", "·"])
        // Polish: its own letter right after the base, nothing on U, I or Y; its currency key
        // is € (no zł), held on Hex's #+= €, while Hex's 123 page keeps English's $.
        let polish = ["a": "ą", "c": "ć", "e": "ę", "l": "ł", "n": "ń", "o": "ó", "s": "ś", "z": "ż"]
        for (letter, own) in polish {
            XCTAssertEqual(AccentCatalog.options(for: letter, uppercase: false, language: .polish)?.prefix(2), [letter, own][...], letter)
        }
        XCTAssertEqual(AccentCatalog.options(for: "z", uppercase: true, language: .polish), ["Z", "Ż", "Ź", "Ž"])
        XCTAssertEqual(AccentCatalog.options(for: "l", uppercase: true, language: .polish), ["L", "Ł"])
        XCTAssertEqual(shownHolds("o", .polish), ["ō", "ø", "œ", "õ", "ò", "ö", "ô", "ó"])
        for letter in KeyboardLanguage.polish.letterRows.joined() where polish[letter] == nil {
            XCTAssertNil(AccentCatalog.options(for: letter, uppercase: false, language: .polish), letter)
        }
        XCTAssertEqual(AccentCatalog.options(for: "€", uppercase: false, language: .polish), ["€", "$", "£", "¥", "₩", "₽"])
        XCTAssertEqual(AccentCatalog.options(for: "$", uppercase: false, language: .polish), AccentCatalog.options(for: "$", uppercase: false))
        XCTAssertEqual(AccentCatalog.options(for: "\"", uppercase: false, language: .polish), AccentCatalog.options(for: "\"", uppercase: false))
        // Romanian: ă and â on A, î on I, ș on S and ț on T right after the base, ș and ț the
        // comma-below letters (U+0219, U+021B), never the cedilla ş ţ; every other letter holds
        // nothing. Its currency key is € and its held " is Romanian's own order.
        XCTAssertEqual(
            AccentCatalog.options(for: "a", uppercase: false, language: .romanian),
            ["a", "ă", "â", "à", "á", "ä", "æ", "ã", "å", "ā"]
        )
        XCTAssertEqual(AccentCatalog.options(for: "i", uppercase: false, language: .romanian), ["i", "î", "ï", "ì", "í", "į", "ī"])
        XCTAssertEqual(AccentCatalog.options(for: "s", uppercase: false, language: .romanian), ["s", "ș", "ß", "ś", "š"])
        XCTAssertEqual(AccentCatalog.options(for: "t", uppercase: false, language: .romanian), ["t", "ț"])
        XCTAssertEqual(AccentCatalog.options(for: "t", uppercase: true, language: .romanian), ["T", "Ț"])
        XCTAssertEqual(Array("șț".unicodeScalars).map(\.value), [0x0219, 0x021B], "comma-below, never the cedilla")
        XCTAssertEqual(Array("ȘȚ".unicodeScalars).map(\.value), [0x0218, 0x021A])
        let romanian: Set<String> = ["a", "i", "s", "t"]
        for letter in KeyboardLanguage.romanian.letterRows.joined() where !romanian.contains(letter) {
            XCTAssertNil(AccentCatalog.options(for: letter, uppercase: false, language: .romanian), letter)
        }
        XCTAssertEqual(AccentCatalog.options(for: "€", uppercase: false, language: .romanian), ["€", "$", "£", "¥", "₩", "₽"])
        // Greek: each vowel's tonos form, ι and υ also with diaeresis and both, ς on σ and ϗ
        // on κ; in capitals Σ holds nothing and Ι, Υ only Ί Ϊ, Ύ Ϋ, as on the system keyboard.
        // The tonos key holds nothing; a held " is French's order.
        XCTAssertEqual(AccentCatalog.options(for: "α", uppercase: false, language: .greek), ["α", "ά"])
        XCTAssertEqual(AccentCatalog.options(for: "σ", uppercase: false, language: .greek), ["σ", "ς"])
        XCTAssertNil(AccentCatalog.options(for: "σ", uppercase: true, language: .greek))
        XCTAssertEqual(AccentCatalog.options(for: "υ", uppercase: false, language: .greek), ["υ", "ύ", "ϋ", "ΰ"])
        XCTAssertEqual(AccentCatalog.options(for: "υ", uppercase: true, language: .greek), ["Υ", "Ύ", "Ϋ"])
        XCTAssertEqual(shownHolds("ι", .greek), ["ί", "ϊ", "ΐ"])
        XCTAssertEqual(AccentCatalog.options(for: "ι", uppercase: true, language: .greek), ["Ι", "Ϊ", "Ί"])
        XCTAssertEqual(AccentCatalog.options(for: "κ", uppercase: true, language: .greek), ["Κ", "Ϗ"])
        XCTAssertEqual(AccentCatalog.options(for: "ω", uppercase: true, language: .greek), ["Ω", "Ώ"])
        let greek: Set<String> = ["ε", "υ", "ι", "ο", "α", "σ", "η", "κ", "ω"]
        for letter in KeyboardLanguage.greek.letterRows.joined() where !greek.contains(letter) {
            XCTAssertNil(AccentCatalog.options(for: letter, uppercase: false, language: .greek), letter)
        }
        XCTAssertEqual(AccentCatalog.options(for: "\"", uppercase: false, language: .greek), ["\"", "»", "«", "”", "“", "„"])
        XCTAssertEqual(AccentCatalog.options(for: "$", uppercase: false, language: .greek), AccentCatalog.options(for: "$", uppercase: false))
        XCTAssertEqual(AccentCatalog.options(for: "?", uppercase: false, language: .greek), ["?", "¿"])
        // Laid out as the system Greek keyboard's callouts: the key's own on the left, capitals
        // without the forms that have none, and its own order for ".
        XCTAssertEqual(AccentCatalog.arrangement(for: "ι", uppercase: false, language: .greek), AccentArrangement(rows: [["ι", "ί", "ϊ", "ΐ"]], baseRow: 0, baseColumn: 0))
        XCTAssertEqual(AccentCatalog.arrangement(for: "ι", uppercase: true, language: .greek), AccentArrangement(rows: [["Ι", "Ί", "Ϊ"]], baseRow: 0, baseColumn: 0))
        XCTAssertEqual(AccentCatalog.arrangement(for: "κ", uppercase: false, language: .greek)?.rows, [["κ", "ϗ"]])
        XCTAssertNil(AccentCatalog.arrangement(for: "σ", uppercase: true, language: .greek))
        XCTAssertEqual(AccentCatalog.arrangement(for: "\"", uppercase: false, language: .greek)?.rows, [["„", "“", "”", "«", "»", "\""]])
        XCTAssertEqual(
            AccentCatalog.arrangement(for: "$", uppercase: false, language: .greek),
            AccentCatalog.arrangement(for: "$", uppercase: false)
        )
        XCTAssertEqual(
            AccentCatalog.options(for: "\"", uppercase: false, language: .romanian),
            ["\"", "„", "”", "»", "«", "“"]
        )
        XCTAssertEqual(AccentCatalog.options(for: "$", uppercase: false, language: .romanian), AccentCatalog.options(for: "$", uppercase: false))
        // Czech: every Czech letter held on its base (á č ď é ě í ň ó ř š ť ú ů ý ž), the ring ˚
        // on the ˇ key, nothing on the ´ key or the letters not listed; callouts laid out as
        // the system's, and every symbol's hold English's.
        XCTAssertEqual(AccentCatalog.options(for: "e", uppercase: false, language: .czech), ["e", "ě", "é", "ê", "è", "ę", "ë", "ē", "ė"])
        XCTAssertEqual(AccentCatalog.options(for: "u", uppercase: true, language: .czech), ["U", "Ú", "Ů", "Û", "Ü", "Ù", "Ū"])
        XCTAssertEqual(AccentCatalog.options(for: "ˇ", uppercase: false, language: .czech), ["ˇ", "˚"])
        XCTAssertNil(AccentCatalog.options(for: "´", uppercase: false, language: .czech))
        let czech: Set<String> = ["e", "r", "t", "z", "u", "i", "o", "a", "s", "d", "l", "y", "c", "n", "ˇ"]
        for letter in KeyboardLanguage.czech.letterRows.joined() where !czech.contains(letter) {
            XCTAssertNil(AccentCatalog.options(for: letter, uppercase: false, language: .czech), letter)
        }
        for word in ["á", "č", "ď", "é", "ě", "í", "ň", "ó", "ř", "š", "ť", "ú", "ů", "ý", "ž"] {
            let base = String(word.decomposedStringWithCanonicalMapping.unicodeScalars.prefix(1).map(Character.init))
            XCTAssertTrue(AccentCatalog.options(for: base, uppercase: false, language: .czech)?.contains(word) == true, word)
        }
        XCTAssertEqual(AccentCatalog.arrangement(for: "e", uppercase: false, language: .czech), AccentArrangement(rows: [["ě", "e", "é", "ê", "è", "ę", "ë", "ē", "ė"]], baseRow: 0, baseColumn: 1))
        XCTAssertEqual(AccentCatalog.arrangement(for: "z", uppercase: false, language: .czech)?.rows, [["z", "ž", "ź", "ż"]])
        XCTAssertEqual(AccentCatalog.arrangement(for: "n", uppercase: true, language: .czech), AccentArrangement(rows: [["Ń", "Ñ", "Ň", "N"]], baseRow: 0, baseColumn: 3))
        XCTAssertEqual(AccentCatalog.arrangement(for: "ˇ", uppercase: false, language: .czech)?.rows, [["ˇ", "˚"]])
        for symbol in ["$", "\"", "'", "0", "&", "-", "?"] {
            XCTAssertEqual(AccentCatalog.options(for: symbol, uppercase: false, language: .czech), AccentCatalog.options(for: symbol, uppercase: false), symbol)
            XCTAssertEqual(AccentCatalog.arrangement(for: symbol, uppercase: false, language: .czech), AccentCatalog.arrangement(for: symbol, uppercase: false), symbol)
        }
        // Hungarian: its nine accented vowels on a e i o u, the base first, ó ö ő on O and ú ü ű
        // on U (ő and ű the double-acute letters, U+0151 and U+0171); every consonant holds
        // nothing. Its 123 and #+= pages are English's.
        XCTAssertEqual(AccentCatalog.options(for: "o", uppercase: false, language: .hungarian)?.prefix(4), ["o", "ó", "ö", "ő"][...])
        XCTAssertEqual(AccentCatalog.options(for: "u", uppercase: false, language: .hungarian)?.prefix(4), ["u", "ú", "ü", "ű"][...])
        XCTAssertEqual(AccentCatalog.options(for: "e", uppercase: false, language: .hungarian), ["e", "é", "è", "ê", "ë", "ě", "ė", "ę", "ē"])
        XCTAssertEqual(AccentCatalog.options(for: "u", uppercase: true, language: .hungarian), ["U", "Ú", "Ü", "Ű", "Û", "Ù", "Ū"])
        for letter in KeyboardLanguage.hungarian.letterRows.joined() where !["a", "e", "i", "o", "u"].contains(letter) {
            XCTAssertNil(AccentCatalog.options(for: letter, uppercase: false, language: .hungarian), letter)
        }
        for symbol in ["$", "\"", "'", "0", "&", "-", "?"] {
            XCTAssertEqual(AccentCatalog.options(for: symbol, uppercase: false, language: .hungarian), AccentCatalog.options(for: symbol, uppercase: false), symbol)
        }
    }

    func testAccentMenusOnTheNewKeysStayInside() {
        let keys: [KeyboardLanguage: [String]] = [
            .norwegian: ["å", "ø", "æ", "a", "o"],
            .spanish: ["ñ", "a", "o", "n"],
            .german: ["ü", "ö", "ä", "s", "a", "u"],
            .russian: ["й", "е", "х", "ж", "э", "ь", "ю", "я"],
            .ukrainian: ["й", "ї", "є", "ʼ", "ґ"],
            .serbianLatin: ["c", "d", "s", "z"],
            .polish: ["a", "c", "e", "l", "n", "o", "s", "z"],
            .romanian: ["a", "i", "s", "t"],
            .greek: ["ε", "υ", "ι", "ο", "α", "σ", "η", "κ", "ω"],
            .czech: ["e", "r", "t", "z", "u", "i", "o", "a", "s", "d", "l", "y", "c", "n", "ˇ"],
            .hungarian: ["a", "e", "i", "o", "u"],
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
        XCTAssertEqual(automatic("uk-UA", modes: ["ru-RU", "en-US"]), .ukrainian)
        XCTAssertEqual(automatic("uk_UA"), .ukrainian)
        XCTAssertEqual(automatic(nil, modes: ["emoji", "uk-UA", "en-US"], preferred: ["ru-RU"]), .ukrainian)
        XCTAssertEqual(automatic(nil, preferred: ["uk-UA", "en-US"]), .ukrainian)
        XCTAssertEqual(automatic("pl-PL", modes: ["en-US"]), .polish)
        XCTAssertEqual(automatic("pl_PL"), .polish)
        XCTAssertEqual(automatic(nil, modes: ["emoji", "pl_PL", "en-US"]), .polish)
        XCTAssertEqual(automatic(nil, preferred: ["pl-PL", "en-US"]), .polish)
        XCTAssertEqual(automatic("ro-RO", modes: ["en-US"]), .romanian)
        XCTAssertEqual(automatic("ro_RO"), .romanian)
        XCTAssertEqual(automatic("el-GR", modes: ["en-US"]), .greek)
        XCTAssertEqual(automatic("el_GR"), .greek)
        XCTAssertEqual(automatic(nil, modes: ["emoji", "el_GR", "en-US"]), .greek)
        XCTAssertEqual(automatic("en-US", modes: ["el-GR"]), .english, "the dictation language still wins")
        XCTAssertEqual(automatic("cs-CZ", modes: ["en-US"]), .czech)
        XCTAssertEqual(automatic("cs_CZ"), .czech)
        XCTAssertEqual(automatic(nil, modes: ["emoji", "cs_CZ", "en-US"]), .czech)
        XCTAssertEqual(automatic(nil, preferred: ["cs-CZ", "en-US"]), .czech)
        XCTAssertEqual(automatic("sk-SK"), .english, "Slovak has no layout of its own here")
        XCTAssertEqual(KeyboardLanguage.czech.dictationLanguage(supported: ["en-US", "cs-CZ"], preferred: ["cs-CZ"]), "cs-CZ")
        XCTAssertEqual(KeyboardLanguage.czech.dictationLanguage(supported: [], preferred: []), "cs-CZ")
        XCTAssertEqual(automatic("hu-HU", modes: ["en-US"]), .hungarian)
        XCTAssertEqual(automatic("hu_HU"), .hungarian)
        XCTAssertEqual(automatic(nil, modes: ["emoji", "hu_HU", "en-US"]), .hungarian)
        XCTAssertEqual(automatic(nil, preferred: ["hu-HU", "en-US"]), .hungarian)
        XCTAssertEqual(KeyboardLanguage.hungarian.dictationLanguage(supported: ["en-US", "hu-HU"], preferred: ["hu-HU"]), "hu-HU")
        XCTAssertEqual(KeyboardLanguage.hungarian.dictationLanguage(supported: [], preferred: []), "hu-HU", "not listed yet")
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
        XCTAssertEqual(automatic(nil, preferred: ["el-GR"]), .greek)
        XCTAssertEqual(automatic(nil, preferred: ["he-IL"]), .english, "Hebrew has no layout yet")
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
        func partner(_ chosen: [KeyboardLanguage], _ current: KeyboardLanguage, _ dictionaries: [String]? = nil) -> KeyboardLanguage? {
            KeyboardLanguageList.resolve(chosen: chosen, stored: current, automatic: .english).partner(available: dictionaries ?? available)
        }
        XCTAssertEqual(partner([.english, .french, .russian], .french), .english)
        XCTAssertEqual(partner([.english, .french, .russian], .english), .french)
        XCTAssertNil(partner([.english, .french, .russian], .russian))
        XCTAssertEqual(partner([.russian, .german, .english], .english), .german, "the first other Latin one")
        XCTAssertNil(partner([.french, .frenchCanadian], .frenchCanadian), "one language, two layouts")
        XCTAssertNil(partner([.serbianLatin, .english], .serbianLatin), "Serbian has no dictionary to check")
        XCTAssertNil(partner([.english, .serbianLatin], .english))
        XCTAssertNil(partner([.ukrainian, .english], .ukrainian), "Cyrillic has no bilingual partner")
        XCTAssertNil(partner([.english, .ukrainian], .english))
        XCTAssertNil(partner([.greek, .english], .greek, ["en_US", "el"]), "Greek letters have no bilingual partner")
        XCTAssertNil(partner([.english, .greek], .english, ["en_US", "el"]))
        XCTAssertEqual(partner([.polish, .english], .polish, ["en_US", "pl"]), .english, "Polish has Latin letters")
        XCTAssertEqual(partner([.english, .polish], .english, ["en_US", "pl"]), .polish)
        XCTAssertNil(partner([.polish, .english], .polish), "no Polish dictionary on this iPhone")
        XCTAssertEqual(partner([.czech, .english], .czech, ["en_US", "cs"]), .english, "Czech has Latin letters")
        XCTAssertEqual(partner([.english, .czech], .english, ["en_US", "cs"]), .czech)
        XCTAssertEqual(KeyboardLanguageList.resolve(chosen: [.english, .czech], stored: .czech, automatic: .english).layout, .czech, "Czech types on its own keys")
        XCTAssertEqual(partner([.hungarian, .english], .hungarian, ["en_US", "hu"]), .english, "Hungarian has Latin letters")
        XCTAssertEqual(partner([.english, .hungarian], .english, ["en_US", "hu"]), .hungarian)
        XCTAssertNil(partner([.hungarian, .english], .hungarian), "no Hungarian dictionary on this iPhone")
        XCTAssertEqual(
            KeyboardLanguageList.resolve(chosen: [.english, .hungarian], stored: .hungarian, automatic: .english).layout,
            .hungarian,
            "Hungarian types on its own keys"
        )
        XCTAssertNil(partner([.english], .english))
    }

    /// English and Spanish chosen together are Apple's bilingual English & Spanish keyboard:
    /// both type on the first one's keys (QWERTY with ñ on a held n when English leads, the ñ
    /// key when Spanish does), whichever the space bar switched to; Spanish alone keeps its
    /// ñ key, and a third language its own letters (TestFlight build 56: English and Spanish
    /// showed the ñ key "stuck" right of L).
    func testEnglishAndSpanishTypeOnTheFirstOnesKeys() {
        func layout(_ chosen: [KeyboardLanguage], _ current: KeyboardLanguage) -> KeyboardLanguage {
            KeyboardLanguageList.resolve(chosen: chosen, stored: current, automatic: .english).layout
        }
        XCTAssertEqual(layout([.english, .spanish], .english), .english)
        XCTAssertEqual(layout([.english, .spanish], .spanish), .english, "Español on English's keys")
        XCTAssertEqual(layout([.spanish, .english], .english), .spanish, "English on Spanish's keys")
        XCTAssertEqual(layout([.spanish, .english], .spanish), .spanish)
        XCTAssertEqual(layout([.spanish], .spanish), .spanish)
        XCTAssertEqual(layout([.english, .spanish, .french], .french), .french)
        XCTAssertEqual(layout([.french, .spanish, .english], .english), .spanish, "the first of the two")
        XCTAssertEqual(layout([.french, .spanish], .spanish), .spanish, "no English beside it")
        XCTAssertEqual(layout([.english, .french], .french), .french)
        XCTAssertFalse(KeyboardLanguage.english.letterRows[1].contains("ñ"))
        XCTAssertEqual(AccentCatalog.options(for: "n", uppercase: false, language: .english), ["n", "ñ", "ń", "ņ", "ň"])
        XCTAssertEqual(AccentCatalog.options(for: "n", uppercase: false, language: .spanish), ["n", "ń"])
    }

    /// While the keyboard switches between two or more languages, Automatic dictates in the
    /// one it shows (TestFlight build 56: Spanish dictation came out "incomprehensible"),
    /// in the iPhone's region; a language picked by hand, one keyboard language, or an iPhone
    /// language without a layout (Hebrew) keeps dictation where it was, unless Switch dictation
    /// too asks to follow.
    func testAutomaticDictationFollowsTheKeyboardsLanguage() {
        let suite = "KeyboardLayoutTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = LanguageSettings(defaults: defaults)
        settings.supportedDictationLanguages = ["en-US", "en-GB", "es-ES", "es-MX", "es-US", "fr-FR"]
        settings.automaticDictationLanguage = "en-US"
        XCTAssertNil(settings.keyboardDictationLanguage(preferred: ["en-US"]), "Automatic keyboard")
        XCTAssertEqual(settings.effectiveDictationLanguage, "en-US")

        settings.keyboardLanguages = [.english, .spanish]
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["en-US"]), "en-US", "the first until switched")
        settings.currentKeyboardLanguage = .spanish
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["en-US"], region: "ES"), "es-ES")
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["en-US"], region: "MX"), "es-MX", "the iPhone's Region")
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["en-US"], region: "US"), "es-US")
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["en-US"], region: "CO"), "es-US", "Latin America")
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["en-US", "es-MX"], region: "ES"), "es-MX", "the iPhone's Spanish")
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["es-419"], region: "ES"), "es-US", "Latin American Spanish")
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["es"], region: "MX"), "es-MX", "no Spanish region")
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["es"], region: "FR"), "es-ES")
        settings.chooseDictationLanguage("es-MX")
        settings.dictationFollowsKeyboard = true
        settings.chooseDictationLanguage(nil)
        XCTAssertEqual(
            settings.keyboardDictationLanguage(preferred: ["en-US"], region: "ES"),
            "es-MX",
            "the variant picked by hand survives a swipe away and back"
        )
        settings.dictationFollowsKeyboard = false
        settings.currentKeyboardLanguage = .english
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["en-GB"]), "en-GB")
        settings.currentKeyboardLanguage = .french
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["en-US"]), "en-US", "not chosen: the first")

        settings.dictationLanguage = "es-MX"
        XCTAssertNil(settings.keyboardDictationLanguage(preferred: ["en-US"]), "picked by hand")
        XCTAssertEqual(settings.effectiveDictationLanguage, "es-MX")
        settings.dictationLanguage = nil

        settings.keyboardLanguages = [.spanish]
        XCTAssertNil(settings.keyboardDictationLanguage(preferred: ["en-US"]), "one language: nothing to follow")

        settings.keyboardLanguages = [.english, .spanish]
        settings.automaticDictationLanguage = "he-IL"
        XCTAssertNil(settings.keyboardDictationLanguage(preferred: ["he-IL"]), "Hebrew is dictated, never typed")
        settings.dictationFollowsKeyboard = true
        XCTAssertEqual(settings.keyboardDictationLanguage(preferred: ["he-IL"]), "en-US", "Switch dictation too")
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
        XCTAssertEqual(
            KeyboardLanguage.suggested(inputModes: ["uk-UA", "emoji", "en-US"], preferred: ["uk-UA", "ru-RU"]),
            [.ukrainian, .english, .russian]
        )
        XCTAssertEqual(KeyboardLanguage.ukrainian.dictationLanguage(supported: supported + ["uk-UA"], preferred: ["uk-UA"]), "uk-UA")
        XCTAssertNil(KeyboardLanguage.ukrainian.dictationLanguage(supported: supported, preferred: []), "not Russian instead")
        XCTAssertEqual(KeyboardLanguage.ukrainian.dictationLanguage(supported: [], preferred: []), "uk-UA", "not listed yet")
        XCTAssertEqual(
            KeyboardLanguage.suggested(inputModes: ["pl_PL", "emoji"], preferred: ["pl-PL", "en-US"]),
            [.polish, .english]
        )
        XCTAssertEqual(KeyboardLanguage.polish.dictationLanguage(supported: supported + ["pl-PL"], preferred: ["pl-PL"]), "pl-PL")
        XCTAssertEqual(KeyboardLanguage.polish.dictationLanguage(supported: [], preferred: []), "pl-PL", "not listed yet")
        XCTAssertEqual(KeyboardLanguage.polish.displayName, "Polski")
        XCTAssertEqual(
            KeyboardLanguage.suggested(inputModes: ["el_GR", "emoji"], preferred: ["el-GR", "en-US"]),
            [.greek, .english]
        )
        XCTAssertEqual(KeyboardLanguage.greek.dictationLanguage(supported: supported + ["el-GR"], preferred: ["el-GR"]), "el-GR")
        XCTAssertEqual(KeyboardLanguage.greek.dictationLanguage(supported: [], preferred: []), "el-GR", "not listed yet")
        XCTAssertEqual(KeyboardLanguage.greek.displayName, "Ελληνικά")
    }

    /// Apple Speech's three Spanish variants: a Latin American, United States or plain
    /// Spanish iPhone dictates the Americas' Spanish (es-US, then es-MX), never Spain's, and a
    /// Spanish layout with no Spanish preferred language follows the iPhone's Region.
    func testSpanishKeyboardsDictateTheAmericasSpanish() {
        let supported = ["en-US", "es-ES", "es-MX", "es-US"]
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es-419"], region: "ES"), "es-US")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es-CO"], region: "ES"), "es-US")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es-US"], region: "ES"), "es-US", "exact")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es-ES"], region: "MX"), "es-ES", "Spain's, named")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es-MX"], region: "ES"), "es-MX", "Mexico's, named")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: [], region: "US"), "es-US")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: [], region: "MX"), "es-MX")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: [], region: "ES"), "es-ES")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: [], region: "FR"), "es-ES")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es"], region: "MX"), "es-MX")
        XCTAssertEqual(KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es"], region: "FR"), "es-ES")
        XCTAssertEqual(
            KeyboardLanguage.spanish.dictationLanguage(supported: ["en-US", "es-ES", "es-MX"], preferred: ["es-419"], region: "US"),
            "es-MX",
            "es-US missing"
        )
        XCTAssertEqual(
            KeyboardLanguage.spanish.dictationLanguage(supported: [], preferred: ["es-419"], region: "US"),
            "es-419",
            "not listed yet"
        )
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
        XCTAssertEqual(KeyboardLanguage.ukrainian.spellCheckLanguage(available: ["en_US", "ru_RU", "uk_UA"]), "uk_UA")
        XCTAssertEqual(KeyboardLanguage.polish.spellCheckLanguage(available: ["en_US", "pl"]), "pl")
        XCTAssertNil(KeyboardLanguage.polish.spellCheckLanguage(available: ["en_US", "cs"]), "never English or Czech")
        XCTAssertEqual(KeyboardLanguage.hungarian.spellCheckLanguage(available: ["en_US", "hu_HU"]), "hu_HU")
        XCTAssertEqual(KeyboardLanguage.hungarian.spellCheckLanguage(available: ["en_US", "hu"]), "hu")
        XCTAssertNil(KeyboardLanguage.hungarian.spellCheckLanguage(available: ["en_US", "cs"]), "never English or Czech")
        XCTAssertEqual(KeyboardLanguage.spellCheckLanguage(forDictation: "hu-HU", available: ["en_US", "hu"]), "hu")
        XCTAssertEqual(KeyboardLanguage.spellCheckLanguage(forDictation: "pl-PL", available: ["en_US", "pl"]), "pl")
        XCTAssertNil(KeyboardLanguage.ukrainian.spellCheckLanguage(available: ["en_US", "ru_RU"]), "never Russian or English")
        XCTAssertEqual(KeyboardLanguage.spellCheckLanguage(forDictation: "uk-UA", available: ["en_US", "uk_UA"]), "uk_UA")
        XCTAssertEqual(KeyboardLanguage.greek.spellCheckLanguage(available: ["en_US", "el"]), "el")
        XCTAssertNil(KeyboardLanguage.greek.spellCheckLanguage(available: ["en_US"]), "never English")
        XCTAssertEqual(KeyboardLanguage.spellCheckLanguage(forDictation: "el-GR", available: ["en_US", "el"]), "el")
        XCTAssertEqual(KeyboardLanguage.serbianLatin.spellCheckLanguage(available: ["en_US", "sr_Latn"]), "sr_Latn")
        XCTAssertNil(KeyboardLanguage.serbianLatin.spellCheckLanguage(available: ["en_US", "sr", "hr_HR"]), "no Cyrillic or Croatian")
    }

    /// The input mode's language (`UIInputViewController.primaryLanguage`) is BCP 47 and
    /// supersedes the Info.plist's en-US, so each layout names the language the host's
    /// spelling and autocorrection follow.
    func testEachLanguageNamesItsInputMode() {
        XCTAssertEqual(KeyboardLanguage.english.inputModeLanguage, "en-US")
        XCTAssertEqual(KeyboardLanguage.italian.inputModeLanguage, "it-IT")
        XCTAssertEqual(KeyboardLanguage.spanish.inputModeLanguage, "es-ES")
        XCTAssertEqual(KeyboardLanguage.frenchCanadian.inputModeLanguage, "fr-CA")
        XCTAssertEqual(KeyboardLanguage.hungarian.inputModeLanguage, "hu-HU")
        XCTAssertEqual(KeyboardLanguage.serbianLatin.inputModeLanguage, "sr-Latn-RS")
        XCTAssertEqual(
            Set(KeyboardLanguage.allCases.map(\.inputModeLanguage)).count,
            KeyboardLanguage.allCases.count,
            "one identifier each"
        )
    }

    // MARK: - Helpers

    // MARK: - iPad

    /// Every iPad keyboard Hex draws: docked on the three sizes either way up, and floating.
    private func padLayouts() -> [KeyboardLayout] {
        var layouts: [KeyboardLayout] = []
        let placements: [(CGFloat, KeyboardOrientation, KeyboardPadPlacement)] = [
            (744, .portrait, .docked(.mini)), (1133, .landscape, .docked(.mini)),
            (834, .portrait, .docked(.medium)), (1210, .landscape, .docked(.medium)),
            (1032, .portrait, .docked(.large)), (1376, .landscape, .docked(.large)),
            (320, .portrait, .floating),
        ]
        for (width, orientation, pad) in placements {
            for layer in layers {
                for field in fields {
                    for (globe, emoji) in [(true, true), (true, false), (false, true)] {
                        layouts.append(KeyboardLayout(
                            width: width, orientation: orientation, layer: layer, field: field,
                            showsGlobe: globe, showsEmoji: emoji, pad: pad
                        ))
                    }
                }
            }
            for language in KeyboardLanguage.allCases {
                layouts.append(KeyboardLayout(width: width, orientation: orientation, showsGlobe: true, language: language, pad: pad))
            }
        }
        return layouts
    }

    func testPadPlacementFollowsTheScreenAndTheKeyboardsWidth() {
        XCTAssertEqual(KeyboardPadSize.current(screenSize: CGSize(width: 744, height: 1133)), .mini)
        XCTAssertEqual(KeyboardPadSize.current(screenSize: CGSize(width: 1133, height: 744)), .mini)
        XCTAssertEqual(KeyboardPadSize.current(screenSize: CGSize(width: 834, height: 1210)), .medium)
        XCTAssertEqual(KeyboardPadSize.current(screenSize: CGSize(width: 820, height: 1180)), .medium)
        XCTAssertEqual(KeyboardPadSize.current(screenSize: CGSize(width: 1032, height: 1376)), .large)
        XCTAssertEqual(KeyboardPadSize.current(screenSize: CGSize(width: 1366, height: 1024)), .large)

        let portrait = CGSize(width: 834, height: 1210), turned = CGSize(width: 1210, height: 834)
        XCTAssertEqual(KeyboardPadPlacement.current(screenSize: portrait, keyboardWidth: 834), .docked(.medium))
        XCTAssertEqual(KeyboardPadPlacement.current(screenSize: portrait, keyboardWidth: 0), .docked(.medium))
        XCTAssertEqual(KeyboardPadPlacement.current(screenSize: turned, keyboardWidth: 320), .floating)
        XCTAssertEqual(KeyboardPadPlacement.orientation(screenSize: portrait, placement: .docked(.medium), keyboardWidth: 834), .portrait)
        XCTAssertEqual(KeyboardPadPlacement.orientation(screenSize: turned, placement: .docked(.medium), keyboardWidth: 1210), .landscape)
        XCTAssertEqual(KeyboardPadPlacement.orientation(screenSize: turned, placement: .docked(.medium)), .landscape)
        XCTAssertEqual(KeyboardPadPlacement.orientation(screenSize: turned, placement: .floating, keyboardWidth: 320), .portrait)
    }

    /// Apple's iPad keyboards beside Hex's in the same field (iPadOS 27.2 simulator: iPad mini
    /// A17 Pro, iPad Pro 11-inch and 13-inch M5), read off screenshots in screen points: the top
    /// of Apple's q, a, z and space rows, their keycaps' height, and their side inset and gap.
    /// iPadOS ends a third-party keyboard's view 20 pt above the screen's bottom.
    func testPadDockedRowsSitOnApplesRows() {
        let apple: [(KeyboardPadPlacement, KeyboardOrientation, CGSize, [CGFloat], CGFloat, CGFloat, CGFloat)] = [
            (.docked(.mini), .portrait, CGSize(width: 744, height: 1133), [856, 920.5, 985, 1049.5], 55.5, 6, 12),
            (.docked(.mini), .landscape, CGSize(width: 1133, height: 744), [382, 468, 553.5, 639.5], 74.75, 7, 14),
            (.docked(.medium), .portrait, CGSize(width: 834, height: 1210), [933, 997.5, 1062, 1126.5], 55.5, 9, 10),
            (.docked(.medium), .landscape, CGSize(width: 1210, height: 834), [472, 558, 643.5, 729.5], 73.75, 15, 14),
            (.docked(.large), .portrait, CGSize(width: 1032, height: 1376), [1086.5, 1154.5, 1222.5, 1291], 61.1, 3.5, 7),
            (.docked(.large), .landscape, CGSize(width: 1376, height: 1032), [665, 753, 841, 929], 79, 5, 10),
        ]
        for (pad, orientation, screen, tops, height, side, gap) in apple {
            let layout = KeyboardLayout(width: screen.width, orientation: orientation, showsGlobe: true, showsEmoji: true, pad: pad)
            let label = "\(pad) \(orientation)"
            let viewTop = screen.height - 20 - layout.totalHeight
            for (row, kind) in [KeyKind.character("q"), .character("a"), .character("z"), .space].enumerated() {
                let key = layout.key(kind)!.frame
                XCTAssertEqual(viewTop + key.minY, tops[row], accuracy: 0.34, "\(label) row \(row)")
                XCTAssertEqual(key.height, height, accuracy: 0.34, "\(label) row \(row)")
            }
            let q = layout.key(.character("q"))!.frame, w = layout.key(.character("w"))!.frame
            XCTAssertEqual(q.minX, side, accuracy: 0.001, label)
            XCTAssertEqual(w.minX - q.maxX, gap, accuracy: 0.001, label)
            XCTAssertEqual(layout.key(.delete)!.frame.maxX, screen.width - side, accuracy: 0.001, label)
            // The strip, Apple's 44 pt bar, under 44 pt kept clear for iPadOS's shortcut bar.
            XCTAssertEqual(layout.metrics.stripTop, 44, label)
            XCTAssertEqual(layout.stripFrame.height, 44, label)
            XCTAssertFalse(layout.showsBalloons, label)
            XCTAssertNil(layout.balloon(for: layout.key(.character("g"))!), label)
            XCTAssertEqual(KeyboardLayout.height(for: orientation, pad: pad), layout.totalHeight, label)
        }
    }

    /// Apple's floating keyboard (the same 320 pt on every iPad): 26 pt keycaps on a 54.3 pt
    /// pitch, 4 pt in, shift and delete 35.5 pt, 123 and return 73.5 pt with space 153 pt
    /// between, all in 271 pt under its 44 pt bar; Hex's strip takes the bar's place.
    /// The docked iPad's own rows against Apple's iPad mini keyboard (iPadOS 27.2 simulator,
    /// screenshots): every key's width and where each row starts, English, German and French,
    /// upright and turned. Hex's bottom row is Apple's with the Hex key where its dictation key
    /// is and emoji (when on) where its second 123 is.
    func testPadMiniRowsMatchApples() {
        struct Apple { let language: KeyboardLanguage; let orientation: KeyboardOrientation; let width: CGFloat
            let letter: CGFloat; let delete: CGFloat; let rowTwoX: CGFloat; let returnW: CGFloat; let shift: CGFloat; let rightShift: CGFloat; let bottom: [CGFloat] }
        let apple = [
            Apple(language: .english, orientation: .portrait, width: 744, letter: 54.5, delete: 67.5, rowTwoX: 32.5, returnW: 107.5, shift: 54.5, rightShift: 67.5, bottom: [57, 57, 57, 318, 91.5, 91.5]),
            Apple(language: .german, orientation: .portrait, width: 744, letter: 54.5, delete: 67.5, rowTwoX: 32.5, returnW: 107.5, shift: 54.5, rightShift: 67.5, bottom: [57, 57, 57, 318, 91.5, 91.5]),
            Apple(language: .french, orientation: .portrait, width: 744, letter: 53.25, delete: 79.5, rowTwoX: 19, returnW: 66.5, shift: 56.5, rightShift: 76, bottom: [57, 57, 57, 318, 91.5, 91.5]),
            Apple(language: .english, orientation: .landscape, width: 1133, letter: 87.1, delete: 107.5, rowTwoX: 47.5, returnW: 168, shift: 87.1, rightShift: 107.5, bottom: [89, 89, 88.5, 502, 140, 140.5]),
            Apple(language: .german, orientation: .landscape, width: 1133, letter: 87.1, delete: 107.5, rowTwoX: 47.5, returnW: 168, shift: 87.1, rightShift: 107.5, bottom: [89, 89, 88.5, 502, 140, 140.5]),
            Apple(language: .french, orientation: .landscape, width: 1133, letter: 85.4, delete: 125, rowTwoX: 27, returnW: 105.5, shift: 90.5, rightShift: 120, bottom: [89, 89, 88.5, 502, 140, 140.5]),
        ]
        for a in apple {
            let layout = KeyboardLayout(width: a.width, orientation: a.orientation, showsGlobe: true, showsEmoji: true, language: a.language, pad: .docked(.mini))
            let label = "\(a.language) \(a.orientation)"
            let rows = (0..<4).map { r in layout.keys.filter { $0.row == r }.sorted { $0.frame.minX < $1.frame.minX } }
            XCTAssertEqual(layout.letterWidth, a.letter, accuracy: 0.5, label)
            XCTAssertEqual(rows[0].last?.kind, .delete, label)
            XCTAssertEqual(rows[0].last!.frame.width, a.delete, accuracy: 1, label)
            XCTAssertEqual(rows[1].first!.frame.minX, a.rowTwoX, accuracy: 0.75, label)
            XCTAssertEqual(rows[1].last?.kind, .returnKey, label)
            XCTAssertEqual(rows[1].last!.frame.width, a.returnW, accuracy: 1, label)
            XCTAssertEqual(rows[2].first?.kind, .shift, label)
            XCTAssertEqual(rows[2].last?.kind, .shift, label)
            XCTAssertEqual(rows[2].first!.frame.width, a.shift, accuracy: 0.75, label)
            XCTAssertEqual(rows[2].last!.frame.width, a.rightShift, accuracy: 1, label)
            XCTAssertEqual(rows[2].count, 11, label)
            XCTAssertEqual(rows[3].map(\.kind), [.globe, .layerSwitch(.numbers), .hex, .space, .emoji, .dismiss], label)
            for (key, width) in zip(rows[3], a.bottom) { XCTAssertEqual(key.frame.width, width, accuracy: 1, "\(label) \(key.label)") }
            XCTAssertNil(layout.key(.hex).flatMap { $0.row < 0 ? $0 : nil }, "no Hex key in the strip: \(label)")
            XCTAssertEqual(layout.stripFrame.width, layout.stripBarFrame.width, label)
        }
        // German on an iPad is QWERTZ without ü ö ä, held on u o a; French ends row 3 in , . and '.
        let german = KeyboardLayout(width: 744, language: .german, pad: .docked(.mini))
        XCTAssertEqual(german.letters, .qwertz)
        XCTAssertNil(german.key(.character("ü")))
        XCTAssertEqual(german.keys.filter { $0.row == 2 }.compactMap(\.character), ["y", "x", "c", "v", "b", "n", "m", ",", "."])
        XCTAssertEqual(AccentCatalog.options(for: "u", uppercase: false, language: .german, letters: german.letters)?.dropFirst().first, "ü")
        let french = KeyboardLayout(width: 744, language: .french, pad: .docked(.mini))
        XCTAssertEqual(french.keys.filter { $0.row == 2 }.compactMap { key in key.shifted.map { "\(key.character!)\($0)" } }, [",?", ".!", "':"])
        let english = KeyboardLayout(width: 744, pad: .docked(.mini))
        XCTAssertEqual(english.key(.character(","))?.shifted, "!")
        XCTAssertEqual(english.key(.character("."))?.shifted, "?")
        // The 123 and #+= pages: digits and delete, Apple's nine symbols and return, a page key
        // either side of row 3; ABC where 123 was.
        let numbers = KeyboardLayout(width: 744, layer: .numbers, showsGlobe: true, pad: .docked(.mini))
        XCTAssertEqual(numbers.keys.filter { $0.row == 1 }.compactMap(\.character), ["@", "#", "$", "&", "*", "(", ")", "'", "\""])
        XCTAssertEqual(numbers.keys.filter { $0.row == 2 && $0.kind == .layerSwitch(.symbols) }.count, 2)
        let symbols = KeyboardLayout(width: 744, layer: .symbols, showsGlobe: true, pad: .docked(.mini))
        XCTAssertEqual(symbols.keys.filter { $0.row == 1 }.compactMap(\.character), ["€", "£", "¥", "_", "^", "[", "]", "{", "}"])
        XCTAssertEqual(symbols.keys.filter { $0.row == 3 && $0.kind == .layerSwitch(.letters) }.count, 2)
        // Settings › Keyboard › Number row is iPhone only: on an iPad, docked or floating, nothing changes.
        for (width, orientation, pad) in [(744.0, KeyboardOrientation.portrait, KeyboardPadPlacement.docked(.mini)), (1376, .landscape, .docked(.large)), (320, .portrait, .floating)] {
            XCTAssertEqual(
                KeyboardLayout(width: width, orientation: orientation, showsGlobe: true, pad: pad, numberRow: true),
                KeyboardLayout(width: width, orientation: orientation, showsGlobe: true, pad: pad),
                "\(pad)"
            )
        }
        // Every language's rows fit the mini, upright and turned, and the 11- and 13-inch use them.
        for language in KeyboardLanguage.allCases {
            for letters in language.letterLayouts {
                for (width, orientation, pad) in [(744.0, KeyboardOrientation.portrait, KeyboardPadPlacement.docked(.mini)), (1133, .landscape, .docked(.mini)), (834, .portrait, .docked(.medium)), (1376, .landscape, .docked(.large))] {
                    let layout = KeyboardLayout(width: width, orientation: orientation, showsGlobe: true, language: language, letters: letters, pad: pad)
                    let label = "\(language) \(letters) \(width)"
                    for row in 0..<4 {
                        let keys = layout.keys.filter { $0.row == row }.sorted { $0.frame.minX < $1.frame.minX }
                        XCTAssertEqual(keys.last!.frame.maxX, width - layout.metrics.side, accuracy: 0.01, "\(label) row \(row)")
                        XCTAssertGreaterThanOrEqual(keys.first!.frame.minX, layout.metrics.side - 0.01, "\(label) row \(row)")
                        for (l, r) in zip(keys, keys.dropFirst()) { XCTAssertEqual(r.frame.minX - l.frame.maxX, layout.metrics.gap, accuracy: 0.01, "\(label) row \(row)") }
                        XCTAssertTrue(keys.allSatisfy { $0.frame.width >= layout.letterWidth - 0.01 }, "\(label) row \(row)")
                    }
                }
            }
        }
    }

    /// Docked on an iPad a held key's accents sit in Apple's cells (30 to 46.5 pt, 53 pt tall),
    /// two rows once there are more than five, the key's own option nearest the key and first
    /// picked, every menu inside the keyboard; floating and on an iPhone, cells stay key-wide.
    func testPadAccentMenusUseApplesCellsInsideTheKeyboard() {
        let cells: [(CGFloat, KeyboardOrientation, KeyboardPadPlacement, CGFloat)] = [
            (744, .portrait, .docked(.mini), 30), (1133, .landscape, .docked(.mini), 46.5),
            (834, .portrait, .docked(.medium), 31), (1210, .landscape, .docked(.medium), 43.5),
            (1032, .portrait, .docked(.large), 34.5), (1376, .landscape, .docked(.large), 45.5),
        ]
        for (width, orientation, pad, cell) in cells {
            let layout = KeyboardLayout(width: width, orientation: orientation, pad: pad)
            XCTAssertEqual(layout.metrics.accentCellWidth, cell, "\(pad) \(orientation)")
            let bounds = CGRect(x: 0, y: 0, width: layout.width, height: layout.totalHeight).insetBy(dx: -0.01, dy: -0.01)
            for ch in ["e", "a", "i", "o", "u", "n", "c", "s", "z", "p", "q"] {
                guard let key = layout.key(.character(ch)), let options = AccentCatalog.options(for: ch, uppercase: false) else { continue }
                let arrangement = AccentMenu.padArrangement(options, opensLeft: key.frame.midX > width * 0.52)
                let menu = AccentMenu.build(for: key.frame, arrangement: arrangement, in: width, cellWidth: cell, cellHeight: AccentMenu.padCellHeight)
                let label = "\(pad) \(orientation) \(ch)"
                XCTAssertTrue(bounds.contains(menu.bubbleFrame), "\(label): \(menu.bubbleFrame)")
                XCTAssertEqual(menu.selectedOption, ch, label)
                XCTAssertEqual(menu.cellFrames.first?.width ?? 0, cell, accuracy: 0.001, label)
                XCTAssertEqual(menu.cellFrames.first?.height, AccentMenu.padCellHeight, label)
                XCTAssertEqual(arrangement.rows.count, options.count > 5 ? 2 : 1, label)
                XCTAssertEqual(menu.cellFrames[menu.selectedIndex].midX, key.frame.midX, accuracy: cell, label)
            }
        }
        XCTAssertNil(KeyboardLayout(width: 320, pad: .floating).metrics.accentCellWidth)
        XCTAssertNil(KeyboardLayout(width: 402).metrics.accentCellWidth)
        XCTAssertEqual(
            AccentMenu.padArrangement(["e", "è", "é", "ê", "ë", "ē", "ė", "ę", "ě"], opensLeft: false),
            AccentArrangement(rows: [["ē", "ė", "ę", "ě"], ["e", "è", "é", "ê", "ë"]], baseRow: 1, baseColumn: 0)
        )
    }

    func testPadFloatingIsApplesFloatingKeyboard() {
        let layout = KeyboardLayout(width: 320, showsGlobe: false, pad: .floating)
        XCTAssertEqual(layout.totalHeight, 271, accuracy: 0.01)
        XCTAssertEqual(KeyboardLayout.height(for: .portrait, pad: .floating), layout.totalHeight)
        let q = layout.key(.character("q"))!.frame
        XCTAssertEqual(q.minX, 4, accuracy: 0.001)
        XCTAssertEqual(q.width, 25.8, accuracy: 0.001)
        XCTAssertEqual(q.height, 38.25, accuracy: 0.001)
        XCTAssertEqual(layout.key(.character("a"))!.frame.minY - q.minY, 54.33, accuracy: 0.01)
        XCTAssertEqual(layout.key(.shift)!.frame.width, 35.5, accuracy: 0.1)
        XCTAssertEqual(layout.key(.delete)!.frame.width, 35.5, accuracy: 0.1)
        XCTAssertEqual(layout.key(.layerSwitch(.numbers))!.frame.width, 73.5, accuracy: 0.5)
        XCTAssertEqual(layout.key(.space)!.frame.width, 153, accuracy: 0.5)
        XCTAssertEqual(layout.key(.returnKey)!.frame.width, 73.5, accuracy: 0.5)
        XCTAssertEqual(layout.stripFrame.minY, 0)
        XCTAssertEqual(layout.stripFrame.height, 44)
        XCTAssertNil(layout.key(.dismiss))
        XCTAssertTrue(layout.showsBalloons)
    }

    /// Docked on an iPad the bottom row ends in Apple's hide-keyboard key, one letter wide, on
    /// every page and field; floating and on an iPhone there is none.
    func testHideKeyboardKeyEndsTheDockedIPadsBottomRow() {
        for layout in padLayouts() {
            let dismiss = layout.key(.dismiss)
            guard layout.metrics.pad?.isDocked == true else {
                XCTAssertNil(dismiss, describe(layout))
                continue
            }
            guard let key = dismiss?.frame else {
                XCTFail("no hide-keyboard key: \(describe(layout))")
                continue
            }
            XCTAssertEqual(key.maxX, layout.width - layout.metrics.side, accuracy: 0.001, describe(layout))
            XCTAssertEqual(key.width, (layout.orientation == .landscape ? 1.61 : 1.679) * (layout.width - 2 * layout.metrics.side - 10 * layout.metrics.gap) / 11.236, accuracy: 0.001, describe(layout))
            XCTAssertEqual(layout.keys.filter { $0.row == 3 }.max { $0.frame.maxX < $1.frame.maxX }?.kind, .dismiss, describe(layout))
        }
        for layout in everyPortraitLayout() + landscapeLayouts() + duoLayouts() {
            XCTAssertNil(layout.key(.dismiss), describe(layout))
            XCTAssertNil(layout.metrics.pad, describe(layout))
        }
        XCTAssertEqual(KeyboardLayout(width: 402).metrics, .portrait)
    }

    func testPadTargetsTileTheKeyAreaAndKeycapsNeverOverlap() {
        for layout in padLayouts() {
            let area = layout.touchAreaFrame
            var rowTop = area.minY
            for row in 0..<4 {
                let targets = layout.keys.filter { $0.row == row }.sorted { $0.hitFrame.minX < $1.hitFrame.minX }
                    .filter { !($0.kind == .emoji && row == 3) }.map(\.hitFrame)
                XCTAssertEqual(targets.first?.minX, 0, describe(layout))
                XCTAssertEqual(targets.last?.maxX, layout.width, describe(layout))
                for target in targets { XCTAssertEqual(target.minY, rowTop, accuracy: 0.0001, "\(describe(layout)) row \(row)") }
                rowTop = targets[0].maxY
            }
            XCTAssertEqual(rowTop, area.maxY, accuracy: 0.0001, describe(layout))
            let bounds = CGRect(x: 0, y: 0, width: layout.width, height: layout.totalHeight).insetBy(dx: -0.001, dy: -0.001)
            for (i, a) in layout.keys.enumerated() {
                // Apple's emoji key takes only the middle of its keycap (`testEmojiKeyTakesOnlyTheMiddleOfItsKeycapAsApplesDoes`).
                if a.kind != .emoji {
                    XCTAssertTrue(a.hitFrame.insetBy(dx: -0.001, dy: -0.001).contains(a.frame), "\(describe(layout)) \(a.label)")
                }
                XCTAssertTrue(bounds.contains(a.frame), "\(describe(layout)) \(a.label)")
                for b in layout.keys[(i + 1)...] {
                    XCTAssertFalse(a.frame.intersects(b.frame), "\(describe(layout)) \(a.label) × \(b.label)")
                }
            }
            // Nothing of Hex's in the room kept clear above a docked strip.
            if layout.metrics.pad?.isDocked == true {
                XCTAssertTrue(layout.keys.allSatisfy { $0.hitFrame.minY >= KeyboardMetrics.padClearance }, describe(layout))
            }
        }
    }

    // MARK: - Number row (Settings › Keyboard › Number row)

    private func assertSameRect(_ a: CGRect, _ b: CGRect, _ message: String, file: StaticString = #filePath, line: UInt = #line) {
        for (x, y) in [(a.minX, b.minX), (a.minY, b.minY), (a.width, b.width), (a.height, b.height)] {
            XCTAssertEqual(x, y, accuracy: 0.0001, "\(message): \(a) vs \(b)", file: file, line: line)
        }
    }

    /// Each keyboard with the number row on and off, and its 123 page with the row on.
    private func numberRowLayouts() -> [(on: KeyboardLayout, off: KeyboardLayout, numbers: KeyboardLayout)] {
        var pairs: [(KeyboardLayout, KeyboardLayout, KeyboardLayout)] = []
        func pair(_ layer: KeyboardLayer = .letters, _ make: (Bool, KeyboardLayer) -> KeyboardLayout) {
            pairs.append((make(true, layer), make(false, layer), make(true, .numbers)))
        }
        for width in portraitWidths {
            for layer in layers {
                for size in [KeyboardPhoneSize.regular, .large, .homeButton] {
                    pair(layer) { KeyboardLayout(width: width, layer: $1, showsEmoji: true, phoneSize: size, numberRow: $0) }
                }
                pair(layer) { KeyboardLayout(width: width, layer: $1, field: .email, showsGlobe: true, numberRow: $0) }
            }
            for language in KeyboardLanguage.allCases {
                pair { KeyboardLayout(width: width, layer: $1, language: language, numberRow: $0) }
                for letters in language.letterLayouts.dropFirst() {
                    pair { KeyboardLayout(width: width, layer: $1, language: language, letters: letters, numberRow: $0) }
                }
            }
        }
        for layer in layers {
            pair(layer) { KeyboardLayout(width: 874, orientation: .landscape, layer: $1, horizontalInset: 62, numberRow: $0) }
            pair(layer) { KeyboardLayout(width: 667, orientation: .landscape, layer: $1, phoneSize: .homeButton, numberRow: $0) }
            for (width, posture, inset) in [(452.0, KeyboardPosture.cover, 7.0), (655, .unfoldedTall, 7), (801, .unfoldedWide, 75)] {
                pair(layer) { KeyboardLayout(width: width, layer: $1, showsGlobe: true, posture: posture, windowInset: inset, numberRow: $0) }
            }
            // The unfolded keyboard joined (Settings › Experiments): its rows never split.
            pair(layer) { KeyboardLayout(width: 801, layer: $1, showsGlobe: true, posture: .unfoldedWide, windowInset: 75, numberRow: $0, joinsUnfoldedKeyboard: true) }
        }
        return pairs
    }

    /// Off (the default) the number row changes nothing, and an iPad never gets one yet.
    func testNumberRowOffLeavesEveryKeyOnApplesRows() {
        for width in portraitWidths {
            XCTAssertEqual(KeyboardLayout(width: width, numberRow: false), KeyboardLayout(width: width))
            XCTAssertEqual(KeyboardLayout(width: width, numberRow: false).totalHeight, 247)
        }
        XCTAssertEqual(KeyboardMetrics.for(.portrait).numberRowHeight, 0)
        XCTAssertEqual(KeyboardLayout.height(for: .portrait, numberRow: false), 247)
        for placement in [KeyboardPadPlacement.docked(.mini), .docked(.large), .floating] {
            let width: CGFloat = placement == .floating ? 320 : 744
            XCTAssertEqual(
                KeyboardLayout(width: width, pad: placement, numberRow: true),
                KeyboardLayout(width: width, pad: placement),
                "\(placement)"
            )
        }
    }

    /// On, 1 to 0 sit above row 1 on the 123 page's digit columns, 0.85 of a letter row's pitch
    /// tall with the row gap under them: the keyboard grows upward by that pitch (45.9 pt on a
    /// 17 Pro, 34 pt turned) and every other key keeps its place counted from the bottom, which
    /// is where iOS keeps a keyboard, so the strip moves up with the keyboard's top.
    func testNumberRowGrowsTheKeyboardUpwardAndLeavesEveryOtherKeyInPlace() {
        XCTAssertEqual(KeyboardLayout.height(for: .portrait, numberRow: true), 247 + 45.9, accuracy: 0.001)
        XCTAssertEqual(KeyboardLayout.height(for: .landscape, numberRow: true), 204 + 34, accuracy: 0.001)
        XCTAssertEqual(KeyboardMetrics.for(.portrait, numberRow: true).numberRowHeight, 34.9, accuracy: 0.001)
        for (on, off, numbers) in numberRowLayouts() {
            let m = on.metrics
            let pitch = KeyboardMetrics.numberRowPitchRatio * (m.keyHeight + m.rowGap)
            XCTAssertTrue(on.hasNumberRow, describe(on))
            XCTAssertFalse(off.hasNumberRow, describe(off))
            XCTAssertEqual(on.totalHeight - off.totalHeight, pitch, accuracy: 0.001, describe(on))
            XCTAssertEqual(m.numberRowPitch, pitch, accuracy: 0.001, describe(on))
            XCTAssertEqual(on.stripFrame, off.stripFrame, describe(on))
            let others = on.keys.filter { !$0.isNumberRow }
            XCTAssertEqual(others.count, off.keys.count, describe(on))
            for (a, b) in zip(others, off.keys) {
                XCTAssertEqual(a.kind, b.kind, describe(on))
                let shift = b.row < 0 ? 0 : pitch
                assertSameRect(a.frame, b.frame.offsetBy(dx: 0, dy: shift), "\(describe(on)) \(a.label)")
                if b.row > 0 { assertSameRect(a.hitFrame, b.hitFrame.offsetBy(dx: 0, dy: shift), "\(describe(on)) \(a.label)") }
            }

            let digits = on.keys.filter(\.isNumberRow)
            guard on.layer != .numbers else {
                XCTAssertTrue(digits.isEmpty, "the 123 page's own row 1 is the digits: \(describe(on))")
                continue
            }
            XCTAssertEqual(digits.map(\.label), KeyboardLayout.numberRowDigits, describe(on))
            XCTAssertEqual(digits.map(\.kind), KeyboardLayout.numberRowDigits.map { .character($0) }, describe(on))
            for key in digits {
                XCTAssertEqual(key.frame.minY, m.stripArea, accuracy: 0.001, describe(on))
                XCTAssertEqual(key.frame.height, m.numberRowHeight, accuracy: 0.001, describe(on))
            }
            // Same widths and gaps as the 123 page's digits (row 1's letter columns), and the 123
            // page keeps the keyboard's height.
            XCTAssertEqual(numbers.totalHeight, on.totalHeight, accuracy: 0.001, describe(on))
            let numberDigits = numbers.keys.filter { $0.row == 0 }
            XCTAssertEqual(digits.map(\.frame.minX), numberDigits.map(\.frame.minX), describe(on))
            XCTAssertEqual(digits.map(\.frame.width), numberDigits.map(\.frame.width), describe(on))
        }
    }

    /// With the number row the touch targets still tile the keys with no gaps: the digits take
    /// the gap under the strip, as row 1 did, and row 1 starts halfway down the gap under them.
    /// On the 123 page the band stays (the keyboard keeps its height) and its row 1, the same
    /// digits, reaches up through it, so a tap where a digit was types that digit.
    func testNumberRowTargetsTileAndThe123PageReachesUpThroughItsBand() {
        var generator = SystemRandomNumberGenerator()
        for (layout, _, _) in numberRowLayouts() {
            let area = layout.touchAreaFrame
            XCTAssertEqual(area.minY, layout.metrics.stripArea - layout.rowOneReach, accuracy: 0.001, describe(layout))
            let top = layout.hasNumberRow && layout.layer != .numbers ? KeyboardLayout.numberRowIndex : 0
            for key in layout.keys where key.row == top {
                XCTAssertEqual(key.hitFrame.minY, area.minY, accuracy: 0.001, "\(describe(layout)) \(key.label)")
            }
            for key in layout.keys where key.isNumberRow {
                let rowOne = layout.keys.first { $0.row == 0 }!
                XCTAssertEqual(key.hitFrame.maxY, rowOne.frame.minY - layout.metrics.rowGap / 2, accuracy: 0.001, describe(layout))
                XCTAssertGreaterThanOrEqual(key.hitFrame.height, 0.8 * (layout.metrics.keyHeight + layout.metrics.rowGap) - 0.01, describe(layout))
                XCTAssertEqual(layout.balloon(for: key) == nil, !layout.showsBalloons, describe(layout))
            }
            for _ in 0..<(layout.isSplit ? 0 : 300) {
                let point = CGPoint(
                    x: CGFloat.random(in: 0..<layout.width, using: &generator),
                    y: CGFloat.random(in: area.minY..<area.maxY, using: &generator)
                )
                let hits = layout.keys.filter { $0.hitFrame.contains(point) }
                let emoji = layout.key(.emoji)
                let inEmojiMargin = emoji.map { $0.row == 3 && !$0.hitFrame.contains(point) && point.y >= $0.frame.minY - layout.metrics.rowGap / 2
                    && point.x >= $0.frame.minX - layout.metrics.gap / 2 && point.x < $0.frame.maxX + layout.metrics.gap / 2 } ?? false
                if !inEmojiMargin {
                    XCTAssertEqual(hits.count, 1, "\(describe(layout)) at \(point): \(hits.map(\.label))")
                } else {
                    XCTAssertLessThanOrEqual(hits.count, 1, "\(describe(layout)) at \(point)")
                }
            }
            if layout.layer == .numbers, !layout.isSplit {
                let three = layout.key(.character("3"))!
                let band = CGPoint(x: three.frame.midX, y: layout.metrics.stripArea + layout.metrics.numberRowHeight / 2)
                XCTAssertEqual(layout.hitTest(band)?.kind, .character("3"), describe(layout))
            }
            for (i, a) in layout.keys.enumerated() {
                if a.kind != .emoji {
                    XCTAssertTrue(a.hitFrame.insetBy(dx: -0.001, dy: -0.001).contains(a.frame), "\(describe(layout)) \(a.label)")
                }
                for b in layout.keys[(i + 1)...] {
                    XCTAssertFalse(a.frame.intersects(b.frame), "\(describe(layout)) \(a.label) × \(b.label)")
                }
            }
        }
    }

    /// The Duo's split keyboard splits the number row over row 1's halves: 1 to 5 and 6 to 0.
    func testNumberRowSplitsWithTheKeyboard() {
        let layout = KeyboardLayout(width: 801, showsGlobe: true, posture: .unfoldedWide, windowInset: 75, numberRow: true)
        guard let split = layout.split else { return XCTFail("not split") }
        let digits = layout.keys.filter(\.isNumberRow)
        XCTAssertEqual(digits.filter { $0.frame.midX < split.middle }.map(\.label), ["1", "2", "3", "4", "5"])
        XCTAssertEqual(digits.filter { $0.frame.midX > split.middle }.map(\.label), ["6", "7", "8", "9", "0"])
        XCTAssertEqual(layout.key(.character("5"))?.frame.maxX ?? 0, split.leftEdge, accuracy: 0.001)
        XCTAssertEqual(layout.key(.character("6"))?.frame.minX ?? 0, split.rightEdge, accuracy: 0.001)
    }

    /// Where iOS gives the keyboard less room than it asked for, the number row shrinks with the
    /// other rows, under the same strip, and everything still fits.
    func testNumberRowFitsAShorterViewWithTheOtherRows() {
        for orientation in [KeyboardOrientation.portrait, .landscape] {
            for size in [KeyboardPhoneSize.regular, .large, .homeButton] {
                let width: CGFloat = orientation == .portrait ? 393 : 852
                let full = KeyboardLayout(width: width, orientation: orientation, showsEmoji: true, phoneSize: size, numberRow: true)
                XCTAssertEqual(KeyboardLayout(width: width, orientation: orientation, showsEmoji: true, phoneSize: size, numberRow: true, availableHeight: full.totalHeight), full)
                let height = full.totalHeight - 20
                let short = KeyboardLayout(width: width, orientation: orientation, showsEmoji: true, phoneSize: size, numberRow: true, availableHeight: height)
                XCTAssertEqual(short.totalHeight, height, accuracy: 0.01, "\(size) \(orientation)")
                XCTAssertEqual(short.stripFrame, full.stripFrame)
                XCTAssertEqual(short.key(.character("1"))?.frame.minY, full.key(.character("1"))?.frame.minY)
                XCTAssertLessThan(short.metrics.numberRowHeight, full.metrics.numberRowHeight)
                XCTAssertEqual(
                    short.metrics.numberRowHeight / short.metrics.keyHeight,
                    full.metrics.numberRowHeight / full.metrics.keyHeight, accuracy: 0.001
                )
                for key in short.keys where key.row != -1 {
                    XCTAssertLessThanOrEqual(key.frame.maxY, height + 0.01, "\(size) \(key.label)")
                }
                XCTAssertEqual(short.key(.space)?.hitFrame.maxY ?? 0, height, accuracy: 0.01)
            }
        }
    }

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
        // The unfolded keyboard joined (Settings › Experiments), its rows' heights kept: every
        // language, the globe and emoji on and off, and the Hex key either side. The number row
        // has its own layouts (`numberRowLayouts`), since these suites assume none.
        for language in KeyboardLanguage.allCases {
            layouts.append(KeyboardLayout(
                width: 801, showsGlobe: true, language: language, posture: .unfoldedWide,
                windowInset: 75, joinsUnfoldedKeyboard: true
            ))
        }
        for (globe, emoji) in [(true, false), (false, true), (false, false)] {
            layouts.append(KeyboardLayout(
                width: 801, showsGlobe: globe, showsEmoji: emoji, posture: .unfoldedWide,
                windowInset: 75, joinsUnfoldedKeyboard: true
            ))
        }
        for placement in [HexKeyPlacement.leftOfSpace, .rightOfSpace] {
            layouts.append(KeyboardLayout(
                width: 801, showsGlobe: true, showsEmoji: true, posture: .unfoldedWide,
                windowInset: 75, hexPlacement: placement, joinsUnfoldedKeyboard: true
            ))
        }
        for layer in layers {
            for field in fields {
                layouts.append(KeyboardLayout(
                    width: 801, layer: layer, field: field, showsGlobe: true, showsEmoji: true,
                    posture: .unfoldedWide, windowInset: 75, joinsUnfoldedKeyboard: true
                ))
            }
        }
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

    // MARK: - Backdrop

    /// Hex draws its backdrop only over an earlier Hex keyboard that iOS never took off screen
    /// in the same field: the one case where iOS leaves Apple's keyboard under a third-party
    /// one (a field with an accessory view that takes focus as its screen appears, opened with
    /// Hex, then Hex, Apple's keyboard, Hex; iOS 27.0 and 27.2 simulators). Everywhere else
    /// the keyboard is clear, so it is the colour of iOS's glass panel on any background.
    func testBackdropShowsOnlyOverAnEarlierKeyboardLeftOnScreen() {
        let field = UUID(), other = UUID()
        var keyboards = KeyboardsOnScreen()
        XCTAssertFalse(keyboards.isOverAnother(1), "not on screen")

        // A field opens with Hex: one keyboard, clear.
        keyboards.appeared(1, in: field)
        XCTAssertFalse(keyboards.isOverAnother(1))

        // Dismissed and opened again (iOS says it disappeared, then makes a new one): clear.
        keyboards.disappeared(1)
        keyboards.appeared(2, in: field)
        XCTAssertFalse(keyboards.isOverAnother(2))
        XCTAssertEqual(keyboards.count, 1)

        // Switched to Apple's keyboard and back where iOS never says the first one disappeared:
        // the new keyboard is over it, and the old one under it is superseded so it retires.
        keyboards.appeared(3, in: field)
        XCTAssertTrue(keyboards.isOverAnother(3))
        XCTAssertFalse(keyboards.isOverAnother(2))
        XCTAssertTrue(keyboards.isSuperseded(2))
        XCTAssertFalse(keyboards.isSuperseded(3))
        // Again: over both, and both earlier ones are superseded while another field's is not.
        keyboards.appeared(4, in: field)
        keyboards.appeared(9, in: other)
        XCTAssertTrue(keyboards.isOverAnother(4))
        XCTAssertTrue(keyboards.isOverAnother(3))
        XCTAssertTrue(keyboards.isSuperseded(2))
        XCTAssertTrue(keyboards.isSuperseded(3))
        XCTAssertFalse(keyboards.isSuperseded(4))
        XCTAssertFalse(keyboards.isSuperseded(9))
        keyboards.disappeared(9)

        // If the newest keyboard in that field disappears without iOS dismissing the earlier one,
        // the one directly under it is no longer superseded and can restore its surface.
        keyboards.disappeared(4)
        XCTAssertFalse(keyboards.isSuperseded(3))
        XCTAssertTrue(keyboards.isOverAnother(3))
        XCTAssertTrue(keyboards.isSuperseded(2))
        keyboards.disappeared(3)
        XCTAssertFalse(keyboards.isSuperseded(2))
        XCTAssertFalse(keyboards.isOverAnother(2))

        // When the host shows again, UIKit calls viewWillAppear on buried keyboards in order:
        // willAppear keeps already-superseded ones in place so they skip mounting.
        keyboards.appeared(3, in: field)
        keyboards.appeared(4, in: field)
        XCTAssertTrue(keyboards.willAppear(2, in: nil))
        XCTAssertTrue(keyboards.willAppear(3, in: field))
        XCTAssertFalse(keyboards.willAppear(4, in: field))
        XCTAssertFalse(keyboards.willAppear(6, in: field), "a new keyboard is not superseded")
        XCTAssertTrue(keyboards.isSuperseded(4))

        // On dismiss and reshow, UIKit clears onScreen in viewWillDisappear, loads a fresh
        // controller (7) with a new field UUID, then calls viewWillAppear on buried ones first.
        for keyboard in [2, 3, 4, 6] { keyboards.disappeared(keyboard) }
        XCTAssertTrue(keyboards.willAppear(2, in: field, under: 7))
        XCTAssertTrue(keyboards.willAppear(6, in: field, under: 7))
        XCTAssertFalse(keyboards.willAppear(7, in: UUID()))
        XCTAssertTrue(keyboards.isSuperseded(2))
        XCTAssertTrue(keyboards.isSuperseded(6))
        XCTAssertFalse(keyboards.isSuperseded(7))
        keyboards.disappeared(7)
        XCTAssertFalse(keyboards.isSuperseded(6), "restorable once 7 leaves")

        // The app closes and iOS frees them all: the next keyboard is clear again.
        for keyboard in [2, 3, 4, 6] { keyboards.disappeared(keyboard) }
        keyboards.appeared(5, in: other)
        XCTAssertFalse(keyboards.isOverAnother(5))
    }

    func testSharedCaptureOwnershipProtectsActiveOwnerFromStaleKeyboards() {
        var ownership = KeyboardCaptureOwnership()
        XCTAssertTrue(ownership.canAcquire(by: 1))
        ownership.acquireIdle(by: 1)
        XCTAssertEqual(ownership.owner, 1)
        XCTAssertFalse(ownership.isCapturing)

        // A second keyboard appears (unnamed field, so 1 was not retired) and starts recording.
        XCTAssertTrue(ownership.canAcquire(by: 2))
        ownership.beginCapture(by: 2)
        XCTAssertEqual(ownership.owner, 2)
        XCTAssertTrue(ownership.isCapturing)
        XCTAssertFalse(ownership.canAcquire(by: 3), "an idle preload must not steal an active capture")

        // Stale keyboard 1 hiding, receiving a memory warning, seeing termination, or deallocating
        // must not park or tear down keyboard 2's active capture.
        XCTAssertFalse(ownership.park(by: 1))
        XCTAssertFalse(ownership.evictForMemoryWarning(by: 1))
        XCTAssertFalse(ownership.dropTerminated(by: 1))
        XCTAssertFalse(ownership.ownerDidDeinit(1))
        XCTAssertEqual(ownership.owner, 2)
        XCTAssertTrue(ownership.isCapturing)
        // Nor does a memory warning on the recording owner itself evict while capturing.
        XCTAssertFalse(ownership.evictForMemoryWarning(by: 2))

        // Once 2 finishes recording and stays idle on screen, 1 still cannot evict, while 2 can
        // park it; once parked (unowned), any idle controller's memory warning may evict it.
        ownership.endCapture(by: 2)
        XCTAssertFalse(ownership.isCapturing)
        XCTAssertFalse(ownership.evictForMemoryWarning(by: 1))
        XCTAssertTrue(ownership.park(by: 2))
        XCTAssertNil(ownership.owner)
        XCTAssertTrue(ownership.evictForMemoryWarning(by: 1))
        XCTAssertFalse(ownership.hasSharedCapture)

        // If iOS frees the owning controller without viewWillDisappear, ownerDidDeinit parks it.
        ownership.acquireIdle(by: 4)
        XCTAssertTrue(ownership.ownerDidDeinit(4))
        XCTAssertNil(ownership.owner)
        XCTAssertTrue(ownership.hasSharedCapture)
    }

    /// A keyboard still on screen in another field (another app's) is not under this one. A
    /// field UIKit hasn't named yet might be the same one, so the doubt covers until it is.
    func testBackdropFollowsTheFieldEachKeyboardIsIn() {
        let field = UUID(), other = UUID()
        var keyboards = KeyboardsOnScreen()
        keyboards.appeared(1, in: other)
        keyboards.appeared(2, in: field)
        XCTAssertFalse(keyboards.isOverAnother(2), "another field")

        keyboards.appeared(3, in: nil)
        XCTAssertTrue(keyboards.isOverAnother(3), "unnamed: it may be over either")
        keyboards.moved(3, to: UUID())
        XCTAssertFalse(keyboards.isOverAnother(3), "named, and a field of its own")
        keyboards.moved(3, to: field)
        XCTAssertTrue(keyboards.isOverAnother(3), "the same field as 2")

        // An earlier keyboard iOS hasn't named counts as under every later one.
        var unnamed = KeyboardsOnScreen()
        unnamed.appeared(1, in: nil)
        unnamed.appeared(2, in: field)
        XCTAssertTrue(unnamed.isOverAnother(2))
        unnamed.moved(1, to: other)
        XCTAssertFalse(unnamed.isOverAnother(2))
        unnamed.moved(9, to: field)
        XCTAssertEqual(unnamed.count, 2, "a keyboard off screen isn't added by its field")

        // A keyboard that appears again goes to the top.
        var again = KeyboardsOnScreen()
        again.appeared(1, in: field)
        again.appeared(2, in: field)
        again.appeared(1, in: field)
        XCTAssertTrue(again.isOverAnother(1))
        XCTAssertFalse(again.isOverAnother(2))
    }

    /// Where it shows, the backdrop is a blurred material in the keyboard's own appearance,
    /// not a fixed colour: a layout iOS leaves behind the keyboard has no shapes through it
    /// (TestFlight build 56, iOS 27.0.1; iOS 27.0 and 27.2 simulators). Only the dark one is
    /// shaded, and never opaque.
    func testBackdropIsAThinMaterialInLightAndDark() {
        XCTAssertEqual(KeyboardBackdropView.blurStyle(isDark: false), .systemUltraThinMaterialLight)
        XCTAssertEqual(KeyboardBackdropView.blurStyle(isDark: true), .systemUltraThinMaterialDark)
        XCTAssertEqual(KeyboardBackdropView.shadeAlpha(isDark: false), 0)
        XCTAssertGreaterThan(KeyboardBackdropView.shadeAlpha(isDark: true), 0)
        XCTAssertLessThan(KeyboardBackdropView.shadeAlpha(isDark: true), 0.5)
    }

    /// Every keycap sits on the backdrop on every iPhone width, and so do the strip's Hex mark
    /// and the middle of each suggestion slot on the words' line, so nothing behind the
    /// keyboard shows through a key or a word.
    func testBackdropCoversEveryKeycapAndTheStripsLine() {
        for width in portraitWidths {
            let layout = KeyboardLayout(width: width)
            let outline = KeyboardBackdropView.outline(in: CGRect(x: 0, y: 0, width: width, height: layout.totalHeight))
            for key in layout.keys where key.row >= 0 {
                let frame = key.frame.insetBy(dx: 0.01, dy: 0.01)
                for corner in [CGPoint(x: frame.minX, y: frame.minY), CGPoint(x: frame.maxX, y: frame.minY),
                               CGPoint(x: frame.minX, y: frame.maxY), CGPoint(x: frame.maxX, y: frame.maxY)] {
                    XCTAssertTrue(outline.contains(corner), "width \(width) key \(key.kind) corner \(corner)")
                }
            }
            let line = layout.metrics.stripTop + layout.metrics.stripTextCenter
            let hex = layout.key(.hex)!
            XCTAssertTrue(outline.contains(CGPoint(x: hex.frame.midX - 12, y: line - 12)), "width \(width): the Hex mark")
            XCTAssertTrue(outline.contains(CGPoint(x: hex.frame.midX + 12, y: line - 12)), "width \(width): the Hex mark")
            for slot in layout.stripSlotFrames {
                XCTAssertTrue(outline.contains(CGPoint(x: slot.midX, y: line - 9)), "width \(width): a chip's top")
            }
        }
    }

    /// The backdrop stays inside Apple's panel and its 1 pt rim: its corners are the panel's
    /// 26 pt corners, 1 pt in, at the keyboard's own top edge, where the panel's edge is when
    /// iOS draws nothing above the keyboard. With iOS's 17.5 pt above, the panel is wider still
    /// at every height. It never draws over the app beside the panel's rounded corners.
    func testBackdropStaysInsideThePanelsCornersAndRim() {
        let width: CGFloat = 402
        let outline = KeyboardBackdropView.outline(in: CGRect(x: 0, y: 0, width: width, height: 247))
        let radius: CGFloat = 26
        // Around each corner's centre: inside at 24.9 pt, outside at 25.1 pt (the rim's inner edge).
        for step in 0 ... 90 {
            let angle = CGFloat(step) * .pi / 180
            for (center, sign) in [(CGPoint(x: radius, y: radius), CGFloat(-1)), (CGPoint(x: width - radius, y: radius), CGFloat(1))] {
                func point(_ r: CGFloat) -> CGPoint { CGPoint(x: center.x + sign * r * cos(angle), y: center.y - r * sin(angle)) }
                XCTAssertTrue(outline.contains(point(radius - 1.1)), "covers, \(step)° \(sign)")
                XCTAssertFalse(outline.contains(point(radius - 0.9)), "inside the rim, \(step)° \(sign)")
            }
        }
        // The panel's edge at each height, with and without iOS's own 17.5 pt above the keyboard.
        for panelTop in [CGFloat(0), -17.5] {
            func panelInset(atY y: CGFloat) -> CGFloat {
                let rise = radius + panelTop - y
                return rise > 0 ? radius - sqrt(radius * radius - rise * rise) : 0
            }
            for step in 0 ..< 120 {
                let y = CGFloat(step) * 0.25 + 0.01
                let edge = panelInset(atY: y)
                XCTAssertFalse(outline.contains(CGPoint(x: edge + 0.9, y: y)), "left, y \(y), panel top \(panelTop)")
                XCTAssertFalse(outline.contains(CGPoint(x: width - edge - 0.9, y: y)), "right, y \(y), panel top \(panelTop)")
            }
        }
        XCTAssertTrue(outline.contains(CGPoint(x: 1.05, y: 246.9)), "down to the bottom")
        XCTAssertTrue(outline.contains(CGPoint(x: width / 2, y: 1.05)), "up to the rim at the top")
        XCTAssertFalse(outline.contains(CGPoint(x: width / 2, y: 0.9)), "not over the top rim")
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

    /// The experiment is off until its switch is turned on, and it keeps its App Group key.
    func testHideWordSuggestionsIsOffByDefaultAndKeepsItsAppGroupKey() throws {
        let suite = "FullKeyboardSettingsTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = FullKeyboardSettings(defaults: defaults)

        XCTAssertFalse(settings.hideWordSuggestions)
        settings.hideWordSuggestions = true
        XCTAssertTrue(FullKeyboardSettings(defaults: defaults).hideWordSuggestions)
        XCTAssertEqual(defaults.object(forKey: "experiments.hideWordSuggestions") as? Bool, true)
    }

    /// The experiment is off until its switch is turned on, and it keeps its App Group key.
    func testDeleteSwipedWordsLetterByLetterIsOffByDefaultAndKeepsItsAppGroupKey() throws {
        let suite = "FullKeyboardSettingsTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = FullKeyboardSettings(defaults: defaults)

        XCTAssertFalse(settings.deletesSwipedWordsLetterByLetter)
        settings.deletesSwipedWordsLetterByLetter = true
        XCTAssertTrue(FullKeyboardSettings(defaults: defaults).deletesSwipedWordsLetterByLetter)
        XCTAssertEqual(defaults.object(forKey: "experiments.swipeDeleteLetterByLetter") as? Bool, true)
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

    /// Swipe typing and Emoji are on by default: an unset key (a fresh install, or a tester
    /// who never touched the old experiments) is on, a stored off stays off, and the keyboard
    /// without the App Group reads them as on.
    func testSwipeTypingAndEmojiAreOnByDefaultAndAStoredOffStaysOff() throws {
        let suite = "FullKeyboardSettingsTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        let fresh = FullKeyboardSettings(defaults: defaults)
        XCTAssertTrue(fresh.swipeTyping)
        XCTAssertTrue(fresh.emoji)
        XCTAssertNil(defaults.object(forKey: "experiments.swipeTyping"), "reading writes nothing")
        XCTAssertNil(defaults.object(forKey: "experiments.emoji"))
        XCTAssertTrue(FullKeyboardSettings(defaults: nil).swipeTyping, "without the App Group")
        XCTAssertTrue(FullKeyboardSettings(defaults: nil).emoji)

        defaults.set(false, forKey: "experiments.swipeTyping")
        defaults.set(false, forKey: "experiments.emoji")
        XCTAssertFalse(FullKeyboardSettings(defaults: defaults).swipeTyping, "an upgrade that turned it off")
        XCTAssertFalse(FullKeyboardSettings(defaults: defaults).emoji)
        fresh.applyLaunchArguments(["HEX"])
        XCTAssertFalse(fresh.swipeTyping, "no launch argument leaves a stored off")
        fresh.applyLaunchArguments(["HEX", FullKeyboardSettings.swipeLaunchArgument, FullKeyboardSettings.emojiLaunchArgument])
        XCTAssertTrue(fresh.swipeTyping)
        XCTAssertTrue(fresh.emoji)
        fresh.swipeTyping = false
        XCTAssertEqual(defaults.object(forKey: "experiments.swipeTyping") as? Bool, false)
    }

    /// Settings › Keyboard › Number row is off by default, and `--number-row on|off` sets it.
    func testNumberRowIsOffByDefault() throws {
        let suite = "FullKeyboardSettingsTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = FullKeyboardSettings(defaults: defaults)
        XCTAssertFalse(settings.numberRow)
        XCTAssertFalse(FullKeyboardSettings(defaults: nil).numberRow, "without the App Group")
        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.numberRowLaunchArgument, "on"])
        XCTAssertTrue(settings.numberRow)
        XCTAssertEqual(defaults.object(forKey: "settings.keyboardNumberRow") as? Bool, true)
        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.numberRowLaunchArgument, "off"])
        XCTAssertFalse(settings.numberRow)
    }

    /// Settings › Experiments › Join the keyboard when unfolded is off by default, keeps its
    /// App Group key, and `--join-unfolded-keyboard on|off` sets it.
    func testJoinUnfoldedKeyboardIsOffByDefault() throws {
        let suite = "FullKeyboardSettingsTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = FullKeyboardSettings(defaults: defaults)
        XCTAssertFalse(settings.joinsKeyboardWhenUnfolded)
        XCTAssertFalse(FullKeyboardSettings(defaults: nil).joinsKeyboardWhenUnfolded, "without the App Group")
        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.joinUnfoldedKeyboardLaunchArgument, "on"])
        XCTAssertTrue(settings.joinsKeyboardWhenUnfolded)
        XCTAssertEqual(defaults.object(forKey: "experiments.joinKeyboardWhenUnfolded") as? Bool, true)
        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.joinUnfoldedKeyboardLaunchArgument, "off"])
        XCTAssertFalse(settings.joinsKeyboardWhenUnfolded)
    }
}
