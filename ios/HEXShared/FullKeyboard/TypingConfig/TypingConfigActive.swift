import CoreGraphics
import Foundation

/// The typing config the keyboard is using: `TypingConfigs/active.json` read once through
/// `TypingConfigReview`, its words, and its key positions for each layout the keys draw.
@MainActor
final class TypingConfigActive {
    let data: Data
    /// The words (they don't depend on the keyboard).
    let words: TypingConfigReview.Applied
    /// Whether any key block has zones: Adaptive touch then records no taps.
    let hasZones: Bool
    private var cache: (layout: KeyboardLayout, applied: TypingConfigReview.Applied)?

    init?(data: Data) {
        let review = TypingConfigReview(data: data, keyboard: KeyboardLayout(width: 402), checksEvidence: false)
        guard review.refused == nil, let file = review.file else { return nil }
        var words = review.applied
        words.blocks = []
        self.data = data
        self.words = words
        hasZones = (file.keys ?? []).contains { !($0.value?.zones ?? []).isEmpty }
    }

    /// Aim and zones for `layout` (letters only), as the review applies them there.
    func keys(on layout: KeyboardLayout) -> TypingConfigReview.Applied {
        guard layout.layer == .letters else { return TypingConfigReview.Applied() }
        if let cache, cache.layout == layout { return cache.applied }
        var applied = TypingConfigReview(data: data, keyboard: layout, checksEvidence: false).applied
        applied.keep = []
        applied.neverCorrectTo = []
        applied.fixes = []
        applied.dictation = []
        applied.blocks = applied.blocks.filter { $0.keyboard == 0 }
        cache = (layout, applied)
        return applied
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
