import UIKit

/// Settings › Experiments › Keyboard themes: the full keyboard drawn in colours of its own
/// instead of its translucent keycaps over iOS's glass. Automatic, the default, leaves every
/// colour exactly as it was. Every other theme paints an opaque panel behind the keys, so Dark
/// Mode's grey glass never shows through, and gives the keys, their pressed and held states,
/// the pop-ups and the return key one palette. The dictation bar keeps the system look.
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

    /// The theme's own colours; nil for Automatic, which keeps iOS's glass.
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

/// One theme's colours. The hex values are kept rather than UIColors so the type stays an
/// ordinary value: Equatable and Sendable without an unchecked conformance.
nonisolated struct KeyboardThemeColors: Equatable, Sendable {
    /// Whether the theme is dark, which decides the labels' ink (white on every dark theme,
    /// black on White) and every other palette colour that follows light or dark.
    let isDark: Bool

    private let backgroundHex: UInt32
    private let keyFillHex: UInt32
    private let keyPressedFillHex: UInt32
    private let keyHeldFillHex: UInt32
    private let functionHeldFillHex: UInt32
    private let spaceHeldFillHex: UInt32
    /// The key's shadow, nil for none: only White's keys have one (a faint edge on a light
    /// key), and nil lets `keyShadow` be exactly `.clear`, so `fillKeycap` skips `setShadow`.
    private let keyShadowHex: UInt32?
    private let keyShadowAlpha: CGFloat
    private let popupFillHex: UInt32
    private let bubbleFillHex: UInt32
    private let accentSelectFillHex: UInt32
    private let actionFillHex: UInt32
    private let actionPressedFillHex: UInt32
    /// The strip's suggestion capsules; nil keeps the palette's own fill, which every dark
    /// theme wants and White cannot use: a white capsule on a white bar is invisible.
    private let stripPillFillHex: UInt32?

    fileprivate init(
        isDark: Bool,
        background: UInt32,
        keyFill: UInt32,
        keyPressedFill: UInt32,
        keyHeldFill: UInt32,
        functionHeldFill: UInt32,
        spaceHeldFill: UInt32,
        keyShadow: UInt32?,
        keyShadowAlpha: CGFloat = 0,
        popupFill: UInt32,
        bubbleFill: UInt32,
        accentSelectFill: UInt32,
        actionFill: UInt32,
        actionPressedFill: UInt32,
        stripPillFill: UInt32? = nil
    ) {
        self.isDark = isDark
        backgroundHex = background
        keyFillHex = keyFill
        keyPressedFillHex = keyPressedFill
        keyHeldFillHex = keyHeldFill
        functionHeldFillHex = functionHeldFill
        spaceHeldFillHex = spaceHeldFill
        keyShadowHex = keyShadow
        self.keyShadowAlpha = keyShadowAlpha
        popupFillHex = popupFill
        bubbleFillHex = bubbleFill
        accentSelectFillHex = accentSelectFill
        actionFillHex = actionFill
        actionPressedFillHex = actionPressedFill
        stripPillFillHex = stripPillFill
    }

    private static func color(_ hex: UInt32) -> UIColor {
        UIColor(
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: 1
        )
    }

    /// The panel behind the keys, opaque so nothing of iOS's glass shows through.
    var background: UIColor { Self.color(backgroundHex) }
    var keyFill: UIColor { Self.color(keyFillHex) }
    var keyPressedFill: UIColor { Self.color(keyPressedFillHex) }
    var keyHeldFill: UIColor { Self.color(keyHeldFillHex) }
    var functionHeldFill: UIColor { Self.color(functionHeldFillHex) }
    var spaceHeldFill: UIColor { Self.color(spaceHeldFillHex) }
    var keyShadow: UIColor {
        keyShadowHex.map { Self.color($0).withAlphaComponent(keyShadowAlpha) } ?? .clear
    }
    var popupFill: UIColor { Self.color(popupFillHex) }
    var bubbleFill: UIColor { Self.color(bubbleFillHex) }
    var accentSelectFill: UIColor { Self.color(accentSelectFillHex) }
    var actionFill: UIColor { Self.color(actionFillHex) }
    var actionPressedFill: UIColor { Self.color(actionPressedFillHex) }
    /// The strip suggestion capsule's fill, nil where the palette's own answers.
    var stripPillFill: UIColor? { stripPillFillHex.map(Self.color) }
}

fileprivate extension KeyboardThemeColors {
    /// A true black OLED panel: no glass, no tint, and keys only a step above the panel.
    static let black = KeyboardThemeColors(
        isDark: true,
        background: 0x000000,
        keyFill: 0x1C1C1E,
        keyPressedFill: 0x3A3A3C,
        keyHeldFill: 0x48484A,
        functionHeldFill: 0x525254,
        spaceHeldFill: 0x2C2C2E,
        keyShadow: nil,
        popupFill: 0x2A2A2C,
        bubbleFill: 0x3A3A3C,
        accentSelectFill: 0x4A4A4D,
        actionFill: 0x007AFF,
        actionPressedFill: 0x0062CC
    )

    /// A crisp white panel with light grey keys, so a key reads against the white rather
    /// than disappearing into it, and a faint shadow for its edge.
    static let white = KeyboardThemeColors(
        isDark: false,
        background: 0xFFFFFF,
        keyFill: 0xF1F2F5,
        keyPressedFill: 0xD8DCE4,
        keyHeldFill: 0xDEDEDE,
        functionHeldFill: 0xC5C5C9,
        spaceHeldFill: 0xE7E8EC,
        keyShadow: 0x000000,
        keyShadowAlpha: 0.2,
        popupFill: 0xFFFFFF,
        bubbleFill: 0xFFFFFF,
        accentSelectFill: 0xE3E3E6,
        actionFill: 0x007AFF,
        actionPressedFill: 0x0062CC,
        stripPillFill: 0xE4E5EA
    )

    /// Deep blue, with keys a step lighter in the same hue and a brighter blue return key.
    static let navy = KeyboardThemeColors(
        isDark: true,
        background: 0x080F1C,
        keyFill: 0x16263D,
        keyPressedFill: 0x223754,
        keyHeldFill: 0x27405F,
        functionHeldFill: 0x2F4C6E,
        spaceHeldFill: 0x1B2E47,
        keyShadow: nil,
        popupFill: 0x18273C,
        bubbleFill: 0x1F3550,
        accentSelectFill: 0x2E4A6B,
        actionFill: 0x2563EB,
        actionPressedFill: 0x1D4ED8
    )

    /// Deep green, the keys and their held states a step lighter, with a green return key.
    static let forest = KeyboardThemeColors(
        isDark: true,
        background: 0x06130C,
        keyFill: 0x173124,
        keyPressedFill: 0x224534,
        keyHeldFill: 0x274E3B,
        functionHeldFill: 0x2F5C45,
        spaceHeldFill: 0x1A3829,
        keyShadow: nil,
        popupFill: 0x173024,
        bubbleFill: 0x1B3A2A,
        accentSelectFill: 0x2A503B,
        actionFill: 0x1F7A4C,
        actionPressedFill: 0x186B41
    )

    /// Deep red-brown, warm keys, and an orange return key.
    static let ember = KeyboardThemeColors(
        isDark: true,
        background: 0x180B07,
        keyFill: 0x331A14,
        keyPressedFill: 0x4A2820,
        keyHeldFill: 0x502C23,
        functionHeldFill: 0x5A3329,
        spaceHeldFill: 0x3D2019,
        keyShadow: nil,
        popupFill: 0x2E1712,
        bubbleFill: 0x3A1E17,
        accentSelectFill: 0x563026,
        actionFill: 0xC2410C,
        actionPressedFill: 0x9A3412
    )

    /// Deep wine red, leaning to purple (OKLCH hue 12°): each colour at the mean lightness of
    /// Navy's, Forest's and Ember's for the same role, with the highest chroma of the three,
    /// and a return key with the same contrast against its white label as theirs (5.2:1).
    static let burgundy = KeyboardThemeColors(
        isDark: true,
        background: 0x1A090C,
        keyFill: 0x3A1B20,
        keyPressedFill: 0x512A2F,
        keyHeldFill: 0x5A3035,
        functionHeldFill: 0x68393F,
        spaceHeldFill: 0x432126,
        keyShadow: nil,
        popupFill: 0x371B1F,
        bubbleFill: 0x472227,
        accentSelectFill: 0x61343A,
        actionFill: 0xBC4059,
        actionPressedFill: 0x9E354A
    )
}
