import os
import QuartzCore
import UIKit

/// How the preview's words move: Core Animation does every frame, so the motion keeps the
/// display's pace however busy the keyboard's main thread is with the capture and the
/// recogniser (on an iPhone 17 Pro a display link drawing the words got only a quarter of its
/// frames on time while recording). One family: a spring with no bounce by its duration, and
/// for the cross-fade the curve closest to it.
nonisolated enum LiveWordsMotion {
    /// What changed in the words (new words, a settled tail turning to ink, a word the
    /// recogniser changed) cross-fades in place; what didn't change stays exactly as it is.
    static let change: TimeInterval = 0.3
    /// A new line: the block moves up by a line, the line leaving the top fading through `edge`.
    static let scroll: TimeInterval = 0.3
    /// With Reduce Motion: the cross-fade only, shorter, and lines jump.
    static let reducedChange: TimeInterval = 0.2
    /// The cubic closest to `springDuration: d, bounce: 0` (1 − (1 + ωt)e^(−ωt), ω = 2π/d):
    /// 47%, 82% and 95% of the way at a quarter, half and three quarters of the time.
    static var curve: CAMediaTimingFunction { CAMediaTimingFunction(controlPoints: 0.25, 0.6, 0.3, 1) }
}

/// The preview's words in a box: one layer drawn from a TextKit layout only when the words
/// change, inside a clip whose bounds scroll by whole lines. Lines well above the box are
/// dropped, whole, so a minute's dictation draws no more than a few lines more than the box.
@MainActor
final class LiveWordsView: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { relayout(animated: false) } } }
    var font = UIFont.systemFont(ofSize: 23) { didSet { if font != oldValue { relayout(animated: false) } } }
    var placeholder = "" { didSet { if placeholder != oldValue { relayout(animated: false) } } }
    /// The band above the box where a line leaving the top fades out.
    static let edge: CGFloat = 14

    private let canvas = LiveWordsCanvas()
    /// Scrolls (its bounds' origin moves) under the box and the band, which stay put.
    private let scroller = UIView()
    private let edgeMask = CAGradientLayer()
    private var text = ""
    private var settled = 0
    /// Characters of `text` dropped from the top, whole lines that left the box.
    private var dropped = 0
    private var scrolled: CGFloat = 0
    private var trimWork: DispatchWorkItem?
    /// Changes with every new layout, so a trim begun before it never lands after it.
    private var generation = 0
    /// The main thread's share of each update (laying out and drawing), for the probe log.
    private var updates = 0
    private var updateTime: CFTimeInterval = 0
    private var slowestUpdate: CFTimeInterval = 0
    private let reduceMotion = UIAccessibility.isReduceMotionEnabled

    var lineHeight: CGFloat { (font.lineHeight * 1.08).rounded(.up) }

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        isUserInteractionEnabled = false
        clipsToBounds = true
        scroller.isUserInteractionEnabled = false
        addSubview(scroller)
        scroller.addSubview(canvas)
        // The band above the first line: clear at the top, opaque by the line.
        edgeMask.colors = [UIColor.clear.cgColor, UIColor.black.cgColor]
        edgeMask.startPoint = CGPoint(x: 0.5, y: 0)
        edgeMask.endPoint = CGPoint(x: 0.5, y: 1)
        layer.mask = edgeMask
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// Where the words are now (their top in this view's scrolled coordinates), for tests.
    var scrollOffset: CGFloat { scroller.bounds.origin.y }
    var drawnHeight: CGFloat { canvas.bounds.height }

    func takeCost() -> (updates: Int, averageMS: Double, slowestMS: Double) {
        defer { updates = 0; updateTime = 0; slowestUpdate = 0 }
        return (updates, updates > 0 ? updateTime / Double(updates) * 1000 : 0, slowestUpdate * 1000)
    }

    func show(text: String, settled: Int) {
        guard text != self.text || settled != self.settled else { return }
        let started = CACurrentMediaTime()
        defer {
            // The drawing it asked for happens before the frame is shown: counted with it.
            canvas.layer.displayIfNeeded()
            let took = CACurrentMediaTime() - started
            updates += 1
            updateTime += took
            slowestUpdate = max(slowestUpdate, took)
        }
        if !text.hasPrefix(String(self.text.prefix(dropped))) || text.count < dropped { dropped = 0 }
        self.text = text
        self.settled = settled
        relayout(animated: window != nil)
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        edgeMask.frame = bounds
        edgeMask.locations = [0, NSNumber(value: Double(Self.edge / max(1, bounds.height)))]
        scroller.bounds.size = bounds.size
        scroller.center = CGPoint(x: bounds.midX, y: bounds.midY)
        CATransaction.commit()
        if canvas.bounds.width != bounds.width { relayout(animated: false) }
    }

    /// Draws the words as they are now. Animated: the change cross-fades, and the block moves
    /// up by whole lines when the last line no longer fits.
    private func relayout(animated: Bool) {
        guard bounds.width > 0 else { return }
        generation += 1
        setBand(clear: false)
        let moves = animated && !reduceMotion
        let attributed = attributedWords()
        let height = canvas.layOut(attributed, width: bounds.width, lineHeight: lineHeight)
        let visible = max(lineHeight, bounds.height - Self.edge)
        let overflow = height - visible
        let target = overflow > 0 ? (overflow / lineHeight).rounded(.up) * lineHeight : 0
        if animated {
            let fade = CATransition()
            fade.type = .fade
            fade.duration = reduceMotion ? LiveWordsMotion.reducedChange : LiveWordsMotion.change
            fade.timingFunction = LiveWordsMotion.curve
            canvas.layer.add(fade, forKey: "words")
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        canvas.frame = CGRect(x: 0, y: Self.edge, width: bounds.width, height: max(height, lineHeight))
        CATransaction.commit()
        canvas.setNeedsDisplay()
        if target != scrolled {
            scrolled = target
            let place = { self.scroller.bounds.origin.y = target }
            if moves {
                UIView.animate(springDuration: LiveWordsMotion.scroll, bounce: 0, initialSpringVelocity: 0, delay: 0,
                               options: [.beginFromCurrentState, .allowUserInteraction], animations: place)
            } else {
                place()
            }
        }
        scheduleTrim()
    }

    private func attributedWords() -> NSAttributedString {
        let shown = Array(text).dropFirst(dropped)
        let paragraph = NSMutableParagraphStyle()
        paragraph.minimumLineHeight = lineHeight
        paragraph.maximumLineHeight = lineHeight
        paragraph.lineBreakMode = .byWordWrapping
        let base: [NSAttributedString.Key: Any] = [.font: font, .paragraphStyle: paragraph]
        guard !shown.isEmpty else {
            return NSAttributedString(string: placeholder, attributes: base.merging([.foregroundColor: palette.textMuted]) { $1 })
        }
        let ink = max(0, min(shown.count, settled - dropped))
        let result = NSMutableAttributedString(string: String(shown.prefix(ink)), attributes: base.merging([.foregroundColor: palette.textBase]) { $1 })
        result.append(NSAttributedString(string: String(shown.dropFirst(ink)), attributes: base.merging([.foregroundColor: palette.textMuted]) { $1 }))
        return result
    }

    /// Once the words are at rest, the lines above the box go, whole: the words are laid out
    /// from the top line's first word and the block moves down by as much in the same frame,
    /// so nothing on screen changes, the lines that stay break as they did, and nothing waits in
    /// the band above the box but the line the next move takes out through it.
    private func scheduleTrim() {
        trimWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.trim() }
        trimWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + max(LiveWordsMotion.change, LiveWordsMotion.scroll) + 0.05, execute: work)
    }

    /// The band's lines fade out first (what's left of the line that just left the top), then
    /// they go from the layout with nothing on screen changing.
    private func trim() {
        let above = Int((scrolled / lineHeight).rounded())
        guard above > 0, canvas.characterStartingLine(above) != nil else { return }
        let started = generation
        CATransaction.begin()
        CATransaction.setAnimationDuration(reduceMotion ? 0 : LiveWordsMotion.reducedChange)
        CATransaction.setAnimationTimingFunction(LiveWordsMotion.curve)
        CATransaction.setCompletionBlock { [weak self] in
            guard let self, self.generation == started else { return }
            self.dropLines(above)
        }
        setBand(clear: true)
        CATransaction.commit()
    }

    /// The band above the box: a fade from clear at its top to opaque at the first line, or
    /// clear all through once the words are at rest.
    private func setBand(clear: Bool) {
        let band = Double(Self.edge / max(1, bounds.height))
        let locations: [NSNumber] = clear ? [NSNumber(value: band * 0.99), NSNumber(value: band)] : [0, NSNumber(value: band)]
        guard edgeMask.locations != locations else { return }
        if !clear {
            CATransaction.begin()
            CATransaction.setDisableActions(true)
        }
        edgeMask.locations = locations
        if !clear { CATransaction.commit() }
    }

    private func dropLines(_ above: Int) {
        guard let start = canvas.characterStartingLine(above) else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        dropped += start
        let shift = CGFloat(above) * lineHeight
        let height = canvas.layOut(attributedWords(), width: bounds.width, lineHeight: lineHeight)
        canvas.frame = CGRect(x: 0, y: Self.edge, width: bounds.width, height: max(height, lineHeight))
        canvas.setNeedsDisplay()
        scrolled -= shift
        scroller.bounds.origin.y = scrolled
        setBand(clear: false)
        CATransaction.commit()
    }
}

/// The words' one drawn layer: a TextKit 1 layout, drawn only when the words change.
@MainActor
private final class LiveWordsCanvas: UIView {
    private let storage = NSTextStorage()
    private let layout = NSLayoutManager()
    private let container = NSTextContainer(size: .zero)
    private var lineHeight: CGFloat = 0

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        isOpaque = false
        isUserInteractionEnabled = false
        contentMode = .topLeft
        container.lineFragmentPadding = 0
        layout.addTextContainer(container)
        storage.addLayoutManager(layout)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// Lays `text` out at `width` and returns its height in whole lines.
    func layOut(_ text: NSAttributedString, width: CGFloat, lineHeight: CGFloat) -> CGFloat {
        self.lineHeight = lineHeight
        container.size = CGSize(width: width, height: .greatestFiniteMagnitude)
        storage.setAttributedString(text)
        layout.ensureLayout(for: container)
        var lines = 0
        if layout.numberOfGlyphs > 0 {
            layout.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: layout.numberOfGlyphs)) { _, _, _, _, _ in lines += 1 }
        }
        return CGFloat(lines) * lineHeight
    }

    /// How many characters come before line `line` (0 is the first).
    func characterStartingLine(_ line: Int) -> Int? {
        var index = 0
        var found: Int?
        guard layout.numberOfGlyphs > 0 else { return nil }
        layout.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: layout.numberOfGlyphs)) { _, _, _, glyphs, stop in
            if index == line {
                found = self.layout.characterIndexForGlyph(at: glyphs.location)
                stop.pointee = true
            }
            index += 1
        }
        guard let found else { return nil }
        // In characters of the string, as `dropped` counts them.
        return (storage.string as NSString).substring(to: found).count
    }

    override func draw(_ rect: CGRect) {
        guard layout.numberOfGlyphs > 0 else { return }
        layout.drawGlyphs(forGlyphRange: NSRange(location: 0, length: layout.numberOfGlyphs), at: .zero)
    }
}
