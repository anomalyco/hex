import UIKit

/// Draws the keys and turns touches into `FullKeyboardEngine` calls. The Hex and globe
/// keys are real controls laid over their slots by `FullKeyboardView`; this view only
/// draws the globe's keycap and leaves the Hex slot empty.
@MainActor
final class KeyboardKeyPlaneView: UIView {
    let engine: FullKeyboardEngine
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { setNeedsDisplay() } } }
    var returnLabel = "return" { didSet { if returnLabel != oldValue { setNeedsDisplay() } } }
    var returnSymbol: String? = "return" { didSet { if returnSymbol != oldValue { setNeedsDisplay() } } }
    var returnIsAction = false { didSet { if returnIsAction != oldValue { setNeedsDisplay() } } }
    var returnIsEnabled = true { didSet { if returnIsEnabled != oldValue { setNeedsDisplay() } } }
    /// Called on every key going down, for clicks and haptics.
    var onKeyDown: ((KeyKind) -> Void)?
    /// Called when long-press accents open.
    var onAccentMenuOpened: (() -> Void)?

    private var touchIDs: [ObjectIdentifier: Int] = [:]
    private var nextTouchID = 0
    private var deleteTimer: Timer?
    private var deleteStartedAt: TimeInterval = 0
    private var accentHoldWork: [Int: DispatchWorkItem] = [:]

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

    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        engine.layout.keysAreaFrame.contains(point)
    }

    // MARK: - Drawing

    override func draw(_ rect: CGRect) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        let layout = engine.layout
        let isLandscape = layout.orientation == .landscape
        let radius = layout.metrics.cornerRadius
        let pressed = engine.pressedKinds
        let preview = layout.showsBalloons ? engine.previewKey?.kind : nil
        // Like Apple's spacebar trackpad, key glyphs blank out while sliding the cursor.
        let hideGlyphs = engine.isTrackpadActive

        for key in layout.keys {
            guard key.frame.intersects(rect) else { continue }
            switch key.kind {
            case .hex:
                continue
            case .character:
                // The pressed key is drawn by its balloon in portrait.
                if key.kind == preview { continue }
                let fill = pressed.contains(key.kind) ? palette.keyPressedFill : palette.keyFill
                context.fillKeycap(key.frame, fill: fill, radius: radius)
                guard !hideGlyphs else { continue }
                let label = engine.displayLabel(for: key)
                let isWidePunctuation = key.frame.width > layout.letterWidth * 1.2
                let isUpperLetter = layout.layer == .letters && engine.isUppercase && label.first?.isUppercase == true
                let fontSize: CGFloat = if isLandscape {
                    isUpperLetter ? 18 : 19
                } else if isWidePunctuation || isUpperLetter {
                    21
                } else {
                    22.5
                }
                let yOffset: CGFloat = isUpperLetter ? 0 : (isLandscape ? -0.5 : -1)
                drawText(
                    label,
                    in: key.frame,
                    font: .systemFont(ofSize: fontSize, weight: .regular),
                    color: palette.textBase,
                    yOffset: yOffset
                )
            case .space:
                let fill = pressed.contains(.space) ? palette.keyPressedFill : palette.keyFill
                context.fillKeycap(key.frame, fill: fill, radius: radius)
            case .returnKey:
                let isPressed = pressed.contains(.returnKey) && returnIsEnabled
                let activeAction = returnIsAction && returnIsEnabled && !hideGlyphs
                let fill: UIColor = activeAction
                    ? (isPressed ? palette.actionPressedFill : palette.actionFill)
                    : (isPressed ? palette.keyPressedFill : palette.keyFill)
                let ink: UIColor = if !returnIsEnabled {
                    palette.textMuted
                } else if activeAction {
                    .white
                } else {
                    palette.textBase
                }
                context.fillKeycap(key.frame, fill: fill, radius: radius)
                guard !hideGlyphs else { continue }
                if let symbol = returnSymbol {
                    drawSymbol(
                        symbol,
                        in: key.frame,
                        color: ink,
                        pointSize: isLandscape ? 15.5 : 17.5,
                        weight: activeAction ? .medium : .regular
                    )
                } else {
                    drawText(returnLabel, in: key.frame, font: .systemFont(ofSize: isLandscape ? 14.5 : 15.5), color: ink)
                }
            case .shift:
                let state = engine.shift.state
                let isPressed = pressed.contains(.shift)
                context.fillKeycap(key.frame, fill: isPressed ? palette.keyPressedFill : palette.keyFill, radius: radius)
                guard !hideGlyphs else { continue }
                let symbol = switch state {
                case .off: engine.shift.isHeld ? "shift.fill" : "shift"
                case .on: "shift.fill"
                case .locked: "capslock.fill"
                }
                drawSymbol(symbol, in: key.frame, color: palette.textBase, pointSize: isLandscape ? 16 : 18)
            case .delete:
                let lit = pressed.contains(.delete)
                context.fillKeycap(key.frame, fill: lit ? palette.keyPressedFill : palette.keyFill, radius: radius)
                guard !hideGlyphs else { continue }
                drawSymbol(lit ? "delete.left.fill" : "delete.left", in: key.frame, color: palette.textBase, pointSize: isLandscape ? 16 : 18)
            case .layerSwitch:
                let lit = pressed.contains(key.kind)
                context.fillKeycap(key.frame, fill: lit ? palette.keyPressedFill : palette.keyFill, radius: radius)
                guard !hideGlyphs else { continue }
                drawText(key.label, in: key.frame, font: .systemFont(ofSize: isLandscape ? 14.5 : 15.5), color: palette.textBase)
            case .globe:
                let lit = pressed.contains(.globe)
                context.fillKeycap(key.frame, fill: lit ? palette.keyPressedFill : palette.keyFill, radius: radius)
                guard !hideGlyphs else { continue }
                drawSymbol("globe", in: key.frame, color: palette.textBase, pointSize: isLandscape ? 16 : 18)
            }
        }
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

    private func drawSymbol(
        _ name: String,
        in frame: CGRect,
        color: UIColor,
        pointSize: CGFloat = 18,
        weight: UIImage.SymbolWeight = .regular
    ) {
        let config = UIImage.SymbolConfiguration(pointSize: pointSize, weight: weight)
        guard let image = UIImage(systemName: name, withConfiguration: config)?
            .withTintColor(color, renderingMode: .alwaysOriginal) else { return }
        let size = image.size
        image.draw(in: CGRect(
            x: frame.midX - size.width / 2,
            y: frame.midY - size.height / 2,
            width: size.width,
            height: size.height
        ))
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
            let point = touch.location(in: self)
            engine.touchBegan(id: id, at: point)
            if let kind = engine.touches[id]?.key.kind {
                onKeyDown?(kind)
                if kind == .delete { startDeleteRepeat() }
                if case .character = kind { armAccentTimer(for: id) }
            }
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let id = id(for: touch, creating: false) else { continue }
            let previousKey = engine.touches[id]?.key.kind
            engine.touchMoved(id: id, to: touch.location(in: self))
            let currentKey = engine.touches[id]?.key.kind
            if currentKey != previousKey {
                cancelAccentTimer(for: id)
                if case .character = currentKey {
                    armAccentTimer(for: id)
                }
            }
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches.sorted(by: { $0.timestamp < $1.timestamp }) {
            guard let id = touchIDs.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            cancelAccentTimer(for: id)
            if engine.touches[id]?.key.kind == .delete { stopDeleteRepeat() }
            engine.touchEnded(id: id, at: touch.location(in: self))
        }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let id = touchIDs.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            cancelAccentTimer(for: id)
            if engine.touches[id]?.key.kind == .delete { stopDeleteRepeat() }
            engine.touchCancelled(id: id)
        }
    }

    func cancelAllTouches() {
        touchIDs.removeAll()
        for work in accentHoldWork.values { work.cancel() }
        accentHoldWork.removeAll()
        stopDeleteRepeat()
        engine.cancelAllTouches()
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

    func show(_ balloon: KeyBalloon?, label: String) {
        guard balloon != self.balloon || label != self.label else { return }
        self.balloon = balloon
        self.label = label
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        guard let balloon, let context = UIGraphicsGetCurrentContext() else { return }
        let path = Self.path(for: balloon)
        context.saveGState()
        context.setShadow(offset: CGSize(width: 0, height: 2), blur: 6, color: palette.popupShadow.cgColor)
        context.addPath(path)
        context.setFillColor(palette.popupFill.cgColor)
        context.fillPath()
        context.restoreGState()

        let scale = max(1, traitCollection.displayScale)
        context.saveGState()
        context.addPath(path)
        context.setLineWidth(1 / scale)
        context.setStrokeColor(palette.popupBorder.cgColor)
        context.strokePath()
        context.restoreGState()

        let attributes: [NSAttributedString.Key: Any] = [
            .font: UIFont.systemFont(ofSize: 33, weight: .light),
            .foregroundColor: palette.textBase,
        ]
        let size = (label as NSString).size(withAttributes: attributes)
        (label as NSString).draw(
            at: CGPoint(x: balloon.bubble.midX - size.width / 2, y: balloon.bubble.midY - size.height / 2),
            withAttributes: attributes
        )
    }

    /// Key at the bottom, wider bubble above, joined by a smooth downward neck.
    static func path(for balloon: KeyBalloon) -> CGPath {
        let kr = FullKeyboardPalette.cornerRadius
        let br: CGFloat = 11
        let kl = balloon.keyFrame.minX
        let krt = balloon.keyFrame.maxX
        let kb = balloon.keyFrame.maxY
        let kt = balloon.keyFrame.minY
        let bb = balloon.bubble.maxY
        let bt = balloon.bubble.minY
        let bl = balloon.bubble.minX
        let brt = balloon.bubble.maxX
        let stemTop = min(kb - kr - 2, max(kt + 6, bb + 7))

        let p = CGMutablePath()
        p.move(to: CGPoint(x: bl, y: bt + br))
        p.addQuadCurve(to: CGPoint(x: bl + br, y: bt), control: CGPoint(x: bl, y: bt))
        p.addLine(to: CGPoint(x: brt - br, y: bt))
        p.addQuadCurve(to: CGPoint(x: brt, y: bt + br), control: CGPoint(x: brt, y: bt))
        if brt - br >= krt + 2 {
            p.addLine(to: CGPoint(x: brt, y: bb - br))
            p.addQuadCurve(to: CGPoint(x: brt - br, y: bb), control: CGPoint(x: brt, y: bb))
            p.addLine(to: CGPoint(x: min(brt - br, krt + 6), y: bb))
            p.addCurve(
                to: CGPoint(x: krt, y: stemTop),
                control1: CGPoint(x: krt + 1, y: bb),
                control2: CGPoint(x: krt, y: (bb + stemTop) * 0.5)
            )
        } else {
            p.addLine(to: CGPoint(x: brt, y: bb))
            p.addLine(to: CGPoint(x: krt, y: max(kt + 4, bb + 4)))
        }
        p.addLine(to: CGPoint(x: krt, y: kb - kr))
        p.addQuadCurve(to: CGPoint(x: krt - kr, y: kb), control: CGPoint(x: krt, y: kb))
        p.addLine(to: CGPoint(x: kl + kr, y: kb))
        p.addQuadCurve(to: CGPoint(x: kl, y: kb - kr), control: CGPoint(x: kl, y: kb))
        if bl + br <= kl - 2 {
            p.addLine(to: CGPoint(x: kl, y: stemTop))
            p.addCurve(
                to: CGPoint(x: max(bl + br, kl - 6), y: bb),
                control1: CGPoint(x: kl, y: (bb + stemTop) * 0.5),
                control2: CGPoint(x: kl - 1, y: bb)
            )
            p.addLine(to: CGPoint(x: bl + br, y: bb))
            p.addQuadCurve(to: CGPoint(x: bl, y: bb - br), control: CGPoint(x: bl, y: bb))
        } else {
            p.addLine(to: CGPoint(x: kl, y: max(kt + 4, bb + 4)))
            p.addLine(to: CGPoint(x: bl, y: bb))
        }
        p.closeSubpath()
        return p
    }
}
