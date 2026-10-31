import Foundation

nonisolated enum KeyboardDictationState: String, Codable, Sendable {
    case offline
    case ready
    case recording
    case transcribing
    case failed
}

nonisolated enum WarmSessionDuration: String, Codable, CaseIterable, Sendable {
    case off
    case fiveMinutes
    case fifteenMinutes
    case oneHour

    var seconds: TimeInterval? {
        switch self {
        case .off: nil
        case .fiveMinutes: 5 * 60
        case .fifteenMinutes: 15 * 60
        case .oneHour: 60 * 60
        }
    }

    var shortLabel: String {
        switch self {
        case .off: "Off"
        case .fiveMinutes: "5m"
        case .fifteenMinutes: "15m"
        case .oneHour: "1h"
        }
    }

    var title: String {
        switch self {
        case .off: "One shot"
        case .fiveMinutes: "5 minutes"
        case .fifteenMinutes: "15 minutes"
        case .oneHour: "1 hour"
        }
    }
}

/// Curated App Store-safe URL schemes for returning from Hex after a cold-start handoff.
/// Apps without a registered scheme (or when unset) use the bottom-edge swipe fallback.
nonisolated struct HostApp: Identifiable, Equatable, Codable, Sendable {
    let id: String
    let name: String
    let schemeURL: String

    var url: URL? {
        URL(string: schemeURL)
    }
}

nonisolated enum HostAppCatalog {
    static let all: [HostApp] = [
        HostApp(id: "messages", name: "Messages", schemeURL: "messages://"),
        HostApp(id: "slack", name: "Slack", schemeURL: "slack://"),
        HostApp(id: "claude", name: "Claude", schemeURL: "claude://"),
        HostApp(id: "chatgpt", name: "ChatGPT", schemeURL: "chatgpt://"),
        HostApp(id: "notes", name: "Notes", schemeURL: "mobilenotes://"),
        HostApp(id: "discord", name: "Discord", schemeURL: "discord://"),
        HostApp(id: "telegram", name: "Telegram", schemeURL: "tg://"),
        HostApp(id: "whatsapp", name: "WhatsApp", schemeURL: "whatsapp://"),
        HostApp(id: "obsidian", name: "Obsidian", schemeURL: "obsidian://"),
        HostApp(id: "linear", name: "Linear", schemeURL: "linear://"),
        HostApp(id: "gmail", name: "Gmail", schemeURL: "googlegmail://"),
        HostApp(id: "reminders", name: "Reminders", schemeURL: "x-apple-reminderkit://"),
    ]

    static let quickChips: [HostApp] = Array(all.prefix(6))

    static func app(for id: String?) -> HostApp? {
        guard let id, !id.isEmpty else { return nil }
        return all.first { $0.id == id }
    }
}

nonisolated enum KeyboardCommandKind: String, Codable, Sendable {
    case start
    case stop
    case cancel
}

nonisolated struct KeyboardCommand: Codable, Equatable, Sendable {
    let id: String
    let kind: KeyboardCommandKind
    let jobID: String
    let style: DictationStyle
    let sentAt: TimeInterval

    init(
        id: String = UUID().uuidString,
        kind: KeyboardCommandKind,
        jobID: String,
        style: DictationStyle = .polished,
        sentAt: TimeInterval = Date().timeIntervalSince1970
    ) {
        self.id = id
        self.kind = kind
        self.jobID = jobID
        self.style = style
        self.sentAt = sentAt
    }
}

nonisolated enum KeyboardLaunchRequest: Equatable, Sendable {
    /// Any cold-start recording handoff older than 10 seconds warms the session instead of surprise-recording.
    static let handoffWindow: TimeInterval = 10.0

    /// Opens Hex's keyboard setup screen without starting a session.
    case setup
    case startSession(requestedAt: TimeInterval?, returnAppID: String?)
    case startRecording(
        jobID: String,
        requestedAt: TimeInterval?,
        style: DictationStyle?,
        returnAppID: String?
    )

    init?(url: URL) {
        guard url.scheme == "hex-dictation", url.host == "keyboard" else { return nil }
        let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        let query = components?.queryItems ?? []

        func value(_ name: String) -> String? {
            guard let v = query.first(where: { $0.name == name })?.value, !v.isEmpty else {
                return nil
            }
            return v
        }

        let requestedAt = value("ts").flatMap(TimeInterval.init)
        let returnAppID = value("return")
        let style = value("style").flatMap(DictationStyle.init(rawValue:))

        switch url.path {
        case "/setup":
            self = .setup
        case "/start":
            self = .startSession(requestedAt: requestedAt, returnAppID: returnAppID)
        case "/record":
            guard let jobID = value("job") else { return nil }
            self = .startRecording(
                jobID: jobID,
                requestedAt: requestedAt,
                style: style,
                returnAppID: returnAppID
            )
        default:
            return nil
        }
    }

    var requestedAt: TimeInterval? {
        switch self {
        case .setup:
            nil
        case .startSession(let ts, _), .startRecording(_, let ts, _, _):
            ts
        }
    }

    var returnAppID: String? {
        switch self {
        case .setup:
            nil
        case .startSession(_, let appID), .startRecording(_, _, _, let appID):
            appID
        }
    }

    func isHandoffExpired(at now: TimeInterval = Date().timeIntervalSince1970) -> Bool {
        guard let requestedAt else { return false }
        return now - requestedAt > Self.handoffWindow
    }

    var url: URL {
        var components = URLComponents()
        components.scheme = "hex-dictation"
        components.host = "keyboard"

        var items: [URLQueryItem] = []
        switch self {
        case .setup:
            components.path = "/setup"

        case .startSession(let requestedAt, let returnAppID):
            components.path = "/start"
            if let requestedAt {
                items.append(URLQueryItem(name: "ts", value: String(format: "%.3f", requestedAt)))
            }
            if let returnAppID, !returnAppID.isEmpty {
                items.append(URLQueryItem(name: "return", value: returnAppID))
            }

        case .startRecording(let jobID, let requestedAt, let style, let returnAppID):
            components.path = "/record"
            items.append(URLQueryItem(name: "job", value: jobID))
            if let requestedAt {
                items.append(URLQueryItem(name: "ts", value: String(format: "%.3f", requestedAt)))
            }
            if let style {
                items.append(URLQueryItem(name: "style", value: style.rawValue))
            }
            if let returnAppID, !returnAppID.isEmpty {
                items.append(URLQueryItem(name: "return", value: returnAppID))
            }
        }

        if !items.isEmpty {
            components.queryItems = items
        }
        return components.url!
    }
}

nonisolated struct KeyboardSnapshot: Codable, Equatable, Sendable {
    let state: KeyboardDictationState
    let heartbeat: TimeInterval
    let expiresAt: TimeInterval
    let recordingStartedAt: TimeInterval?
    let audioLevel: Float
    let jobID: String?
    let resultID: String?
    let resultExpiresAt: TimeInterval?
    let transcript: String?
    let message: String?

    init(
        state: KeyboardDictationState,
        heartbeat: TimeInterval,
        expiresAt: TimeInterval,
        recordingStartedAt: TimeInterval? = nil,
        audioLevel: Float = 0,
        jobID: String? = nil,
        resultID: String? = nil,
        resultExpiresAt: TimeInterval? = nil,
        transcript: String? = nil,
        message: String? = nil
    ) {
        self.state = state
        self.heartbeat = heartbeat
        self.expiresAt = expiresAt
        self.recordingStartedAt = recordingStartedAt
        self.audioLevel = audioLevel
        self.jobID = jobID
        self.resultID = resultID
        self.resultExpiresAt = resultExpiresAt
        self.transcript = transcript
        self.message = message
    }

    static let offline = KeyboardSnapshot(
        state: .offline,
        heartbeat: 0,
        expiresAt: 0
    )

    var isAvailable: Bool {
        isAvailable(at: Date().timeIntervalSince1970)
    }

    /// True while the background warm session or an active capture/transcription heartbeat is live.
    func isAvailable(at now: TimeInterval) -> Bool {
        let heartbeatFresh = now - heartbeat < 3.0 && heartbeat > 0
        guard heartbeatFresh else { return false }
        if state == .recording || state == .transcribing {
            return true
        }
        return expiresAt > now
    }

    /// Accepted jobs keep their completed transcript deliverable even after a one-shot or expiring session closes.
    func hasDeliverableResult(at now: TimeInterval = Date().timeIntervalSince1970) -> Bool {
        guard let resultID, !resultID.isEmpty,
              let transcript, !transcript.isEmpty else {
            return false
        }
        if let resultExpiresAt {
            return resultExpiresAt > now
        }
        return true
    }

    func recordingElapsed(at now: TimeInterval = Date().timeIntervalSince1970) -> TimeInterval {
        guard state == .recording, let recordingStartedAt else { return 0 }
        return max(0, now - recordingStartedAt)
    }
}

@MainActor
struct KeyboardBridge {
    static let appGroup = "group.com.kitlangton.hex.mobile"

    private enum Key {
        static let command = "keyboard.command"
        static let modelInstalled = "model.installed"
        static let snapshot = "keyboard.snapshot"
        static let warmDuration = "settings.warmDuration"
        static let style = "settings.dictationStyle"
        static let returnAppID = "settings.returnAppID"
        static let allowBluetoothMic = "settings.allowBluetoothMic"
        static let lastInsertedResultID = "keyboard.lastInsertedResultID"
        static let fullAccessSeenAt = "keyboard.fullAccessSeenAt"
    }

    private let defaults: UserDefaults?
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init(defaults: UserDefaults? = UserDefaults(suiteName: Self.appGroup)) {
        self.defaults = defaults
    }

    func send(
        _ kind: KeyboardCommandKind,
        jobID: String,
        style: DictationStyle? = nil,
        sentAt: TimeInterval = Date().timeIntervalSince1970
    ) {
        let command = KeyboardCommand(
            kind: kind,
            jobID: jobID,
            style: style ?? self.style,
            sentAt: sentAt
        )
        guard let data = try? encoder.encode(command) else { return }
        defaults?.set(data, forKey: Key.command)
    }

    func latestCommand() -> KeyboardCommand? {
        guard let data = defaults?.data(forKey: Key.command) else { return nil }
        return try? decoder.decode(KeyboardCommand.self, from: data)
    }

    var isModelInstalled: Bool {
        defaults?.bool(forKey: Key.modelInstalled) ?? false
    }

    func setModelInstalled(_ installed: Bool) {
        defaults?.set(installed, forKey: Key.modelInstalled)
    }

    var warmDuration: WarmSessionDuration {
        get {
            guard let raw = defaults?.string(forKey: Key.warmDuration),
                  let parsed = WarmSessionDuration(rawValue: raw) else {
                return .fifteenMinutes
            }
            return parsed
        }
        nonmutating set {
            defaults?.set(newValue.rawValue, forKey: Key.warmDuration)
        }
    }

    var style: DictationStyle {
        get {
            guard let raw = defaults?.string(forKey: Key.style),
                  let parsed = DictationStyle(rawValue: raw) else {
                return .polished
            }
            return parsed
        }
        nonmutating set {
            defaults?.set(newValue.rawValue, forKey: Key.style)
        }
    }

    var returnAppID: String? {
        get {
            let value = defaults?.string(forKey: Key.returnAppID)
            return (value?.isEmpty == true) ? nil : value
        }
        nonmutating set {
            if let newValue, !newValue.isEmpty {
                defaults?.set(newValue, forKey: Key.returnAppID)
            } else {
                defaults?.removeObject(forKey: Key.returnAppID)
            }
        }
    }

    var returnApp: HostApp? {
        HostAppCatalog.app(for: returnAppID)
    }

    var allowBluetoothMic: Bool {
        get {
            defaults?.bool(forKey: Key.allowBluetoothMic) ?? false
        }
        nonmutating set {
            defaults?.set(newValue, forKey: Key.allowBluetoothMic)
        }
    }

    var lastInsertedResultID: String? {
        get {
            defaults?.string(forKey: Key.lastInsertedResultID)
        }
        nonmutating set {
            defaults?.set(newValue, forKey: Key.lastInsertedResultID)
        }
    }

    /// When the keyboard last ran with Full Access. Without Full Access the keyboard
    /// cannot write to the App Group at all, so only the keyboard sets this.
    var fullAccessSeenAt: TimeInterval? {
        get {
            let value = defaults?.double(forKey: Key.fullAccessSeenAt) ?? 0
            return value > 0 ? value : nil
        }
        nonmutating set {
            if let newValue {
                defaults?.set(newValue, forKey: Key.fullAccessSeenAt)
            } else {
                defaults?.removeObject(forKey: Key.fullAccessSeenAt)
            }
        }
    }

    func publish(_ snapshot: KeyboardSnapshot) {
        guard let data = try? encoder.encode(snapshot) else { return }
        defaults?.set(data, forKey: Key.snapshot)
    }

    func snapshot() -> KeyboardSnapshot {
        guard let data = defaults?.data(forKey: Key.snapshot) else { return .offline }
        return (try? decoder.decode(KeyboardSnapshot.self, from: data)) ?? .offline
    }
}
