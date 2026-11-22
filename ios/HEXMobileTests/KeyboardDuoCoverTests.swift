import CoreGraphics
import XCTest
@testable import HEX

/// The iPhone Duo folded, measured beside Apple's keyboard on its 466 pt cover screen: Apple's
/// keycaps start 54.5 px of 1375 in at 2.95 px per point, and its bottom row stands on row 3's
/// columns.
final class KeyboardDuoCoverTests: XCTestCase {
    private let screenWidth: CGFloat = 466

    /// The two views iOS gives a third-party keyboard on the cover screen: the whole screen,
    /// and an inset platter with the 7 pt the simulator gives.
    private func coverLayouts() -> [(layout: KeyboardLayout, windowInset: CGFloat)] {
        [
            (layout: KeyboardLayout(width: 466, showsGlobe: true, posture: .cover), windowInset: 0),
            (layout: KeyboardLayout(width: 452, showsGlobe: true, posture: .cover, windowInset: 7), windowInset: 7),
        ]
    }

    /// Apple's keycaps start 18.5 pt in from the screen's side, so Hex's do too, counting the
    /// platter's inset whichever way iOS splits it between the view's frame and its width.
    func testCoverKeysStartAsFarInAsApples() {
        for (layout, windowInset) in coverLayouts() {
            let q = layout.key(.character("q"))!.frame
            let p = layout.key(.character("p"))!.frame
            XCTAssertEqual(windowInset + q.minX, 18.5, accuracy: 0.5, "q on the \(layout.width) pt view")
            XCTAssertEqual(windowInset + p.maxX, screenWidth - 18.5, accuracy: 0.5, "p on the \(layout.width) pt view")
        }
    }

    /// Folded, Apple's row 4 stands on row 3's columns: 123 from the edge to a gap before z,
    /// the globe on z, space from x, and return from a gap after m to the edge.
    func testCoverBottomRowStandsOnRowThreesColumns() {
        for (layout, _) in coverLayouts() {
            let gap = layout.metrics.gap
            let q = layout.key(.character("q"))!.frame
            let p = layout.key(.character("p"))!.frame
            let x = layout.key(.character("x"))!.frame
            let z = layout.key(.character("z"))!.frame
            let m = layout.key(.character("m"))!.frame
            let page = layout.key(.layerSwitch(.numbers))!.frame
            let globe = layout.key(.globe)!.frame
            let space = layout.key(.space)!.frame
            let ret = layout.key(.returnKey)!.frame
            let name = "\(layout.width) pt view"
            XCTAssertEqual(page.minX, q.minX, accuracy: 0.001, name)
            XCTAssertEqual(page.width, 1.5 * (q.width + gap) - gap, accuracy: 0.001, name)
            XCTAssertEqual(page.maxX + gap, z.minX, accuracy: 0.001, name)
            XCTAssertEqual(globe.minX, z.minX, accuracy: 0.001, name)
            XCTAssertEqual(globe.width, z.width, accuracy: 0.001, name)
            XCTAssertEqual(space.minX, x.minX, accuracy: 0.001, name)
            XCTAssertEqual(ret.minX, m.maxX + gap, accuracy: 0.001, name)
            XCTAssertEqual(ret.maxX, p.maxX, accuracy: 0.001, name)
        }
    }

    /// Everywhere else row 4's leading keys stay one letter wide from the edge, as they were.
    func testOnlyTheFoldedCoverScreenWidensThePageKey() {
        for layout in [
            KeyboardLayout(width: 528, orientation: .landscape, showsGlobe: true, posture: .cover, windowInset: 75),
            KeyboardLayout(width: 874, orientation: .landscape, showsEmoji: true),
            KeyboardLayout(width: 655, posture: .unfoldedTall, windowInset: 7),
            KeyboardLayout(width: 402),
        ] {
            XCTAssertNil(layout.metrics.bottomRowLeadColumn, "\(layout.width) pt \(layout.posture)")
        }
        let landscape = KeyboardLayout(width: 528, orientation: .landscape, showsGlobe: true, posture: .cover, windowInset: 75)
        let page = landscape.key(.layerSwitch(.numbers))!.frame
        let q = landscape.key(.character("q"))!.frame
        let globe = landscape.key(.globe)!.frame
        XCTAssertEqual(page.width, q.width, accuracy: 0.001)
        XCTAssertEqual(globe.minX, q.maxX + landscape.metrics.gap, accuracy: 0.001)
    }

    /// Folded, Apple's key labels are 0.875 of its upright iPhone's: its 123's label is 70 px
    /// wide there against Hex's 80 at 2.95 px per point (iOS 27.2). So the cover draws every
    /// label smaller, and every other layout keeps the iPhone's sizes.
    @MainActor
    func testOnlyTheFoldedCoverScreenDrawsSmallerLabels() {
        let cover = KeyboardLayout(width: 466, showsGlobe: true, posture: .cover)
        XCTAssertEqual(cover.keyLabelScale, 0.875, accuracy: 0.0001)
        let styles: [KeyboardKeyPlaneView.LabelStyle] = [
            .lowercase, .capital, .symbol, .pageKey, .symbolsPageKey,
        ]
        for style in styles {
            XCTAssertLessThan(style.scaled(by: cover.keyLabelScale).size, style.size)
        }
        XCTAssertEqual(styles[0].scaled(by: cover.keyLabelScale).size, 24.5 * 0.875, accuracy: 0.0001)

        for layout in [
            KeyboardLayout(width: 402),
            KeyboardLayout(width: 655, posture: .unfoldedTall, windowInset: 7),
            KeyboardLayout(width: 801, posture: .unfoldedWide, windowInset: 75),
            KeyboardLayout(width: 528, orientation: .landscape, showsGlobe: true, posture: .cover, windowInset: 75),
        ] {
            XCTAssertEqual(layout.keyLabelScale, 1, "\(layout.width) pt \(layout.posture) \(layout.orientation)")
            for style in styles {
                XCTAssertEqual(style.scaled(by: layout.keyLabelScale).size, style.size)
            }
        }
    }
}
