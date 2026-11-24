import Foundation

/// An in-keyboard recording the keyboard closed during: the screen locked, or another app came
/// forward. A keyboard extension can't record or run in the background, so the recording stops
/// there and keeps what was said. Its file stays in the keyboard's temporary folder under this
/// record until its text is held for the next keyboard (`KeyboardHeldDictation`): iOS freezes a
/// keyboard seconds after the screen locks and may end it before it has transcribed, and the
/// next keyboard then transcribes the recording instead (`KeyboardLocalDictation`).
nonisolated struct KeyboardInterruptedRecording: Codable, Equatable, Sendable {
    let jobID: String
    /// The recording's file name in the keyboard's temporary folder.
    let fileName: String
    let document: UUID?
    /// The recording's length.
    let seconds: TimeInterval
    /// Whether the keyboard closed while it recorded, rather than while its text was worked out.
    let cutShort: Bool
    let at: TimeInterval

    /// How long a later keyboard still transcribes a recording an earlier one left; after
    /// that it is deleted unheard.
    static let keptFor: TimeInterval = 24 * 60 * 60
    private static let key = "keyboard.interruptedRecording"

    var url: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent(fileName)
    }

    func isExpired(at now: TimeInterval) -> Bool {
        now - at > Self.keptFor
    }

    /// The one recording left, if any: a newer one takes its place.
    static func load(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) -> Self? {
        guard let data = defaults?.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(Self.self, from: data)
    }

    func save(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        if let data = try? JSONEncoder().encode(self) {
            defaults?.set(data, forKey: Self.key)
        }
    }

    /// Forgets `jobID`'s recording and deletes its file; another job's is left alone.
    static func clear(jobID: String, defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        guard let record = load(defaults: defaults), record.jobID == jobID else { return }
        try? FileManager.default.removeItem(at: record.url)
        defaults?.removeObject(forKey: key)
    }

    /// What the keyboard says for a few seconds beside a dictation it offers that the keyboard
    /// closing cut short: where it stopped, and why. A keyboard can't tell the screen locking
    /// from another app coming forward, so it names both.
    static func note(stoppedAfter seconds: TimeInterval) -> (title: String, detail: String) {
        let whole = max(0, Int(seconds))
        let point = String(format: "%d:%02d", whole / 60, whole % 60)
        return (
            String(localized: "Stopped at \(point)"),
            String(localized: "The screen locked or you left the app")
        )
    }
}
