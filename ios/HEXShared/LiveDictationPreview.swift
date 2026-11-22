import Combine
import UIKit

/// The words so far over the full keyboard's keys while it records (Settings › Dictation ›
/// Show words as you speak): the keys go faint (`dim`) or away (`hide`) under them, the
/// settled words in the keys' ink and the tentative tail muted, the newest line always in view.
/// It takes every touch on the keys, so nothing types while it shows; the recording bar above
/// and the globe stay as they are.
@MainActor
final class LiveDictationPreviewView: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { render() } } }
    var look: LiveDictationSettings.Look = .dim { didSet { if look != oldValue { render() } } }
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
    private let textView = UITextView(usingTextLayoutManager: true)
    private let globe = GlobeKeycapView()
    /// When the person last scrolled back: the newest words wait a moment before taking the view again.
    private var scrolledBackAt: Date?
    private let scrollDelegate = ScrollWatcher()

    static let placeholder = String(localized: "Listening…")

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        isOpaque = false
        textView.backgroundColor = .clear
        textView.isEditable = false
        textView.isSelectable = false
        textView.isScrollEnabled = true
        textView.showsVerticalScrollIndicator = false
        textView.alwaysBounceVertical = false
        textView.textContainer.lineFragmentPadding = 0
        textView.contentInsetAdjustmentBehavior = .never
        textView.isAccessibilityElement = false
        textView.accessibilityElementsHidden = true
        scrollDelegate.onDrag = { [weak self] in self?.scrolledBackAt = Date() }
        textView.delegate = scrollDelegate
        addSubview(textView)
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
        // The words' own box, so scrolling shows whole lines at its top.
        let frame = bounds.inset(by: textInsets)
        if textView.frame != frame {
            textView.frame = frame
            textView.textContainerInset = .zero
            scrollToNewest(animated: false)
        }
    }

    /// A touch anywhere on it stays with it, scrolling the words at most.
    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        bounds.contains(point)
    }

    private var font: UIFont {
        let size: CGFloat = orientation == .landscape ? 19 : (look == .hide ? 23 : 21)
        return .systemFont(ofSize: size, weight: look == .dim ? .medium : .regular)
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
            text.append(NSAttributedString(string: transcript.settled, attributes: base.merging([.foregroundColor: palette.textBase]) { $1 }))
            text.append(NSAttributedString(string: transcript.tentativeJoined, attributes: base.merging([.foregroundColor: palette.textMuted]) { $1 }))
            accessibilityLabel = transcript.text
        }
        textView.attributedText = text
        scrollToNewest(animated: true)
    }

    private var lineHeight: CGFloat { (font.lineHeight * 1.08).rounded(.up) }

    /// Keeps the last line in view with whole lines at the top, unless the person scrolled
    /// back in the last 3 s.
    private func scrollToNewest(animated: Bool) {
        if let scrolledBackAt, Date().timeIntervalSince(scrolledBackAt) < 3 { return }
        guard textView.bounds.height > 0 else { return }
        textView.layoutIfNeeded()
        let line = lineHeight
        // Room to scroll a part line further, so the top line is never cut.
        if textView.contentInset.bottom != line { textView.contentInset.bottom = line }
        let overflow = textView.contentSize.height - textView.bounds.height
        let target = overflow > 0 ? (overflow / line).rounded(.up) * line : 0
        guard abs(textView.contentOffset.y - target) > 0.5 else { return }
        textView.setContentOffset(CGPoint(x: 0, y: target), animated: animated && window != nil)
    }
}

/// The globe key as the key plane draws it, for the preview to keep over the words.
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

/// Notes when the person drags the words, so the newest line doesn't snatch the view back.
@MainActor
private final class ScrollWatcher: NSObject, UITextViewDelegate {
    var onDrag: (() -> Void)?

    func scrollViewWillBeginDragging(_ scrollView: UIScrollView) { onDrag?() }
    func scrollViewDidEndDragging(_ scrollView: UIScrollView, willDecelerate decelerate: Bool) { onDrag?() }
}

/// Puts `LiveDictationPreviewView` over a full keyboard's keys while it records and there are
/// live words (`KeyboardViewModel.liveText`), and takes it away, with the keys back, once
/// the dictation is in.
@MainActor
final class LiveDictationPreviewHost {
    private weak var keyboard: FullKeyboardView?
    private var preview: LiveDictationPreviewView?
    private var subscription: AnyCancellable?
    /// The keys' opacity under the words in the dim look.
    static let dimmedKeys: CGFloat = 0.1
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
        let preview = self.preview ?? show(in: keyboard, look: live.look)
        preview.show(transcript)
    }

    private func show(in keyboard: FullKeyboardView, look: LiveDictationSettings.Look) -> LiveDictationPreviewView {
        let preview = LiveDictationPreviewView()
        preview.palette = keyboard.palette
        preview.look = look
        preview.alpha = 0
        keyboard.insertSubview(preview, aboveSubview: keyboard.plane)
        self.preview = preview
        layout()
        let keys = look == .dim ? Self.dimmedKeys : 0
        let duration = keyboard.window == nil ? 0 : Self.fade
        UIView.animate(withDuration: duration, delay: 0, options: [.beginFromCurrentState, .allowUserInteraction]) {
            preview.alpha = 1
            keyboard.plane.alpha = keys
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

    /// The keys' area, under the recording bar to the keyboard's bottom.
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
