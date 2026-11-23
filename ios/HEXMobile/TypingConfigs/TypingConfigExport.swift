import Foundation
import UIKit

/// Export for your agent (Settings › Privacy › Typing logs): one zip holding the agent package
/// (`ios/TypingConfig/package`, bundled as `package`), the chosen days' logs and `state/`
/// (`package/reference/state.md`). It holds everything the logs hold, words included, so it
/// is written to a temporary folder and deleted once shared.
enum TypingConfigExport {
    /// The text fields of a log line that hold what was typed or dictated.
    nonisolated static let textKeys: Set<String> = ["inserted", "from", "to", "text", "before", "after",
                                                     "raw", "fixed", "spelled", "tagged", "cleaned", "translated", "formatted"]

    struct Built: Sendable {
        let url: URL
        let bytes: Int
        let seconds: Double
    }

    enum Failure: Error {
        case noPackage
        case zip
    }

    /// Builds the zip for `days`. The state is read on the main actor; copying, the vocabulary's
    /// words and zipping run off it.
    @MainActor
    static func build(days: [TypingLogDay], now: Date = Date()) async throws -> Built {
        let start = Date()
        guard let package = Bundle.main.url(forResource: "package", withExtension: nil) else { throw Failure.noPackage }
        let exportID = "export-\(stamp(now, "yyyyMMdd"))-\(UUID().uuidString.prefix(8).lowercased())"
        let keyboards = TypingConfigSettings().keyboards
        let phone = try JSONSerialization.data(withJSONObject: phoneState(exportID: exportID, keyboards: keyboards), options: [.prettyPrinted, .sortedKeys])
        let language = keyboards.first.flatMap { KeyboardLanguage(rawValue: $0.context.language) }
            ?? LanguageSettings().keyboardLanguages.first ?? .english
        let model = TypingDecoderModel.bundled(language.rawValue)
        let folderName = "hex-typing-export-\(stamp(now, "yyyy-MM-dd"))"
        let work = FileManager.default.temporaryDirectory.appendingPathComponent("TypingConfigExport-\(exportID)", isDirectory: true)
        let root = work.appendingPathComponent(folderName, isDirectory: true)
        let logs = days.map(\.url)

        let words = try await Task.detached(priority: .userInitiated) { () throws -> [String: Int] in
            let fm = FileManager.default
            try? fm.removeItem(at: work)
            try fm.createDirectory(at: work, withIntermediateDirectories: true)
            try fm.copyItem(at: package, to: root)
            let logFolder = root.appendingPathComponent("logs", isDirectory: true)
            try fm.createDirectory(at: logFolder, withIntermediateDirectories: true)
            for url in logs { try fm.copyItem(at: url, to: logFolder.appendingPathComponent(url.lastPathComponent)) }
            let state = root.appendingPathComponent("state", isDirectory: true)
            try fm.createDirectory(at: state, withIntermediateDirectories: true)
            try phone.write(to: state.appendingPathComponent("phone.json"))
            return tokens(in: logs)
        }.value

        // The vocabulary: each word's cost in Hex's model and whether the spell checker takes it.
        let checker = UITextChecker()
        let checkLanguage = language.spellCheckLanguage(available: UITextChecker.availableLanguages)
        var vocabulary: [String: Any] = [:]
        for (n, word) in words.keys.sorted().enumerated() {
            if n % 400 == 0 { await Task.yield() }
            var entry: [String: Any] = ["count": words[word] ?? 0]
            entry["model"] = model.flatMap { m in m.index(of: word).map { (m.unigramCost($0) * 100).rounded() / 100 } } ?? NSNull()
            if let checkLanguage {
                let range = NSRange(location: 0, length: (word as NSString).length)
                entry["dictionary"] = checker.rangeOfMisspelledWord(in: word, range: range, startingAt: 0, wrap: false, language: checkLanguage).location == NSNotFound
            } else {
                entry["dictionary"] = NSNull()
            }
            vocabulary[word] = entry
        }
        let vocabularyFile = try JSONSerialization.data(withJSONObject: ["language": language.rawValue, "words": vocabulary] as [String: Any], options: [.sortedKeys])

        let zipName = folderName + ".zip"
        let zip = try await Task.detached(priority: .userInitiated) { () throws -> URL in
            try vocabularyFile.write(to: root.appendingPathComponent("state/vocabulary.json"))
            let out = work.appendingPathComponent(zipName)
            var copied = false
            var error: NSError?
            // A folder read "for uploading" comes back zipped.
            NSFileCoordinator().coordinate(readingItemAt: root, options: .forUploading, error: &error) { zipped in
                copied = (try? FileManager.default.copyItem(at: zipped, to: out)) != nil
            }
            try? FileManager.default.removeItem(at: root)
            guard copied else { throw Failure.zip }
            return out
        }.value
        TypingConfigHistory().recordExport(exportID)
        let bytes = (try? zip.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
        return Built(url: zip, bytes: bytes, seconds: Date().timeIntervalSince(start))
    }

    /// Deletes an export once it has been shared (or not).
    nonisolated static func discard(_ built: Built) {
        try? FileManager.default.removeItem(at: built.url.deletingLastPathComponent())
    }

    /// Every word-like token in the logs' text, lowercased, with how often it appears.
    nonisolated static func tokens(in logs: [URL]) -> [String: Int] {
        var counts: [String: Int] = [:]
        func add(_ text: String) {
            var word = ""
            func flush() {
                let trimmed = word.trimmingCharacters(in: CharacterSet(charactersIn: "'"))
                if trimmed.count >= 1, trimmed.count <= 40 { counts[trimmed, default: 0] += 1 }
                word = ""
            }
            for ch in text.lowercased() {
                if ch.isLetter || ((ch == "'" || ch == "’") && !word.isEmpty) {
                    word.append(ch == "’" ? "'" : ch)
                } else if !word.isEmpty {
                    flush()
                }
            }
            if !word.isEmpty { flush() }
        }
        for url in logs {
            guard let data = try? Data(contentsOf: url, options: .mappedIfSafe) else { continue }
            for line in data.split(separator: UInt8(ascii: "\n")) {
                guard let object = (try? JSONSerialization.jsonObject(with: Data(line))) as? [String: Any] else { continue }
                for key in textKeys {
                    if let text = object[key] as? String { add(text) }
                }
            }
        }
        return counts
    }

    // MARK: - state/phone.json

    @MainActor
    static func phoneState(exportID: String, keyboards: [TypingConfigKeyboard]) -> [String: Any] {
        let info = Bundle.main.infoDictionary
        let keyboardSettings = FullKeyboardSettings()
        let languages = LanguageSettings()
        let journal = TypingJournal.load(from: FileManager.default
            .containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)?
            .appendingPathComponent(TypingJournal.fileName))
        let learning = TypingLearning.derive(from: journal.corrections)
        let defaults = UserDefaults(suiteName: KeyboardBridge.appGroup)
        let learned = LearnedWordsStore(defaults: defaults).words
        // Learn how I type's averages as the decoder weighs them (TypingDecoder.offsets), in keycaps.
        var offsets: [String: [Double]] = [:]
        for (key, stats) in journal.keys where key.count == 1 && key.first?.isLetter == true && stats.count >= 20 {
            let weight = Double(stats.count) / Double(stats.count + 20)
            offsets[key] = [round2(stats.meanOffset.dx * weight), round2(stats.meanOffset.dy * weight)]
        }
        let adaptive = AdaptiveTouchStatus.current(fullAccessConfirmed: KeyboardBridge().fullAccessSeenAt != nil)
        let history = TypingConfigHistory()
        let active: Any = history.active.map { entry -> [String: Any] in
            let id = history.data(entry.id).flatMap { try? JSONDecoder().decode(TypingConfigFile.self, from: $0) }?.id
            return ["id": id ?? NSNull(), "sha256": entry.id, "name": entry.name]
        } ?? NSNull()
        return [
            "exportId": exportID,
            "formats": TypingConfigExportState.formats,
            "device": TypingLogSessionInfo.deviceModel,
            "system": "\(UIDevice.current.systemName) \(UIDevice.current.systemVersion)",
            "hexVersion": info?["CFBundleShortVersionString"] as? String ?? "",
            "hexBuild": info?["CFBundleVersion"] as? String ?? "",
            "locale": Locale.current.identifier,
            "keyboardLanguages": languages.keyboardLanguages.isEmpty
                ? Array(Set(keyboards.map(\.context.language))).sorted()
                : languages.keyboardLanguages.map(\.rawValue),
            // As session_start.settings in the logs (KeyboardViewController.typingLogSessionInfo).
            "settings": [
                "autocorrect": keyboardSettings.autocorrect,
                "spatialAutocorrect": keyboardSettings.spatialAutocorrect,
                "swipeTyping": keyboardSettings.swipeTyping,
                "emoji": keyboardSettings.emoji,
                "key_popups": keyboardSettings.keyPopups,
                "haptics": keyboardSettings.haptics,
                "learnHowIType": TypingJournalSettings().isEnabled,
                "cleanupTagger": CleanupTaggerSettings().isEnabled,
                "hexKeyPlacement": keyboardSettings.hexKeyPlacement.rawValue,
                "keyboardLayout": keyboards.first?.context.language ?? "",
                "dictationLanguage": languages.dictationLanguage ?? "automatic",
                "numberRow": keyboardSettings.numberRow,
            ] as [String: Any],
            "experiments": [
                "adaptiveTouch": AdaptiveTouchSettings().isEnabled,
                "learnEachLanguageSeparately": TypingJournalSettings().learnsPerLanguage,
                "typingConfigs": TypingConfigSettings().isEnabled,
            ],
            // The keyboard Hex's keys last ran on (TypingConfigSettings.keyboards), and every one
            // they have run on; null when the keyboard hasn't been used with the experiment on.
            "layout": keyboards.first.map(Self.layout) ?? NSNull(),
            "keyboards": keyboards.map { Self.context($0.context) },
            "learnedWords": ["count": learned.count, "known": learned, "complete": learned.count < LearnedWordsStore.maxWords],
            "learnedFixes": ["pairs": learning.typos.sorted { $0.key < $1.key }.map { [$0.key, $0.value] }, "complete": true],
            "neverCorrect": learning.rejected.sorted(),
            "tapOffsets": ["keycaps": offsets],
            "adaptiveTouch": ["inUse": adaptive == .active, "offsets": NSNull()],
            "activeTypingConfig": active,
        ]
    }

    private static func round2(_ v: CGFloat) -> Double { (Double(v) * 1000).rounded() / 1000 }

    /// `layout` as state.md names it, from a recorded keyboard (`TypingConfigExportState.layout`'s
    /// shape, which reads a live layout).
    static func layout(_ keyboard: TypingConfigKeyboard) -> [String: Any] {
        func rect(_ r: CGRect) -> [Double] { [r.minX, r.minY, r.width, r.height].map { (Double($0) * 100).rounded() / 100 } }
        let g = keyboard.geometry
        // The gap between two neighbouring letter keycaps in a row.
        let row = g.keys.filter { $0.row == 0 }.sorted { $0.frame.minX < $1.frame.minX }
        let gap = zip(row, row.dropFirst()).map { $1.frame.minX - $0.frame.maxX }.min() ?? 0
        let keys: [[String: Any]] = g.keys.map { key in
            var k: [String: Any] = ["id": key.id, "row": key.row, "frame": rect(key.frame), "hitFrame": rect(key.hitFrame)]
            if let band = key.gapHitFrame { k["gapHitFrame"] = rect(band) }
            return k
        }
        return [
            "context": context(keyboard.context), "letterWidth": Double(g.letterWidth), "keyHeight": Double(g.keyHeight),
            "gap": (Double(gap) * 100).rounded() / 100, "rowGap": ((keyboard.context.rowPitch - Double(g.keyHeight)) * 100).rounded() / 100,
            "touchArea": rect(g.touchArea), "keys": keys,
        ]
    }

    static func context(_ c: TypingConfigFile.Context) -> [String: Any] {
        ["width": c.width, "orientation": c.orientation, "posture": c.posture, "language": c.language,
         "letters": c.letters, "split": c.split, "hexPlacement": c.hexPlacement, "numberRow": c.numberRow,
         "keyHeight": c.keyHeight, "rowPitch": c.rowPitch, "letterWidth": c.letterWidth, "hitModel": c.hitModel]
    }

    private static func stamp(_ date: Date, _ format: String) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.dateFormat = format
        return formatter.string(from: date)
    }
}
