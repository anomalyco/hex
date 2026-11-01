import UIKit

/// The full keyboard's colours, light and dark. Dark is chosen by the field's
/// `keyboardAppearance` first, then the host's interface style.
struct FullKeyboardPalette: Equatable {
    let isDark: Bool

    static func isDark(appearance: UIKeyboardAppearance?, traits: UITraitCollection) -> Bool {
        switch appearance {
        case .dark?:
            return true
        case .light?:
            return false
        default:
            return traits.userInterfaceStyle == .dark
        }
    }

    private static func rgb(_ hex: UInt32, alpha: CGFloat = 1) -> UIColor {
        UIColor(
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha
        )
    }

    /// Stand-in for the system keyboard backdrop, used only where no system backdrop exists.
    var backdrop: UIColor { isDark ? Self.rgb(0x1D1E21) : Self.rgb(0xD3D5DB) }

    var textBase: UIColor { isDark ? UIColor(white: 1, alpha: 0.94) : Self.rgb(0x14161B) }
    var textMuted: UIColor { isDark ? UIColor(white: 1, alpha: 0.56) : Self.rgb(0x4E5461) }

    var keyFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.135) : .white }
    var functionFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.06) : UIColor(white: 0, alpha: 0.11) }
    var keyShadow: UIColor { UIColor(white: 0, alpha: isDark ? 0.30 : 0.16) }

    /// Opaque so it can float over the strip and neighbouring keys.
    var popupFill: UIColor { isDark ? Self.rgb(0x45474C) : .white }
    var popupShadow: UIColor { UIColor(white: 0, alpha: isDark ? 0.45 : 0.22) }

    var chipFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.09) : UIColor(white: 0, alpha: 0.07) }
    var wellFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.04) : UIColor(white: 0, alpha: 0.045) }
    var separator: UIColor { isDark ? UIColor(white: 1, alpha: 0.14) : UIColor(white: 0, alpha: 0.14) }
    var selectFill: UIColor { isDark ? UIColor(white: 1, alpha: 0.96) : Self.rgb(0x14161B) }
    var selectText: UIColor { isDark ? Self.rgb(0x0A0B0D) : .white }

    var actionFill: UIColor { Self.rgb(0x0A84FF) }
    var warmCore: UIColor { Self.rgb(0xFF9500) }
    var recordingDot: UIColor { Self.rgb(0xE54D42) }

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
    /// A keycap: rounded rectangle with Apple's one-point drop shadow.
    func fillKeycap(_ rect: CGRect, fill: UIColor, shadow: UIColor, radius: CGFloat = FullKeyboardPalette.cornerRadius) {
        let path = UIBezierPath(roundedRect: rect, cornerRadius: radius).cgPath
        saveGState()
        setShadow(offset: CGSize(width: 0, height: 1), blur: 0.5, color: shadow.cgColor)
        // A translucent fill would let its own shadow show through; draw the shadow
        // from an opaque copy clipped to outside the key, then the fill on top.
        beginTransparencyLayer(auxiliaryInfo: nil)
        addPath(path)
        setFillColor(UIColor.black.cgColor)
        fillPath()
        endTransparencyLayer()
        restoreGState()

        saveGState()
        addPath(path)
        setBlendMode(.copy)
        setFillColor(UIColor.clear.cgColor)
        fillPath()
        restoreGState()

        saveGState()
        addPath(path)
        setFillColor(fill.cgColor)
        fillPath()
        restoreGState()
    }
}
