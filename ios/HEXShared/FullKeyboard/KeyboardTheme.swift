import UIKit

/// Settings › Experiments › Key colors: the full keyboard's keys in colours of their own.
/// A theme colours the keys only: every keycap (letters and the special keys alike), their
/// pressed and held states, the key pop-ups and accent callout, the return key's action colour
/// and the labels on them. The background stays iOS's own glass panel in light and dark, the
/// same as the band iOS draws above a third-party keyboard and the globe row under it, so a
/// theme never shows a panel of its own inside iOS's. Everything that sits on that glass (the
/// strip, the emoji panel, menus, the dictation bar) keeps the system look. Automatic, the
/// default, leaves every colour exactly as it was.
nonisolated enum KeyboardTheme: String, CaseIterable, Sendable {
    case automatic
    case black
    case white
    case navy
    case forest
    case ember
    case burgundy

    /// The theme's name, in Settings › Experiments' Theme menu.
    var name: String {
        switch self {
        case .automatic: String(localized: "Automatic")
        case .black: String(localized: "Black")
        case .white: String(localized: "White")
        case .navy: String(localized: "Navy")
        case .forest: String(localized: "Forest")
        case .ember: String(localized: "Ember")
        case .burgundy: String(localized: "Burgundy")
        }
    }

    /// The theme's key colours; nil for Automatic, whose keys follow iOS's light or dark.
    var colors: KeyboardThemeColors? {
        switch self {
        case .automatic: nil
        case .black: KeyboardThemeColors.black
        case .white: KeyboardThemeColors.white
        case .navy: KeyboardThemeColors.navy
        case .forest: KeyboardThemeColors.forest
        case .ember: KeyboardThemeColors.ember
        case .burgundy: KeyboardThemeColors.burgundy
        }
    }
}

/// One theme's key colours, the same in light and dark: the theme is the keys' colour, and the
/// glass around them follows the phone. The hex values are kept rather than UIColors so the
/// type stays an ordinary value: Equatable and Sendable without an unchecked conformance.
nonisolated struct KeyboardThemeColors: Equatable, Sendable {
    /// Whether the keys take black labels (White) rather than white ones (every other theme).
    let hasDarkLabels: Bool

    private let keyFillHex: UInt32
    private let keyPressedFillHex: UInt32
    private let keyHeldFillHex: UInt32
    private let functionHeldFillHex: UInt32
    private let spaceHeldFillHex: UInt32
    private let bubbleFillHex: UInt32
    private let accentSelectFillHex: UInt32
    private let actionFillHex: UInt32
    private let actionPressedFillHex: UInt32

    fileprivate init(
        hasDarkLabels: Bool = false,
        keyFill: UInt32,
        keyPressedFill: UInt32,
        keyHeldFill: UInt32,
        functionHeldFill: UInt32,
        spaceHeldFill: UInt32,
        bubbleFill: UInt32,
        accentSelectFill: UInt32,
        actionFill: UInt32,
        actionPressedFill: UInt32
    ) {
        self.hasDarkLabels = hasDarkLabels
        keyFillHex = keyFill
        keyPressedFillHex = keyPressedFill
        keyHeldFillHex = keyHeldFill
        functionHeldFillHex = functionHeldFill
        spaceHeldFillHex = spaceHeldFill
        bubbleFillHex = bubbleFill
        accentSelectFillHex = accentSelectFill
        actionFillHex = actionFill
        actionPressedFillHex = actionPressedFill
    }

    private static func color(_ hex: UInt32) -> UIColor {
        UIColor(
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: 1
        )
    }

    var keyFill: UIColor { Self.color(keyFillHex) }
    var keyPressedFill: UIColor { Self.color(keyPressedFillHex) }
    var keyHeldFill: UIColor { Self.color(keyHeldFillHex) }
    var functionHeldFill: UIColor { Self.color(functionHeldFillHex) }
    var spaceHeldFill: UIColor { Self.color(spaceHeldFillHex) }
    /// The key pop-up and the accent callout: a magnified key, a step lighter than the keys.
    var bubbleFill: UIColor { Self.color(bubbleFillHex) }
    var accentSelectFill: UIColor { Self.color(accentSelectFillHex) }
    var actionFill: UIColor { Self.color(actionFillHex) }
    var actionPressedFill: UIColor { Self.color(actionPressedFillHex) }
}

fileprivate extension KeyboardThemeColors {
    /// Black keys with white labels, on whichever glass the phone shows.
    static let black = KeyboardThemeColors(
        keyFill: 0x000000,
        keyPressedFill: 0x2C2C2E,
        keyHeldFill: 0x3A3A3C,
        functionHeldFill: 0x48484A,
        spaceHeldFill: 0x1C1C1E,
        bubbleFill: 0x1C1C1E,
        accentSelectFill: 0x3A3A3C,
        actionFill: 0x007AFF,
        actionPressedFill: 0x0062CC
    )

    /// White keys with black labels: Apple's light keys, kept in Dark Mode too.
    static let white = KeyboardThemeColors(
        hasDarkLabels: true,
        keyFill: 0xFFFFFF,
        keyPressedFill: 0xD8DCE4,
        keyHeldFill: 0xDEDEDE,
        functionHeldFill: 0xC5C5C9,
        spaceHeldFill: 0xF2F2F7,
        bubbleFill: 0xFFFFFF,
        accentSelectFill: 0xE3E3E6,
        actionFill: 0x007AFF,
        actionPressedFill: 0x0062CC
    )

    // The tinted themes share one recipe in OKLCH: keys at lightness 0.40 (light enough to stand
    // off dark glass as Apple's dark keys do, dark enough for white labels at 9:1 or more), each
    // state a fixed step lighter (pressed +0.09, held +0.11, a held shift, delete, globe, emoji
    // or return +0.14, a held space +0.045, the pop-up +0.04, the accent callout's choice +0.14),
    // at one hue and chroma per theme. The return key keeps each theme's own action colour.

    /// Blue keys (hue 258, chroma 0.085) and a brighter blue return key.
    static let navy = KeyboardThemeColors(
        keyFill: 0x294875,
        keyPressedFill: 0x416190,
        keyHeldFill: 0x476796,
        functionHeldFill: 0x4F70A0,
        spaceHeldFill: 0x355482,
        bubbleFill: 0x345381,
        accentSelectFill: 0x4F70A0,
        actionFill: 0x2563EB,
        actionPressedFill: 0x1D4ED8
    )

    /// Green keys (hue 158, chroma 0.07) and a green return key.
    static let forest = KeyboardThemeColors(
        keyFill: 0x215339,
        keyPressedFill: 0x3B6D51,
        keyHeldFill: 0x417257,
        functionHeldFill: 0x4A7B5F,
        spaceHeldFill: 0x2E6045,
        bubbleFill: 0x2D5E44,
        accentSelectFill: 0x4A7B5F,
        actionFill: 0x1F7A4C,
        actionPressedFill: 0x186B41
    )

    /// Red-brown keys (hue 38, chroma 0.085) and an orange return key.
    static let ember = KeyboardThemeColors(
        keyFill: 0x6E3524,
        keyPressedFill: 0x894E3C,
        keyHeldFill: 0x905441,
        functionHeldFill: 0x995D4A,
        spaceHeldFill: 0x7B4230,
        bubbleFill: 0x7A402E,
        accentSelectFill: 0x995D4A,
        actionFill: 0xC2410C,
        actionPressedFill: 0x9A3412
    )

    /// Wine-red keys (hue 10, chroma 0.09) and a rose return key (#2458's, 5.2:1 under white).
    static let burgundy = KeyboardThemeColors(
        keyFill: 0x6F303C,
        keyPressedFill: 0x8B4955,
        keyHeldFill: 0x924F5A,
        functionHeldFill: 0x9B5763,
        spaceHeldFill: 0x7D3D48,
        bubbleFill: 0x7C3B47,
        accentSelectFill: 0x9B5763,
        actionFill: 0xBC4059,
        actionPressedFill: 0x9E354A
    )
}
