import UIKit

/// The full keyboard's colours, light and dark, calibrated against Apple's iOS 26
/// Liquid Glass system keyboard. Dark follows the keyboard's trait collection, which iOS
/// sets from the field's `keyboardAppearance` and the host's current style, as it does
/// the system platter behind the keys. The document proxy's `keyboardAppearance` can't
/// decide it: the host resolves a default field to light or dark when it sends its traits
/// and doesn't send them again when only its style changes, so after switching apps it
/// could still say light in dark mode (TestFlight build 29).
struct FullKeyboardPalette: Equatable {
    /// Light or dark, as iOS's glass behind the keyboard is: everything drawn on the glass
    /// follows it, with or without a theme.
    let isDark: Bool
    /// Settings › Labs › Key colors: the keys' own colours. Automatic keeps
    /// iOS's light or dark keys; a theme changes the keys and nothing on the glass around them.
    let theme: KeyboardTheme

    init(isDark: Bool, theme: KeyboardTheme = .automatic) {
        self.theme = theme
        self.isDark = isDark
    }

    /// The theme's key colours, nil for Automatic.
    private var colors: KeyboardThemeColors? { theme.colors }

    static func isDark(traits: UITraitCollection) -> Bool {
        traits.userInterfaceStyle == .dark
    }

    private static func rgb(_ hex: UInt32, alpha: CGFloat = 1) -> UIColor {
        UIColor(
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha
        )
    }

    /// The panel behind the keys: a stand-in for the system Liquid Glass platter in standalone
    /// snapshot scenes. A theme never changes it.
    var backdrop: UIColor { isDark ? Self.rgb(0x17181A) : Self.rgb(0xDFE1E6) }

    /// Text and marks on the glass: the strip, the emoji panel, menus.
    var textBase: UIColor { isDark ? .white : .black }
    var textMuted: UIColor { isDark ? UIColor(white: 1, alpha: 0.56) : Self.rgb(0x525763) }

    /// Whether the keys are dark under light labels: Automatic's in Dark Mode, and every theme
    /// but White. Decides the labels and the emoji key's face.
    var keysAreDark: Bool { colors.map { !$0.hasDarkLabels } ?? isDark }
    /// A label or symbol on a key, its pop-up or the accent callout.
    var keyText: UIColor { keysAreDark ? .white : .black }
    /// The space bar's language name. On a tinted key, white at 0.7 keeps 5:1 or more where
    /// Automatic's 0.56 over grey keys would drop under 4.5:1.
    var keyTextMuted: UIColor {
        guard let colors else { return textMuted }
        return colors.hasDarkLabels ? Self.rgb(0x525763) : UIColor(white: 1, alpha: 0.7)
    }

    /// Unified iOS 26 Liquid Glass keycap fill across letters and modifiers:
    /// `white @ 0.16` composites to `(61,61,63)` on dark `(23,24,26)` and `(94,94,96)` on dark-field `(64,64,66)`.
    var keyFill: UIColor { colors?.keyFill ?? (isDark ? UIColor(white: 1, alpha: 0.16) : .white) }
    var keyPressedFill: UIColor { colors?.keyPressedFill ?? systemPressedFill }
    /// Automatic's pressed key, for what's pressed on the glass rather than on a key: a menu
    /// row, the Hex mark in the strip.
    var systemPressedFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.28) : Self.rgb(0xD8DCE4) }
    /// A held key on the key plane, under its pop-up: Apple's grey (222 on white keys; 105 on
    /// dark ones, iOS 27.2).
    var keyHeldFill: UIColor { colors?.keyHeldFill ?? systemHeldFill }
    /// Automatic's held key, for what's held on the glass rather than on a key: the number
    /// pad's delete and separator.
    var systemHeldFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.32) : Self.rgb(0xDEDEDE) }
    /// A held shift, delete, globe, emoji or return key: Apple's darker grey (197, 197, 201;
    /// 131 on dark keys).
    var functionHeldFill: UIColor { colors?.functionHeldFill ?? (isDark ? UIColor(white: 1, alpha: 0.44) : Self.rgb(0xC5C5C9)) }
    /// A held space bar: only a shade darker (242, 242, 247; 56 on dark keys).
    var spaceHeldFill: UIColor { colors?.spaceHeldFill ?? (isDark ? UIColor(white: 1, alpha: 0.1) : Self.rgb(0xF2F2F7)) }
    /// Secondary fill for non-key chips and the recording panel's cancel pill.
    var functionFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.09) : UIColor(white: 0, alpha: 0.065) }
    /// Flat keycaps, with or without a theme.
    var keyShadow: UIColor { .clear }

    /// Opaque so balloons and menus float cleanly over the strip and neighbouring keys.
    var popupFill: UIColor { isDark ? Self.rgb(0x4E5056) : .white }
    /// The key pop-up's and the accent callout's bubble: white, or in dark Apple's neutral grey
    /// (63 at its top, lightening downward).
    var bubbleFill: UIColor { colors?.bubbleFill ?? (isDark ? Self.rgb(0x3F3F41) : .white) }
    var bubbleShadow: UIColor { UIColor(white: 0, alpha: isDark ? 0.3 : 0.1) }
    /// The accent callout's selected option: a grey square behind it (227, 227, 230; 87 in dark).
    var accentSelectFill: UIColor { colors?.accentSelectFill ?? (isDark ? Self.rgb(0x57575A) : Self.rgb(0xE3E3E6)) }
    var popupBorder: UIColor { isDark ? UIColor(white: 1, alpha: 0.14) : UIColor(white: 0, alpha: 0.08) }
    var popupShadow: UIColor { UIColor(white: 0, alpha: isDark ? 0.42 : 0.18) }

    var chipFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.11) : UIColor(white: 0, alpha: 0.065) }
    var separator: UIColor { isDark ? UIColor(white: 1, alpha: 0.12) : UIColor(white: 0, alpha: 0.10) }
    var selectFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.18) : UIColor(white: 0, alpha: 0.075) }
    var selectText: UIColor { textBase }

    /// Apple's predictive bar, measured on its platter (dark (34,34,35), light (226,227,232),
    /// iOS 27.2): words (163,163,164) / (74,74,76), dividers (50,50,53) / (211,212,217), and the
    /// pending autocorrection's and a pressed word's capsule (115,115,115) under white words /
    /// (235,237,240) under (44,46,49).
    var stripText: UIColor { isDark ? UIColor(white: 1, alpha: 0.58) : UIColor(white: 0, alpha: 0.67) }
    var stripDivider: UIColor { isDark ? UIColor(white: 1, alpha: 0.075) : UIColor(white: 0, alpha: 0.066) }
    /// A pending autocorrection's or pressed word's capsule.
    var stripPillFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.37) : UIColor(white: 1, alpha: 0.33) }
    /// A pressed word gets the same capsule (measured the same on Apple's bar).
    var stripPillPressedFill: UIColor { stripPillFill }
    var stripPillText: UIColor { isDark ? .white : UIColor(white: 0, alpha: 0.81) }

    /// Apple system keyboard action blue (`#007AFF`).
    var actionFill: UIColor { colors?.actionFill ?? Self.rgb(0x007AFF) }
    var actionPressedFill: UIColor { colors?.actionPressedFill ?? Self.rgb(0x0062CC) }
    /// A send, go or search key that's off while the field is empty: Apple's grey (191, 192, 197;
    /// 121 on dark, iOS 27.2).
    /// With a theme, a step lighter than its keys, so it stays one of them.
    var actionDisabledFill: UIColor {
        colors?.keyHeldFill ?? (isDark ? UIColor(white: 1, alpha: 0.39) : Self.rgb(0xBFC0C5))
    }
    /// An off return key's glyph: barely there (10% of the label colour on Apple's, light or dark).
    var returnDisabledInk: UIColor { keyText.withAlphaComponent(0.1) }
    var warmCore: UIColor { Self.rgb(0xFF9500) }

    static let cornerRadius: CGFloat = 8.5
}

/// The Hex mark (two hexagon outlines and a filled core) as UIKit paths.
enum HexMarkPath {
    private static let outer: [CGPoint] = [
        CGPoint(x: 32, y: 4), CGPoint(x: 57, y: 18.5), CGPoint(x: 57, y: 45.5),
        CGPoint(x: 32, y: 60), CGPoint(x: 7, y: 45.5), CGPoint(x: 7, y: 18.5),
    ]
    private static let middle: [CGPoint] = [
        CGPoint(x: 32, y: 15), CGPoint(x: 48, y: 24.25), CGPoint(x: 48, y: 39.75),
        CGPoint(x: 32, y: 49), CGPoint(x: 16, y: 39.75), CGPoint(x: 16, y: 24.25),
    ]
    private static let core: [CGPoint] = [
        CGPoint(x: 32, y: 24), CGPoint(x: 39.5, y: 28.25), CGPoint(x: 39.5, y: 36.75),
        CGPoint(x: 32, y: 41), CGPoint(x: 24.5, y: 36.75), CGPoint(x: 24.5, y: 28.25),
    ]

    static func draw(in context: CGContext, rect: CGRect, color: UIColor, coreColor: UIColor?) {
        let side = min(rect.width, rect.height)
        let s = side / 64
        let origin = CGPoint(x: rect.midX - side / 2, y: rect.midY - side / 2)

        func path(_ points: [CGPoint]) -> CGPath {
            let p = CGMutablePath()
            p.addLines(between: points.map { CGPoint(x: origin.x + $0.x * s, y: origin.y + $0.y * s) })
            p.closeSubpath()
            return p
        }

        context.saveGState()
        context.setLineWidth(max(1.15, 4.5 * s))
        context.setLineCap(.round)
        context.setLineJoin(.round)
        context.setStrokeColor(color.cgColor)
        context.addPath(path(outer))
        context.addPath(path(middle))
        context.strokePath()
        context.setFillColor((coreColor ?? color).cgColor)
        context.addPath(path(core))
        context.fillPath()
        context.restoreGState()
    }
}

extension CGContext {
    /// Draws a continuous-corner iOS 26 Liquid Glass keycap with no legacy under-shadow.
    func fillKeycap(_ rect: CGRect, fill: UIColor, shadow: UIColor = .clear, radius: CGFloat = FullKeyboardPalette.cornerRadius) {
        let path = UIBezierPath(roundedRect: rect, cornerRadius: radius).cgPath
        saveGState()
        if shadow != .clear {
            setShadow(offset: CGSize(width: 0, height: 1), blur: 0.5, color: shadow.cgColor)
        }
        addPath(path)
        setFillColor(fill.cgColor)
        fillPath()
        restoreGState()
    }
}
