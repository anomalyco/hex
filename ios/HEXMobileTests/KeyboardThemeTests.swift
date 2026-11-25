import UIKit
import XCTest
@testable import HEX

/// Settings › Experiments › Keyboard themes: the full keyboard's keys in their own colours,
/// off by default, over iOS's glass, and Automatic exactly as it was before them.
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
            "burgundy",
        ])
        XCTAssertTrue(settings.keyboardThemesEnabled)
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
            // Its keys' labels are the glass's ink, as they always were.
            XCTAssertEqual(automatic.keysAreDark, isDark)
            assertSameColor(automatic.keyText, automatic.textBase)
            assertSameColor(automatic.keyTextMuted, automatic.textMuted)
            assertSameColor(automatic.systemPressedFill, automatic.keyPressedFill)
        }

        let light = FullKeyboardPalette(isDark: false)
        assertSameColor(light.backdrop, color(0xDFE1E6))
        assertSameColor(light.textBase, .black)
        assertSameColor(light.textMuted, color(0x525763))
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
        assertSameColor(light.actionDisabledFill, color(0xBFC0C5))
        assertSameColor(light.returnDisabledInk, UIColor(white: 0, alpha: 0.1))

        let dark = FullKeyboardPalette(isDark: true)
        assertSameColor(dark.backdrop, color(0x17181A))
        assertSameColor(dark.textBase, .white)
        assertSameColor(dark.textMuted, UIColor(white: 1, alpha: 0.56))
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
        assertSameColor(dark.actionDisabledFill, UIColor(white: 1, alpha: 0.39))
        assertSameColor(dark.returnDisabledInk, UIColor(white: 1, alpha: 0.1))
    }

    /// A theme colours the keys and nothing on the glass around them: the palette follows the
    /// field's light or dark like Automatic's, and everything drawn on the glass (the stand-in
    /// panel, the strip's words and capsule, menus, the emoji grid's pressed grey) is
    /// Automatic's for that appearance. Only the keys, their pop-ups and labels change.
    func testAThemeColoursOnlyTheKeys() throws {
        for theme in KeyboardTheme.allCases where theme != .automatic {
            let colors = try XCTUnwrap(theme.colors, theme.rawValue)
            for isDark in [false, true] {
                let themed = FullKeyboardPalette(isDark: isDark, theme: theme)
                let system = FullKeyboardPalette(isDark: isDark)
                let label = "\(theme.rawValue), \(isDark ? "dark" : "light")"
                XCTAssertEqual(themed.isDark, isDark, label)
                assertSameColor(themed.backdrop, system.backdrop, label)
                assertSameColor(themed.textBase, system.textBase, label)
                assertSameColor(themed.textMuted, system.textMuted, label)
                assertSameColor(themed.popupFill, system.popupFill, label)
                assertSameColor(themed.stripText, system.stripText, label)
                assertSameColor(themed.stripPillFill, system.stripPillFill, label)
                assertSameColor(themed.chipFill, system.chipFill, label)
                assertSameColor(themed.functionFill, system.functionFill, label)
                assertSameColor(themed.systemPressedFill, system.keyPressedFill, label)
                // The keys are the theme's, the same in light and dark.
                assertSameColor(themed.keyFill, colors.keyFill, label)
                assertSameColor(themed.keyPressedFill, colors.keyPressedFill, label)
                assertSameColor(themed.bubbleFill, colors.bubbleFill, label)
                assertSameColor(themed.actionFill, colors.actionFill, label)
                assertSameColor(themed.keyText, colors.hasDarkLabels ? .black : .white, label)
                assertSameColor(themed.keyShadow, .clear, label)
            }
        }
    }

    /// Labels stay readable on every theme's keys, light and dark: WCAG AA's 4.5:1 for body
    /// text, on a key at rest and in every pressed or held state, on the pop-up and the accent
    /// callout's choice, and the space bar's dimmer language name on its key. Automatic is
    /// Apple's own colours, checked at rest only (its dark held shift is 4.1:1, as Apple's),
    /// with its translucent dark keys measured over the glass's stand-in.
    func testEveryThemeKeepsItsLabelsLegible() {
        for theme in KeyboardTheme.allCases {
            for isDark in [false, true] {
                let palette = FullKeyboardPalette(isDark: isDark, theme: theme)
                let glass = palette.backdrop
                let label = "\(theme.rawValue), \(isDark ? "dark" : "light")"
                for (role, fill) in [
                    ("key", palette.keyFill), ("pressed", palette.keyPressedFill),
                    ("held", palette.keyHeldFill), ("special key held", palette.functionHeldFill),
                    ("space held", palette.spaceHeldFill), ("pop-up", palette.bubbleFill),
                    ("accent choice", palette.accentSelectFill),
                ] where theme != .automatic || role == "key" {
                    let ratio = contrast(palette.keyText, over(fill, glass))
                    XCTAssertGreaterThanOrEqual(ratio, 4.5, "\(label) labels on the \(role): \(ratio):1")
                }
                guard theme != .automatic else { continue }
                let key = over(palette.keyFill, glass)
                let muted = contrast(over(palette.keyTextMuted, key), key)
                XCTAssertGreaterThanOrEqual(muted, 4.5, "\(label) space bar's language: \(muted):1")
            }
        }
        // A tinted theme's own return key keeps its white label at 4.5:1; Black and White keep
        // Apple's #007AFF, which Automatic has always used (4.0:1).
        for theme in [KeyboardTheme.navy, .forest, .ember, .burgundy] {
            let palette = FullKeyboardPalette(isDark: false, theme: theme)
            let ratio = contrast(.white, palette.actionFill)
            XCTAssertGreaterThanOrEqual(ratio, 4.5, "\(theme.rawValue) return key: \(ratio):1")
        }
    }

    /// A theme draws no panel of its own: the keyboard's background stays clear over iOS's
    /// glass with every theme, and the palette still follows the field's light or dark, so the
    /// strip and the emoji panel match the band iOS draws above and below the keys.
    func testAThemeLeavesTheBackgroundToTheGlass() {
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
        for isDark in [false, true] {
            view.appearanceOverride = isDark
            view.layoutIfNeeded()
            XCTAssertEqual(view.palette.theme, .navy)
            XCTAssertEqual(view.palette.isDark, isDark, "the glass's light or dark, not the theme's")
            XCTAssertEqual(view.backgroundColor, .clear, "no panel over iOS's glass")
        }
    }

    /// The stored switch reaches a keyboard already up, as every other App Group experiment
    /// does: its next `syncSettings` rebuilds the palette.
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
        assertSameColor(view.palette.keyFill, color(0x215339), "Forest's keys")

        settings.keyboardTheme = .burgundy
        view.syncSettings()
        XCTAssertEqual(view.palette.theme, .burgundy, "a theme change alone reaches the keyboard")
        assertSameColor(view.palette.keyFill, color(0x6F303C), "Burgundy's keys")

        settings.keyboardThemesEnabled = false
        view.syncSettings()
        XCTAssertEqual(view.palette.theme, .automatic, "off again: back on the system look")
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

    /// `top` composited over an opaque `bottom`, as a translucent key or label draws.
    private func over(_ top: UIColor, _ bottom: UIColor) -> UIColor {
        var tr: CGFloat = 0, tg: CGFloat = 0, tb: CGFloat = 0, ta: CGFloat = 0
        var br: CGFloat = 0, bg: CGFloat = 0, bb: CGFloat = 0, ba: CGFloat = 0
        XCTAssertTrue(top.getRed(&tr, green: &tg, blue: &tb, alpha: &ta))
        XCTAssertTrue(bottom.getRed(&br, green: &bg, blue: &bb, alpha: &ba))
        return UIColor(red: tr * ta + br * (1 - ta), green: tg * ta + bg * (1 - ta), blue: tb * ta + bb * (1 - ta), alpha: 1)
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
