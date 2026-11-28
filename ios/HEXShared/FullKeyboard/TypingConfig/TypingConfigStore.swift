import Foundation

/// Settings › Experiments › Typing › Typing configs (off by default) and the App Group keys
/// the app and the keyboard share for it (`reference/typing-config.md`, "On the phone").
nonisolated struct TypingConfigSettings {
    enum Keys {
        static let enabled = "experiments.typingConfigs"
        /// Bumped whenever `TypingConfigs/active.json` is written or removed.
        static let generation = "typingConfig.generation"
        /// The keyboards the keys have run on (`TypingConfigKeyboard`), JSON.
        static let keyboards = "typingConfig.keyboards"
    }

    var defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Keys.enabled) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Keys.enabled) }
    }

    var generation: Int { defaults?.integer(forKey: Keys.generation) ?? 0 }

    func bumpGeneration() {
        defaults?.set(generation + 1, forKey: Keys.generation)
    }
}

/// `TypingConfigs/active.json` in the App Group: the compiled config the keyboard reads. The
/// app writes and removes it (`TypingConfigReview.compiled`); the keyboard only reads it, with
/// Full Access, and a missing or unreadable file is no config.
nonisolated struct TypingConfigStore {
    static let folder = "TypingConfigs"
    static let activeFile = "active.json"

    var settings = TypingConfigSettings()
    var directory: URL? = FileManager.default
        .containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)?
        .appendingPathComponent(folder, isDirectory: true)

    var activeURL: URL? { directory?.appendingPathComponent(Self.activeFile) }

    /// Writes `config` as the active one, atomically (a temporary file, then a rename), and
    /// bumps the generation so a running keyboard reloads it.
    func writeActive(_ config: TypingConfigFile) throws {
        guard let directory, let url = activeURL else { throw CocoaError(.fileNoSuchFile) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        try encoder.encode(config).write(to: url, options: [.atomic])
        settings.bumpGeneration()
    }

    /// Hex defaults: no active config.
    func removeActive() {
        if let url = activeURL { try? FileManager.default.removeItem(at: url) }
        settings.bumpGeneration()
    }

    /// The active file's bytes, or nil when the experiment is off, without Full Access, or
    /// when there is none or it is larger than a config may be.
    func activeData(fullAccess: Bool) -> Data? {
        guard fullAccess, settings.isEnabled, let url = activeURL,
              let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize,
              size <= TypingConfigLimits.maxBytes else { return nil }
        return try? Data(contentsOf: url)
    }
}
