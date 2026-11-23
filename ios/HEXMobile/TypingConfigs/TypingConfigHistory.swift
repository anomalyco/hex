import Foundation

/// One typing config Hex was given: the file exactly as it arrived, and what the person did
/// with it. Kept in the App Group's `TypingConfigs/history.json` beside the file.
struct TypingConfigEntry: Codable, Equatable, Identifiable, Sendable {
    enum Source: String, Codable, Sendable {
        case file
        case link
    }

    /// SHA-256 of the file's bytes (`TypingConfigReview.contentHash`): the same file opened
    /// twice is one entry. The file's own `id` isn't checked, so it isn't used.
    var id: String
    /// The file's name without `.mhexconfig`, which the person can change.
    var name: String
    var bytes: Int
    var source: Source
    var addedAt: Date
    /// When it was last made the active config; nil if it never was.
    var appliedAt: Date?
    /// The parts switched off on its review, by their place in the file (`Item.path`), which
    /// is the same for the same bytes.
    var partsOff: [String] = []
}

/// Typing configs' history (Settings › Keyboard › Typing configs): every config Hex was given,
/// unchanged, and which one is active (nil: Hex defaults). Only the app reads it; the keyboard
/// reads only `TypingConfigs/active.json` (`TypingConfigStore`).
@MainActor
final class TypingConfigHistory {
    /// At most this many kept. A new one is never refused: past the limit the oldest that is
    /// neither active nor best so far goes (`removedByAdding`), and the review names it first.
    static let limit = 20

    enum AddResult: Equatable {
        case added(TypingConfigEntry)
        /// The same bytes were kept before: the entry they already have.
        case existing(TypingConfigEntry)
        /// The file couldn't be written.
        case failed
    }

    private struct Index: Codable {
        var version = 1
        var active: String?
        /// The config with the lowest trouble rate over enough words (the regression watch
        /// sets it); kept past the limit like the active one.
        var best: String?
        var entries: [TypingConfigEntry] = []
        /// This phone's exports (`state/phone.json` `exportId`), so a config made from
        /// another iPhone's typing can say so.
        var exportIDs: [String] = []
    }

    static var defaultDirectory: URL? {
        FileManager.default
            .containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)?
            .appendingPathComponent(TypingConfigStore.folder, isDirectory: true)
    }

    let directory: URL?
    private var index = Index()

    init(directory: URL? = TypingConfigHistory.defaultDirectory) {
        self.directory = directory
        load()
    }

    /// Newest first.
    var entries: [TypingConfigEntry] { index.entries.sorted { $0.addedAt > $1.addedAt } }
    var activeID: String? { index.active }
    var active: TypingConfigEntry? { index.active.flatMap { entry($0) } }
    var bestID: String? { index.best }
    var exportIDs: [String] { index.exportIDs }

    /// The entry keeping a config with `hash` would remove: none unless it is new and the
    /// history is full.
    func removedByAdding(_ hash: String) -> TypingConfigEntry? {
        guard entry(hash) == nil, index.entries.count >= Self.limit else { return nil }
        return index.entries.filter { $0.id != index.active && $0.id != index.best }.min { $0.addedAt < $1.addedAt }
    }

    func recordExport(_ id: String) {
        index.exportIDs = Array((index.exportIDs + [id]).suffix(50))
        save()
    }

    func entry(_ id: String) -> TypingConfigEntry? {
        index.entries.first { $0.id == id }
    }

    /// Keeps `data` unchanged as a new entry, unapplied. `hash` is the review's.
    func add(_ data: Data, hash: String, name: String, source: TypingConfigEntry.Source = .file, now: Date = Date()) -> AddResult {
        if let existing = entry(hash) { return .existing(existing) }
        let entry = TypingConfigEntry(id: hash, name: name, bytes: data.count, source: source, addedAt: now)
        guard let url = fileURL(entry.id), (try? data.write(to: url, options: .atomic)) != nil else { return .failed }
        while index.entries.count >= Self.limit, let oldest = removedByAdding(hash) {
            remove(oldest.id)
        }
        index.entries.append(entry)
        save()
        return .added(entry)
    }

    /// The file's bytes, exactly as they arrived.
    func data(_ id: String) -> Data? {
        fileURL(id).flatMap { try? Data(contentsOf: $0) }
    }

    /// Records `id` as the active config (nil: Hex defaults), with the parts switched off on
    /// its review. `TypingConfigActivation` writes what the keyboard reads.
    func setActive(_ id: String?, partsOff: [String]? = nil, now: Date = Date()) {
        if let id, let i = index.entries.firstIndex(where: { $0.id == id }) {
            index.entries[i].appliedAt = now
            if let partsOff { index.entries[i].partsOff = partsOff }
            index.active = id
        } else {
            index.active = nil
        }
        save()
    }

    func rename(_ id: String, to name: String) {
        let cleaned = TypingConfigInbox.displayName(name)
        guard !cleaned.isEmpty, let i = index.entries.firstIndex(where: { $0.id == id }) else { return }
        index.entries[i].name = cleaned
        save()
    }

    /// Deletes one; the caller goes back to Hex defaults first if it was active.
    func delete(_ id: String) {
        remove(id)
        save()
    }

    /// The regression watch's best so far (`TypingConfigWatch.refresh`).
    func setBest(_ id: String?) {
        guard index.best != id else { return }
        index.best = id
        save()
    }

    func setPartsOff(_ id: String, _ partsOff: [String]) {
        guard let i = index.entries.firstIndex(where: { $0.id == id }) else { return }
        index.entries[i].partsOff = partsOff
        save()
    }

    func deleteAll() {
        if let directory { try? FileManager.default.removeItem(at: directory.appendingPathComponent("history", isDirectory: true)) }
        let exports = index.exportIDs
        index = Index()
        index.exportIDs = exports
        save()
    }

    /// A copy named after the entry, to share back out.
    func shareableCopy(_ id: String) -> URL? {
        guard let entry = entry(id), let data = data(id) else { return nil }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("TypingConfigShare-\(id.prefix(16))", isDirectory: true)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appendingPathComponent(entry.name).appendingPathExtension(TypingConfigInbox.fileExtension)
        return (try? data.write(to: url, options: .atomic)) == nil ? nil : url
    }

    // MARK: - Files

    private var indexURL: URL? { directory?.appendingPathComponent("history.json") }

    private func fileURL(_ id: String) -> URL? {
        guard let folder = directory?.appendingPathComponent("history", isDirectory: true) else { return nil }
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder.appendingPathComponent(id).appendingPathExtension(TypingConfigInbox.fileExtension)
    }

    private func remove(_ id: String) {
        if let url = fileURL(id) { try? FileManager.default.removeItem(at: url) }
        index.entries.removeAll { $0.id == id }
        if index.active == id { index.active = nil }
        if index.best == id { index.best = nil }
    }

    private func load() {
        guard let url = indexURL, let data = try? Data(contentsOf: url),
              let decoded = try? Self.decoder.decode(Index.self, from: data) else { return }
        index = decoded
    }

    private func save() {
        guard let directory, let url = indexURL else { return }
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        guard let data = try? Self.encoder.encode(index) else { return }
        try? data.write(to: url, options: .atomic)
    }

    private static var encoder: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }

    private static var decoder: JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }
}

/// Making a config the one the keyboard uses, or going back to Hex defaults: the history's
/// record and `active.json` together. Going back to an older config compiles its file again,
/// on today's keyboard, with the parts it had switched off.
@MainActor
enum TypingConfigActivation {
    enum Failure: Error {
        case nothingToApply
    }

    static func apply(_ entry: TypingConfigEntry, review: TypingConfigReview, partsOff: Set<String>,
                      history: TypingConfigHistory, store: TypingConfigStore = TypingConfigStore()) throws {
        let on = Set(review.items.filter { $0.outcome.applies && !partsOff.contains($0.path) }.map(\.part))
        guard let compiled = review.compiled(switchedOn: on) else { throw Failure.nothingToApply }
        let previous = history.activeID ?? TypingConfigHealth.defaultsKey
        try store.writeActive(compiled)
        history.setActive(entry.id, partsOff: partsOff.sorted())
        // The regression watch compares it with the one in use before.
        TypingConfigHealth(defaults: store.settings.defaults).setActive(entry.id, previous: previous)
    }

    /// One tap back to a config in the history (deletes nothing).
    static func goBack(to entry: TypingConfigEntry, history: TypingConfigHistory, keyboards: [TypingConfigKeyboard] = TypingConfigSettings().keyboards,
                       store: TypingConfigStore = TypingConfigStore()) throws {
        guard let data = history.data(entry.id) else { throw Failure.nothingToApply }
        let review = TypingConfigReview(data: data, keyboards: keyboards)
        try apply(entry, review: review, partsOff: Set(entry.partsOff), history: history, store: store)
    }

    /// The active config compiled again for the keyboards recorded since it was applied (a block
    /// for a keyboard first used afterwards joins then); written only when that changes it.
    static func refreshActive(history: TypingConfigHistory = TypingConfigHistory(), keyboards: [TypingConfigKeyboard] = TypingConfigSettings().keyboards,
                              store: TypingConfigStore = TypingConfigStore()) {
        guard let entry = history.active, let data = history.data(entry.id) else { return }
        let review = TypingConfigReview(data: data, keyboards: keyboards)
        let on = Set(review.items.filter { $0.outcome.applies && !entry.partsOff.contains($0.path) }.map(\.part))
        guard let compiled = review.compiled(switchedOn: on) else { return }
        let current = store.activeURL.flatMap { try? Data(contentsOf: $0) }.flatMap { try? JSONDecoder().decode(TypingConfigFile.self, from: $0) }
        guard current != compiled else { return }
        try? store.writeActive(compiled)
    }

    static func useDefaults(history: TypingConfigHistory, store: TypingConfigStore = TypingConfigStore()) {
        let previous = history.activeID ?? TypingConfigHealth.defaultsKey
        store.removeActive()
        history.setActive(nil)
        TypingConfigHealth(defaults: store.settings.defaults).setActive(nil, previous: previous)
    }
}
