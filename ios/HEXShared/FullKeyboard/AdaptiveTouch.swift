import CoreGraphics
import Foundation
import QuartzCore

/// Settings › Experiments › Adaptive touch, off by default. In the App Group so the app and
/// the keyboard both see the switch and the reset; what the keyboard learned is not here.
struct AdaptiveTouchSettings {
    fileprivate enum Key {
        static let enabled = "experiments.adaptiveTouch"
        static let resetAt = "adaptiveTouch.resetAt"
        static let summaryTaps = "adaptiveTouch.summary.taps"
        static let summaryActive = "adaptiveTouch.summary.active"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Key.enabled) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.enabled) }
    }

    /// When Reset touch learning (or Delete what Hex learned) last ran: a keyboard holding
    /// older averages, in memory or on disk, drops them.
    var resetAt: TimeInterval {
        get { defaults?.double(forKey: Key.resetAt) ?? 0 }
        nonmutating set { defaults?.set(newValue, forKey: Key.resetAt) }
    }

    /// What the keyboard last reported, for Settings: the most taps learned on one keyboard
    /// size, and whether personal centres are in use. Counts only.
    var summary: (taps: Int, active: Bool)? {
        get {
            guard let defaults, defaults.object(forKey: Key.summaryTaps) != nil else { return nil }
            return (defaults.integer(forKey: Key.summaryTaps), defaults.bool(forKey: Key.summaryActive))
        }
        nonmutating set {
            if let newValue {
                defaults?.set(newValue.taps, forKey: Key.summaryTaps)
                defaults?.set(newValue.active, forKey: Key.summaryActive)
            } else {
                defaults?.removeObject(forKey: Key.summaryTaps)
                defaults?.removeObject(forKey: Key.summaryActive)
            }
        }
    }

    /// Reset touch learning: every keyboard forgets what it learned, at once if it is running.
    func requestReset(now: TimeInterval = Date().timeIntervalSince1970) {
        resetAt = max(now, resetAt + 0.001)
        summary = nil
    }
}

/// What Settings says under the switch.
enum AdaptiveTouchStatus: Equatable {
    /// Without Full Access the keyboard keeps its averages in memory until it closes.
    case sessionOnly
    case learning(taps: Int)
    case active

    static func current(settings: AdaptiveTouchSettings = AdaptiveTouchSettings(), fullAccessConfirmed: Bool) -> AdaptiveTouchStatus {
        guard fullAccessConfirmed else { return .sessionOnly }
        guard let summary = settings.summary else { return .learning(taps: 0) }
        return summary.active ? .active : .learning(taps: min(summary.taps, AdaptiveTouchModel.activeTaps))
    }
}

/// The letter keys of one letters page, as Adaptive touch reads taps on it: their physical
/// slots (a split layout's G and V twice), frames, rows and pitch. `key` names the geometry
/// coarsely (letters, posture, orientation, split, Hex key side, width), so a new key plane
/// of the same size keeps what was learned: offsets are in keycap widths and heights.
nonisolated struct AdaptiveTouchGeometry: Equatable {
    var key: UInt64
    var letters: [Unicode.Scalar]
    var frames: [CGRect]
    var hitFrames: [CGRect]
    /// Same row and half, for sharing evidence between neighbours.
    var groups: [Int]
    /// Letters on two keys (a split layout's G and V): learned per key, never moved in the decoder.
    var duplicated: [Bool]
    var pitch: CGSize
    var touchTop: CGFloat

    init?(layout: KeyboardLayout) {
        guard layout.layer == .letters else { return nil }
        var letters: [Unicode.Scalar] = []
        var frames: [CGRect] = []
        var hitFrames: [CGRect] = []
        var groups: [Int] = []
        var signature = "\(layout.posture)|\(layout.orientation)|\(layout.isSplit)|\(layout.hexPlacement)|\(Int((layout.width / 10).rounded()))|"
        for key in layout.keys {
            guard let c = key.character, c.count == 1, c.first?.isLetter == true,
                  let scalar = c.lowercased().unicodeScalars.first,
                  letters.count < AdaptiveTouchModel.slotLimit else { continue }
            let right = layout.split.map { key.frame.midX >= $0.middle } ?? false
            letters.append(scalar)
            frames.append(key.frame)
            hitFrames.append(key.hitFrame)
            groups.append(key.row * 2 + (right ? 1 : 0))
            signature.unicodeScalars.append(scalar)
            signature += "\(key.row)\(right ? "r" : "")"
        }
        guard !letters.isEmpty else { return nil }
        var duplicated = [Bool](repeating: false, count: letters.count)
        for i in letters.indices where letters.filter({ $0 == letters[i] }).count > 1 { duplicated[i] = true }
        self.letters = letters
        self.frames = frames
        self.hitFrames = hitFrames
        self.groups = groups
        self.duplicated = duplicated
        key = Self.hash(signature)
        let m = layout.metrics
        pitch = CGSize(width: layout.isSplit ? SplitGeometry.pitch : layout.letterWidth + m.gap, height: m.keyHeight + m.rowGap)
        touchTop = layout.touchAreaFrame.minY
    }

    /// FNV-1a, stable across launches (Swift's `Hasher` is seeded per process).
    static func hash(_ text: String) -> UInt64 {
        var h: UInt64 = 0xCBF2_9CE4_8422_2325
        for byte in text.utf8 {
            h ^= UInt64(byte)
            h = h &* 0x100_0000_01B3
        }
        return h == 0 ? 1 : h
    }

    /// The physical key whose hit rectangle holds `point`.
    func slot(at point: CGPoint) -> Int? {
        hitFrames.firstIndex { $0.contains(point) }
    }

    /// The key for `letter` nearest `point` (one of a split layout's two G keys).
    func slot(for letter: Unicode.Scalar, near point: CGPoint) -> Int? {
        var best: Int?
        var bestDistance = CGFloat.infinity
        for i in letters.indices where letters[i] == letter {
            let d = hypot(frames[i].midX - point.x, frames[i].midY - point.y)
            if d < bestDistance {
                best = i
                bestDistance = d
            }
        }
        return best
    }
}

/// Adaptive touch's averages: fixed-size sums per physical key, in four rotating bins of
/// `binTaps` labelled taps for each context (a keyboard geometry and a grip regime), and the
/// frozen centre offsets the decoder reads. No letters, words, times or tap sequences; the
/// whole thing serializes to `byteCount` bytes.
nonisolated struct AdaptiveTouchModel: Equatable {
    static let contextLimit = 8
    static let slotLimit = 40
    static let binCount = 4
    /// Labelled taps per bin: the newest 600–800 count once all four are full.
    static let binTaps = 200
    /// A candidate forms from this many labelled taps in a context, and is promoted every
    /// `promotionEvery` more.
    static let candidateTaps = 100
    static let promotionEvery = 25
    /// The decoder reads a context's centres only after this many labelled taps over
    /// `activeSessions` keyboard sessions.
    static let activeTaps = 400
    static let activeSessions = 3
    /// Each axis of an offset stays within this many keycaps (about a quarter key pitch), and a
    /// promotion moves it at most `step`.
    static let bound: Float = 0.3
    static let step: Float = 0.05
    /// Prior strengths (in weak-label taps) pulling a key to its row, the row to the whole
    /// keyboard, and the keyboard to the stock centre.
    static let keyPrior: Float = 20
    static let groupPrior: Float = 50
    static let allPrior: Float = 40

    static let fieldsPerBin = 4
    static var statCount: Int { contextLimit * slotLimit * binCount * fieldsPerBin }
    static var snapshotCount: Int { contextLimit * slotLimit * 2 }

    struct Context: Equatable {
        /// `AdaptiveTouchGeometry.key`; 0 for a free slot.
        var geometry: UInt64 = 0
        var regime: UInt8 = 0
        var hasSnapshot = false
        /// The decoder reads this context's snapshot.
        var inUse = false
        /// Labelled taps go to this regime of its geometry.
        var learning = false
        var bin: UInt8 = 0
        var binFill: UInt16 = 0
        var sessions: UInt16 = 0
        var lastUsed: UInt32 = 0
        var accepted: UInt32 = 0
        var sessionStamp: UInt32 = 0
        var sincePromotion: UInt32 = 0
        var promotions: UInt32 = 0
        /// Labelled taps by evidence: kept as typed, a picked suggestion, a retype or undo.
        var weak: UInt32 = 0
        var picked: UInt32 = 0
        var strong: UInt32 = 0

        var isActiveEligible: Bool {
            hasSnapshot && accepted >= UInt32(AdaptiveTouchModel.activeTaps)
                && sessions >= UInt16(AdaptiveTouchModel.activeSessions)
        }
    }

    /// The reset generation this model was learned under (`AdaptiveTouchSettings.resetAt`).
    var generation: TimeInterval = 0
    var clock: UInt32 = 0
    var session: UInt32 = 0
    var contexts = [Context](repeating: Context(), count: contextLimit)
    /// [context][slot][bin][W, Σw², Σw·x, Σw·y], in keycaps.
    var stats = [Float](repeating: 0, count: statCount)
    /// [context][slot][x, y]: the frozen offsets, in keycaps.
    var snapshot = [Float](repeating: 0, count: snapshotCount)

    init() {}

    @inline(__always) static func statIndex(_ context: Int, _ slot: Int, _ bin: Int) -> Int {
        ((context * slotLimit + slot) * binCount + bin) * fieldsPerBin
    }

    func offset(context: Int, slot: Int) -> (x: Float, y: Float) {
        let i = (context * Self.slotLimit + slot) * 2
        return (snapshot[i], snapshot[i + 1])
    }

    func context(geometry: UInt64, regime: UInt8) -> Int? {
        contexts.firstIndex { $0.geometry == geometry && $0.regime == regime }
    }

    /// The slot for a geometry and regime, taking the least recently used one if it is new.
    mutating func ensureContext(geometry: UInt64, regime: UInt8) -> Int {
        clock &+= 1
        if let found = context(geometry: geometry, regime: regime) {
            contexts[found].lastUsed = clock
            return found
        }
        let free = contexts.firstIndex { $0.geometry == 0 }
            ?? contexts.indices.min { contexts[$0].lastUsed < contexts[$1].lastUsed }!
        clear(free)
        contexts[free].geometry = geometry
        contexts[free].regime = regime
        contexts[free].lastUsed = clock
        return free
    }

    mutating func clear(_ c: Int) {
        contexts[c] = Context()
        let start = Self.statIndex(c, 0, 0)
        for i in start..<(start + Self.slotLimit * Self.binCount * Self.fieldsPerBin) { stats[i] = 0 }
        for i in (c * Self.slotLimit * 2)..<((c + 1) * Self.slotLimit * 2) { snapshot[i] = 0 }
    }

    /// One labelled tap, `x` and `y` its offset from the key's centre in keycaps. O(1), but a
    /// full bin rotates (O(keys)).
    mutating func add(context c: Int, slot: Int, x: Float, y: Float, weight w: Float) {
        let bin = Int(contexts[c].bin)
        let i = Self.statIndex(c, slot, bin)
        stats[i] += w
        stats[i + 1] += w * w
        stats[i + 2] += w * x
        stats[i + 3] += w * y
        contexts[c].accepted &+= 1
        contexts[c].sincePromotion &+= 1
        contexts[c].binFill &+= 1
        if contexts[c].binFill >= UInt16(Self.binTaps) {
            let next = (bin + 1) % Self.binCount
            for s in 0..<Self.slotLimit {
                let j = Self.statIndex(c, s, next)
                stats[j] = 0
                stats[j + 1] = 0
                stats[j + 2] = 0
                stats[j + 3] = 0
            }
            contexts[c].bin = UInt8(next)
            contexts[c].binFill = 0
        }
    }

    /// Rebuilds a context's frozen offsets from its bins: each key's mean shrunk toward its
    /// row's (`keyPrior`), the row's toward the keyboard's (`groupPrior`), that toward zero
    /// (`allPrior`); bounded, and at most `step` from the last snapshot.
    mutating func promote(context c: Int, groups: [Int]) {
        let slots = min(groups.count, Self.slotLimit)
        var w = [Float](repeating: 0, count: slots)
        var sx = [Float](repeating: 0, count: slots)
        var sy = [Float](repeating: 0, count: slots)
        var gw: [Int: Float] = [:], gx: [Int: Float] = [:], gy: [Int: Float] = [:]
        var aw: Float = 0, ax: Float = 0, ay: Float = 0
        for s in 0..<slots {
            for b in 0..<Self.binCount {
                let i = Self.statIndex(c, s, b)
                w[s] += stats[i]
                sx[s] += stats[i + 2]
                sy[s] += stats[i + 3]
            }
            gw[groups[s], default: 0] += w[s]
            gx[groups[s], default: 0] += sx[s]
            gy[groups[s], default: 0] += sy[s]
            aw += w[s]
            ax += sx[s]
            ay += sy[s]
        }
        let allX = ax / (aw + Self.allPrior), allY = ay / (aw + Self.allPrior)
        for s in 0..<slots {
            let g = groups[s]
            let rowW = gw[g] ?? 0
            let rowX = ((gx[g] ?? 0) + Self.groupPrior * allX) / (rowW + Self.groupPrior)
            let rowY = ((gy[g] ?? 0) + Self.groupPrior * allY) / (rowW + Self.groupPrior)
            let keyX = (sx[s] + Self.keyPrior * rowX) / (w[s] + Self.keyPrior)
            let keyY = (sy[s] + Self.keyPrior * rowY) / (w[s] + Self.keyPrior)
            let i = (c * Self.slotLimit + s) * 2
            let oldX = snapshot[i], oldY = snapshot[i + 1]
            snapshot[i] = Self.clamp(Self.clamp(keyX, -Self.bound, Self.bound), oldX - Self.step, oldX + Self.step)
            snapshot[i + 1] = Self.clamp(Self.clamp(keyY, -Self.bound, Self.bound), oldY - Self.step, oldY + Self.step)
        }
        contexts[c].hasSnapshot = true
        contexts[c].sincePromotion = 0
        contexts[c].promotions &+= 1
    }

    private static func clamp(_ v: Float, _ lo: Float, _ hi: Float) -> Float { min(hi, max(lo, v)) }

    // MARK: - File

    static let magic: UInt32 = 0x5441_5848 // "HXAT"
    static let version: UInt32 = 1
    static let headerBytes = 32
    static let contextBytes = 64
    /// The serialized size: always this, whatever was learned.
    static var byteCount: Int { headerBytes + contextLimit * contextBytes + 4 * (statCount + snapshotCount) + 4 }

    func serialized() -> Data {
        var out = Data(capacity: Self.byteCount)
        func put<T: FixedWidthInteger>(_ v: T) { withUnsafeBytes(of: v.littleEndian) { out.append(contentsOf: $0) } }
        put(Self.magic)
        put(Self.version)
        put(generation.bitPattern)
        put(clock)
        put(session)
        put(UInt32(Self.contextLimit))
        put(UInt32(Self.slotLimit))
        for c in contexts {
            let start = out.count
            put(c.geometry)
            put(c.regime)
            put(UInt8((c.hasSnapshot ? 1 : 0) | (c.inUse ? 2 : 0) | (c.learning ? 4 : 0)))
            put(c.bin)
            put(UInt8(0))
            put(c.binFill)
            put(c.sessions)
            for v in [c.lastUsed, c.accepted, c.sessionStamp, c.sincePromotion, c.promotions, c.weak, c.picked, c.strong] { put(v) }
            out.append(contentsOf: [UInt8](repeating: 0, count: Self.contextBytes - (out.count - start)))
        }
        for v in stats { put(v.bitPattern) }
        for v in snapshot { put(v.bitPattern) }
        put(Self.checksum(out))
        return out
    }

    /// Nil for anything but a whole, unchanged file of this version.
    init?(data: Data) {
        guard data.count == Self.byteCount else { return nil }
        let bytes = [UInt8](data)
        guard Self.checksum(Data(bytes[0..<(bytes.count - 4)])) == Self.read(UInt32.self, bytes, bytes.count - 4) else { return nil }
        var at = 0
        func get<T: FixedWidthInteger>(_: T.Type) -> T {
            let v = Self.read(T.self, bytes, at)
            at += MemoryLayout<T>.size
            return v
        }
        guard get(UInt32.self) == Self.magic, get(UInt32.self) == Self.version else { return nil }
        generation = TimeInterval(bitPattern: get(UInt64.self))
        clock = get(UInt32.self)
        session = get(UInt32.self)
        guard get(UInt32.self) == UInt32(Self.contextLimit), get(UInt32.self) == UInt32(Self.slotLimit) else { return nil }
        for i in 0..<Self.contextLimit {
            let start = at
            var c = Context()
            c.geometry = get(UInt64.self)
            c.regime = get(UInt8.self)
            let flags = get(UInt8.self)
            c.hasSnapshot = flags & 1 != 0
            c.inUse = flags & 2 != 0
            c.learning = flags & 4 != 0
            c.bin = min(get(UInt8.self), UInt8(Self.binCount - 1))
            _ = get(UInt8.self)
            c.binFill = get(UInt16.self)
            c.sessions = get(UInt16.self)
            c.lastUsed = get(UInt32.self)
            c.accepted = get(UInt32.self)
            c.sessionStamp = get(UInt32.self)
            c.sincePromotion = get(UInt32.self)
            c.promotions = get(UInt32.self)
            c.weak = get(UInt32.self)
            c.picked = get(UInt32.self)
            c.strong = get(UInt32.self)
            at = start + Self.contextBytes
            contexts[i] = c
        }
        for i in 0..<Self.statCount { stats[i] = Float(bitPattern: get(UInt32.self)) }
        for i in 0..<Self.snapshotCount { snapshot[i] = Float(bitPattern: get(UInt32.self)) }
        guard stats.allSatisfy(\.isFinite), snapshot.allSatisfy({ $0.isFinite && abs($0) <= Self.bound }) else { return nil }
    }

    private static func read<T: FixedWidthInteger>(_: T.Type, _ bytes: [UInt8], _ at: Int) -> T {
        var v = T.zero
        withUnsafeMutableBytes(of: &v) { raw in
            for k in 0..<MemoryLayout<T>.size { raw[k] = bytes[at + k] }
        }
        return T(littleEndian: v)
    }

    private static func checksum(_ data: Data) -> UInt32 {
        var h: UInt32 = 0x811C_9DC5
        for byte in data { h = (h ^ UInt32(byte)) &* 0x0100_0193 }
        return h
    }
}

/// Adaptive touch for one keyboard process: the model, the labels it admits
/// (`AdaptiveTouchSpans`), which grip regime fits, and the personal centres the decoder
/// reads. With Full Access the averages live in a protected file in the keyboard's own
/// container, excluded from backup and never in the App Group or its exports, written in
/// batches off the main thread; without, in memory until the keyboard closes.
@MainActor
final class AdaptiveTouch {
    enum Storage: Equatable {
        case file(URL)
        case memory
    }

    /// For replays: `.frozen` reads the centres and learns nothing; `.monitor` also watches
    /// how they fit (and falls back) without adding to the averages.
    enum Mode {
        case learn
        case monitor
        case frozen
    }

    static let fileName = "adaptive-touch.bin"
    /// Grip regimes: the profile in use falls back to the stock centres once it fits recent
    /// taps worse than they do (mean log-likelihood gain per tap and axis, nats, over the last
    /// `monitorWindow` labels): a grip that moves taps sideways but not up or down still
    /// counts. Labels then teach the other regime, and a profile is taken back once it fits
    /// better again on both axes.
    static let monitorWindow = 32
    static let monitorMinimum = 16
    static let disengageAxis: Float = -0.06
    static let disengageGain: Float = -0.1
    static let switchAxis: Float = -0.08
    static let switchGain: Float = -0.12
    static let engageAxis: Float = -0.02
    /// Label weights: a word kept as typed, a picked suggestion, a retype or an undone fix.
    static let weakQuality: Float = 1
    static let pickedQuality: Float = 2
    static let strongQuality: Float = 3
    /// Residuals past this many key pitches are misses, not aims; past `huberPitches` they count less.
    static let missPitches: CGFloat = 1.5
    static let huberPitches: CGFloat = 0.6
    static let saveEvery = 400

    private static var shared: AdaptiveTouch?

    /// The one instance this process uses while the experiment is on; nil while it is off.
    /// `persistent` (Full Access) keeps the averages in the keyboard's container.
    static func sharedIfEnabled(settings: AdaptiveTouchSettings = AdaptiveTouchSettings(), persistent: Bool) -> AdaptiveTouch? {
        guard settings.isEnabled else {
            shared?.save()
            shared = nil
            return nil
        }
        let storage: Storage = persistent ? (defaultURL.map(Storage.file) ?? .memory) : .memory
        if let shared, shared.storage == storage {
            shared.applyResetIfNeeded()
            return shared
        }
        shared?.save()
        let touch = AdaptiveTouch(storage: storage, settings: settings)
        shared = touch
        return touch
    }

    /// The process's instance while the experiment is on (the keyboard saves it when it hides).
    static var current: AdaptiveTouch? { shared }

    static var defaultURL: URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("AdaptiveTouch", isDirectory: true)
            .appendingPathComponent(fileName)
    }

    let storage: Storage
    private let settings: AdaptiveTouchSettings
    private(set) var model: AdaptiveTouchModel
    var mode: Mode = .learn
    /// Called when the centres the decoder should read changed (a promotion, a fallback).
    var onChange: (() -> Void)?
    var clock: () -> TimeInterval = { CACurrentMediaTime() }
    private(set) var version = 0
    private(set) var geometry: AdaptiveTouchGeometry?
    var spans = AdaptiveTouchSpans()
    /// [context][window][x, y]: each recent label's log-likelihood gain of the context's
    /// centres over the stock ones, per axis. Memory only.
    private var gains = [Float](repeating: 0, count: AdaptiveTouchModel.contextLimit * AdaptiveTouch.monitorWindow * 2)
    private var gainCount = [Int](repeating: 0, count: AdaptiveTouchModel.contextLimit)
    private var gainHead = [Int](repeating: 0, count: AdaptiveTouchModel.contextLimit)
    private var unsaved = 0
    private var offsetsCache: (geometry: UInt64, version: Int, offsets: [Character: CGVector])?
    private static let writer = DispatchQueue(label: "anomaly.hex.adaptive-touch.writer", qos: .utility)

    init(storage: Storage, settings: AdaptiveTouchSettings = AdaptiveTouchSettings()) {
        self.storage = storage
        self.settings = settings
        var model = AdaptiveTouchModel()
        let resetAt = settings.resetAt
        if case .file(let url) = storage {
            if let data = try? Data(contentsOf: url), let loaded = AdaptiveTouchModel(data: data), loaded.generation >= resetAt {
                model = loaded
            } else {
                try? FileManager.default.removeItem(at: url)
            }
        }
        model.generation = max(model.generation, resetAt)
        self.model = model
    }

    // MARK: - Decoder

    /// The personal centre offsets (points, by letter) for `layout`'s letter keys, or none
    /// when this geometry has no profile in use: thin evidence, a new size, or a grip the
    /// profile doesn't fit. Also keeps the geometry labels are read against.
    func offsets(for layout: KeyboardLayout) -> [Character: CGVector] {
        guard layout.layer == .letters else { return [:] }
        sync(layout)
        guard let geometry else { return [:] }
        if let cache = offsetsCache, cache.geometry == geometry.key, cache.version == version { return cache.offsets }
        var out: [Character: CGVector] = [:]
        if let c = model.contexts.firstIndex(where: { $0.geometry == geometry.key && $0.inUse && $0.hasSnapshot }) {
            for s in geometry.letters.indices where !geometry.duplicated[s] {
                let o = model.offset(context: c, slot: s)
                guard o.x != 0 || o.y != 0 else { continue }
                out[Character(geometry.letters[s])] = CGVector(
                    dx: CGFloat(o.x) * geometry.frames[s].width,
                    dy: CGFloat(o.y) * geometry.frames[s].height
                )
            }
        }
        offsetsCache = (geometry.key, version, out)
        return out
    }

    /// A new letters page: keeps its geometry, and forgets taps typed on another.
    private func sync(_ layout: KeyboardLayout) {
        let next = AdaptiveTouchGeometry(layout: layout)
        guard next != geometry else { return }
        if next?.key != geometry?.key || next?.frames != geometry?.frames { spans.forget() }
        geometry = next
    }

    /// Whether personal centres are in use for the current geometry.
    var isActive: Bool {
        guard let geometry else { return false }
        return model.contexts.contains { $0.geometry == geometry.key && $0.inUse }
    }

    // MARK: - Sessions and reset

    /// A keyboard session began (the keyboard appeared, or a new field).
    func beginSession() {
        spans.forget()
        model.session &+= 1
        applyResetIfNeeded()
    }

    /// Drops everything when Reset touch learning ran since this was learned.
    func applyResetIfNeeded() {
        let resetAt = settings.resetAt
        guard resetAt > model.generation else { return }
        let hadProfile = model.contexts.contains(where: \.inUse)
        model = AdaptiveTouchModel()
        model.generation = resetAt
        spans.forget()
        for i in gainCount.indices {
            gainCount[i] = 0
            gainHead[i] = 0
        }
        unsaved = 0
        if case .file(let url) = storage { try? FileManager.default.removeItem(at: url) }
        if hadProfile { changed() }
    }

    /// Writes the averages (Full Access) off the main thread, never over a newer reset.
    func save() {
        applyResetIfNeeded()
        guard case .file(let url) = storage, unsaved > 0 else { return }
        unsaved = 0
        let data = model.serialized()
        let generation = model.generation
        settings.summary = (Int(model.contexts.map(\.accepted).max() ?? 0), model.contexts.contains(where: \.inUse))
        Self.writer.async {
            let resetAt = UserDefaults(suiteName: KeyboardBridge.appGroup)?.double(forKey: AdaptiveTouchSettings.Key.resetAt) ?? 0
            guard resetAt <= generation else {
                try? FileManager.default.removeItem(at: url)
                return
            }
            Self.write(data, to: url)
        }
    }

    /// Blocks until queued writes finished (tests).
    static func waitForWrites() {
        writer.sync {}
    }

    nonisolated private static func write(_ data: Data, to url: URL) {
        let folder = url.deletingLastPathComponent()
        var excluded = URLResourceValues()
        excluded.isExcludedFromBackup = true
        try? FileManager.default.createDirectory(
            at: folder, withIntermediateDirectories: true,
            attributes: [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
        )
        var folderURL = folder
        try? folderURL.setResourceValues(excluded)
        guard (try? data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])) != nil else { return }
        var fileURL = url
        try? fileURL.setResourceValues(excluded)
    }

    // MARK: - Labels

    /// One tap with evidence of the letter it meant, on the current geometry: `quality` is
    /// `weakQuality`, `pickedQuality` or `strongQuality`, `point` where the finger went down.
    /// O(1): sums, a bin and the fit monitor; a promotion every `promotionEvery` taps.
    func admit(slot: Int, point: CGPoint, quality: Float) {
        guard mode != .frozen, let geometry, geometry.letters.indices.contains(slot) else { return }
        let frame = geometry.frames[slot]
        guard frame.width > 0, frame.height > 0 else { return }
        let x = (point.x - frame.midX) / frame.width
        let y = (point.y - frame.midY) / frame.height
        let px = (point.x - frame.midX) / geometry.pitch.width
        let py = (point.y - frame.midY) / geometry.pitch.height
        let distance = (px * px + py * py).squareRoot()
        guard x.isFinite, y.isFinite, distance <= Self.missPitches else { return }
        let shrink = Float(min(1, Self.huberPitches / max(0.000_1, distance)))

        // How each profile of this geometry fits the tap, before the tap teaches anything.
        let sx = Float(TypingDecoder.Parameters().sigmaX * geometry.pitch.width / frame.width)
        let sy = Float(TypingDecoder.Parameters().sigmaY * geometry.pitch.height / frame.height)
        let rx = Float(x) * shrink, ry = Float(y) * shrink
        for c in model.contexts.indices where model.contexts[c].geometry == geometry.key && model.contexts[c].hasSnapshot {
            let o = model.offset(context: c, slot: slot)
            pushGain(x: (rx * o.x - 0.5 * o.x * o.x) / (sx * sx), y: (ry * o.y - 0.5 * o.y * o.y) / (sy * sy), context: c)
        }

        if mode == .learn {
            let learning = learningContext(for: geometry.key)
            if model.contexts[learning].sessionStamp != model.session {
                model.contexts[learning].sessionStamp = model.session
                model.contexts[learning].sessions &+= 1
            }
            model.add(context: learning, slot: slot, x: Float(x), y: Float(y), weight: quality * shrink)
            if quality >= Self.strongQuality {
                model.contexts[learning].strong &+= 1
            } else if quality >= Self.pickedQuality {
                model.contexts[learning].picked &+= 1
            } else {
                model.contexts[learning].weak &+= 1
            }
            let ctx = model.contexts[learning]
            if ctx.accepted >= UInt32(AdaptiveTouchModel.candidateTaps), ctx.sincePromotion >= UInt32(AdaptiveTouchModel.promotionEvery) {
                model.promote(context: learning, groups: geometry.groups)
                if model.contexts[learning].inUse { changed() }
            }
            unsaved += 1
            if unsaved >= Self.saveEvery { save() }
        }
        decideRegime(geometry: geometry.key)
    }

    /// The regime of `geometry` labels go to, making its first one.
    private func learningContext(for geometry: UInt64) -> Int {
        if let c = model.contexts.firstIndex(where: { $0.geometry == geometry && $0.learning }) {
            model.clock &+= 1
            model.contexts[c].lastUsed = model.clock
            return c
        }
        let c = model.ensureContext(geometry: geometry, regime: 0)
        model.contexts[c].learning = true
        return c
    }

    /// Falls back to the stock centres when the profile in use fits recent taps worse than
    /// they do; lets labels teach the other regime when the learning one stopped fitting (a
    /// new grip); takes a profile into use once it has the evidence and fits.
    private func decideRegime(geometry: UInt64) {
        let regimes = model.contexts.indices.filter { model.contexts[$0].geometry == geometry }
        var didChange = false
        for c in regimes where model.contexts[c].inUse && gainCount[c] >= Self.monitorMinimum
            && misfits(c, axis: Self.disengageAxis, total: Self.disengageGain) {
            model.contexts[c].inUse = false
            didChange = true
        }
        // The window and its newest quarter agree the learning profile, which had the evidence
        // to be used, stopped fitting. (Before that, a new grip only mixes into what is learned
        // and nothing is in use.)
        if mode == .learn, let learning = regimes.first(where: { model.contexts[$0].learning }),
           model.contexts[learning].isActiveEligible, gainCount[learning] >= Self.monitorMinimum + 8,
           misfits(learning, axis: Self.switchAxis, total: Self.switchGain), recentGain(learning) < 0 {
            let regime: UInt8 = model.contexts[learning].regime == 0 ? 1 : 0
            let next: Int
            if let other = model.context(geometry: geometry, regime: regime) {
                // The other profile fits the newest taps: that grip is back. One that doesn't is
                // a third grip's to learn from scratch.
                if model.contexts[other].hasSnapshot, !(gainCount[other] > 0 && recentGain(other) > 0) {
                    model.clear(other)
                    model.contexts[other].geometry = geometry
                    model.contexts[other].regime = regime
                    resetGains(other)
                }
                next = other
            } else {
                next = model.ensureContext(geometry: geometry, regime: regime)
                resetGains(next)
            }
            model.contexts[learning].learning = false
            model.contexts[next].learning = true
        }
        if !model.contexts.indices.contains(where: { model.contexts[$0].geometry == geometry && model.contexts[$0].inUse }) {
            let ready = model.contexts.indices.filter {
                guard model.contexts[$0].geometry == geometry, model.contexts[$0].isActiveEligible,
                      gainCount[$0] >= Self.monitorMinimum else { return false }
                let g = meanGain($0)
                return g.x + g.y > 0 && min(g.x, g.y) > Self.engageAxis && recentGain($0) > 0
            }
            if let best = ready.max(by: { meanGain($0).x + meanGain($0).y < meanGain($1).x + meanGain($1).y }) {
                model.contexts[best].inUse = true
                if mode == .learn {
                    for c in model.contexts.indices where model.contexts[c].geometry == geometry {
                        model.contexts[c].learning = c == best
                    }
                }
                didChange = true
            }
        }
        if didChange { changed() }
    }

    private func pushGain(x: Float, y: Float, context c: Int) {
        let i = (c * Self.monitorWindow + gainHead[c]) * 2
        gains[i] = x
        gains[i + 1] = y
        gainHead[c] = (gainHead[c] + 1) % Self.monitorWindow
        gainCount[c] = min(Self.monitorWindow, gainCount[c] + 1)
    }

    /// The profile fits worse than the stock centres along one axis, or overall.
    private func misfits(_ c: Int, axis: Float, total: Float) -> Bool {
        let g = meanGain(c)
        return min(g.x, g.y) < axis || g.x + g.y < total
    }

    private func resetGains(_ c: Int) {
        gainCount[c] = 0
        gainHead[c] = 0
    }

    private func meanGain(_ c: Int) -> (x: Float, y: Float) {
        guard gainCount[c] > 0 else { return (0, 0) }
        var x: Float = 0, y: Float = 0
        for k in 0..<gainCount[c] {
            x += gains[(c * Self.monitorWindow + k) * 2]
            y += gains[(c * Self.monitorWindow + k) * 2 + 1]
        }
        return (x / Float(gainCount[c]), y / Float(gainCount[c]))
    }

    /// The newest quarter of the window, both axes: a second, later window that must agree.
    private func recentGain(_ c: Int) -> Float {
        let n = min(gainCount[c], Self.monitorWindow / 4)
        guard n > 0 else { return 0 }
        var sum: Float = 0
        for k in 1...n {
            let i = (c * Self.monitorWindow + (gainHead[c] - k + Self.monitorWindow) % Self.monitorWindow) * 2
            sum += gains[i] + gains[i + 1]
        }
        return sum / Float(n)
    }

    private func changed() {
        version += 1
        onChange?()
    }
}
