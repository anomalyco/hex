import os
import UIKit

/// How the preview's words move, in one easing family: a critically damped spring by its
/// duration (`UIView.animate(springDuration:bounce: 0)`'s curve), 99% there at the duration.
nonisolated enum LiveWordsMotion {
    /// A new word: fades in and rises `rise` points.
    static let arrive: TimeInterval = 0.3
    static let rise: CGFloat = 4
    /// The words of one burst follow each other by at most `stagger`, all of them within
    /// `burstSpread`, so the last is in long before the next burst (Apple's come about once a second).
    static let stagger: TimeInterval = 0.04
    static let burstSpread: TimeInterval = 0.24
    /// A tentative word easing to the keys' ink once the recogniser settles it.
    static let settle: TimeInterval = 0.3
    /// A word the recogniser changed: the old one fades out where it was as the new one fades in.
    static let replaceOut: TimeInterval = 0.15
    static let replaceIn: TimeInterval = 0.2
    /// A new line taking the last one's place at the foot, the top line fading as it leaves.
    static let scroll: TimeInterval = 0.3
    /// The first words wait this long for "Listening…" to be mostly gone.
    static let placeholderLead: TimeInterval = 0.06
    /// With Reduce Motion: fades only, nothing moves.
    static let reducedFade: TimeInterval = 0.2

    /// The spring's progress from 0 to 1 after `elapsed` of `duration`.
    static func progress(_ elapsed: TimeInterval, _ duration: TimeInterval) -> CGFloat {
        guard elapsed > 0 else { return 0 }
        let omega = 2 * Double.pi / duration
        let value = 1 - (1 + omega * elapsed) * exp(-omega * elapsed)
        return value > 0.999 ? 1 : CGFloat(value)
    }

    /// When each of `count` new words starts, from the burst's start.
    static func delays(count: Int) -> [TimeInterval] {
        guard count > 1 else { return Array(repeating: 0, count: count) }
        let step = min(stagger, burstSpread / Double(count - 1))
        return (0..<count).map { Double($0) * step }
    }
}

/// The words of a live transcript as pieces that keep their place: each word with the space
/// before it, and each Chinese or Japanese character on its own, since those run on unspaced.
nonisolated enum LiveWords {
    static func tokens(_ text: String) -> [String] {
        var tokens: [String] = []
        var word = ""
        var space = ""
        for character in text {
            if character.isWhitespace {
                if !word.isEmpty { tokens.append(word); word = "" }
                space.append(character)
            } else if isUnspaced(character) {
                if !word.isEmpty { tokens.append(word); word = "" }
                tokens.append(space + String(character))
                space = ""
            } else {
                if word.isEmpty { word = space; space = "" }
                word.append(character)
            }
        }
        if !word.isEmpty { tokens.append(word) }
        return tokens
    }

    /// Chinese and Japanese characters and marks, which run on without spaces.
    static func isUnspaced(_ character: Character) -> Bool {
        guard let value = character.unicodeScalars.first?.value else { return false }
        return (0x3000...0x30FF).contains(value) || (0x3400...0x4DBF).contains(value)
            || (0x4E00...0x9FFF).contains(value) || (0xF900...0xFAFF).contains(value) || (0xFF00...0xFFEF).contains(value)
    }

    enum Change: Equatable {
        /// The same word, or the same word grown (a mark added): it stays as it is.
        case kept
        /// A different word in its place: cross-faded.
        case replaced
        /// A word past the last one shown: arrives.
        case added
    }

    /// What each of `new` is to `old`, by position: words keep their place unless the recogniser
    /// changed them, so one changed word never re-fades the words after it.
    static func changes(from old: [String], to new: [String]) -> [Change] {
        new.indices.map { index in
            guard index < old.count else { return .added }
            let before = old[index]
            let after = new[index]
            if after == before || after.hasPrefix(before) { return .kept }
            return .replaced
        }
    }
}

/// The preview's words, drawn by one view from a TextKit layout: no view or layer per word, so
/// a minute's dictation costs no more than a screenful. A display link runs only while
/// something moves (words arriving, settling or being replaced, a new line), and every frame
/// is drawn from where each animation is at that moment.
@MainActor
final class LiveWordsView: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { setNeedsDisplay() } } }
    var font = UIFont.systemFont(ofSize: 23) { didSet { if font != oldValue { rebuild(animated: false) } } }
    var placeholder = "" { didSet { if placeholder != oldValue { setNeedsDisplay() } } }
    /// The band above the first line where a line leaving the top fades out.
    static let edge: CGFloat = 14

    private struct Word {
        var text: String
        var settled: Bool
        /// When it began to arrive (or to replace another), and how long that takes.
        var since: CFTimeInterval
        var fadeIn: TimeInterval
        var rises: Bool
        var settledSince: CFTimeInterval?
    }

    private struct Ghost {
        let text: NSAttributedString
        let origin: CGPoint
        let alpha: CGFloat
        let since: CFTimeInterval
    }

    private var words: [Word] = []
    /// The first word laid out: lines scrolled well out of view are dropped, whole.
    private var first = 0
    private var ghosts: [Ghost] = []
    private let storage = NSTextStorage()
    private let layout = NSLayoutManager()
    private let container = NSTextContainer(size: .zero)
    /// For each laid-out word: its characters, its glyphs, and its line's top.
    private var ranges: [NSRange] = []
    private var glyphRanges: [NSRange] = []
    private var lineTops: [CGFloat] = []
    private var lineCount = 0
    private var offset: CGFloat = 0
    private var velocity: CGFloat = 0
    private var target: CGFloat = 0
    private var link: CADisplayLink?
    private var lastTick: CFTimeInterval = 0
    private var frames = 0
    private var lateFrames = 0
    private var placeholderSince: CFTimeInterval?
    private let reduceMotion = UIAccessibility.isReduceMotionEnabled

    var lineHeight: CGFloat { (font.lineHeight * 1.08).rounded(.up) }

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        isOpaque = false
        isUserInteractionEnabled = false
        contentMode = .redraw
        container.lineFragmentPadding = 0
        container.maximumNumberOfLines = 0
        layout.addTextContainer(container)
        storage.addLayoutManager(layout)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// The words now: what changed arrives, settles or cross-fades from what was shown.
    func show(text: String, settled: Int) {
        let now = CACurrentMediaTime()
        let tokens = LiveWords.tokens(text)
        let changes = LiveWords.changes(from: words.map(\.text), to: tokens)
        var lead: TimeInterval = 0
        if words.isEmpty, !tokens.isEmpty, !placeholder.isEmpty, window != nil {
            // "Listening…" fades out where the first words come in, a beat ahead of them.
            ghosts.append(Ghost(text: attributed(placeholder, color: palette.textMuted), origin: .zero, alpha: 1, since: now))
            lead = LiveWordsMotion.placeholderLead
        }
        // Words the recogniser changed, or took away, fade out where they were.
        for index in words.indices where index >= first && (index >= tokens.count || changes[index] == .replaced) {
            ghost(index, now: now)
        }
        var settledSoFar = 0
        var next: [Word] = []
        let added = changes.filter { $0 == .added }.count
        let delays = LiveWordsMotion.delays(count: added)
        var addedIndex = 0
        for (index, token) in tokens.enumerated() {
            settledSoFar += token.count
            let isSettled = settledSoFar <= settled
            switch changes[index] {
            case .kept:
                var word = words[index]
                word.text = token
                if isSettled, !word.settled { word.settledSince = now }
                word.settled = isSettled
                next.append(word)
            case .replaced:
                next.append(Word(text: token, settled: isSettled, since: now, fadeIn: fade(LiveWordsMotion.replaceIn), rises: false, settledSince: nil))
            case .added:
                next.append(Word(
                    text: token, settled: isSettled, since: now + lead + delays[addedIndex],
                    fadeIn: fade(LiveWordsMotion.arrive), rises: !reduceMotion, settledSince: nil
                ))
                addedIndex += 1
            }
        }
        words = next
        if first >= words.count { first = 0 }
        rebuild(animated: window != nil)
    }

    private func fade(_ duration: TimeInterval) -> TimeInterval {
        reduceMotion ? LiveWordsMotion.reducedFade : duration
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if container.size.width != bounds.width { rebuild(animated: false) }
    }

    // MARK: - Layout

    private var paragraph: NSParagraphStyle {
        let style = NSMutableParagraphStyle()
        style.minimumLineHeight = lineHeight
        style.maximumLineHeight = lineHeight
        style.lineBreakMode = .byWordWrapping
        return style
    }

    private func attributed(_ text: String, color: UIColor) -> NSAttributedString {
        NSAttributedString(string: text, attributes: [.font: font, .paragraphStyle: paragraph, .foregroundColor: color])
    }

    /// Lays out the words from `first`, finds where the last line puts the text, and drops lines
    /// that are well out of view, whole, so the lines that stay never break differently.
    private func rebuild(animated: Bool) {
        guard bounds.width > 0 else { return }
        layOut()
        let visible = max(lineHeight, bounds.height - Self.edge)
        let overflow = CGFloat(lineCount) * lineHeight - visible
        target = overflow > 0 ? (overflow / lineHeight).rounded(.up) * lineHeight : 0
        let linesAbove = Int((target / lineHeight).rounded())
        if linesAbove > 3, let start = wordStartingLine(linesAbove - 2) {
            let dropped = CGFloat(linesAbove - 2) * lineHeight
            first = start
            offset -= dropped
            target -= dropped
            ghosts = ghosts.map { Ghost(text: $0.text, origin: CGPoint(x: $0.origin.x, y: $0.origin.y - dropped), alpha: $0.alpha, since: $0.since) }
            layOut()
        }
        if !animated || reduceMotion {
            offset = target
            velocity = 0
        }
        setNeedsDisplay()
        if animated { startLink() }
    }

    private func layOut() {
        container.size = CGSize(width: bounds.width, height: .greatestFiniteMagnitude)
        let shown = words.dropFirst(first)
        var text = ""
        ranges = []
        for (position, word) in shown.enumerated() {
            // The first word laid out starts its line: its space goes.
            let piece = position == 0 ? String(word.text.drop(while: \.isWhitespace)) : word.text
            let start = (text as NSString).length
            text += piece
            ranges.append(NSRange(location: start, length: (text as NSString).length - start))
        }
        storage.setAttributedString(attributed(text, color: palette.textBase))
        layout.ensureLayout(for: container)
        glyphRanges = ranges.map { layout.glyphRange(forCharacterRange: $0, actualCharacterRange: nil) }
        lineTops = glyphRanges.map { range in
            guard range.length > 0 else { return 0 }
            return layout.lineFragmentRect(forGlyphAt: max(range.location, NSMaxRange(range) - 1), effectiveRange: nil).minY
        }
        var lines = 0
        if layout.numberOfGlyphs > 0 {
            layout.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: layout.numberOfGlyphs)) { _, _, _, _, _ in lines += 1 }
        }
        lineCount = lines
    }

    /// The index (in `words`) of the word that starts laid-out line `line`.
    private func wordStartingLine(_ line: Int) -> Int? {
        let top = CGFloat(line) * lineHeight
        guard let local = lineTops.firstIndex(where: { $0 >= top - 0.5 }), local > 0 else { return nil }
        return first + local
    }

    /// A shown word, as it looks now, fading out where it is.
    private func ghost(_ index: Int, now: CFTimeInterval) {
        let local = index - first
        guard local >= 0, local < glyphRanges.count, glyphRanges[local].length > 0 else { return }
        let word = words[index]
        let alpha = LiveWordsMotion.progress(now - word.since, word.fadeIn)
        guard alpha > 0.05 else { return }
        let core = String(word.text.drop(while: \.isWhitespace))
        let range = ranges[local]
        let lead = word.text.count - core.count
        let glyph = layout.glyphIndexForCharacter(at: min(range.location + (local == 0 ? 0 : lead), NSMaxRange(range) - 1))
        let line = layout.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
        let x = line.minX + layout.location(forGlyphAt: glyph).x
        ghosts.append(Ghost(text: attributed(core, color: color(of: word, now: now)), origin: CGPoint(x: x, y: line.minY), alpha: alpha, since: now))
    }

    // MARK: - Motion

    private func color(of word: Word, now: CFTimeInterval) -> UIColor {
        let settle: CGFloat
        if !word.settled {
            settle = 0
        } else if let since = word.settledSince {
            settle = LiveWordsMotion.progress(now - since, fade(LiveWordsMotion.settle))
        } else {
            settle = 1
        }
        return Self.mix(palette.textMuted, palette.textBase, settle)
    }

    private static func mix(_ from: UIColor, _ to: UIColor, _ amount: CGFloat) -> UIColor {
        guard amount > 0 else { return from }
        guard amount < 1 else { return to }
        var a = (r: CGFloat(0), g: CGFloat(0), b: CGFloat(0), o: CGFloat(0))
        var b = a
        from.getRed(&a.r, green: &a.g, blue: &a.b, alpha: &a.o)
        to.getRed(&b.r, green: &b.g, blue: &b.b, alpha: &b.o)
        return UIColor(
            red: a.r + (b.r - a.r) * amount, green: a.g + (b.g - a.g) * amount,
            blue: a.b + (b.b - a.b) * amount, alpha: a.o + (b.o - a.o) * amount
        )
    }

    private func startLink() {
        guard link == nil, window != nil else { return }
        let link = CADisplayLink(target: LinkTarget(self), selector: #selector(LinkTarget.tick(_:)))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        link.add(to: .main, forMode: .common)
        self.link = link
        lastTick = 0
    }

    fileprivate func tick(_ link: CADisplayLink) {
        let dt = lastTick > 0 ? min(link.timestamp - lastTick, 1.0 / 20) : 1.0 / 60
        if lastTick > 0 {
            frames += 1
            if link.timestamp - lastTick > 1.5 / 60 { lateFrames += 1 }
        }
        lastTick = link.timestamp
        // The block's place: a critically damped spring toward `target`, so a new line arriving
        // mid-move carries on from where it is.
        let omega = 2 * CGFloat.pi / CGFloat(LiveWordsMotion.scroll)
        let acceleration = omega * omega * (target - offset) - 2 * omega * velocity
        velocity += acceleration * CGFloat(dt)
        offset += velocity * CGFloat(dt)
        let now = CACurrentMediaTime()
        ghosts.removeAll { LiveWordsMotion.progress(now - $0.since, fade(LiveWordsMotion.replaceOut)) >= 1 }
        let moving = abs(target - offset) > 0.25 || abs(velocity) > 1
        if !moving {
            offset = target
            velocity = 0
        }
        setNeedsDisplay()
        if !moving, ghosts.isEmpty, words.allSatisfy({ settledAt(now, $0) }) { stopLink() }
    }

    private func settledAt(_ now: CFTimeInterval, _ word: Word) -> Bool {
        guard LiveWordsMotion.progress(now - word.since, word.fadeIn) >= 1 else { return false }
        guard let since = word.settledSince else { return true }
        return LiveWordsMotion.progress(now - since, fade(LiveWordsMotion.settle)) >= 1
    }

    private func stopLink() {
        link?.invalidate()
        link = nil
    }

    /// Frames drawn while moving, and those that came late (over 1.5 frames at 60 Hz), since the
    /// last call: the probe log's evidence for smooth motion.
    func takeFrameStats() -> (frames: Int, late: Int) {
        defer { frames = 0; lateFrames = 0 }
        return (frames, lateFrames)
    }

    override func willMove(toWindow newWindow: UIWindow?) {
        super.willMove(toWindow: newWindow)
        if newWindow == nil { stopLink() }
    }

    // MARK: - Drawing

    override func draw(_ rect: CGRect) {
        let now = CACurrentMediaTime()
        let top = Self.edge - offset
        if words.isEmpty {
            if !placeholder.isEmpty {
                attributed(placeholder, color: palette.textMuted).draw(at: CGPoint(x: 0, y: Self.edge))
            }
        } else {
            storage.beginEditing()
            var alphas: [CGFloat] = []
            for (local, range) in ranges.enumerated() {
                let word = words[first + local]
                let arrived = LiveWordsMotion.progress(now - word.since, word.fadeIn)
                let alpha = arrived * edgeAlpha(lineTop: lineTops[local], top: top)
                alphas.append(alpha)
                let color = color(of: word, now: now)
                storage.addAttribute(.foregroundColor, value: color.withAlphaComponent(color.cgColor.alpha * alpha), range: range)
            }
            storage.endEditing()
            for (local, glyphs) in glyphRanges.enumerated() where alphas[local] > 0.004 && glyphs.length > 0 {
                let word = words[first + local]
                let rise = word.rises ? (1 - LiveWordsMotion.progress(now - word.since, word.fadeIn)) * LiveWordsMotion.rise : 0
                layout.drawGlyphs(forGlyphRange: glyphs, at: CGPoint(x: 0, y: top + rise))
            }
        }
        for ghost in ghosts {
            let left = 1 - LiveWordsMotion.progress(now - ghost.since, fade(LiveWordsMotion.replaceOut))
            let alpha = ghost.alpha * left * edgeAlpha(lineTop: ghost.origin.y, top: top)
            guard alpha > 0.004, let context = UIGraphicsGetCurrentContext() else { continue }
            context.saveGState()
            context.setAlpha(alpha)
            ghost.text.draw(at: CGPoint(x: ghost.origin.x, y: top + ghost.origin.y))
            context.restoreGState()
        }
    }

    /// Lines in view are whole; one leaving the top fades through the edge band above it.
    private func edgeAlpha(lineTop: CGFloat, top: CGFloat) -> CGFloat {
        let y = top + lineTop
        guard y < Self.edge else { return 1 }
        return max(0, min(1, (y - Self.edge + lineHeight * 0.6) / (lineHeight * 0.6)))
    }
}

/// A display link's target that doesn't keep the view alive.
@MainActor
private final class LinkTarget: NSObject {
    weak var view: LiveWordsView?

    init(_ view: LiveWordsView) {
        self.view = view
    }

    @objc func tick(_ link: CADisplayLink) {
        guard let view else {
            link.invalidate()
            return
        }
        view.tick(link)
    }
}
