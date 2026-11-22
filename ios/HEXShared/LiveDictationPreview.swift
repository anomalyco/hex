import Combine
import UIKit

/// The words so far in place of the full keyboard's keys while it records (Settings ›
/// Dictation › Show what I say): the keys go and the words have their space, the settled
/// words in the keys' ink and the tentative tail muted, the newest line always in view with
/// whole lines above it. It takes every touch where the keys were, so nothing types while it
/// shows; the recording bar above and the globe stay as they are.
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
    /// The words' box, and the words in it: one label, as tall as the text it shows, moved up
    /// by whole lines once that's taller than the box.
    private let box = UIView()
    private let label = UILabel()
    private let globe = GlobeKeycapView()

    static let placeholder = String(localized: "Listening…")

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        isOpaque = false
        box.clipsToBounds = true
        box.isUserInteractionEnabled = false
        label.numberOfLines = 0
        label.isAccessibilityElement = false
        box.addSubview(label)
        addSubview(box)
        globe.isHidden = true
        globe.isUserInteractionEnabled = false
        addSubview(globe)
        // One element for VoiceOver, read when it's focused, never announced as words change.
        isAccessibilityElement = true
        accessibilityTraits = .staticText
        render()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func show(_ next: LiveTranscript) {
        guard next != transcript else { return }
        transcript = next
        render()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let frame = bounds.inset(by: textInsets)
        if box.frame != frame {
            box.frame = frame
            placeWords()
        }
    }

    /// A touch anywhere on it stays with it.
    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        bounds.contains(point)
    }

    private var font: UIFont {
        .systemFont(ofSize: orientation == .landscape ? 19 : 23, weight: .regular)
    }

    private func render() {
        globe.palette = palette
        globe.isLandscape = orientation == .landscape
        let paragraph = NSMutableParagraphStyle()
        paragraph.minimumLineHeight = lineHeight
        paragraph.maximumLineHeight = lineHeight
        paragraph.lineBreakMode = .byWordWrapping
        let base: [NSAttributedString.Key: Any] = [.font: font, .paragraphStyle: paragraph]
        let text = NSMutableAttributedString()
        if transcript.isEmpty {
            text.append(NSAttributedString(string: Self.placeholder, attributes: base.merging([.foregroundColor: palette.textMuted]) { $1 }))
            accessibilityLabel = Self.placeholder
        } else {
            let shown = Self.tail(of: transcript, characters: shownCharacters)
            text.append(NSAttributedString(string: shown.settled, attributes: base.merging([.foregroundColor: palette.textBase]) { $1 }))
            text.append(NSAttributedString(string: shown.tentative, attributes: base.merging([.foregroundColor: palette.textMuted]) { $1 }))
            accessibilityLabel = transcript.text
        }
        label.attributedText = text
        placeWords()
    }

    /// About as many characters as fill the box and three lines more: a minute's text laid out
    /// whole in a scrolling text view held 27 MB more in the keyboard (iPhone 17 Pro).
    private var shownCharacters: Int {
        let lines = max(1, box.bounds.height / lineHeight) + 3
        let perLine = max(10, box.bounds.width / (font.pointSize * 0.42))
        return max(200, Int(lines * perLine))
    }

    /// The end of `transcript` in at most `characters`, from a word's start: its settled part,
    /// then its tentative tail as it follows them.
    static func tail(of transcript: LiveTranscript, characters: Int) -> (settled: String, tentative: String) {
        let settled = transcript.settled
        let tentative = transcript.tentativeJoined
        let total = settled.count + tentative.count
        guard total > characters else { return (settled, tentative) }
        let full = settled + tentative
        var start = full.index(full.startIndex, offsetBy: total - characters)
        // From the next word's start, so no word shows cut.
        if let space = full[start...].firstIndex(where: \.isWhitespace) {
            start = full[space...].firstIndex(where: { !$0.isWhitespace }) ?? start
        }
        let dropped = full.distance(from: full.startIndex, to: start)
        if dropped >= settled.count {
            return ("", String(full[start...]))
        }
        return (String(settled.dropFirst(dropped)), tentative)
    }

    private var lineHeight: CGFloat { (font.lineHeight * 1.08).rounded(.up) }

    /// The newest line at the box's foot once the words overflow it, with whole lines above.
    private func placeWords() {
        let width = box.bounds.width
        guard width > 0 else { return }
        let height = label.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude)).height.rounded(.up)
        let overflow = height - box.bounds.height
        let y = overflow > 0 ? -(overflow / lineHeight).rounded(.up) * lineHeight : 0
        label.frame = CGRect(x: 0, y: y, width: width, height: height)
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
    static let fade: TimeInterval = 0.18

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
        preview.alpha = 0
        keyboard.insertSubview(preview, aboveSubview: keyboard.plane)
        self.preview = preview
        layout()
        let duration = keyboard.window == nil ? 0 : Self.fade
        UIView.animate(withDuration: duration, delay: 0, options: [.beginFromCurrentState, .allowUserInteraction]) {
            preview.alpha = 1
            keyboard.plane.alpha = 0
        }
        return preview
    }

    private func hide() {
        guard let preview, let keyboard else { return }
        self.preview = nil
        let duration = keyboard.window == nil ? 0 : Self.fade
        UIView.animate(withDuration: duration, delay: 0, options: [.beginFromCurrentState, .allowUserInteraction]) {
            preview.alpha = 0
            keyboard.plane.alpha = 1
        } completion: { _ in
            preview.removeFromSuperview()
        }
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
