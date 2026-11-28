import CoreGraphics
import Foundation

/// The typing config the keyboard is using: `TypingConfigs/active.json`, read and compiled once
/// per `typingConfig.generation` (`current(fullAccess:)`) into what the keys use, and kept for
/// the life of the process: its words, and each key block's keyboard, aim and zones as plain
/// values read and clamped as `TypingConfigReview` reads them. A layout only picks its block.
@MainActor
final class TypingConfigActive {
    /// One key block as the keys use it.
    struct Block: Equatable, Sendable {
        var context: TypingConfigFile.Context
        /// By letter, in keycaps.
        var aim: [Character: CGVector]
        var zones: [KeyboardZone]
    }

    /// The words (they don't depend on the keyboard).
    let words: TypingConfigReview.Applied
    /// The key blocks a reader looks at (`TypingConfigLimits.maxContexts`), in the file's order.
    let blocks: [Block]
    /// Whether any key block has zones: Adaptive touch then records no taps.
    let hasZones: Bool
    private var live: (context: TypingConfigFile.Context, block: Block?)?
    private var zoneMap: (geometry: KeyboardZoneGeometry, zones: [KeyboardZone], map: KeyboardZoneMap)?

    init?(data: Data) {
        let review = TypingConfigReview(data: data, keyboards: [], checksEvidence: false)
        guard review.refused == nil, let file = review.file else { return nil }
        var words = review.applied
        words.blocks = []
        self.words = words
        blocks = (file.keys ?? []).prefix(TypingConfigLimits.maxContexts).compactMap { entry in
            entry.value.map { block in
                var aim: [Character: CGVector] = [:]
                for (letter, value) in block.aim ?? [:] {
                    guard let value = value.value, let to = TypingConfigReview.aim(letter: letter, value), let c = letter.first else { continue }
                    aim[c] = CGVector(dx: to[0], dy: to[1])
                }
                return Block(context: block.context, aim: aim, zones: (block.zones ?? []).compactMap { $0.value.flatMap(TypingConfigReview.zone) })
            }
        }
        hasZones = blocks.contains { !$0.zones.isEmpty }
    }

    /// The block for `layout` (letters only): the first made for its keyboard, as the review
    /// applies them, with no zones when Hex's edges changed since (`needsCheckingAgain`).
    func keys(on layout: KeyboardLayout) -> Block? {
        guard layout.layer == .letters else { return nil }
        let context = TypingConfigFile.Context(layout: layout)
        if let live, live.context == context { return live.block }
        var block = blocks.first { $0.context.differences(from: context).isEmpty }
        if block?.context.hitModel != KeyboardZone.hitModel { block?.zones = [] }
        live = (context, block)
        return block
    }

    /// `zones` compiled against `layout`'s keys, kept while the keys' geometry stays the same.
    func zoneMap(_ zones: [KeyboardZone], on layout: KeyboardLayout) -> KeyboardZoneMap {
        let geometry = KeyboardZoneGeometry(layout: layout)
        if let zoneMap, zoneMap.geometry == geometry, zoneMap.zones == zones { return zoneMap.map }
        let map = geometry.zoneMap(zones)
        zoneMap = (geometry, zones, map)
        return map
    }

    private static var loaded: (generation: Int, data: Data?, config: TypingConfigActive?)?

    /// Whether the keyboard records the keyboards it runs on (the experiment on, with Full Access).
    static var recordsKeyboards = false
    private static var lastRecorded: TypingConfigKeyboard?
    private static var pending: TypingConfigKeyboard?

    /// Records a letters layout the keys run on, for the app's review (`TypingConfigSettings.keyboards`),
    /// once it has been the live one for a second: the layouts the keyboard passes through while
    /// it sizes itself (the engine's default width, a first pass at another height) never count.
    static func record(_ layout: KeyboardLayout) {
        guard recordsKeyboards, layout.layer == .letters else { return }
        let keyboard = TypingConfigKeyboard(layout: layout)
        guard keyboard != lastRecorded else {
            pending = nil
            return
        }
        pending = keyboard
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(1))
            guard pending == keyboard else { return }
            pending = nil
            lastRecorded = keyboard
            TypingConfigSettings().record(keyboard)
        }
    }

    /// The active config, read again only when the app changed it (`typingConfig.generation`);
    /// nil when the experiment is off, without Full Access, or when the file is missing or bad.
    static func current(fullAccess: Bool, store: TypingConfigStore = TypingConfigStore()) -> TypingConfigActive? {
        recordsKeyboards = fullAccess && store.settings.isEnabled
        guard fullAccess, store.settings.isEnabled else { return nil }
        let generation = store.settings.generation
        if let loaded, loaded.generation == generation { return loaded.config }
        let data = store.activeData(fullAccess: fullAccess)
        if let loaded, loaded.data == data {
            self.loaded = (generation, data, loaded.config)
            return loaded.config
        }
        let config = data.flatMap { TypingConfigActive(data: $0) }
        loaded = (generation, data, config)
        return config
    }

    /// The active config's dictation fixes, for dictation in the keyboard.
    static func dictationCorrections(fullAccess: Bool) -> [DictationCorrection] {
        current(fullAccess: fullAccess)?.words.dictation ?? []
    }
}
