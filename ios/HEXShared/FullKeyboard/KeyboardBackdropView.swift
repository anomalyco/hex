import UIKit

/// A thin blurred material under everything the keyboard extension draws, shown only when iOS
/// may have left Apple's keyboard under Hex. The rest of the time Hex draws nothing behind its
/// keys, so its background is iOS's own glass panel: the same colour as the band iOS adds above
/// a third-party keyboard and the globe row under it, over any app or wallpaper.
///
/// iOS 26 and later draw a glass panel behind a third-party keyboard, and Hex's keys are
/// translucent. In a field that takes focus as its screen appears and has an input accessory
/// view, opened with Hex, switching to Apple's keyboard and back leaves Apple's whole key plane
/// in the app's view tree under the new Hex (`KeyboardsOnScreen` has the mechanism; iOS 27.0
/// and 27.2 simulators, every switch, also under a bare keyboard extension with a clear view).
/// A keyboard can't see or remove that view, only cover it, and `KeyboardsOnScreen.isOverAnother`
/// says when: an earlier Hex keyboard iOS never took off screen in the same field.
///
/// The cover was always on: first an opaque fill (34,34,35 dark), a slab 20 darker than the
/// glass over light pages; then this material everywhere, which reads 36 where the glass is 31
/// (Notes) and 46 where it is 54 (a light page, iPhone 17 Pro, iOS 27.2, dark mode) and stays
/// neutral grey where the glass takes a wallpaper's tint (Spotlight: 37,38,38 under a band of
/// 32,37,39). Either way Hex was a panel of another colour inside iOS's panel, with its own
/// corners and a seam under the band. Now the material is only there in the one case that
/// needs it, where the leftover keys are an even, slightly lighter tone with no shapes.
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
