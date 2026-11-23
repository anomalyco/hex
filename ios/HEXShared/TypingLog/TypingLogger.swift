import CoreGraphics
import QuartzCore
import UIKit

/// Record typing logs, on the main thread: turns what the keyboard does into events, keeps
/// them in memory, and hands them to `TypingLogStore` every 2 s, when the keyboard goes
/// away, and on a memory warning. Exists only while the experiment is on.
@MainActor
final class TypingLogger {
    static let flushInterval: TimeInterval = 2
    static let maxBuffered = 1500
    static let contextLimit = 200
    /// A moving finger is written once it has gone this far and this long since its last line.
    static let moveMinInterval: TimeInterval = 0.008
    static let moveMinDistance: CGFloat = 2

    private static var shared: TypingLogger?

    /// The one logger this process writes while the experiment is on; nil while it is off.
    static func sharedIfEnabled(settings: TypingLogSettings = TypingLogSettings()) -> TypingLogger? {
        guard settings.isEnabled else {
            shared?.endSession(reason: "disabled")
            shared = nil
            return nil
        }
        if let shared { return shared }
        let logger = TypingLogger(store: TypingLogStore())
        logger.store.prune()
        shared = logger
        return logger
    }

    let store: TypingLogStore
    var wallClock: () -> TimeInterval = { Date().timeIntervalSince1970 }
    var monoClock: () -> TimeInterval = { CACurrentMediaTime() }
    /// The host's text around the cursor (`documentContextBeforeInput`/`AfterInput`).
    var contextSource: (() -> TypingLogContext)?

    private struct LayoutInputs: Equatable {
        var width: CGFloat
        var orientation: KeyboardOrientation
        var layer: KeyboardLayer
        var field: KeyboardFieldKind
        var showsGlobe: Bool
        var horizontalInset: CGFloat
        var placement: HexKeyPlacement
        var language: KeyboardLanguage
        var letters: KeyboardLetterLayout

        init(_ layout: KeyboardLayout) {
            width = layout.width
            orientation = layout.orientation
            layer = layout.layer
            field = layout.field
            showsGlobe = layout.showsGlobe
            horizontalInset = layout.horizontalInset
            placement = HexKeyPlacement.current
            language = layout.language
            letters = layout.letters
        }
    }

    private struct Move {
        var time: TimeInterval
        var point: CGPoint
        var key: String?
    }

    private struct Session {
        let id: String
        let startedAt: TimeInterval
        let skipped: Bool
        var seq = 0
        var touches = 0
        var edits = 0
        var layoutInputs: LayoutInputs?
        var layout: KeyboardLayout?
        var layoutHash: String?
        var shift: ShiftState?
        var shiftHeld = false
        var viewSize: CGSize = .zero
        var safeArea: UIEdgeInsets = .zero
        /// The view's size or safe area changed since the last `layout` line.
        var geometryChanged = false
        var context: TypingLogContext?
        var shownCandidates: [KeyboardSuggestion]?
        var candidates: [KeyboardSuggestion] = []
        var moves: [Int: Move] = [:]
        var dictations = 0
    }

    private var session: Session?
    private var buffer: [TypingLogEvent] = []
    private var timer: Timer?
    /// The last dictation logged, which an outcome refers to.
    private var lastDictation: (session: String, seq: Int)?

    init(store: TypingLogStore) {
        self.store = store
    }

    /// A session is open and its field may be logged.
    var isRecording: Bool { session?.skipped == false }
    var sessionID: String? { session?.id }

    // MARK: - Sessions

    /// The keyboard appeared, or moved to another field. Logs `session_start`, or one
    /// `skipped` line and nothing else until the session ends when the field must not be logged.
    func beginSession(_ info: TypingLogSessionInfo) {
        if session != nil { endSession(reason: "replaced") }
        let id = String(UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(12)).lowercased()
        if let reason = info.traits.skipReason {
            session = Session(id: id, startedAt: monoClock(), skipped: true)
            emit("skipped", [
                TypingLogField("reason", .string(reason)),
                TypingLogField("trigger", .string(info.trigger)),
                TypingLogField("surface", .string(info.surface)),
                TypingLogField("keyboardType", .string(info.traits.keyboardType)),
                TypingLogField("textContentType", .text(info.traits.textContentType)),
                TypingLogField("secure", .bool(info.traits.isSecure)),
            ], force: true)
            flush()
            return
        }
        session = Session(id: id, startedAt: monoClock(), skipped: false)
        emit("session_start", info.fields)
        context("session_start", force: true)
        startTimer()
    }

    func endSession(reason: String) {
        guard let current = session else { return }
        if !current.skipped {
            emitPendingCandidates()
            context("session_end")
            emit("session_end", [
                TypingLogField("reason", .string(reason)),
                TypingLogField("durationMS", .ms((monoClock() - current.startedAt) * 1000)),
                TypingLogField("touches", .int(current.touches)),
                TypingLogField("edits", .int(current.edits)),
                TypingLogField("dictations", .int(current.dictations)),
            ])
        }
        session = nil
        timer?.invalidate()
        timer = nil
        flush()
        store.close()
    }

    /// Hands buffered events to the writer.
    func flush() {
        guard !buffer.isEmpty else { return }
        store.append(buffer)
        buffer.removeAll(keepingCapacity: true)
    }

    private func startTimer() {
        timer?.invalidate()
        let timer = Timer(timeInterval: Self.flushInterval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.flush() }
        }
        timer.tolerance = 0.5
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    // MARK: - Keys

    /// The layout or shift changed (or may have): writes a `layout` line when either did.
    /// The key list goes in full the first time a layout appears in a day's file.
    func keyboardState(layout: KeyboardLayout, shift: ShiftStateMachine, cause: String) {
        guard var current = session, !current.skipped else { return }
        let inputs = LayoutInputs(layout)
        let layoutChanged = inputs != current.layoutInputs
        guard layoutChanged || shift.state != current.shift || shift.isHeld != current.shiftHeld else { return }
        if layoutChanged {
            current.layoutInputs = inputs
            current.layout = layout
            current.layoutHash = Self.hash(layout)
        }
        current.shift = shift.state
        current.shiftHeld = shift.isHeld
        session = current
        emitLayout(cause: cause)
    }

    /// The keyboard view's size or safe area, before the layout follows it. The next
    /// `layout` line carries it; `commitGeometry` writes one if the layout didn't change.
    func viewGeometry(size: CGSize, safeArea: UIEdgeInsets) {
        guard var current = session, !current.skipped,
              size != current.viewSize || safeArea != current.safeArea else { return }
        current.viewSize = size
        current.safeArea = safeArea
        current.geometryChanged = true
        session = current
    }

    func commitGeometry() {
        guard let current = session, !current.skipped, current.geometryChanged, current.layout != nil else { return }
        emitLayout(cause: "geometry")
    }

    private func emitLayout(cause: String) {
        session?.geometryChanged = false
        guard let current = session, let layout = current.layout, let hash = current.layoutHash else { return }
        var fields = [
            TypingLogField("hash", .string(hash)),
            TypingLogField("cause", .string(cause)),
            TypingLogField("layer", .string(Self.name(layout.layer))),
            TypingLogField("shift", .string(Self.name(current.shift ?? .off))),
            TypingLogField("field", .string(Self.name(layout.field))),
            TypingLogField("orientation", .string(layout.orientation == .portrait ? "portrait" : "landscape")),
            TypingLogField("width", .pt(layout.width)),
            TypingLogField("height", .pt(layout.totalHeight)),
            TypingLogField("view", .array([.pt(current.viewSize.width), .pt(current.viewSize.height)])),
            TypingLogField("safeArea", TypingLogSessionInfo.insets(current.safeArea)),
            TypingLogField("horizontalInset", .pt(layout.horizontalInset)),
            TypingLogField("globe", .bool(layout.showsGlobe)),
            TypingLogField("hexPlacement", .string(HexKeyPlacement.current.rawValue)),
            TypingLogField("language", .string(layout.language.rawValue)),
            TypingLogField("letters", .string(layout.letters.rawValue)),
        ]
        if current.shiftHeld {
            fields.insert(TypingLogField("shiftHeld", .bool(true)), at: 4)
        }
        emit("layout", fields, detail: TypingLogEvent.Detail(key: hash) { Self.layoutDetail(layout) })
    }

    nonisolated private static func layoutDetail(_ layout: KeyboardLayout) -> [TypingLogField] {
        let m = layout.metrics
        let strip = layout.stripFrame
        let slots = layout.stripSlotFrames.map { TypingLogValue.rect($0) }
        let keys = layout.keys.map { key in
            TypingLogValue.object([
                TypingLogField("id", .string(keyID(key.kind))),
                TypingLogField("label", .string(key.label)),
                TypingLogField("row", .int(key.row)),
                TypingLogField("frame", .rect(key.frame)),
                TypingLogField("hitFrame", .rect(key.hitFrame)),
            ])
        }
        return [
            TypingLogField("letterWidth", .pt(layout.letterWidth)),
            TypingLogField("metrics", .object([
                TypingLogField("side", .pt(m.side)),
                TypingLogField("gap", .pt(m.gap)),
                TypingLogField("rowGap", .pt(m.rowGap)),
                TypingLogField("keyHeight", .pt(m.keyHeight)),
                TypingLogField("stripHeight", .pt(m.stripHeight)),
                TypingLogField("keysBottom", .pt(m.keysBottom)),
                TypingLogField("cornerRadius", .pt(m.cornerRadius)),
            ])),
            TypingLogField("strip", .rect(strip)),
            TypingLogField("slots", .array(slots)),
            TypingLogField("keys", .array(keys)),
        ]
    }

    /// A finger on the keys. `key` is the key the engine holds it on (for `began`, the
    /// key it is about to press); `under` is written when the key under the raw point differs.
    func touch(_ phase: TypingLogTouchPhase, id: Int, sample: TypingLogTouchSample, key: KeyboardKey?, under: KeyboardKey? = nil) {
        guard var current = session, !current.skipped else { return }
        let keyName = key.map { Self.keyID($0.kind) }
        switch phase {
        case .began:
            emitPendingCandidates()
            current = session ?? current
            current.touches += 1
            current.moves[id] = Move(time: sample.timestamp, point: sample.point, key: keyName)
        case .moved:
            if let last = current.moves[id] {
                let distance = hypot(sample.point.x - last.point.x, sample.point.y - last.point.y)
                let slid = keyName != last.key
                guard slid || (distance >= Self.moveMinDistance && sample.timestamp - last.time >= Self.moveMinInterval) else {
                    return
                }
            }
            current.moves[id] = Move(time: sample.timestamp, point: sample.point, key: keyName)
        case .ended, .cancelled:
            current.moves.removeValue(forKey: id)
        }
        session = current
        var fields = [
            TypingLogField("phase", .string(phase.rawValue)),
            TypingLogField("id", .int(id)),
            TypingLogField("x", .pt(sample.point.x)),
            TypingLogField("y", .pt(sample.point.y)),
            TypingLogField("r", .pt(sample.majorRadius)),
            TypingLogField("rt", .pt(sample.majorRadiusTolerance)),
            TypingLogField("ts", .ms(sample.timestamp * 1000)),
            TypingLogField("layout", .text(current.layoutHash)),
            TypingLogField("key", .text(keyName)),
        ]
        if sample.maximumPossibleForce > 0 {
            fields.insert(TypingLogField("force", .number(Double(sample.force), places: 3)), at: 6)
            fields.insert(TypingLogField("maxForce", .number(Double(sample.maximumPossibleForce), places: 3)), at: 7)
        }
        if let under, under.kind != key?.kind {
            fields.append(TypingLogField("under", .string(Self.keyID(under.kind))))
        } else if under == nil, phase != .moved, key != nil {
            fields.append(TypingLogField("under", .null))
        }
        emit("touch", fields)
    }

    /// Something the keys did to the text.
    func edit(_ op: String, _ fields: [TypingLogField] = []) {
        guard session?.skipped == false else { return }
        session?.edits += 1
        emit("edit", [TypingLogField("op", .string(op))] + fields)
    }

    /// A tap on the strip, the Hex key or its menu, an accent picker, or the trackpad.
    func ui(_ action: String, _ fields: [TypingLogField] = []) {
        emit("ui", [TypingLogField("action", .string(action))] + fields)
    }

    /// The suggestions the strip shows. Written lazily: before the next touch goes down,
    /// with a pick, or when the session ends, so a line is what was on screen.
    func suggestionsChanged(_ suggestions: [KeyboardSuggestion]) {
        guard session?.skipped == false else { return }
        session?.candidates = suggestions
    }

    var shownSuggestions: [KeyboardSuggestion] { session?.candidates ?? [] }

    private func emitPendingCandidates() {
        guard let current = session, !current.skipped, current.candidates != current.shownCandidates else { return }
        session?.shownCandidates = current.candidates
        emit("candidates", [TypingLogField("items", Self.candidates(current.candidates))])
    }

    static func candidates(_ suggestions: [KeyboardSuggestion]) -> TypingLogValue {
        .array(suggestions.enumerated().map { index, suggestion in
            var fields = [
                TypingLogField("slot", .int(index)),
                TypingLogField("text", .string(suggestion.displayText)),
            ]
            if suggestion.replacement != suggestion.displayText {
                fields.append(TypingLogField("replacement", .string(suggestion.replacement)))
            }
            switch suggestion.kind {
            case .keepLiteral:
                fields.append(TypingLogField("kind", .string("literal")))
            case .primary(let auto):
                fields.append(TypingLogField("kind", .string("primary")))
                fields.append(TypingLogField("auto", .bool(auto)))
            case .secondary:
                fields.append(TypingLogField("kind", .string("secondary")))
            case .nextWord:
                fields.append(TypingLogField("kind", .string("next_word")))
            case .emoji:
                fields.append(TypingLogField("kind", .string("emoji")))
            case .swiped:
                fields.append(TypingLogField("kind", .string("swiped")))
            case .revert:
                fields.append(TypingLogField("kind", .string("revert")))
            }
            return .object(fields)
        })
    }

    // MARK: - Text

    /// The host's text around the cursor, after a word, a return, a dictation, or a change
    /// from outside the keys. Identical context is written once. `cleared` marks the host
    /// emptying a field that had text (most apps do that when they send).
    func context(_ why: String, force: Bool = false) {
        guard var current = session, !current.skipped, let read = contextSource?() else { return }
        guard force || read != current.context else { return }
        let previous = current.context
        current.context = read
        session = current
        var fields = [
            TypingLogField("why", .string(why)),
            TypingLogField("before", .text(read.before.map { String($0.suffix(Self.contextLimit)) })),
            TypingLogField("after", .text(read.after.map { String($0.prefix(Self.contextLimit)) })),
            TypingLogField("beforeLength", read.before.map { .int($0.count) } ?? .null),
            TypingLogField("afterLength", read.after.map { .int($0.count) } ?? .null),
            TypingLogField("hasText", .bool(read.hasText)),
        ]
        if let previous, !read.hasText,
           previous.hasText || !(previous.before ?? "").isEmpty || !(previous.after ?? "").isEmpty {
            fields.append(TypingLogField("cleared", .bool(true)))
        }
        emit("context", fields)
    }

    // MARK: - Dictation

    /// Hex inserted a dictation's text.
    func dictation(source: String, resultID: String?, style: String, stages: DictationStages?, delivered: String, inserted: String) {
        guard var current = session, !current.skipped else { return }
        current.dictations += 1
        session = current
        var fields = [
            TypingLogField("source", .string(source)),
            TypingLogField("resultID", .text(resultID)),
            TypingLogField("style", .string(stages?.style ?? style)),
        ]
        if let stages { fields += stages.fields }
        fields += [
            TypingLogField("text", .string(delivered)),
            TypingLogField("inserted", .string(inserted)),
        ]
        let seq = emit("dictation", fields)
        lastDictation = seq.map { (current.id, $0) }
        context("dictation")
    }

    /// What became of a dictation (`DictationEditTracker`): it always concerns the latest
    /// `dictation` line written before it, which `dictation`/`dictationSession` name.
    func dictationOutcome(_ result: DictationEditResult) {
        var fields = [
            TypingLogField("outcome", .string(result.outcome.rawValue)),
            TypingLogField("words", .int(result.wordCount)),
            TypingLogField("changed", .int(result.changedWordCount)),
        ]
        if let lastDictation {
            fields.insert(TypingLogField("dictationSession", .string(lastDictation.session)), at: 0)
            fields.insert(TypingLogField("dictation", .int(lastDictation.seq)), at: 1)
        }
        fields.append(TypingLogField("pairs", .array(result.pairs.map { pair in
            .object([
                TypingLogField("kind", .string(pair.kind.rawValue)),
                TypingLogField("heard", .string(pair.heard)),
                TypingLogField("inserted", .string(pair.inserted)),
                TypingLogField("meant", .string(pair.meant)),
                TypingLogField("before", .text(pair.before)),
                TypingLogField("after", .text(pair.after)),
            ])
        })))
        emit("dictation_outcome", fields)
    }

    // MARK: - Lines

    @discardableResult
    private func emit(_ type: String, _ fields: [TypingLogField], detail: TypingLogEvent.Detail? = nil, force: Bool = false) -> Int? {
        guard var current = session, force || !current.skipped else { return nil }
        current.seq += 1
        let seq = current.seq
        session = current
        buffer.append(TypingLogEvent(
            type: type,
            wallMS: Int64((wallClock() * 1000).rounded()),
            monoMS: monoClock() * 1000,
            session: current.id,
            seq: seq,
            fields: fields,
            detail: detail
        ))
        if buffer.count >= Self.maxBuffered { flush() }
        return seq
    }

    // MARK: - Names

    /// A key's id in logs: the character itself for character keys (always one character),
    /// otherwise a word: `shift`, `delete`, `space`, `return`, `hex`, `globe`, `layer:numbers`.
    nonisolated static func keyID(_ kind: KeyKind) -> String {
        switch kind {
        case .character(let text): text
        case .shift: "shift"
        case .layerSwitch(let layer): "layer:\(name(layer))"
        case .delete: "delete"
        case .space: "space"
        case .returnKey: "return"
        case .hex: "hex"
        case .globe: "globe"
        case .emoji: "emoji"
        case .dismiss: "dismiss"
        }
    }

    nonisolated static func name(_ layer: KeyboardLayer) -> String {
        switch layer {
        case .letters: "letters"
        case .numbers: "numbers"
        case .symbols: "symbols"
        }
    }

    nonisolated static func name(_ field: KeyboardFieldKind) -> String {
        switch field {
        case .text: "text"
        case .url: "url"
        case .email: "email"
        case .webSearch: "webSearch"
        }
    }

    nonisolated static func name(_ shift: ShiftState) -> String {
        switch shift {
        case .off: "off"
        case .on: "on"
        case .locked: "locked"
        }
    }

    /// 64-bit FNV-1a over every key's kind and frames and the strip, as 16 hex digits.
    nonisolated static func hash(_ layout: KeyboardLayout) -> String {
        var hash: UInt64 = 0xcbf2_9ce4_8422_2325
        func mix(_ text: String) {
            for byte in text.utf8 {
                hash ^= UInt64(byte)
                hash = hash &* 0x0000_0100_0000_01b3
            }
        }
        func mix(_ value: CGFloat) {
            // Rounded to 1/100 pt so the hash is the same wherever the same frames come from.
            var bits = UInt64(bitPattern: Int64((Double(value) * 100).rounded()))
            for _ in 0..<8 {
                hash ^= bits & 0xff
                hash = hash &* 0x0000_0100_0000_01b3
                bits >>= 8
            }
        }
        func mix(_ rect: CGRect) {
            mix(rect.minX)
            mix(rect.minY)
            mix(rect.width)
            mix(rect.height)
        }
        mix(name(layout.layer))
        mix(name(layout.field))
        mix(layout.orientation == .portrait ? "p" : "l")
        mix(layout.stripFrame)
        for key in layout.keys {
            mix(keyID(key.kind))
            mix(key.frame)
            mix(key.hitFrame)
        }
        return String(format: "%016llx", hash)
    }
}

extension DictationStages {
    var fields: [TypingLogField] {
        var fields: [TypingLogField] = []
        func add(_ key: String, _ value: String?) {
            if let value { fields.append(TypingLogField(key, .string(value))) }
        }
        add("engine", engine)
        add("locale", locale)
        add("language", language)
        if let recordedMS { fields.append(TypingLogField("recordedMS", .int(recordedMS))) }
        if let transcribeMS { fields.append(TypingLogField("transcribeMS", .int(transcribeMS))) }
        add("raw", raw)
        add("fixed", fixed)
        add("spelled", spelled)
        add("tagged", tagged)
        add("cleaned", cleaned)
        add("translated", translated)
        add("formatted", formatted)
        return fields
    }
}
