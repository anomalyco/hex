import UIKit

/// Draws the keys and turns touches into `FullKeyboardEngine` calls. The Hex and globe
/// keys are real controls laid over their slots by `FullKeyboardView`; this view only
/// draws the globe's keycap and leaves the Hex slot empty.
@MainActor
final class KeyboardKeyPlaneView: UIView {
    let engine: FullKeyboardEngine
    var palette = FullKeyboardPalette(isDark: false) {
        didSet {
            guard palette != oldValue else { return }
            swipe?.trail.palette = palette
            setNeedsDisplay()
        }
    }
    var returnStyle = ReturnKeyStyle(.default) { didSet { if returnStyle != oldValue { setNeedsDisplay() } } }
    var returnIsEnabled = true { didSet { if returnIsEnabled != oldValue { setNeedsDisplay() } } }
    /// Drawn faintly on the space bar: the keyboard's language while it switches between two
    /// or more ("Français"), else the dictation language's name ("English", "Norsk"), with
    /// the language dictations paste in while they're translated ("Español → English").
    var spaceLabel: String? { didSet { if spaceLabel != oldValue { setNeedsDisplay() } } }
    /// A key just let go whose pop-up is still up: drawn held a moment longer, with it.
    var lingeringKind: KeyKind? { didSet { if lingeringKind != oldValue { setNeedsDisplay() } } }
    /// Called on every key going down, for clicks and haptics.
    var onKeyDown: ((KeyKind) -> Void)?
    /// Called when long-press accents open.
    var onAccentMenuOpened: (() -> Void)?

    private var touchIDs: [ObjectIdentifier: Int] = [:]
    private var nextTouchID = 0
    private var deleteTimer: Timer?
    private var deleteStartedAt: TimeInterval = 0
    /// Hold delete and slide left to delete word by word (`DeleteWordSwipe`).
    private(set) var deleteSwipe: DeleteWordSwipe?
    /// Each held key's pending hold: a letter's accents, or the space bar's trackpad.
    private var accentHoldWork: [Int: DispatchWorkItem] = [:]
    /// Function keys just tapped, still drawn shaded (`functionLinger`).
    private var lingeringFunctionKinds: Set<KeyKind> = []
    private var functionLingerWork: [KeyKind: DispatchWorkItem] = [:]
    /// Apple's delete, space, return, globe and emoji keys stay shaded about 60 ms after a tap
    /// lifts (iOS 27.2, 60 fps: 265–284 ms shown for 200 ms taps, Hex 200–210); a delete held
    /// long enough to repeat lets go at once.
    static let functionLinger: TimeInterval = 0.06
    /// The key labels' trackpad fade, out as the space bar becomes the trackpad and back in as it
    /// lets go: about 200 ms of visible change each way on Apple's, quick at first and starting a
    /// frame after the change (iOS 27.2, 60 fps); none with Reduce Motion.
    static let trackpadFade: TimeInterval = 0.24
    /// The keycaps' opacity while the space bar is the trackpad: Apple's go from white to
    /// (242, 242, 247) on its (226, 227, 232) light, and from 68 to 54 on 32 dark, about 60% of
    /// the way from the backdrop to the key (iOS 27.2).
    static let trackpadKeycapAlpha: CGFloat = 0.6
    private var glyphAlpha: CGFloat = 1
    private var glyphFade: (from: CGFloat, to: CGFloat, start: CFTimeInterval)?
    private var glyphFadeLink: CADisplayLink?
    /// Settings › Keyboard › Swipe typing, while it's on for the layout's language; its trail draws
    /// above the keys.
    var swipe: SwipeTyping? {
        didSet {
            guard swipe !== oldValue else { return }
            oldValue?.cancelAll()
            oldValue?.trail.removeFromSuperview()
            guard let swipe else { return }
            swipe.trail.frame = bounds
            swipe.trail.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            swipe.trail.palette = palette
            addSubview(swipe.trail)
        }
    }

    init(engine: FullKeyboardEngine) {
        self.engine = engine
        super.init(frame: .zero)
        isOpaque = false
        backgroundColor = .clear
        isMultipleTouchEnabled = true
        contentMode = .redraw
        isAccessibilityElement = false
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// The key area, and while you type the strip's lower half (`takesStripTouch(at:)`):
    /// this view lies over the strip, so it gets those touches before the suggestions.
    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        engine.layout.touchAreaFrame.contains(point) || engine.takesStripTouch(at: point)
    }

    // MARK: - Drawing

    override func draw(_ rect: CGRect) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        let layout = engine.layout
        let isLandscape = layout.orientation == .landscape
        let radius = layout.metrics.cornerRadius
        var pressed = engine.pressedKinds
        if let lingeringKind { pressed.insert(lingeringKind) }
        pressed.formUnion(lingeringFunctionKinds)
        // Like Apple's spacebar trackpad, key glyphs fade out while sliding the cursor, and the
        // keycaps fade most of the way to blank (`trackpadKeycapAlpha`).
        let hideGlyphs = engine.isTrackpadActive
        let glyphAlpha = currentGlyphAlpha()
        let keycapAlpha = 1 - (1 - Self.trackpadKeycapAlpha) * (1 - glyphAlpha)
        let showGlyphs = { () -> Bool in
            context.setAlpha(glyphAlpha)
            return glyphAlpha > 0
        }

        for key in layout.keys {
            guard key.frame.intersects(rect) else { continue }
            context.setAlpha(keycapAlpha)
            // Keycaps on whole device pixels, as Apple draws them: crisp edges, not half-covered ones.
            let frame = pixelAligned(key.frame)
            // Row 4's labels sit where they would on Apple's taller row 4 (`bottomRowTrim`).
            let labelFrame = key.row == 3
                ? CGRect(x: frame.minX, y: frame.minY, width: frame.width, height: frame.height + layout.metrics.bottomRowTrim)
                : frame
            switch key.kind {
            case .hex:
                continue
            case .character:
                // The pressed key stays in place, grey, under iOS 27's floating pop-up.
                let fill = pressed.contains(key.kind) ? palette.keyHeldFill : palette.keyFill
                context.fillKeycap(frame, fill: fill, radius: radius)
                guard showGlyphs() else { continue }
                var label = Self.keycapLabel(engine.keycapLabel(for: key))
                let isUpperLetter = layout.layer == .letters && engine.keycapsUppercase && label.first?.isUppercase == true
                let style = isLandscape
                    ? (isUpperLetter ? Self.landscapeCapital
                        : label.first?.isLetter == true ? Self.landscapeLowercase
                        : Self.landscapeSymbolOverrides[label] ?? Self.landscapeSymbol)
                    : isUpperLetter ? LabelStyle.capital
                    : label.first?.isLetter == true ? LabelStyle.lowercase
                    : LabelStyle.symbolOverrides[label] ?? LabelStyle.symbol
                if let shown = style.shows { label = shown }
                if !isLandscape, layout.layer == .letters, label.first?.isLetter == true {
                    drawLetter(label, in: labelFrame, style: style, capital: isUpperLetter)
                } else {
                    drawText(label, in: labelFrame, font: style.font, color: palette.textBase, yOffset: style.yOffset)
                }
            case .space:
                // Apple's space bar unshades as it becomes the trackpad.
                let fill = pressed.contains(.space) && !hideGlyphs ? palette.spaceHeldFill : palette.keyFill
                context.fillKeycap(frame, fill: fill, radius: radius)
                guard showGlyphs(), let spaceLabel else { continue }
                drawText(
                    spaceLabel,
                    in: labelFrame,
                    font: .systemFont(ofSize: isLandscape ? 13 : 14.5, weight: .regular),
                    color: palette.textMuted
                )
            case .returnKey:
                let isPressed = pressed.contains(.returnKey) && returnIsEnabled
                let isAction = returnStyle.isAction && !hideGlyphs
                let activeAction = isAction && returnIsEnabled
                let fill: UIColor = if activeAction {
                    isPressed ? palette.actionPressedFill : palette.actionFill
                } else if isAction {
                    palette.actionDisabledFill
                } else {
                    isPressed ? palette.functionHeldFill : palette.keyFill
                }
                let ink: UIColor = if !returnIsEnabled {
                    palette.returnDisabledInk
                } else if activeAction {
                    .white
                } else {
                    palette.textBase
                }
                context.fillKeycap(frame, fill: fill, radius: radius)
                guard showGlyphs() else { continue }
                drawSymbol(
                    returnStyle.symbol,
                    in: labelFrame,
                    color: ink,
                    pointSize: isLandscape ? 18 : 19,
                    weight: activeAction ? .medium : .regular,
                    yOffset: isLandscape ? -0.5 : -0.33
                )
            case .shift:
                let shift = engine.shift
                let isPressed = pressed.contains(.shift) && !shift.pressShowsOff
                context.fillKeycap(frame, fill: isPressed ? palette.functionHeldFill : palette.keyFill, radius: radius)
                guard showGlyphs() else { continue }
                let symbol = if shift.pressShowsOff {
                    "shift"
                } else {
                    switch shift.state {
                    case .off: shift.isHeld ? "shift.fill" : "shift"
                    case .on: "shift.fill"
                    case .locked: "capslock.fill"
                    }
                }
                drawSymbol(symbol, in: labelFrame, color: palette.textBase, pointSize: isLandscape ? 18 : 19, yOffset: isLandscape ? -0.16 : 0.67)
            case .delete:
                let lit = pressed.contains(.delete)
                context.fillKeycap(frame, fill: lit ? palette.functionHeldFill : palette.keyFill, radius: radius)
                guard showGlyphs() else { continue }
                drawSymbol(lit ? "delete.left.fill" : "delete.left", in: labelFrame, color: palette.textBase, pointSize: isLandscape ? 18.5 : 19, yOffset: isLandscape ? -0.16 : 0.33)
            case .layerSwitch:
                let lit = pressed.contains(key.kind)
                context.fillKeycap(frame, fill: lit ? palette.keyHeldFill : palette.keyFill, radius: radius)
                guard showGlyphs() else { continue }
                let style = isLandscape
                    ? (key.label == "123" && key.row == 3 ? LabelStyle(size: 15.5, weight: UIFont.Weight(rawValue: -0.05), yOffset: 0)
                        : key.row == 3 ? LabelStyle(size: 17.75, weight: UIFont.Weight(rawValue: 0.08), yOffset: -0.83)
                        : key.label == "#+=" ? LabelStyle(size: 12.25, weight: UIFont.Weight(rawValue: 0.2), yOffset: -1.33)
                        : LabelStyle(size: 15.5, weight: UIFont.Weight(rawValue: -0.05), yOffset: 0))
                    : key.row == 3 ? LabelStyle.pageKey
                    : key.label == "#+=" ? LabelStyle.symbolsPageKey
                    : LabelStyle(size: 15.5, yOffset: 0)
                drawText(key.label, in: labelFrame, font: style.font, color: palette.textBase, yOffset: style.yOffset)
            case .globe:
                let lit = pressed.contains(.globe)
                context.fillKeycap(frame, fill: lit ? palette.functionHeldFill : palette.keyFill, radius: radius)
                guard showGlyphs() else { continue }
                drawSymbol("globe", in: labelFrame, color: palette.textBase, pointSize: isLandscape ? 16 : 19, yOffset: isLandscape ? 0 : -1)
            case .emoji:
                let lit = pressed.contains(.emoji)
                let fill = lit ? palette.functionHeldFill : palette.keyFill
                context.fillKeycap(frame, fill: fill, radius: radius)
                guard showGlyphs() else { continue }
                drawEmojiFace(in: labelFrame, diameter: isLandscape ? 18 : 19, context: context, keyFill: fill)
            }
        }
    }

    /// `rect` with its edges on this screen's pixel grid.
    private func pixelAligned(_ rect: CGRect) -> CGRect {
        let scale = max(1, traitCollection.displayScale)
        let x0 = (rect.minX * scale).rounded() / scale
        let x1 = (rect.maxX * scale).rounded() / scale
        let y0 = (rect.minY * scale).rounded() / scale
        let y1 = (rect.maxY * scale).rounded() / scale
        return CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
    }

    private func drawText(_ text: String, in frame: CGRect, font: UIFont, color: UIColor, yOffset: CGFloat = 0) {
        let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: color]
        let size = (text as NSString).size(withAttributes: attributes)
        let origin = CGPoint(
            x: frame.midX - size.width / 2,
            y: frame.midY - size.height / 2 + yOffset
        )
        (text as NSString).draw(at: origin, withAttributes: attributes)
    }

    /// Apple's portrait letters sit a little right of where centring by advance puts them, by
    /// the same amount on every iPhone of a scale (iOS 27.2 simulators, ink centroids of all 26
    /// letters at native resolution, light and dark): lowercase 0.13-0.22 pt on 3x screens (17
    /// Pro, 17e, Air, Pro Max), capitals within 0.13 either way, and 0.55 and 0.23 on the 2x SE.
    /// Each key's letter also strays up to about 0.6 pt from that, in a pattern that differs from
    /// one iPhone to the next, so it isn't modelled.
    private func drawLetter(_ text: String, in frame: CGRect, style: LabelStyle, capital: Bool) {
        let attributes: [NSAttributedString.Key: Any] = [.font: style.font, .foregroundColor: palette.textBase]
        let size = (text as NSString).size(withAttributes: attributes)
        let nudge: CGFloat = traitCollection.displayScale >= 3 ? (capital ? 0 : 0.17) : (capital ? 0.23 : 0.55)
        (text as NSString).draw(
            at: CGPoint(x: frame.midX + nudge - size.width / 2, y: frame.midY - size.height / 2 + style.yOffset),
            withAttributes: attributes
        )
    }


    /// Apple's emoji key shows a grinning face that no public SF Symbol has (iOS 27.2: an open
    /// mouth with a band of teeth; Hex drew face.smiling, a closed smile, at 16 pt turned).
    /// Drawn to measurements of its 3x screenshots on every iPhone, in pt for a 19 pt face
    /// (18 turned): centred 0.67 above the key's middle; a 1.58 ring (a solid face in dark, its
    /// features cut out); 2 x 2.67 eyes 2.78 either side and 2.13 above the centre; a mouth 12.33
    /// wide, its top 0.5 below the centre at the corners and 1.5 in the middle, its bottom an arc
    /// 6.5 down; teeth a 1 pt band 1 under its top, always the light colour.
    private func drawEmojiFace(in frame: CGRect, diameter: CGFloat, context: CGContext, keyFill: UIColor) {
        let ink = palette.textBase.cgColor
        context.saveGState()
        context.translateBy(x: frame.midX, y: frame.midY - 0.67)
        let k = diameter / 19
        context.scaleBy(x: k, y: k)
        let eyes = [CGRect(x: -3.78, y: -3.465, width: 2, height: 2.67), CGRect(x: 1.78, y: -3.465, width: 2, height: 2.67)]
        let w: CGFloat = 6.17
        let kappa: CGFloat = 0.5523
        let mouth = CGMutablePath()
        mouth.move(to: CGPoint(x: -w, y: 0.5))
        mouth.addQuadCurve(to: CGPoint(x: w, y: 0.5), control: CGPoint(x: 0, y: 2.5))
        mouth.addCurve(to: CGPoint(x: 0, y: 6.5), control1: CGPoint(x: w, y: 0.5 + 6 * kappa), control2: CGPoint(x: w * kappa, y: 6.5))
        mouth.addCurve(to: CGPoint(x: -w, y: 0.5), control1: CGPoint(x: -w * kappa, y: 6.5), control2: CGPoint(x: -w, y: 0.5 + 6 * kappa))
        mouth.closeSubpath()
        if palette.isDark {
            // A solid face, its eyes and mouth cut through to the keycap.
            let face = CGMutablePath()
            face.addEllipse(in: CGRect(x: -9.5, y: -9.5, width: 19, height: 19))
            eyes.forEach { face.addEllipse(in: $0) }
            face.addPath(mouth)
            context.addPath(face)
            context.setFillColor(ink)
            context.fillPath(using: .evenOdd)
        } else {
            context.setStrokeColor(ink)
            context.setLineWidth(1.58)
            context.strokeEllipse(in: CGRect(x: -8.71, y: -8.71, width: 17.42, height: 17.42))
            context.setFillColor(ink)
            eyes.forEach { context.fillEllipse(in: $0) }
            context.addPath(mouth)
            context.fillPath()
        }
        // The teeth: a band under the mouth's top, short of its edge, in the light colour.
        let band = CGMutablePath()
        band.move(to: CGPoint(x: -w, y: 1.5))
        band.addQuadCurve(to: CGPoint(x: w, y: 1.5), control: CGPoint(x: 0, y: 3.5))
        band.addLine(to: CGPoint(x: w, y: 2.5))
        band.addQuadCurve(to: CGPoint(x: -w, y: 2.5), control: CGPoint(x: 0, y: 4.5))
        band.closeSubpath()
        context.addPath(mouth)
        context.clip()
        context.addPath(mouth)
        context.addPath(mouth.copy(strokingWithWidth: 1.8, lineCap: .butt, lineJoin: .round, miterLimit: 10))
        context.clip(using: .evenOdd)
        context.setFillColor(palette.isDark ? ink : keyFill.cgColor)
        context.addPath(band)
        context.fillPath()
        context.restoreGState()
    }

    private func drawSymbol(
        _ name: String,
        in frame: CGRect,
        color: UIColor,
        pointSize: CGFloat = 18,
        weight: UIImage.SymbolWeight = .regular,
        yOffset: CGFloat = 0
    ) {
        let config = UIImage.SymbolConfiguration(pointSize: pointSize, weight: weight)
        guard let image = UIImage(systemName: name, withConfiguration: config)?
            .withTintColor(color, renderingMode: .alwaysOriginal) else { return }
        let size = image.size
        image.draw(in: CGRect(
            x: frame.midX - size.width / 2,
            y: frame.midY - size.height / 2 + yOffset,
            width: size.width,
            height: size.height
        ))
    }

    /// A key label's type, fitted to Apple's keys from screenshots (iOS 27.2, the same on every
    /// iPhone): each label's ink box and darkness against labels Hex drew at known sizes and
    /// weights. `yOffset` moves it from the key's centre.
    struct LabelStyle {
        var size: CGFloat
        var weight: UIFont.Weight = .regular
        var yOffset: CGFloat
        /// The font's case-sensitive forms, as Apple's symbol labels use: brackets, dashes and
        /// the like sit centred on the digits instead of on lowercase letters.
        var caseSensitive = false
        /// A different glyph to draw (the key still types its own character).
        var shows: String?

        @MainActor var font: UIFont {
            let key = FontKey(size: size, weight: weight.rawValue, caseSensitive: caseSensitive)
            if let font = Self.fonts[key] { return font }
            var font = UIFont.systemFont(ofSize: size, weight: weight)
            if caseSensitive {
                let settings: [[UIFontDescriptor.FeatureKey: Int]] = [[.type: kCaseSensitiveLayoutType, .selector: kCaseSensitiveLayoutOnSelector]]
                font = UIFont(descriptor: font.fontDescriptor.addingAttributes([.featureSettings: settings]), size: size)
            }
            Self.fonts[key] = font
            return font
        }

        private struct FontKey: Hashable {
            let size: CGFloat
            let weight: CGFloat
            let caseSensitive: Bool
        }

        @MainActor private static var fonts: [FontKey: UIFont] = [:]

        /// Lowercase letters: 24.5 pt (Hex drew 22.5), 2.3 pt above the centre.
        static let lowercase = LabelStyle(size: 24.5, yOffset: -2.33)
        /// Capitals: 21 pt a little heavier than regular, 1.33 pt up. At 1 pt they sat 0.27 pt
        /// below Apple's (ink centroids, every letter, light and dark, 17 Pro and Pro Max); text
        /// lands on whole pixels, so the fix is a whole pixel.
        static let capital = LabelStyle(size: 21, weight: UIFont.Weight(rawValue: 0.12), yOffset: -1.33)
        /// Digits, punctuation and symbols: 21.5 pt, a touch heavier, case-sensitive forms.
        static let symbol = LabelStyle(size: 21.5, weight: UIFont.Weight(rawValue: 0.08), yOffset: -1.33, caseSensitive: true)
        /// The symbols Apple draws at their own sizes, fitted one by one (`<` and `>` fit
        /// no size: Apple's are a different shape).
        static let symbolOverrides: [String: LabelStyle] = [
            "/": LabelStyle(size: 20.25, weight: UIFont.Weight(rawValue: 0.08), yOffset: -1, caseSensitive: true),
            "\\": LabelStyle(size: 20.25, weight: UIFont.Weight(rawValue: 0.08), yOffset: -1, caseSensitive: true),
            "|": LabelStyle(size: 20.25, weight: UIFont.Weight(rawValue: 0.08), yOffset: -1.33, caseSensitive: true),
            ":": LabelStyle(size: 25.5, weight: UIFont.Weight(rawValue: 0.08), yOffset: -1.66),
            ".": LabelStyle(size: 22.25, weight: UIFont.Weight(rawValue: 0.23), yOffset: 1.66, caseSensitive: true),
            ",": LabelStyle(size: 26.75, weight: UIFont.Weight(rawValue: -0.05), yOffset: -0.34, caseSensitive: true),
            "#": LabelStyle(size: 25.5, weight: UIFont.Weight(rawValue: -0.05), yOffset: -1.67),
            "*": LabelStyle(size: 20, weight: UIFont.Weight(rawValue: 0.04), yOffset: -1.33, caseSensitive: true),
            "=": LabelStyle(size: 20, weight: UIFont.Weight(rawValue: -0.05), yOffset: -2.67),
            "~": LabelStyle(size: 22.25, weight: UIFont.Weight(rawValue: 0.23), yOffset: -3, caseSensitive: true),
            "_": LabelStyle(size: 20.5, weight: UIFont.Weight(rawValue: 0.08), yOffset: 2.33, caseSensitive: true),
            // Apple's bullet is a large dot, the shape of U+2981.
            "\u{2022}": LabelStyle(size: 25.25, weight: UIFont.Weight(rawValue: 0.12), yOffset: -3, caseSensitive: true, shows: "\u{2981}"),
        ]
        /// 123 and ABC on row 4: 17.75 pt (Hex drew 15.5).
        static let pageKey = LabelStyle(size: 17.75, weight: UIFont.Weight(rawValue: 0.06), yOffset: -1)
        /// #+= on row 3: small, 12.25 pt and heavier.
        static let symbolsPageKey = LabelStyle(size: 12.25, weight: UIFont.Weight(rawValue: 0.2), yOffset: -0.33)
    }

    /// Turned, Apple's labels are about 0.875 of their portrait sizes (fitted the same way on
    /// the 17 Pro, 17e and SE), with their own exceptions.
    static let landscapeLowercase = LabelStyle(size: 21.5, weight: UIFont.Weight(rawValue: -0.05), yOffset: -2.17)
    static let landscapeCapital = LabelStyle(size: 18.75, weight: UIFont.Weight(rawValue: 0.08), yOffset: -0.17)
    static let landscapeSymbol = LabelStyle(size: 18.75, weight: UIFont.Weight(rawValue: 0.08), yOffset: -0.17, caseSensitive: true)
    static let landscapeSymbolOverrides: [String: LabelStyle] = [
            "-": LabelStyle(size: 19.5, weight: UIFont.Weight(rawValue: 0), yOffset: -0.5, caseSensitive: true),
            "/": LabelStyle(size: 16.5, weight: UIFont.Weight(rawValue: 0.2), yOffset: -0.5, caseSensitive: true),
            ":": LabelStyle(size: 21.75, weight: UIFont.Weight(rawValue: 0.04), yOffset: 2.17, caseSensitive: true),
            ";": LabelStyle(size: 16.75, weight: UIFont.Weight(rawValue: 0.2), yOffset: 0.17, caseSensitive: true),
            "@": LabelStyle(size: 19, weight: UIFont.Weight(rawValue: -0.05), yOffset: 0.5, caseSensitive: true),
            "\u{201D}": LabelStyle(size: 17.5, weight: UIFont.Weight(rawValue: 0.12), yOffset: -1, caseSensitive: true),
            ".": LabelStyle(size: 19.75, weight: UIFont.Weight(rawValue: 0.16), yOffset: 2.5, caseSensitive: true),
            ",": LabelStyle(size: 19.5, weight: UIFont.Weight(rawValue: 0.23), yOffset: 2.16, caseSensitive: true),
            "\u{2019}": LabelStyle(size: 16.75, weight: UIFont.Weight(rawValue: 0.23), yOffset: -1, caseSensitive: true),
            "#": LabelStyle(size: 26, weight: UIFont.Weight(rawValue: -0.2), yOffset: -0.66, caseSensitive: true),
            "*": LabelStyle(size: 16.75, weight: UIFont.Weight(rawValue: 0.08), yOffset: -0.5, caseSensitive: true),
            "+": LabelStyle(size: 19.25, weight: UIFont.Weight(rawValue: -0.12), yOffset: -0.5, caseSensitive: true),
            "=": LabelStyle(size: 15.75, weight: UIFont.Weight(rawValue: 0.16), yOffset: -0.67, caseSensitive: true),
            "_": LabelStyle(size: 17.75, weight: UIFont.Weight(rawValue: 0.12), yOffset: 3.5, caseSensitive: true),
            "\\": LabelStyle(size: 16.5, weight: UIFont.Weight(rawValue: 0.2), yOffset: -0.5, caseSensitive: true),
            "|": LabelStyle(size: 17, weight: UIFont.Weight(rawValue: 0.16), yOffset: -0.17, caseSensitive: true),
            "~": LabelStyle(size: 19.25, weight: UIFont.Weight(rawValue: 0.23), yOffset: -1.67, caseSensitive: true),
            "^": LabelStyle(size: 15.75, weight: UIFont.Weight(rawValue: 0.16), yOffset: -1, caseSensitive: true),
            "\u{2022}": LabelStyle(size: 29, weight: UIFont.Weight(rawValue: 0.23), yOffset: -0.5, caseSensitive: true),
    ]

    /// What a character key shows: Apple's numbers page labels its quote keys with curly marks
    /// (” and ’); the keys still type straight ones.
    static func keycapLabel(_ label: String) -> String {
        switch label {
        case "\"": "\u{201D}"
        case "'": "\u{2019}"
        default: label
        }
    }

    // MARK: - Touches

    private func id(for touch: UITouch, creating: Bool) -> Int? {
        let key = ObjectIdentifier(touch)
        if let id = touchIDs[key] { return id }
        guard creating else { return nil }
        nextTouchID += 1
        touchIDs[key] = nextTouchID
        return nextTouchID
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches.sorted(by: { $0.timestamp < $1.timestamp }) {
            guard let id = id(for: touch, creating: true) else { continue }
            began(id: id, sample: TypingLogTouchSample(touch, in: self))
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let id = id(for: touch, creating: false) else { continue }
            moved(id: id, sample: TypingLogTouchSample(touch, in: self))
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches.sorted(by: { $0.timestamp < $1.timestamp }) {
            guard let id = touchIDs.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            ended(id: id, sample: TypingLogTouchSample(touch, in: self))
        }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let id = touchIDs.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            cancelled(id: id, sample: TypingLogTouchSample(touch, in: self))
        }
    }

    // One finger's samples, from UIKit or a test. Record typing logs writes the touch line
    // before the engine acts on a press or a release, so the edits it causes follow it,
    // and after a move, so the line names the key the finger slid onto.

    func began(id: Int, sample: TypingLogTouchSample) {
        // A second finger on the trackpad's blank keys: no click, no key (`touchBegan`).
        if engine.isTrackpadActive { return }
        // A swipe still under way types its word before this key's letter.
        swipe?.touchWillBegin()
        // The key's tick and click go first, on touch-down as Apple's do: the press's own work
        // (a rollover's commit, autocorrect, suggestions) used to run before them and could
        // delay them by tens of milliseconds while typing fast.
        if let kind = engine.keyKind(at: sample.point) {
            onKeyDown?(kind)
        }
        if let log = typingLog {
            log.touch(.began, id: id, sample: sample, key: engine.key(at: sample.point), under: engine.layout.hitTest(sample.point))
        }
        engine.touchBegan(id: id, at: sample.point)
        if let kind = engine.touches[id]?.key.kind {
            if kind == .delete {
                startDeleteRepeat()
                deleteSwipe?.finish()
                deleteSwipe = DeleteWordSwipe(engine: engine, touchID: id, startX: sample.point.x)
            }
            if case .character = kind { armAccentTimer(for: id) }
            if kind == .space { armTrackpadHold(for: id) }
        }
        swipe?.began(id: id, at: sample.point, time: sample.timestamp)
    }

    func moved(id: Int, sample: TypingLogTouchSample) {
        // A letter touch that went a key's pitch is a swipe: it moves the trail, not the keys.
        if let swipe, swipe.moved(id: id, to: sample.point, time: sample.timestamp) {
            cancelAccentTimer(for: id)
            typingLog?.touch(.moved, id: id, sample: sample, key: nil, under: engine.layout.hitTest(sample.point))
            return
        }
        let previousKey = engine.touches[id]?.key.kind
        engine.touchMoved(id: id, to: sample.point, time: sample.timestamp)
        let current = engine.touches[id]?.key
        typingLog?.touch(.moved, id: id, sample: sample, key: current, under: engine.layout.hitTest(sample.point))
        if current?.kind != previousKey {
            cancelAccentTimer(for: id)
            if case .character = current?.kind {
                armAccentTimer(for: id)
            }
        }
        if let swipe = deleteSwipe, swipe.touchID == id, swipe.move(to: sample.point.x) {
            stopDeleteRepeat()
        }
    }

    func ended(id: Int, sample: TypingLogTouchSample) {
        cancelAccentTimer(for: id)
        if let swipe, swipe.owns(id) {
            typingLog?.touch(.ended, id: id, sample: sample, key: nil, under: engine.layout.hitTest(sample.point))
            swipe.ended(id: id, at: sample.point, time: sample.timestamp)
            return
        }
        swipe?.forget(id)
        let lingering = lingerKind(forEnding: id)
        if engine.touches[id]?.key.kind == .delete { stopDeleteRepeat() }
        finishDeleteSwipe(for: id)
        typingLog?.touch(.ended, id: id, sample: sample, key: engine.touches[id]?.key, under: engine.layout.hitTest(sample.point))
        engine.touchEnded(id: id, at: sample.point)
        if let lingering { linger(lingering) }
    }

    /// The function key a lifting touch leaves shaded a moment, if any.
    private func lingerKind(forEnding id: Int) -> KeyKind? {
        guard let touch = engine.touches[id], !touch.isTrackpad else { return nil }
        switch touch.key.kind {
        case .delete:
            let repeated = CACurrentMediaTime() - deleteStartedAt >= FullKeyboardEngine.repeatDelay
            return repeated || deleteSwipe?.touchID == id && deleteSwipe?.isActive == true ? nil : .delete
        case .space, .returnKey, .globe, .emoji:
            return touch.key.kind
        default:
            return nil
        }
    }

    private func linger(_ kind: KeyKind) {
        functionLingerWork.removeValue(forKey: kind)?.cancel()
        lingeringFunctionKinds.insert(kind)
        let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.functionLingerWork.removeValue(forKey: kind)
                if self.lingeringFunctionKinds.remove(kind) != nil { self.setNeedsDisplay() }
            }
        }
        functionLingerWork[kind] = work
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.functionLinger, execute: work)
        setNeedsDisplay()
    }

    private func endFunctionLingers() {
        functionLingerWork.values.forEach { $0.cancel() }
        functionLingerWork.removeAll()
        if !lingeringFunctionKinds.isEmpty {
            lingeringFunctionKinds.removeAll()
            setNeedsDisplay()
        }
    }

    /// The labels' opacity for this frame, easing toward hidden while the trackpad is on.
    private func currentGlyphAlpha() -> CGFloat {
        let target: CGFloat = engine.isTrackpadActive ? 0 : 1
        if glyphFade?.to != target, glyphFade != nil || glyphAlpha != target {
            if window == nil || UIAccessibility.isReduceMotionEnabled {
                endGlyphFade(at: target)
            } else {
                glyphFade = (glyphAlpha, target, CACurrentMediaTime())
                if glyphFadeLink == nil {
                    let link = CADisplayLink(target: GlyphFadeTicker(self), selector: #selector(GlyphFadeTicker.tick))
                    link.add(to: .main, forMode: .common)
                    glyphFadeLink = link
                }
            }
        }
        if let fade = glyphFade {
            let t = min(1, max(0, (CACurrentMediaTime() - fade.start) / Self.trackpadFade))
            let eased = t * (2 - t)
            glyphAlpha = fade.from + (fade.to - fade.from) * eased
        }
        return glyphAlpha
    }

    fileprivate func glyphFadeTick() {
        guard let fade = glyphFade else { return endGlyphFade(at: glyphAlpha) }
        if CACurrentMediaTime() - fade.start >= Self.trackpadFade { endGlyphFade(at: fade.to) }
        setNeedsDisplay()
    }

    private func endGlyphFade(at alpha: CGFloat) {
        glyphFade = nil
        glyphAlpha = alpha
        glyphFadeLink?.invalidate()
        glyphFadeLink = nil
    }

    func cancelled(id: Int, sample: TypingLogTouchSample) {
        cancelAccentTimer(for: id)
        if let swipe, swipe.owns(id) {
            typingLog?.touch(.cancelled, id: id, sample: sample, key: nil, under: engine.layout.hitTest(sample.point))
            swipe.cancelled(id: id)
            return
        }
        swipe?.forget(id)
        if engine.touches[id]?.key.kind == .delete { stopDeleteRepeat() }
        finishDeleteSwipe(for: id)
        typingLog?.touch(.cancelled, id: id, sample: sample, key: engine.touches[id]?.key, under: engine.layout.hitTest(sample.point))
        engine.touchCancelled(id: id)
    }

    private var typingLog: TypingLogger? { engine.typingLog }

    func cancelAllTouches() {
        if !touchIDs.isEmpty {
            typingLog?.ui("touches_cancelled", [TypingLogField("count", .int(touchIDs.count))])
        }
        touchIDs.removeAll()
        for work in accentHoldWork.values { work.cancel() }
        accentHoldWork.removeAll()
        stopDeleteRepeat()
        deleteSwipe?.finish()
        deleteSwipe = nil
        swipe?.cancelAll()
        endFunctionLingers()
        engine.cancelAllTouches()
    }

    private func finishDeleteSwipe(for id: Int) {
        guard let swipe = deleteSwipe, swipe.touchID == id else { return }
        swipe.finish()
        deleteSwipe = nil
    }

    private func armAccentTimer(for id: Int) {
        cancelAccentTimer(for: id)
        let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.accentHoldWork.removeValue(forKey: id)
                if self.engine.openAccentMenuIfEligible(forTouch: id) {
                    self.onAccentMenuOpened?()
                }
            }
        }
        accentHoldWork[id] = work
        DispatchQueue.main.asyncAfter(deadline: .now() + FullKeyboardEngine.accentHoldDelay, execute: work)
    }

    private func armTrackpadHold(for id: Int) {
        cancelAccentTimer(for: id)
        let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.accentHoldWork.removeValue(forKey: id)
                self.engine.holdSpace(forTouch: id)
            }
        }
        accentHoldWork[id] = work
        DispatchQueue.main.asyncAfter(deadline: .now() + FullKeyboardEngine.trackpadHoldDelay, execute: work)
    }

    private func cancelAccentTimer(for id: Int) {
        accentHoldWork.removeValue(forKey: id)?.cancel()
    }

    private func startDeleteRepeat() {
        stopDeleteRepeat()
        deleteStartedAt = CACurrentMediaTime()
        let timer = Timer(timeInterval: FullKeyboardEngine.repeatInterval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                let held = CACurrentMediaTime() - self.deleteStartedAt
                guard held >= FullKeyboardEngine.repeatDelay else { return }
                self.engine.repeatDelete(heldFor: held)
                self.onKeyDown?(.delete)
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        deleteTimer = timer
    }

    private func stopDeleteRepeat() {
        deleteTimer?.invalidate()
        deleteTimer = nil
    }
}

/// Drives `KeyboardKeyPlaneView`'s label fade without the display link retaining the plane.
@MainActor
private final class GlyphFadeTicker: NSObject {
    weak var plane: KeyboardKeyPlaneView?
    init(_ plane: KeyboardKeyPlaneView) { self.plane = plane }
    @objc func tick(_ link: CADisplayLink) {
        guard let plane else { return link.invalidate() }
        plane.glyphFadeTick()
    }
}

/// The pop-up balloon over a pressed key in portrait.
@MainActor
final class KeyCalloutView: UIView {
    var palette = FullKeyboardPalette(isDark: false)
    private var balloon: KeyBalloon?
    private var label = ""

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        isUserInteractionEnabled = false
        contentMode = .redraw
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func show(_ balloon: KeyBalloon?, label: String, style: KeyboardKeyPlaneView.LabelStyle? = nil) {
        let style = style ?? Self.letterStyle
        guard balloon != self.balloon || label != self.label || style.size != labelStyle.size || style.yOffset != labelStyle.yOffset else { return }
        self.balloon = balloon
        self.label = label
        labelStyle = style
        setNeedsDisplay()
    }

    /// How long the pop-up stays after the finger lifts, as Apple's does.
    static let linger: TimeInterval = 0.045

    /// A letter in the bubble, either case.
    static let letterStyle = KeyboardKeyPlaneView.LabelStyle(size: 36.5, yOffset: 0.8)

    /// The bubble's style for a key's label: a letter's, or for a digit or symbol the key's own
    /// type scaled by the same 1.5.
    static func style(for label: String) -> KeyboardKeyPlaneView.LabelStyle {
        if label.first?.isLetter == true { return letterStyle }
        let key = KeyboardKeyPlaneView.LabelStyle.symbolOverrides[label] ?? .symbol
        return KeyboardKeyPlaneView.LabelStyle(
            size: key.size * 1.5, weight: key.weight, yOffset: 0.8, caseSensitive: key.caseSensitive, shows: key.shows
        )
    }

    /// The bubble's label: iOS 27 draws a letter 36.5 pt, whatever its case, a little below the
    /// bubble's centre (fitted to the 17e's q and the 17 Pro's G); digits and symbols the key's
    /// own font scaled the same way.
    private(set) var labelStyle = KeyCalloutView.letterStyle

    override func draw(_ rect: CGRect) {
        guard let balloon, let context = UIGraphicsGetCurrentContext() else { return }
        let path = Self.path(for: balloon)
        context.saveGState()
        context.setShadow(offset: CGSize(width: 0, height: 1), blur: 6, color: palette.bubbleShadow.cgColor)
        context.addPath(path)
        context.setFillColor(palette.bubbleFill.cgColor)
        context.fillPath()
        context.restoreGState()
        if palette.isDark {
            // Apple's dark bubble lightens toward its bottom (63 to 85 grey).
            context.saveGState()
            context.addPath(path)
            context.clip()
            let colors = [palette.bubbleFill.cgColor, UIColor(white: 0.26, alpha: 1).cgColor, UIColor(white: 0.333, alpha: 1).cgColor] as CFArray
            if let gradient = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: colors, locations: [0, 0.5, 1]) {
                context.drawLinearGradient(
                    gradient,
                    start: CGPoint(x: 0, y: balloon.bubble.minY),
                    end: CGPoint(x: 0, y: balloon.bubble.maxY),
                    options: []
                )
            }
            context.restoreGState()
        }

        let attributes: [NSAttributedString.Key: Any] = [
            .font: labelStyle.font,
            .foregroundColor: palette.textBase,
        ]
        let shown = labelStyle.shows ?? label
        let size = (shown as NSString).size(withAttributes: attributes)
        (shown as NSString).draw(
            at: CGPoint(x: balloon.bubble.midX - size.width / 2, y: balloon.bubble.midY - size.height / 2 + labelStyle.yOffset),
            withAttributes: attributes
        )
    }

    /// The bubble: a rounded rectangle floating over the key (iOS 27), with no stem.
    static func path(for balloon: KeyBalloon) -> CGPath {
        UIBezierPath(roundedRect: balloon.bubble, cornerRadius: 12).cgPath
    }
}
