import Foundation

/// Where Hex keeps Parakeet's Core ML files: in the App Group, so the keyboard can load them too
/// (Experiments › Speech model in keyboard). Before, they were in the app's own Application
/// Support, which the keyboard can't read; the app moves them here (`moveLegacyDownloads`),
/// never downloading them again. Each folder is marked complete once every file is on disk,
/// and the keyboard loads only a marked folder.
nonisolated enum SpeechModelStore {
    /// FluidAudio's `Repo.parakeetV2.folderName`: its loader finds the files by this name.
    static let parakeetV2Folder = "parakeet-tdt-0.6b-v2"
    /// `ParakeetUltraModel`'s folder, named for its pinned revision.
    static let parakeetUltraFolder = "parakeet-ultra-coreml-95eaa59"
    static let completeMarker = ".hex-complete"

    /// `Library/Application Support/SpeechModels` in the App Group (where devicectl can list it
    /// too, which a group container's root isn't); the process's own Application Support where
    /// there is no App Group (an unsigned simulator build).
    static var root: URL {
        let base = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)?
            .appendingPathComponent("Library/Application Support", isDirectory: true)
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("SpeechModels", isDirectory: true)
    }

    static var parakeetV2: URL { root.appendingPathComponent(parakeetV2Folder, isDirectory: true) }
    static var parakeetUltra: URL { root.appendingPathComponent(parakeetUltraFolder, isDirectory: true) }

    /// Every file of the model in `directory` is on disk.
    static func isComplete(_ directory: URL) -> Bool {
        FileManager.default.fileExists(atPath: directory.appendingPathComponent(completeMarker).path)
    }

    static func markComplete(_ directory: URL) {
        guard FileManager.default.fileExists(atPath: directory.path) else { return }
        FileManager.default.createFile(atPath: directory.appendingPathComponent(completeMarker).path, contents: Data())
    }

    static func clearComplete(_ directory: URL) {
        try? FileManager.default.removeItem(at: directory.appendingPathComponent(completeMarker))
    }

    /// Creates `root`, out of backups (the models download again).
    static func prepareRoot() {
        var folder = root
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? folder.setResourceValues(values)
    }
}
