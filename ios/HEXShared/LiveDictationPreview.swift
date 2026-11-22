import Combine
import os
import UIKit

/// The words so far in place of the full keyboard's keys while it records (Settings ›
/// Dictation › Show what I say): the keys go and the words have their space, the settled
/// words in the keys' ink and the tentative tail muted, the newest line always in view with
/// whole lines above it. It takes every touch where the keys were, so nothing types while it
/// shows, and a tap on it finishes the dictation as Done does; the recording bar above and the
/// globe stay as they are.
@MainActor
final class LiveDictationPreviewView: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { render() } } }
    var orientation: KeyboardOrientation = .portrait { didSet { if orientation != oldValue { render() } } }
    /// Where the words sit inside this view: the keys' own edges.
    var textInsets: UIEdgeInsets = .zero { didSet { if textInsets != oldValue { setNeedsLayout() } } }
    /// Where the keyboard has its own globe key (an iPhone with a Home button), its keycap in
    /// this view: drawn whole over the words, since it still switches keyboards.
    var globeFrame: CGRect? {
        didSet {
            guard globeFrame != oldValue else { return }
            globe.isHidden = globeFrame == nil
            globe.frame = globeFrame ?? .zero
            globe.setNeedsDisplay()
        }
    }
    var globeRadius: CGFloat = FullKeyboardPalette.cornerRadius { didSet { globe.cornerRadius = globeRadius } }
    private(set) var transcript = LiveTranscript()
    /// The words, drawn by one view (`LiveWordsView`), which also moves them.
    let words = LiveWordsView()
    private let globe = GlobeKeycapView()
    /// Finishes the dictation as Done does, once: a tap on the words, or VoiceOver's activate
    /// or Finish dictation action on them.
    var onFinish: (() -> Void)?
    /// When the words appeared, on the touches' clock.
    var shownAt = ProcessInfo.processInfo.systemUptime
    private var down: (point: CGPoint, time: TimeInterval)?
    private var finished = false

    static let placeholder = String(localized: "Listening…")
    /// A touch that lands this soon after the words appear is the end of the tap that started
    /// the dictation or a thumb settling where the keys were, not a tap on the words: the swap's
    /// longer half (the words coming in) and a tenth of a second more.
    static let settle: TimeInterval = LiveDictationPreviewHost.Motion.wordsIn + 0.1

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        isOpaque = false
        words.placeholder = Self.placeholder
        addSubview(words)
        globe.isHidden = true
        globe.isUserInteractionEnabled = false
        addSubview(globe)
        // One element for VoiceOver, read when it's focused, never announced as words change.
        isAccessibilityElement = true
        accessibilityTraits = .staticText
        accessibilityCustomActions = [
            UIAccessibilityCustomAction(name: String(localized: "Finish dictation")) { [weak self] _ in
                MainActor.assumeIsolated { self?.finish() ?? false }
            },
        ]
        render()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func show(_ next: LiveTranscript) {
        guard next != transcript else { return }
        transcript = next
        accessibilityLabel = next.isEmpty ? Self.placeholder : next.text
        words.show(text: next.text, settled: next.settled.count)
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        // The words' box, with the band above it where a line leaving the top fades out.
        let box = bounds.inset(by: textInsets)
        words.frame = CGRect(x: box.minX, y: box.minY - LiveWordsView.edge, width: box.width, height: box.height + LiveWordsView.edge)
    }

    /// A touch anywhere on it stays with it.
    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        bounds.contains(point)
    }

    /// Only a tap finishes, by the keys' own measures: a touch that moves further than a key's
    /// slop or is held as long as a key takes to open its accents does nothing, so a thumb
    /// resting or sliding where the keys were never ends a dictation.
    func touchDown(at point: CGPoint, time: TimeInterval) {
        down = time - shownAt >= Self.settle ? (point, time) : nil
    }

    func touchMoved(to point: CGPoint) {
        guard let down, hypot(point.x - down.point.x, point.y - down.point.y) > FullKeyboardEngine.accentSlop else { return }
        self.down = nil
    }

    func touchUp(at point: CGPoint, time: TimeInterval) {
        defer { down = nil }
        guard let down, time - down.time < FullKeyboardEngine.accentHoldDelay,
              hypot(point.x - down.point.x, point.y - down.point.y) <= FullKeyboardEngine.accentSlop else { return }
        finish()
    }

    @discardableResult
    private func finish() -> Bool {
        guard !finished, let onFinish else { return false }
        finished = true
        onFinish()
        return true
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = touches.first else { return }
        touchDown(at: touch.location(in: self), time: touch.timestamp)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = touches.first else { return }
        touchMoved(to: touch.location(in: self))
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = touches.first else { return }
        touchUp(at: touch.location(in: self), time: touch.timestamp)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        down = nil
    }

    override func accessibilityActivate() -> Bool {
        finish()
    }

    private func render() {
        globe.palette = palette
        globe.isLandscape = orientation == .landscape
        words.palette = palette
        words.font = .systemFont(ofSize: orientation == .landscape ? 19 : 23, weight: .regular)
        accessibilityLabel = transcript.isEmpty ? Self.placeholder : transcript.text
    }
}

/// The globe key as the key plane draws it, for the preview to keep where it was.
@MainActor
private final class GlobeKeycapView: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { setNeedsDisplay() } } }
    var isLandscape = false { didSet { if isLandscape != oldValue { setNeedsDisplay() } } }
    var cornerRadius = FullKeyboardPalette.cornerRadius { didSet { if cornerRadius != oldValue { setNeedsDisplay() } } }

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        isOpaque = false
        contentMode = .redraw
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override func draw(_ rect: CGRect) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        context.fillKeycap(bounds, fill: palette.keyFill, shadow: palette.keyShadow, radius: cornerRadius)
        let config = UIImage.SymbolConfiguration(pointSize: isLandscape ? 16 : 19, weight: .regular)
        guard let image = UIImage(systemName: "globe", withConfiguration: config)?
            .withTintColor(palette.textBase, renderingMode: .alwaysOriginal) else { return }
        image.draw(at: CGPoint(x: bounds.midX - image.size.width / 2, y: bounds.midY - image.size.height / 2 + (isLandscape ? 0 : -1)))
    }
}

/// Puts `LiveDictationPreviewView` in place of a full keyboard's keys while it records with
/// live words (`KeyboardViewModel.liveText`), and takes it away, with the keys back, once
/// the dictation is in: the preview fades with the words it had while the keys and the
/// dictation's own text come in, so different words never flash in it.
@MainActor
final class LiveDictationPreviewHost {
    private weak var keyboard: FullKeyboardView?
    private var preview: LiveDictationPreviewView?
    private var subscription: AnyCancellable?

    /// The keys out and the words in as recording starts, and back as the dictation goes in, in
    /// one change each, never waiting for the other half: springs with no bounce. The words
    /// settle from a few points below as they come, and drift a few points up, toward the field
    /// their text goes into, as they go. With Reduce Motion, fades only.
    enum Motion {
        static let keysOut: TimeInterval = 0.2
        static let wordsIn: TimeInterval = 0.3
        static let wordsOut: TimeInterval = 0.15
        static let keysIn: TimeInterval = 0.3
        static let travel: CGFloat = 6
    }

    func attach(to keyboard: FullKeyboardView) {
        self.keyboard = keyboard
        subscription = keyboard.model.liveText.$transcript.sink { [weak self] transcript in
            self?.sync(transcript)
        }
    }

    var palette = FullKeyboardPalette(isDark: false) {
        didSet { preview?.palette = palette }
    }

    /// The recording state changed.
    func sync() {
        sync(keyboard?.model.liveText.transcript)
    }

    private func sync(_ transcript: LiveTranscript?) {
        guard let keyboard else { return }
        let live = keyboard.model.liveText
        guard keyboard.showsRecording, live.showsPreview, let transcript else {
            hide()
            return
        }
        let preview = self.preview ?? show(in: keyboard)
        preview.show(transcript)
    }

    private func show(in keyboard: FullKeyboardView) -> LiveDictationPreviewView {
        let preview = LiveDictationPreviewView()
        preview.palette = keyboard.palette
        preview.onFinish = { [weak keyboard] in
            keyboard?.typingLog?.ui("dictation_stop", [TypingLogField("from", .string("live_preview"))])
            keyboard?.model.onStopTap?()
        }
        keyboard.insertSubview(preview, aboveSubview: keyboard.plane)
        self.preview = preview
        layout()
        guard keyboard.window != nil else {
            keyboard.plane.alpha = 0
            return preview
        }
        let moves = !UIAccessibility.isReduceMotionEnabled
        preview.alpha = 0
        preview.transform = moves ? CGAffineTransform(translationX: 0, y: Motion.travel) : .identity
        Self.spring(moves ? Motion.wordsIn : 0.2) {
            preview.alpha = 1
            preview.transform = .identity
        }
        Self.spring(moves ? Motion.keysOut : 0.2) {
            keyboard.plane.alpha = 0
        }
        return preview
    }

    /// The keys take touches again at once: the words stop taking them as they start to go.
    private func hide() {
        guard let preview, let keyboard else { return }
        self.preview = nil
        preview.isUserInteractionEnabled = false
        let cost = preview.words.takeCost()
        if cost.updates > 0 {
            Self.log.notice("live: preview drew \(cost.updates, privacy: .public) updates, \(String(format: "%.2f", cost.averageMS), privacy: .public) ms on average, \(String(format: "%.2f", cost.slowestMS), privacy: .public) at most")
        }
        guard keyboard.window != nil else {
            keyboard.plane.alpha = 1
            preview.removeFromSuperview()
            return
        }
        let moves = !UIAccessibility.isReduceMotionEnabled
        Self.spring(moves ? Motion.wordsOut : 0.2) {
            preview.alpha = 0
            if moves { preview.transform = CGAffineTransform(translationX: 0, y: -Motion.travel) }
        } completion: {
            preview.removeFromSuperview()
        }
        Self.spring(moves ? Motion.keysIn : 0.2) {
            keyboard.plane.alpha = 1
        }
    }

    private static let log = Logger(subsystem: "anomaly.hex.mobile.keyboard", category: "capture-probe")

    private static func spring(_ duration: TimeInterval, _ changes: @escaping () -> Void, completion: (() -> Void)? = nil) {
        UIView.animate(
            springDuration: duration, bounce: 0, initialSpringVelocity: 0, delay: 0,
            options: [.beginFromCurrentState, .allowUserInteraction],
            animations: changes,
            completion: { _ in completion?() }
        )
    }

    /// Where the keys were, under the recording bar to the keyboard's bottom.
    func layout() {
        guard let preview, let keyboard else { return }
        let layout = keyboard.engine.layout
        preview.frame = layout.keysAreaFrame.offsetBy(dx: keyboard.plane.frame.minX, dy: keyboard.plane.frame.minY)
        let side = layout.horizontalInset + layout.metrics.side + 4
        var bottom: CGFloat = 10
        // The globe keeps working where the keyboard has one: its button is above the words.
        if let globe = layout.key(.globe), !keyboard.globeButton.isHidden {
            let frame = globe.frame.offsetBy(dx: keyboard.plane.frame.minX - preview.frame.minX, dy: keyboard.plane.frame.minY - preview.frame.minY)
            preview.globeFrame = frame
            bottom = max(bottom, preview.bounds.height - frame.minY + 4)
            preview.globeRadius = layout.metrics.cornerRadius
        } else {
            preview.globeFrame = nil
        }
        preview.textInsets = UIEdgeInsets(top: layout.metrics.rowGap + 4, left: side, bottom: bottom, right: side)
        preview.orientation = keyboard.orientation
    }
}
