import UIKit

/// The keyboard panel's colour, opaque, under everything the keyboard extension draws.
///
/// iOS 26 and later draw a glass panel behind a third-party keyboard, and Hex drew nothing of
/// its own under its translucent keys. Switching from Apple's keyboard, iOS 27.0.1 could leave
/// Apple's layout in that panel, pinned to its top, and it showed through Hex a row above
/// Hex's own keys, switch after switch (TestFlight build 56). The panel's colour, opaque,
/// hides whatever iOS leaves behind the keyboard.
///
/// The colours are Apple's panel over a plain background, measured from simulator
/// screenshots (iOS 27.0 and 27.2). Over vivid content the glass takes on some of its colour
/// (about a quarter) and this doesn't, so there the strip iOS draws above the keyboard and the
/// globe row below can be tinted while the keys' area isn't: that's the cost of hiding
/// everything under the keys.
@MainActor
final class KeyboardBackdropView: UIView {
    /// Apple's keyboard panel over a plain background: dark over black or Safari's dark page,
    /// light over white (over Safari's light grey page it's 224, 225, 232).
    nonisolated static func fill(isDark: Bool) -> UIColor {
        isDark
            ? UIColor(red: 34 / 255, green: 34 / 255, blue: 35 / 255, alpha: 1)
            : UIColor(red: 226 / 255, green: 227 / 255, blue: 232 / 255, alpha: 1)
    }

    /// The panel's top corners are circular with a 26 pt radius, and iOS starts a third-party
    /// keyboard about 17.5 pt below the panel's top edge, so the corners reach 8.5 pt into the
    /// keyboard (1.4 pt in at its top). Measured on the iPhone 17, 17 Pro, 17 Pro Max, Air, 17e
    /// and SE, iOS 27.0 and 27.2.
    nonisolated static let panelCornerRadius: CGFloat = 26
    /// A little under the measured 17.5 pt, so the outline errs inside the panel.
    nonisolated static let panelTopAbove: CGFloat = 17
    /// The glass's rim (an outline, and a highlight on one side) is about 1 pt wide: the fill
    /// stops inside it so the panel's edge looks as it does around Apple's keys.
    nonisolated static let rimInset: CGFloat = 1

    override class var layerClass: AnyClass { CAShapeLayer.self }
    private var shapeLayer: CAShapeLayer {
        // swiftlint:disable:next force_cast
        layer as! CAShapeLayer
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        isAccessibilityElement = false
        backgroundColor = .clear
        applyAppearance()
        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (view: KeyboardBackdropView, _) in
            view.applyAppearance()
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }


    override func layoutSubviews() {
        super.layoutSubviews()
        shapeLayer.path = Self.outline(in: bounds).cgPath
    }

    private func applyAppearance() {
        // Light or dark with the keyboard's traits, like its keys.
        shapeLayer.fillColor = Self.fill(isDark: FullKeyboardPalette.isDark(traits: traitCollection)).cgColor
    }

    /// Where the backdrop draws in a keyboard of `bounds`: the panel's outline, `rimInset`
    /// inside it. Straight sides and bottom, and at the top the panel's rounded corners, whose
    /// circles are centred `radius - topAbove` below the keyboard's top edge.
    nonisolated static func outline(
        in bounds: CGRect,
        radius: CGFloat = KeyboardBackdropView.panelCornerRadius,
        topAbove: CGFloat = KeyboardBackdropView.panelTopAbove,
        inset: CGFloat = KeyboardBackdropView.rimInset
    ) -> UIBezierPath {
        let rect = bounds.insetBy(dx: inset, dy: 0)
        guard rect.width > 0, rect.height > 0 else { return UIBezierPath() }
        let centerY = bounds.minY + radius - topAbove
        let r = radius - inset
        let rise = centerY - bounds.minY
        // The panel's corners end above the keyboard (or it's too narrow for them): a rectangle.
        guard rise > 0, rise < r, rect.width > 2 * radius else { return UIBezierPath(rect: rect) }
        // The inner circle meets the keyboard's top edge `reach` in from the circle's centre.
        let reach = sqrt(max(0, r * r - rise * rise))
        let sweep = asin(min(1, rise / r))
        let left = CGPoint(x: bounds.minX + radius, y: centerY)
        let right = CGPoint(x: bounds.maxX - radius, y: centerY)

        let path = UIBezierPath()
        path.move(to: CGPoint(x: rect.minX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: centerY))
        path.addArc(withCenter: left, radius: r, startAngle: .pi, endAngle: .pi + sweep, clockwise: true)
        path.addLine(to: CGPoint(x: right.x + reach, y: bounds.minY))
        path.addArc(withCenter: right, radius: r, startAngle: -sweep, endAngle: 0, clockwise: true)
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
        path.close()
        return path
    }
}
