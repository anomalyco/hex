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
    /// At most this many kept. Past it the oldest never applied goes first; one that was
    /// applied goes only when the person deletes it.
    static let limit = 20

    enum AddResult: Equatable {
        case added(TypingConfigEntry)
        /// The same bytes were kept before: the entry they already have.
        case existing(TypingConfigEntry)
        /// `limit` entries, every one of them applied at some point.
        case full
    }

    private struct Index: Codable {
        var version = 1
        var active: String?
        var entries: [TypingConfigEntry] = []
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
    var isFull: Bool { index.entries.count >= Self.limit && !index.entries.contains { $0.appliedAt == nil && $0.id != index.active } }

    func entry(_ id: String) -> TypingConfigEntry? {
        index.entries.first { $0.id == id }
    }

    /// Keeps `data` unchanged as a new entry, unapplied. `hash` is the review's.
    func add(_ data: Data, hash: String, name: String, source: TypingConfigEntry.Source = .file, now: Date = Date()) -> AddResult {
        if let existing = entry(hash) { return .existing(existing) }
        if index.entries.count >= Self.limit {
            let unapplied = index.entries.filter { $0.appliedAt == nil && $0.id != index.active }
            guard let oldest = unapplied.min(by: { $0.addedAt < $1.addedAt }) else { return .full }
            remove(oldest.id)
        }
        let entry = TypingConfigEntry(id: hash, name: name, bytes: data.count, source: source, addedAt: now)
        guard let url = fileURL(entry.id), (try? data.write(to: url, options: .atomic)) != nil else { return .full }
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

    func deleteAll() {
        if let directory { try? FileManager.default.removeItem(at: directory.appendingPathComponent("history", isDirectory: true)) }
        index = Index()
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
        try store.writeActive(compiled)
        history.setActive(entry.id, partsOff: partsOff.sorted())
    }

    /// One tap back to a config in the history (deletes nothing).
    static func goBack(to entry: TypingConfigEntry, history: TypingConfigHistory, keyboard: KeyboardLayout = TypingConfigKeyboard.current()) throws {
        guard let data = history.data(entry.id) else { throw Failure.nothingToApply }
        let review = TypingConfigReview(data: data, keyboard: keyboard)
        try apply(entry, review: review, partsOff: Set(entry.partsOff), history: history)
    }

    static func useDefaults(history: TypingConfigHistory, store: TypingConfigStore = TypingConfigStore()) {
        store.removeActive()
        history.setActive(nil)
    }
}
