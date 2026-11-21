import QuartzCore
import UIKit

/// Settings › Keyboard › Swipe typing: slide from a letter through the letters of a word and lift,
/// and the word goes in, as with Apple's QuickPath and Gboard's glide typing. English and
/// Spanish layouts only (the languages with a typing model); every other layout keeps a drag
/// as a slide to another key.
///
/// A touch on a letter key becomes a swipe once it travels more than a key's pitch from
/// where it went down: the key no longer types, a light trail follows the finger, and the
/// strip shows what the path reads as so far. Lifting the finger types the best reading
/// (`FullKeyboardEngine.typeSwipedWord`, which spaces and capitalises it) and offers the
/// next three in the strip. Taps, holds for accents, and slides of a key or less keep
/// working as before, and so do delete, the space bar's trackpad, 123 and the Hex key, which
/// no swipe starts on.
@MainActor
final class SwipeTyping {
    private(set) var decoder: SwipeDecoder
    private unowned let engine: FullKeyboardEngine
    /// The light trail behind the finger; the key plane holds it above its keys.
    let trail = SwipeTrailView()

    /// Letter touches that may still become swipes: their points so far.
    private var pending: [Int: [CGPoint]] = [:]
    /// The swipe under way.
    private var active: (id: Int, points: [CGPoint])?
    /// Bumped by every swipe's start and end, so a reading of an older one is dropped.
    private var generation = 0
    private var previewInFlight = false
    private var lastPreviewAt: TimeInterval = 0
    /// Decodes while the finger moves, for the strip, off the main thread.
    private static let previewQueue = DispatchQueue(label: "anomaly.hex.swipe-preview", qos: .userInitiated)
    static let previewInterval: TimeInterval = 0.07
    /// Your own words (`AutocorrectEngine.ownWords`) as the decoder has them.
    private var ownWordsRevision: Int?

    /// How long the last swipe's decode took, for Record typing logs and probes.
    private(set) var lastDecodeTime: TimeInterval?

    var clock: () -> TimeInterval = { CACurrentMediaTime() }

    init(engine: FullKeyboardEngine, model: TypingDecoderModel) {
        self.engine = engine
        decoder = SwipeDecoder(model: model)
        syncWords()
    }

    var model: TypingDecoderModel { decoder.model }

    /// How far a letter touch travels before it is a swipe: a key's width and gap.
    var threshold: CGFloat { engine.layout.swipePitch }

    var isSwiping: Bool { active != nil }

    /// Words never offered (slurs, and swear words unless Suggest swear words is on), the
    /// language's contractions, and your own words; rebuilt only when one of them changed.
    func syncWords() {
        let autocorrect = engine.autocorrect
        let revision = autocorrect.ownWordsRevision
        guard revision != ownWordsRevision else { return }
        ownWordsRevision = revision
        let model = decoder.model
        decoder.neverSuggest = TypingDecoder.NeverSuggest(
            autocorrect.offensiveWords.blocked(allowSwearing: autocorrect.allowsSwearing), model: model
        )
        var extra = SwipeDecoder.ExtraWords()
        if model.language == KeyboardLanguage.english.rawValue {
            extra.add(AutocorrectEngine.englishContractions, cost: { SwipeDecoder.costOfContraction($0) }, model: model)
        }
        extra.add(autocorrect.ownWords, cost: SwipeDecoder.ownWordCost, model: model)
        decoder.extraWords = extra
    }

    // MARK: - Touches (from `KeyboardKeyPlaneView`)

    /// Before a touch goes down: a swipe still under way ends first, so its word goes in
    /// before the new key's letter.
    func touchWillBegin() {
        if let swipe = active { finish(swipe.id, points: swipe.points) }
    }

    /// After the engine took a touch: a letter key's may become a swipe.
    func began(id: Int, at point: CGPoint) {
        guard engine.allowsSwipes, let touch = engine.touches[id], !touch.committed,
              touch.key.character?.first?.isLetter == true else { return }
        pending[id] = [point]
    }

    /// True when the touch is a swipe (now or already): the plane leaves it to this.
    func moved(id: Int, to point: CGPoint) -> Bool {
        if var swipe = active, swipe.id == id {
            swipe.points.append(point)
            active = swipe
            trail.add(point)
            schedulePreview()
            return true
        }
        guard var points = pending[id] else { return false }
        points.append(point)
        pending[id] = points
        guard let first = points.first, hypot(point.x - first.x, point.y - first.y) > threshold,
              let touch = engine.touches[id], !touch.committed, engine.activeAccentMenu == nil,
              engine.allowsSwipes else { return false }
        pending[id] = nil
        syncWords()
        engine.beginSwipe(touchID: id)
        active = (id, points)
        generation += 1
        lastPreviewAt = 0
        trail.begin(points)
        schedulePreview()
        return true
    }

    /// Whether the touch is the swipe under way.
    func owns(_ id: Int) -> Bool { active?.id == id }

    /// A touch that never became a swipe ended.
    func forget(_ id: Int) {
        pending[id] = nil
    }

    /// The swipe's finger lifted: its word goes in.
    func ended(id: Int, at point: CGPoint) {
        guard let swipe = active, swipe.id == id else { return }
        finish(id, points: swipe.points + [point])
    }

    /// The system cancelled the swipe's touch: nothing goes in.
    func cancelled(id: Int) {
        pending[id] = nil
        guard let swipe = active, swipe.id == id else { return }
        active = nil
        generation += 1
        trail.fade()
        engine.endSwipe(typing: [], points: swipe.points.count, decodeTime: nil)
    }

    func cancelAll() {
        pending.removeAll()
        guard active != nil else { return }
        active = nil
        generation += 1
        trail.clear()
        engine.endSwipe(typing: [], points: 0, decodeTime: nil)
    }

    private func finish(_ id: Int, points: [CGPoint]) {
        active = nil
        pending[id] = nil
        generation += 1
        trail.fade()
        let started = clock()
        let layout = engine.layout
        let candidates = decoder.decode(
            points.map(layout.joinedForSwipe), keys: engine.swipeKeys, previous: engine.swipePreviousWord, limit: 8
        )
        let elapsed = clock() - started
        lastDecodeTime = elapsed
        engine.endSwipe(typing: candidates.map(\.text), points: points.count, decodeTime: elapsed)
    }

    // MARK: - The word so far

    /// Reads the path so far off the main thread, at most every `previewInterval`, and shows
    /// its best word in the strip while the swipe is still the same one.
    private func schedulePreview() {
        guard let swipe = active, !previewInFlight, swipe.points.count >= 3 else { return }
        let now = clock()
        guard now - lastPreviewAt >= Self.previewInterval else { return }
        previewInFlight = true
        lastPreviewAt = now
        let decoder = decoder, keys = engine.swipeKeys, previous = engine.swipePreviousWord
        let points = swipe.points.map(engine.layout.joinedForSwipe), generation = generation
        Self.previewQueue.async { [weak self] in
            let words = decoder.decode(points, keys: keys, previous: previous, limit: 3).map(\.text)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.previewInFlight = false
                    guard self.generation == generation, self.active != nil else { return }
                    self.engine.showSwipePreview(words)
                }
            }
        }
    }

    // MARK: - Spacing

    /// Whether a swiped word needs a space before it, as Apple's keyboard adds one: unless
    /// the text before the cursor is empty or ends in a space, a line break or opening
    /// punctuation (a bracket, an opening quote, ¿ or ¡, or a mark that starts a word such
    /// as @ # /). A straight quote opens when nothing or a space comes before it.
    static func needsSpace(before: String?) -> Bool {
        guard let before, let last = before.last else { return false }
        if last.isWhitespace || last.isNewline || opening.contains(last) { return false }
        if last == "\"" || last == "'" {
            let previous = before.dropLast().last
            return !(previous == nil || previous!.isWhitespace || previous!.isNewline || opening.contains(previous!))
        }
        return true
    }

    private static let opening: Set<Character> = ["(", "[", "{", "<", "«", "‹", "“", "‘", "„", "¿", "¡", "@", "#", "/"]
}

extension KeyboardLanguage {
    /// Swipe typing reads only the layouts with a typing model; elsewhere a drag stays a
    /// slide to another key.
    var supportsSwipeTyping: Bool { self == .english || self == .spanish }
}

/// The light trail behind a swiping finger: the last `tail` seconds of its path, which
/// shortens behind the finger and fades when it lifts. A shape layer, so the keyboard draws
/// no bitmap of its own for it.
@MainActor
final class SwipeTrailView: UIView {
    static let tail: TimeInterval = 0.45
    static let width: CGFloat = 6

    var palette = FullKeyboardPalette(isDark: false) {
        didSet { if palette != oldValue { shape.strokeColor = Self.color(palette).cgColor } }
    }
    var clock: () -> TimeInterval = { CACurrentMediaTime() }

    private let shape = CAShapeLayer()
    private var points: [(point: CGPoint, at: TimeInterval)] = []
    private var link: CADisplayLink?

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        isOpaque = false
        backgroundColor = .clear
        isAccessibilityElement = false
        shape.fillColor = nil
        shape.lineCap = .round
        shape.lineJoin = .round
        shape.lineWidth = Self.width
        shape.strokeColor = Self.color(palette).cgColor
        layer.addSublayer(shape)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    static func color(_ palette: FullKeyboardPalette) -> UIColor {
        palette.isDark ? UIColor(white: 1, alpha: 0.5) : palette.actionFill.withAlphaComponent(0.42)
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        shape.frame = bounds
    }

    /// Off screen, the display link (which holds this view) stops.
    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil { clear() }
    }

    var isShowing: Bool { shape.path != nil }

    func begin(_ start: [CGPoint]) {
        shape.removeAllAnimations()
        shape.opacity = 1
        let now = clock()
        points = start.map { ($0, now) }
        if link == nil, window != nil {
            let link = CADisplayLink(target: self, selector: #selector(tick))
            link.add(to: .main, forMode: .common)
            self.link = link
        }
        redraw()
    }

    func add(_ point: CGPoint) {
        points.append((point, clock()))
        redraw()
    }

    /// The finger lifted: the trail fades out.
    func fade() {
        stopTicking()
        guard shape.path != nil else { return }
        CATransaction.begin()
        CATransaction.setCompletionBlock { [weak self] in
            MainActor.assumeIsolated {
                guard let self, self.link == nil else { return }
                self.clear()
            }
        }
        let fade = CABasicAnimation(keyPath: "opacity")
        fade.fromValue = 1
        fade.toValue = 0
        fade.duration = 0.2
        shape.opacity = 0
        shape.add(fade, forKey: "fade")
        CATransaction.commit()
    }

    func clear() {
        stopTicking()
        points.removeAll()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        shape.removeAllAnimations()
        shape.path = nil
        CATransaction.commit()
    }

    private func stopTicking() {
        link?.invalidate()
        link = nil
    }

    @objc private func tick() {
        redraw()
    }

    private func redraw() {
        // Points older than the tail drop off behind the finger, down to its last stretch.
        if let old = points.lastIndex(where: { $0.at < clock() - Self.tail }) {
            let drop = min(old, points.count - 2)
            if drop > 0 { points.removeFirst(drop) }
        }
        let path = CGMutablePath()
        if let first = points.first {
            path.move(to: first.point)
            for p in points.dropFirst() { path.addLine(to: p.point) }
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        shape.path = points.count >= 2 ? path : nil
        CATransaction.commit()
    }
}
