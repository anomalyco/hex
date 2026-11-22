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

    /// A touch's points so far and when each came (`UITouch.timestamp`): the decoder reads
    /// where the finger paused from the times.
    struct Stroke {
        var points: [CGPoint] = []
        var times: [TimeInterval] = []

        mutating func add(_ point: CGPoint, at time: TimeInterval) {
            points.append(point)
            times.append(time)
        }
    }

    /// Letter touches that may still become swipes: their strokes so far.
    private var pending: [Int: Stroke] = [:]
    /// The swipe under way.
    private var active: (id: Int, stroke: Stroke)?
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
            extra.add(AutocorrectEngine.englishContractions, cost: { SwipeDecoder.costOfContraction($0) }, model: model, contractions: true)
        }
        extra.add(autocorrect.ownWords, cost: SwipeDecoder.ownWordCost, model: model)
        decoder.extraWords = extra
    }

    // MARK: - Touches (from `KeyboardKeyPlaneView`)

    /// Before a touch goes down: a swipe still under way ends first, so its word goes in
    /// before the new key's letter.
    func touchWillBegin() {
        if let swipe = active { finish(swipe.id, stroke: swipe.stroke) }
    }

    /// After the engine took a touch: a letter key's may become a swipe. `time` is the
    /// touch's timestamp (now when nil).
    func began(id: Int, at point: CGPoint, time: TimeInterval? = nil) {
        guard engine.allowsSwipes, let touch = engine.touches[id], !touch.committed,
              touch.key.character?.first?.isLetter == true else { return }
        var stroke = Stroke()
        stroke.add(point, at: time ?? clock())
        pending[id] = stroke
    }

    /// True when the touch is a swipe (now or already): the plane leaves it to this.
    func moved(id: Int, to point: CGPoint, time: TimeInterval? = nil) -> Bool {
        if var swipe = active, swipe.id == id {
            swipe.stroke.add(point, at: time ?? clock())
            active = swipe
            trail.add(point)
            schedulePreview()
            return true
        }
        guard var stroke = pending[id] else { return false }
        stroke.add(point, at: time ?? clock())
        pending[id] = stroke
        guard let first = stroke.points.first, hypot(point.x - first.x, point.y - first.y) > threshold,
              let touch = engine.touches[id], !touch.committed, engine.activeAccentMenu == nil,
              engine.allowsSwipes else { return false }
        pending[id] = nil
        syncWords()
        engine.beginSwipe(touchID: id)
        active = (id, stroke)
        generation += 1
        lastPreviewAt = 0
        trail.begin(stroke.points)
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
    func ended(id: Int, at point: CGPoint, time: TimeInterval? = nil) {
        guard var swipe = active, swipe.id == id else { return }
        swipe.stroke.add(point, at: time ?? clock())
        finish(id, stroke: swipe.stroke)
    }

    /// The system cancelled the swipe's touch: nothing goes in.
    func cancelled(id: Int) {
        pending[id] = nil
        guard let swipe = active, swipe.id == id else { return }
        active = nil
        generation += 1
        trail.fade()
        engine.endSwipe(typing: [], points: swipe.stroke.points.count, decodeTime: nil)
    }

    func cancelAll() {
        pending.removeAll()
        guard active != nil else { return }
        active = nil
        generation += 1
        trail.clear()
        engine.endSwipe(typing: [], points: 0, decodeTime: nil)
    }

    private func finish(_ id: Int, stroke: Stroke) {
        active = nil
        pending[id] = nil
        generation += 1
        trail.fade()
        let started = clock()
        let layout = engine.layout
        let candidates = decoder.decode(
            stroke.points.map(layout.joinedForSwipe), times: stroke.times, keys: engine.swipeKeys,
            previous: engine.swipePreviousWord, limit: 8
        )
        let words = ranked(candidates)
        let elapsed = clock() - started
        lastDecodeTime = elapsed
        if let start = stroke.points.first,
           SwipeDecoder.isSlidTap(stroke.points, pitch: threshold, key: slidTapKey(stroke), firstReading: words.first) {
            // A tap that slid: its letter goes in as a tap's would, after the space a swiped
            // word before it keeps pending and with the capital shift gives it.
            engine.endSwipe(typing: [], points: stroke.points.count, decodeTime: elapsed)
            engine.touchBegan(id: id, at: start)
            engine.touchEnded(id: id, at: start)
            return
        }
        engine.endSwipe(typing: words, points: stroke.points.count, decodeTime: elapsed)
    }

    /// The character of the key a swipe went down and lifted on, for the core's slid-tap
    /// rule (`SwipeDecoder.isSlidTap`: a swipe that came back to its key without going more
    /// than 1.6 key pitches from it, "a" dragged a little too far and back, is a tap unless
    /// it read a word that starts and ends on that key); nil when it lifted on another.
    private func slidTapKey(_ stroke: Stroke) -> String? {
        guard let first = stroke.points.first, let last = stroke.points.last else { return nil }
        let layout = engine.layout
        guard let down = layout.hitTest(first), down == layout.hitTest(last) else { return nil }
        return down.character
    }

    /// Whether `word` begins and ends on `letter`'s key ("lol" on l, "pop" on p): a word a
    /// swipe that comes back to its key can mean.
    static func word(_ word: String, startsAndEndsWith letter: String) -> Bool {
        SwipeDecoder.word(word, startsAndEndsWith: letter)
    }

    /// The readings best first once a rare name or abbreviation costs more: the model's
    /// lowercase list holds names and abbreviations ("gs", "ut", "kent") that a loose swipe of
    /// a common word could read as, and they would go in capitalised mid-sentence (the core's
    /// `SwipeDecoder.ranked`).
    func ranked(_ candidates: [SwipeDecoder.Candidate]) -> [String] {
        guard let words = engine.autocorrect.swipeWords else { return candidates.map(\.text) }
        let checker = engine.autocorrect.checker
        return decoder.ranked(candidates, words: words) { checker.isWordValid($0) }.map { candidates[$0].text }
    }

    // MARK: - The word so far

    /// Reads the path so far off the main thread, at most every `previewInterval`, and shows
    /// its best word in the strip while the swipe is still the same one.
    private func schedulePreview() {
        guard let swipe = active, !previewInFlight, swipe.stroke.points.count >= 3 else { return }
        let now = clock()
        guard now - lastPreviewAt >= Self.previewInterval else { return }
        previewInFlight = true
        lastPreviewAt = now
        let decoder = decoder, keys = engine.swipeKeys, previous = engine.swipePreviousWord
        let points = swipe.stroke.points.map(engine.layout.joinedForSwipe), times = swipe.stroke.times, generation = generation
        Self.previewQueue.async { [weak self] in
            let candidates = decoder.decode(points, times: times, keys: keys, previous: previous, limit: 3)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.previewInFlight = false
                    guard self.generation == generation, self.active != nil else { return }
                    self.engine.showSwipePreview(self.ranked(candidates))
                }
            }
        }
    }

    // MARK: - Spacing

    /// Whether a swiped word needs a space before it, as Apple's keyboard adds one: unless
    /// the text before the cursor is empty or ends in a space, a line break or opening
    /// punctuation (the core's `SwipeDecoder.needsSpace`).
    static func needsSpace(before: String?) -> Bool {
        SwipeDecoder.needsSpace(before: before)
    }
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
