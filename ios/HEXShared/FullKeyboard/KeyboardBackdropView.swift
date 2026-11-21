import UIKit

/// A thin blurred material under everything the keyboard extension draws. It takes most of
/// the colour of iOS's glass panel behind it, and blurs away the shapes of anything iOS leaves
/// there.
///
/// iOS 26 and later draw a glass panel behind a third-party keyboard, and Hex's keys are
/// translucent. In a field that takes focus as its screen appears and has an input accessory
/// view, switching from Hex to Apple's keyboard and back leaves Apple's whole key plane in the
/// host's view tree under the remote keyboard (a `UIKeyboardAutomatic` beside the remote view
/// in `_UIKBCompatInputView`; iOS 27.0 and 27.2 simulators, every switch, also under a bare
/// keyboard extension with a clear view). A clear Hex showed Apple's keys a row above its own
/// (TestFlight build 56, iPhone 17, iOS 27.0.1). A keyboard can't see or remove that view, only
/// cover it.
///
/// The cover was an opaque fill in the panel's colour over a plain background (34,34,35 dark,
/// 226,227,232 light). But the glass follows what is behind it: on an iPhone 17 Pro (iOS 27.2)
/// in dark mode it reads 31 (of 255) over Notes and 54 over a light web page, so over light
/// pages Hex was a slab 20 darker with a hard, straight top edge and square corners inside
/// iOS's rounded panel, and the top bar looked clipped. Through the material's blur the
/// leftover keys are an even, slightly lighter tone with no shapes, and Hex's area reads 36
/// over Notes and 46 over that page: within 8 of the glass both ways, with the panel's own
/// corners. A clear keyboard is the glass exactly, but shows the leftover keys.
@MainActor
final class KeyboardBackdropView: UIView {
    /// The thinnest system material: closest to the bare glass (228,230,232 on a glass of
    /// 226,227,232, light keyboard over a white app, iOS 27.2 simulator), with the same blur as
    /// the thicker ones.
    nonisolated static func blurStyle(isDark: Bool) -> UIBlurEffect.Style {
        isDark ? .systemUltraThinMaterialDark : .systemUltraThinMaterialLight
    }

    /// The panel's top corners are circular with a 26 pt radius (iPhone 17, 17 Pro, 17 Pro Max,
    /// Air, 17e and SE, iOS 27.0 and 27.2). Where iOS draws nothing above a third-party
    /// keyboard, the keyboard's top edge is the panel's, so the material rounds its own top
    /// corners the same way and never draws over the app beside them. Where iOS adds its
    /// 17 pt above, the panel's corners end 8.5 pt into the keyboard and the rest of each
    /// rounded corner is bare glass.
    nonisolated static let panelCornerRadius: CGFloat = 26
    /// The glass's rim (an outline, and a highlight on one side) is about 1 pt wide: the
    /// material stops inside it, at the sides and the top, so the panel's edge looks as it does
    /// around Apple's keys and the material's corners are the panel's, 1 pt in.
    nonisolated static let rimInset: CGFloat = 1

    /// The dark material lightens the glass: 46 on Notes' 31 and 59 on a light page's 54
    /// (iPhone 17 Pro, iOS 27.2). This much black over it brings both as near the glass as one
    /// shade can (36 and 46, about a seventh off each way). The light material needs none.
    nonisolated static func shadeAlpha(isDark: Bool) -> CGFloat { isDark ? 0.22 : 0 }

    private let material = UIVisualEffectView(effect: nil)
    private let shade = UIView()

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        isAccessibilityElement = false
        backgroundColor = .clear
        material.isUserInteractionEnabled = false
        material.clipsToBounds = true
        material.layer.cornerCurve = .circular
        material.layer.maskedCorners = [.layerMinXMinYCorner, .layerMaxXMinYCorner]
        addSubview(material)
        shade.isUserInteractionEnabled = false
        shade.backgroundColor = .black
        material.contentView.addSubview(shade)
        applyAppearance()
        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (view: KeyboardBackdropView, _) in
            view.applyAppearance()
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override func layoutSubviews() {
        super.layoutSubviews()
        material.frame = Self.frame(in: bounds)
        material.layer.cornerRadius = Self.cornerRadius(in: bounds)
        shade.frame = material.bounds
    }

    private func applyAppearance() {
        // Light or dark with the keyboard's traits, like its keys.
        let isDark = FullKeyboardPalette.isDark(traits: traitCollection)
        material.effect = UIBlurEffect(style: Self.blurStyle(isDark: isDark))
        shade.alpha = Self.shadeAlpha(isDark: isDark)
    }

    /// Where the material draws in a keyboard of `bounds`: `inset` inside the panel's rim at
    /// each side and the top, down to the keyboard's bottom.
    nonisolated static func frame(in bounds: CGRect, inset: CGFloat = KeyboardBackdropView.rimInset) -> CGRect {
        let dx = min(inset, bounds.width / 2), dy = min(inset, bounds.height)
        return CGRect(x: bounds.minX + dx, y: bounds.minY + dy, width: bounds.width - 2 * dx, height: bounds.height - dy)
    }

    /// The material's top corner radius: the panel's, less the rim, where the keyboard is wide
    /// and tall enough for it.
    nonisolated static func cornerRadius(
        in bounds: CGRect,
        radius: CGFloat = KeyboardBackdropView.panelCornerRadius,
        inset: CGFloat = KeyboardBackdropView.rimInset
    ) -> CGFloat {
        let rect = frame(in: bounds, inset: inset)
        return max(0, min(radius - inset, rect.width / 2, rect.height))
    }

    /// The material's outline: straight sides and bottom, and at the top the panel's rounded
    /// corners, `inset` inside them (the same centres, where the keyboard's top is the panel's).
    nonisolated static func outline(
        in bounds: CGRect,
        radius: CGFloat = KeyboardBackdropView.panelCornerRadius,
        inset: CGFloat = KeyboardBackdropView.rimInset
    ) -> UIBezierPath {
        let rect = frame(in: bounds, inset: inset)
        guard rect.width > 0, rect.height > 0 else { return UIBezierPath() }
        let r = cornerRadius(in: bounds, radius: radius, inset: inset)
        guard r > 0 else { return UIBezierPath(rect: rect) }
        let path = UIBezierPath()
        path.move(to: CGPoint(x: rect.minX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + r))
        path.addArc(
            withCenter: CGPoint(x: rect.minX + r, y: rect.minY + r), radius: r,
            startAngle: .pi, endAngle: 1.5 * .pi, clockwise: true
        )
        path.addLine(to: CGPoint(x: rect.maxX - r, y: rect.minY))
        path.addArc(
            withCenter: CGPoint(x: rect.maxX - r, y: rect.minY + r), radius: r,
            startAngle: 1.5 * .pi, endAngle: 0, clockwise: true
        )
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
        path.close()
        return path
    }
}
