import Foundation

/// The Apple Intelligence cleanup experiment's switch in the shared App Group.
nonisolated struct AppleIntelligenceCleanupSettings {
    private enum Key {
        static let cleanup = "experiments.appleIntelligenceCleanup"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Experiment: one on-device pass fixes misheard words after dictation. Off by default.
    var isEnabled: Bool {
        get { defaults?.bool(forKey: Key.cleanup) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.cleanup) }
    }
}
