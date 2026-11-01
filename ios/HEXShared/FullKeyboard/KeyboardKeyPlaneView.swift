import UIKit

/// Draws the keys and turns touches into `FullKeyboardEngine` calls. The Hex and globe
/// keys are real controls laid over their slots by `FullKeyboardView`; this view only
/// draws the globe's keycap and leaves the Hex slot empty.
@MainActor
final class KeyboardKeyPlaneView: UIView {
    let engine: FullKeyboardEngine
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { setNeedsDisplay() } } }
    var returnLabel = "return" { didSet { if returnLabel != oldValue { setNeedsDisplay() } } }
    var returnIsAction = false { didSet { if returnIsAction != oldValue { setNeedsDisplay() } } }
    /// Called on every key going down, for clicks and haptics.
    var onKeyDown: ((KeyKind) -> Void)?

    private var touchIDs: [ObjectIdentifier: Int] = [:]
    private var nextTouchID = 0
    private var deleteTimer: Timer?
    private var deleteStartedAt: TimeInterval = 0

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
        let pressed = engine.pressedKinds
        let preview = layout.showsBalloons ? engine.previewKey?.kind : nil

        for key in layout.keys {
            guard key.frame.intersects(rect) else { continue }
            switch key.kind {
            case .hex:
                continue
            case .character:
                // The pressed key is drawn by its balloon in portrait.
                if key.kind == preview { continue }
                let fill = pressed.contains(key.kind) ? palette.functionFill : palette.keyFill
                context.fillKeycap(key.frame, fill: fill, shadow: palette.keyShadow)
                drawText(
                    engine.displayLabel(for: key),
                    in: key.frame,
                    font: .systemFont(ofSize: key.frame.width > layout.letterWidth * 1.2 ? 21 : 23, weight: .regular),
                    color: palette.textBase,
                    yOffset: -1
                )
            case .space:
                let fill = pressed.contains(.space) ? palette.functionFill : palette.keyFill
                context.fillKeycap(key.frame, fill: fill, shadow: palette.keyShadow)
                drawText("space", in: key.frame, font: .systemFont(ofSize: 16), color: palette.textMuted)
            case .returnKey:
                let isPressed = pressed.contains(.returnKey)
                let fill: UIColor = returnIsAction
                    ? (isPressed ? palette.keyFill : palette.actionFill)
                    : (isPressed ? palette.keyFill : palette.functionFill)
                let ink: UIColor = returnIsAction && !isPressed ? .white : palette.textBase
                context.fillKeycap(key.frame, fill: fill, shadow: palette.keyShadow)
                drawText(returnLabel, in: key.frame, font: .systemFont(ofSize: 16), color: ink)
            case .shift:
                let state = engine.shift.state
                let lit = state != .off || engine.shift.isHeld
                context.fillKeycap(key.frame, fill: lit ? palette.keyFill : palette.functionFill, shadow: palette.keyShadow)
                let symbol = switch state {
                case .off: engine.shift.isHeld ? "shift.fill" : "shift"
                case .on: "shift.fill"
                case .locked: "capslock.fill"
                }
                drawSymbol(symbol, in: key.frame, color: palette.textBase)
            case .delete:
                let lit = pressed.contains(.delete)
                context.fillKeycap(key.frame, fill: lit ? palette.keyFill : palette.functionFill, shadow: palette.keyShadow)
                drawSymbol(lit ? "delete.left.fill" : "delete.left", in: key.frame, color: palette.textBase)
            case .layerSwitch:
                let lit = pressed.contains(key.kind)
                context.fillKeycap(key.frame, fill: lit ? palette.keyFill : palette.functionFill, shadow: palette.keyShadow)
                drawText(key.label, in: key.frame, font: .systemFont(ofSize: 16), color: palette.textBase)
            case .globe:
                context.fillKeycap(key.frame, fill: palette.functionFill, shadow: palette.keyShadow)
                drawSymbol("globe", in: key.frame, color: palette.textBase)
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

    private func drawSymbol(_ name: String, in frame: CGRect, color: UIColor) {
        let config = UIImage.SymbolConfiguration(pointSize: 18, weight: .regular)
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
            }
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let id = id(for: touch, creating: false) else { continue }
            engine.touchMoved(id: id, to: touch.location(in: self))
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches.sorted(by: { $0.timestamp < $1.timestamp }) {
            guard let id = touchIDs.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            if engine.touches[id]?.key.kind == .delete { stopDeleteRepeat() }
            engine.touchEnded(id: id, at: touch.location(in: self))
        }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let id = touchIDs.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            if engine.touches[id]?.key.kind == .delete { stopDeleteRepeat() }
            engine.touchCancelled(id: id)
        }
    }

    func cancelAllTouches() {
        touchIDs.removeAll()
        stopDeleteRepeat()
        engine.cancelAllTouches()
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
        context.setShadow(offset: CGSize(width: 0, height: 1), blur: 3, color: palette.popupShadow.cgColor)
        context.addPath(path)
        context.setFillColor(palette.popupFill.cgColor)
        context.fillPath()
        context.restoreGState()

        let attributes: [NSAttributedString.Key: Any] = [
            .font: UIFont.systemFont(ofSize: 36, weight: .light),
            .foregroundColor: palette.textBase,
        ]
        let size = (label as NSString).size(withAttributes: attributes)
        (label as NSString).draw(
            at: CGPoint(x: balloon.bubble.midX - size.width / 2, y: balloon.bubble.midY - size.height / 2),
            withAttributes: attributes
        )
    }

    /// The mockup's balloon: the key at the bottom, a wider bubble above, joined by a neck.
    static func path(for balloon: KeyBalloon) -> CGPath {
        let kr = FullKeyboardPalette.cornerRadius
        let br: CGFloat = 11
        let neck = KeyBalloon.neck
        let kl = balloon.keyFrame.minX
        let krt = balloon.keyFrame.maxX
        let kb = balloon.keyFrame.maxY
        let kt = balloon.keyFrame.minY
        let bb = balloon.bubble.maxY
        let bt = balloon.bubble.minY
        let bl = balloon.bubble.minX
        let brt = balloon.bubble.maxX

        let p = CGMutablePath()
        p.move(to: CGPoint(x: bl, y: bt + br))
        p.addQuadCurve(to: CGPoint(x: bl + br, y: bt), control: CGPoint(x: bl, y: bt))
        p.addLine(to: CGPoint(x: brt - br, y: bt))
        p.addQuadCurve(to: CGPoint(x: brt, y: bt + br), control: CGPoint(x: brt, y: bt))
        // Each side: a rounded shoulder into the neck when the bubble overhangs the key,
        // or a straight taper when an edge key's bubble is pushed flush with it.
        if brt - br >= krt + 2 {
            p.addLine(to: CGPoint(x: brt, y: bb - br))
            p.addQuadCurve(to: CGPoint(x: brt - br, y: bb), control: CGPoint(x: brt, y: bb))
            p.addLine(to: CGPoint(x: min(brt - br, krt + 6), y: bb))
            p.addCurve(
                to: CGPoint(x: krt, y: kt + 6),
                control1: CGPoint(x: krt + 1, y: bb),
                control2: CGPoint(x: krt, y: bb + neck * 0.4)
            )
        } else {
            p.addLine(to: CGPoint(x: brt, y: bb))
            p.addLine(to: CGPoint(x: krt, y: kt + 4))
        }
        p.addLine(to: CGPoint(x: krt, y: kb - kr))
        p.addQuadCurve(to: CGPoint(x: krt - kr, y: kb), control: CGPoint(x: krt, y: kb))
        p.addLine(to: CGPoint(x: kl + kr, y: kb))
        p.addQuadCurve(to: CGPoint(x: kl, y: kb - kr), control: CGPoint(x: kl, y: kb))
        if bl + br <= kl - 2 {
            p.addLine(to: CGPoint(x: kl, y: kt + 6))
            p.addCurve(
                to: CGPoint(x: max(bl + br, kl - 6), y: bb),
                control1: CGPoint(x: kl, y: bb + neck * 0.4),
                control2: CGPoint(x: kl - 1, y: bb)
            )
            p.addLine(to: CGPoint(x: bl + br, y: bb))
            p.addQuadCurve(to: CGPoint(x: bl, y: bb - br), control: CGPoint(x: bl, y: bb))
        } else {
            p.addLine(to: CGPoint(x: kl, y: kt + 4))
            p.addLine(to: CGPoint(x: bl, y: bb))
        }
        p.closeSubpath()
        return p
    }
}
