import FluidAudio
import Foundation

extension SpeechModelStore {
    /// Where Parakeet V2 was before it moved to the App Group: FluidAudio's own cache folder.
    nonisolated static var legacyParakeetV2: URL {
        AsrModels.defaultCacheDirectory(for: .v2)
    }

    /// Moves Parakeet's files from the app's Application Support into the App Group, once, at
    /// launch before anything reads or writes them: a rename on the same volume, so nothing
    /// downloads again, `.partial` files included (their downloads resume where they were). A
    /// folder already in the App Group wins unless only the old one is complete.
    nonisolated static func moveLegacyDownloads(fileManager: FileManager = .default) {
        move(
            from: legacyParakeetV2,
            to: ParakeetTranscriber.cacheDirectory,
            isComplete: { AsrModels.modelsExist(at: $0, version: .v2) },
            fileManager: fileManager
        )
        move(
            from: ParakeetUltraModel.legacyDirectory,
            to: ParakeetUltraModel.directory,
            isComplete: { ParakeetUltraModel.isDownloaded(in: $0) },
            fileManager: fileManager
        )
        // Ultra's files are each checked against their digest as they land, so a complete
        // folder from before the marker is complete.
        if ParakeetUltraModel.isDownloaded(), !isComplete(ParakeetUltraModel.directory) {
            markComplete(ParakeetUltraModel.directory)
        }
    }

    nonisolated static func move(
        from old: URL,
        to new: URL,
        isComplete: (URL) -> Bool,
        fileManager: FileManager
    ) {
        guard old.standardizedFileURL != new.standardizedFileURL,
              fileManager.fileExists(atPath: old.path) else { return }
        if fileManager.fileExists(atPath: new.path) {
            guard !isComplete(new), isComplete(old) else {
                try? fileManager.removeItem(at: old)
                return
            }
            try? fileManager.removeItem(at: new)
        }
        prepareRoot()
        try? fileManager.createDirectory(at: new.deletingLastPathComponent(), withIntermediateDirectories: true)
        do {
            try fileManager.moveItem(at: old, to: new)
            print("speech model moved to the App Group: \(new.lastPathComponent)")
        } catch {
            print("speech model didn't move to the App Group (\(new.lastPathComponent)): \(error)")
        }
    }
}
