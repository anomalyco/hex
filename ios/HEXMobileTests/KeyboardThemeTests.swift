import UIKit
import XCTest
@testable import HEX

/// Settings › Experiments › Keyboard themes: the full keyboard's own colours, off by default,
/// and Automatic (the system glass) exactly as it was before them.
@MainActor
final class KeyboardThemeTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "KeyboardThemeTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    /// Off by default, like every experiment, and the stored theme is ignored while it's off.
    func testTheExperimentDefaultsOffAndAutomaticUntilOn() {
        let settings = FullKeyboardSettings(defaults: defaults)
        XCTAssertFalse(settings.keyboardThemesEnabled, "off by default")
        XCTAssertEqual(settings.keyboardTheme, .automatic)
        XCTAssertEqual(settings.effectiveKeyboardTheme, .automatic)

        settings.keyboardTheme = .navy
        XCTAssertEqual(settings.keyboardTheme, .navy)
        XCTAssertEqual(settings.effectiveKeyboardTheme, .automatic, "off, whatever theme is stored")

        settings.keyboardThemesEnabled = true
        XCTAssertEqual(settings.effectiveKeyboardTheme, .navy)

        XCTAssertFalse(FullKeyboardSettings(defaults: nil).keyboardThemesEnabled, "without the App Group")
        XCTAssertEqual(FullKeyboardSettings(defaults: nil).keyboardTheme, .automatic)
        XCTAssertEqual(FullKeyboardSettings(defaults: nil).effectiveKeyboardTheme, .automatic)
    }

    /// An unknown raw value (a newer build's theme, a damaged write) reads as Automatic.
    func testUnknownStoredThemeIsAutomatic() {
        defaults.set("chartreuse", forKey: "settings.keyboardTheme")
        let settings = FullKeyboardSettings(defaults: defaults)
        XCTAssertEqual(settings.keyboardTheme, .automatic)
        settings.keyboardThemesEnabled = true
        XCTAssertEqual(settings.effectiveKeyboardTheme, .automatic)
    }

    /// `--experiment-keyboard-themes` turns it on and `--keyboard-theme <raw>` picks the theme.
    func testLaunchArguments() {
        let settings = FullKeyboardSettings(defaults: defaults)
        settings.applyLaunchArguments([
            "HEX",
            FullKeyboardSettings.keyboardThemesLaunchArgument,
            FullKeyboardSettings.keyboardThemeLaunchArgument,
            "forest",
        ])
        XCTAssertTrue(settings.keyboardThemesEnabled)
        XCTAssertEqual(settings.effectiveKeyboardTheme, .forest)

        settings.applyLaunchArguments(["HEX", FullKeyboardSettings.keyboardThemeLaunchArgument, "burgundy"])
        XCTAssertEqual(settings.effectiveKeyboardTheme, .burgundy)
    }

    /// Automatic is exactly what the keyboard drew before themes: every themed property keeps
    /// its old value, pinned here rather than compared with a second palette of the same code
    /// (`git show origin/dev:ios/HEXShared/FullKeyboard/FullKeyboardPalette.swift`).
    func testAutomaticIsTheOldPalette() {
        for isDark in [false, true] {
            let automatic = FullKeyboardPalette(isDark: isDark, theme: .automatic)
            XCTAssertEqual(automatic.theme, .automatic)
            XCTAssertEqual(automatic.isDark, isDark)
            XCTAssertFalse(automatic.drawsOwnBackground)
        }

        let light = FullKeyboardPalette(isDark: false)
        assertSameColor(light.backdrop, color(0xDFE1E6))
        assertSameColor(light.keyFill, .white)
        assertSameColor(light.keyPressedFill, color(0xD8DCE4))
        assertSameColor(light.keyHeldFill, color(0xDEDEDE))
        assertSameColor(light.functionHeldFill, color(0xC5C5C9))
        assertSameColor(light.spaceHeldFill, color(0xF2F2F7))
        assertSameColor(light.keyShadow, .clear)
        assertSameColor(light.popupFill, .white)
        assertSameColor(light.bubbleFill, .white)
        assertSameColor(light.accentSelectFill, color(0xE3E3E6))
        assertSameColor(light.stripPillFill, UIColor(white: 1, alpha: 0.33))
        assertSameColor(light.actionFill, color(0x007AFF))
        assertSameColor(light.actionPressedFill, color(0x0062CC))

        let dark = FullKeyboardPalette(isDark: true)
        assertSameColor(dark.backdrop, color(0x17181A))
        assertSameColor(dark.keyFill, UIColor(white: 1, alpha: 0.16))
        assertSameColor(dark.keyPressedFill, UIColor(white: 1, alpha: 0.28))
        assertSameColor(dark.keyHeldFill, UIColor(white: 1, alpha: 0.32))
        assertSameColor(dark.functionHeldFill, UIColor(white: 1, alpha: 0.44))
        assertSameColor(dark.spaceHeldFill, UIColor(white: 1, alpha: 0.1))
        assertSameColor(dark.keyShadow, .clear)
        assertSameColor(dark.popupFill, color(0x4E5056))
        assertSameColor(dark.bubbleFill, color(0x3F3F41))
        assertSameColor(dark.accentSelectFill, color(0x57575A))
        assertSameColor(dark.stripPillFill, UIColor(white: 1, alpha: 0.37))
        assertSameColor(dark.actionFill, color(0x007AFF))
        assertSameColor(dark.actionPressedFill, color(0x0062CC))
    }

    /// A theme decides light or dark for itself, whatever the field or fixture asked for.
    func testAThemeIgnoresTheField() {
        XCTAssertTrue(FullKeyboardPalette(isDark: false, theme: .black).isDark)
        XCTAssertFalse(FullKeyboardPalette(isDark: true, theme: .white).isDark)
        XCTAssertTrue(FullKeyboardPalette(isDark: false, theme: .black).drawsOwnBackground)
        XCTAssertFalse(FullKeyboardPalette(isDark: false, theme: .automatic).drawsOwnBackground)
    }

    /// Labels stay readable on every theme's keys: WCAG AA's 4.5:1 for body text.
    func testEveryThemeKeepsItsLabelsLegible() {
        for theme in KeyboardTheme.allCases where theme != .automatic {
            let palette = FullKeyboardPalette(isDark: false, theme: theme)
            let ratio = contrast(palette.textBase, palette.keyFill)
            XCTAssertGreaterThanOrEqual(ratio, 4.5, "\(theme.rawValue) labels: \(ratio):1")
        }
    }

    /// White's suggestion capsule has to read on its white bar, where Automatic's translucent
    /// white vanishes; every other theme keeps the palette's own capsule, Automatic included.
    func testWhitesStripCapsuleReadsOnItsBar() {
        let white = FullKeyboardPalette(isDark: false, theme: .white)
        assertSameColor(white.stripPillFill, color(0xE4E5EA))
        XCTAssertNotEqual(white.stripPillFill, .white, "not invisible on the white bar")
        // A pressed word keeps the same capsule, which derives from the fill.
        assertSameColor(white.stripPillPressedFill, white.stripPillFill)
        XCTAssertGreaterThan(contrast(white.stripPillFill, white.backdrop), 1.05)
        // The bar behind the capsule is White's own white.
        assertSameColor(white.backdrop, .white)

        // Automatic, light and dark, is the palette it always was.
        assertSameColor(
            FullKeyboardPalette(isDark: false, theme: .automatic).stripPillFill,
            UIColor(white: 1, alpha: 0.33)
        )
        assertSameColor(
            FullKeyboardPalette(isDark: true, theme: .automatic).stripPillFill,
            UIColor(white: 1, alpha: 0.37)
        )

        // The dark themes have no capsule of their own: the palette's translucent white stands.
        for theme in KeyboardTheme.allCases where theme != .automatic && theme != .white {
            let palette = FullKeyboardPalette(isDark: false, theme: theme)
            XCTAssertNil(theme.colors?.stripPillFill, "\(theme.rawValue) keeps the palette's")
            assertSameColor(palette.stripPillFill, UIColor(white: 1, alpha: 0.37))
        }
    }

    /// A theme paints its own opaque panel behind the keys, in iOS's glass panel's own place:
    /// the panel's top corners, 1 pt inside its rim (`KeyboardBackdropView`'s frame and corner
    /// radius), never this view's whole rect, whose square corners would show over the app as
    /// a panel inside iOS's panel. The view itself stays clear; Automatic has no panel at all.
    func testAThemePaintsItsPanelInTheGlassPanelsCorners() throws {
        let settings = FullKeyboardSettings()
        let wasEnabled = settings.keyboardThemesEnabled
        let wasTheme = settings.keyboardTheme
        defer {
            settings.keyboardThemesEnabled = wasEnabled
            settings.keyboardTheme = wasTheme
        }
        settings.keyboardThemesEnabled = true
        settings.keyboardTheme = .navy

        let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget())
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()

        XCTAssertEqual(view.palette.theme, .navy)
        assertSameColor(try XCTUnwrap(view.backgroundColor), .clear, "not a full-rect fill")
        let panel = view.themePanel
        XCTAssertFalse(panel.isHidden)
        assertSameColor(try XCTUnwrap(panel.backgroundColor), color(0x080F1C), "Navy's panel")
        XCTAssertEqual(panel.layer.maskedCorners, [.layerMinXMinYCorner, .layerMaxXMinYCorner])
        XCTAssertEqual(panel.layer.cornerCurve, .circular)
        // The glass panel's own frame and corners: 1 pt inside its rim, 25 pt at the top.
        XCTAssertEqual(panel.frame, KeyboardBackdropView.frame(in: view.bounds))
        XCTAssertEqual(panel.frame.minX, KeyboardBackdropView.rimInset)
        XCTAssertEqual(panel.frame.minY, KeyboardBackdropView.rimInset)
        XCTAssertEqual(panel.layer.cornerRadius, KeyboardBackdropView.cornerRadius(in: view.bounds))
        XCTAssertEqual(panel.layer.cornerRadius, KeyboardBackdropView.panelCornerRadius - KeyboardBackdropView.rimInset)

        // Automatic: the keyboard is iOS's glass, so the view's own background is clear and
        // nothing draws over the panel.
        settings.keyboardThemesEnabled = false
        view.syncSettings()
        view.layoutIfNeeded()
        XCTAssertEqual(view.palette.theme, .automatic)
        XCTAssertTrue(panel.isHidden)
        assertSameColor(try XCTUnwrap(view.backgroundColor), .clear)
    }

    /// The stored switch reaches a keyboard already up, as every other App Group experiment
    /// does: its next `syncSettings` rebuilds the palette, and the panel with it.
    func testFlippingTheStoredThemeRebuildsThePalette() throws {
        let settings = FullKeyboardSettings()
        let wasEnabled = settings.keyboardThemesEnabled
        let wasTheme = settings.keyboardTheme
        defer {
            settings.keyboardThemesEnabled = wasEnabled
            settings.keyboardTheme = wasTheme
        }
        settings.keyboardThemesEnabled = false
        settings.keyboardTheme = .automatic
        let view = FullKeyboardView(model: KeyboardViewModel(), target: FixtureTextTarget())
        XCTAssertEqual(view.palette.theme, .automatic)

        settings.keyboardThemesEnabled = true
        settings.keyboardTheme = .forest
        view.syncSettings()
        XCTAssertEqual(view.palette.theme, .forest)
        XCTAssertFalse(view.themePanel.isHidden)
        assertSameColor(try XCTUnwrap(view.themePanel.backgroundColor), color(0x06130C), "Forest's panel")

        settings.keyboardTheme = .burgundy
        view.syncSettings()
        XCTAssertEqual(view.palette.theme, .burgundy)
        assertSameColor(try XCTUnwrap(view.themePanel.backgroundColor), color(0x1A090C), "Burgundy's panel")

        settings.keyboardTheme = .white
        view.syncSettings()
        XCTAssertEqual(view.palette.theme, .white, "a theme change alone reaches the keyboard")

        settings.keyboardThemesEnabled = false
        view.syncSettings()
        XCTAssertEqual(view.palette.theme, .automatic, "off again: back on the system look")
        XCTAssertTrue(view.themePanel.isHidden)
    }

    /// Only White's keys take a shadow, a faint edge on its light panel. Every other theme
    /// answers exactly `.clear`, so `fillKeycap` skips `setShadow` rather than drawing black
    /// at alpha 0.
    func testOnlyWhitesKeysHaveAShadow() {
        for theme in KeyboardTheme.allCases where theme != .automatic && theme != .white {
            let shadow = FullKeyboardPalette(isDark: false, theme: theme).keyShadow
            assertSameColor(shadow, .clear, "\(theme.rawValue)")
            XCTAssertEqual(shadow, .clear, "\(theme.rawValue): exactly .clear")
        }
        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        let white = FullKeyboardPalette(isDark: false, theme: .white).keyShadow
        XCTAssertNotEqual(white, .clear)
        XCTAssertTrue(white.getRed(&r, green: &g, blue: &b, alpha: &a))
        XCTAssertGreaterThan(a, 0)
        XCTAssertLessThanOrEqual(a, 0.25, "faint")

        // Automatic keeps the flat keycaps it always had: no shadow at all.
        assertSameColor(FullKeyboardPalette(isDark: false).keyShadow, .clear)
        assertSameColor(FullKeyboardPalette(isDark: true).keyShadow, .clear)
    }

    private func color(_ hex: UInt32) -> UIColor {
        UIColor(
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: 1
        )
    }

    private func assertSameColor(
        _ lhs: UIColor,
        _ rhs: UIColor,
        _ message: String = "",
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        var lr: CGFloat = 0, lg: CGFloat = 0, lb: CGFloat = 0, la: CGFloat = 0
        var rr: CGFloat = 0, rg: CGFloat = 0, rb: CGFloat = 0, ra: CGFloat = 0
        XCTAssertTrue(lhs.getRed(&lr, green: &lg, blue: &lb, alpha: &la), message, file: file, line: line)
        XCTAssertTrue(rhs.getRed(&rr, green: &rg, blue: &rb, alpha: &ra), message, file: file, line: line)
        XCTAssertEqual(lr, rr, accuracy: 0.001, message, file: file, line: line)
        XCTAssertEqual(lg, rg, accuracy: 0.001, message, file: file, line: line)
        XCTAssertEqual(lb, rb, accuracy: 0.001, message, file: file, line: line)
        XCTAssertEqual(la, ra, accuracy: 0.001, message, file: file, line: line)
    }

    private func contrast(_ a: UIColor, _ b: UIColor) -> CGFloat {
        let luminanceA = luminance(a)
        let luminanceB = luminance(b)
        return (max(luminanceA, luminanceB) + 0.05) / (min(luminanceA, luminanceB) + 0.05)
    }

    private func luminance(_ color: UIColor) -> CGFloat {
        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        XCTAssertTrue(color.getRed(&r, green: &g, blue: &b, alpha: &a))
        func linear(_ component: CGFloat) -> CGFloat {
            guard component > 0.04045 else { return component / 12.92 }
            return CGFloat(pow((Double(component) + 0.055) / 1.055, 2.4))
        }
        return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }
}
