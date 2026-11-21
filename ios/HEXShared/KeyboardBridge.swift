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
        case .off: String(localized: "Off")
        case .fiveMinutes: String(localized: "5m")
        case .fifteenMinutes: String(localized: "15m")
        case .oneHour: String(localized: "1h")
        }
    }

    var title: String {
        switch self {
        case .off: String(localized: "One shot")
        case .fiveMinutes: String(localized: "5 minutes")
        case .fifteenMinutes: String(localized: "15 minutes")
        case .oneHour: String(localized: "1 hour")
        }
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
    case startSession(requestedAt: TimeInterval?)
    case startRecording(
        jobID: String,
        requestedAt: TimeInterval?,
        style: DictationStyle?
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
        let style = value("style").flatMap(DictationStyle.init(rawValue:))

        switch url.path {
        case "/setup":
            self = .setup
        case "/start":
            self = .startSession(requestedAt: requestedAt)
        case "/record":
            guard let jobID = value("job") else { return nil }
            self = .startRecording(
                jobID: jobID,
                requestedAt: requestedAt,
                style: style
            )
        default:
            return nil
        }
    }

    var requestedAt: TimeInterval? {
        switch self {
        case .setup:
            nil
        case .startSession(let ts), .startRecording(_, let ts, _):
            ts
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

        case .startSession(let requestedAt):
            components.path = "/start"
            if let requestedAt {
                items.append(URLQueryItem(name: "ts", value: String(format: "%.3f", requestedAt)))
            }

        case .startRecording(let jobID, let requestedAt, let style):
            components.path = "/record"
            items.append(URLQueryItem(name: "job", value: jobID))
            if let requestedAt {
                items.append(URLQueryItem(name: "ts", value: String(format: "%.3f", requestedAt)))
            }
            if let style {
                items.append(URLQueryItem(name: "style", value: style.rawValue))
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
    let loudnessDB: Float?
    let jobID: String?
    let resultID: String?
    let resultExpiresAt: TimeInterval?
    let transcript: String?
    let message: String?
    /// The raw transcript behind `transcript`, before cleanup and formatting, sent only while
    /// Learn how I type is on so the keyboard can tell misheard words from formatting.
    /// Nil when it can't be learned from (translated) or the experiment is off.
    let heard: String?
    /// Record typing logs: what each step of the dictation made of it, sent only while
    /// that experiment is on.
    let stages: DictationStages?

    init(
        state: KeyboardDictationState,
        heartbeat: TimeInterval,
        expiresAt: TimeInterval,
        recordingStartedAt: TimeInterval? = nil,
        audioLevel: Float = 0,
        loudnessDB: Float? = nil,
        jobID: String? = nil,
        resultID: String? = nil,
        resultExpiresAt: TimeInterval? = nil,
        transcript: String? = nil,
        message: String? = nil,
        heard: String? = nil,
        stages: DictationStages? = nil
    ) {
        self.state = state
        self.heartbeat = heartbeat
        self.expiresAt = expiresAt
        self.recordingStartedAt = recordingStartedAt
        self.audioLevel = audioLevel
        self.loudnessDB = loudnessDB
        self.jobID = jobID
        self.resultID = resultID
        self.resultExpiresAt = resultExpiresAt
        self.transcript = transcript
        self.message = message
        self.heard = heard
        self.stages = stages
    }

    var effectiveLoudnessDB: Float {
        loudnessDB ?? DotScopeSimulation.loudness(fromNormalized: audioLevel)
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

/// What a keyboard saw of Full Access, written from inside the keyboard: iOS tells only the
/// keyboard. `launchedAt` is when that keyboard process started. iOS can keep a keyboard
/// process running, so one started earlier may still say what it saw then.
nonisolated struct KeyboardFullAccessReport: Codable, Equatable, Sendable {
    var isOn: Bool
    var at: TimeInterval
    var launchedAt: TimeInterval
}

/// Full Access as Hex can know it: on once a keyboard ran with it, off only when a keyboard
/// said so since Hex last came to the foreground, and unconfirmed otherwise.
nonisolated enum KeyboardFullAccess: Equatable, Sendable {
    case on
    case off
    case unconfirmed

    /// `activeSince` is when Hex last came to the foreground. Full Access changes only in
    /// Settings, outside Hex, so a keyboard's "off" from before then may predate a grant.
    /// An "off" counts over an "on" only when it is newer and from the same or a later
    /// keyboard process: an older process may still say what it saw before the grant.
    static func resolve(
        on: KeyboardFullAccessReport?,
        off: KeyboardFullAccessReport?,
        activeSince: TimeInterval
    ) -> KeyboardFullAccess {
        if let off, on.map({ off.at > $0.at && off.launchedAt >= $0.launchedAt }) ?? true {
            return off.at >= activeSince ? .off : .unconfirmed
        }
        return on == nil ? .unconfirmed : .on
    }
}

@MainActor
struct KeyboardBridge {
    nonisolated static let appGroup = "group.anomaly.hex.mobile"
    /// Posted by the keyboard each time it reports Full Access (`recordFullAccess`).
    nonisolated static let fullAccessDarwinNotification = "anomaly.hex.mobile.keyboard.fullAccess"
    /// The keyboard's Darwin notification, forwarded on the main thread once
    /// `forwardFullAccessReports()` has run in the app.
    nonisolated static let fullAccessReported = Notification.Name("KeyboardBridge.fullAccessReported")

    private enum Key {
        static let command = "keyboard.command"
        static let modelInstalled = "model.installed"
        static let snapshot = "keyboard.snapshot"
        static let warmDuration = "settings.warmDuration"
        static let style = "settings.dictationStyle"
        static let allowBluetoothMic = "settings.allowBluetoothMic"
        static let lastInsertedResultID = "keyboard.lastInsertedResultID"
        static let fullAccessSeenAt = "keyboard.fullAccessSeenAt"
        static let fullAccessOn = "keyboard.fullAccessOn"
        static let fullAccessOff = "keyboard.fullAccessOff"
        static let appMicrophoneAllowed = "app.microphoneAllowed"
        static let lastEmptyDictationAt = "keyboard.lastEmptyDictationAt"
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

    /// Only the keyboard writes these, and a keyboard without Full Access may not be able to
    /// (an iPhone's App Group can refuse it). An "on" also sets `fullAccessSeenAt`, which
    /// builds before these reports read.
    func recordFullAccess(_ report: KeyboardFullAccessReport) {
        guard let data = try? encoder.encode(report) else { return }
        if report.isOn {
            defaults?.set(data, forKey: Key.fullAccessOn)
            defaults?.set(report.at, forKey: Key.fullAccessSeenAt)
        } else {
            defaults?.set(data, forKey: Key.fullAccessOff)
        }
        CFNotificationCenterPostNotification(
            CFNotificationCenterGetDarwinNotifyCenter(),
            CFNotificationName(Self.fullAccessDarwinNotification as CFString),
            nil,
            nil,
            true
        )
    }

    /// The last "on". A keyboard from before these reports left only `fullAccessSeenAt`:
    /// it counts as an "on" from the oldest process.
    var fullAccessOnReport: KeyboardFullAccessReport? {
        if let data = defaults?.data(forKey: Key.fullAccessOn),
           let report = try? decoder.decode(KeyboardFullAccessReport.self, from: data) {
            return report
        }
        return fullAccessSeenAt.map { KeyboardFullAccessReport(isOn: true, at: $0, launchedAt: 0) }
    }

    var fullAccessOffReport: KeyboardFullAccessReport? {
        guard let data = defaults?.data(forKey: Key.fullAccessOff) else { return nil }
        return try? decoder.decode(KeyboardFullAccessReport.self, from: data)
    }

    func fullAccess(activeSince: TimeInterval) -> KeyboardFullAccess {
        KeyboardFullAccess.resolve(on: fullAccessOnReport, off: fullAccessOffReport, activeSince: activeSince)
    }

    /// Forwards the keyboard's Darwin notification to `fullAccessReported`, once per process.
    static func forwardFullAccessReports() {
        _ = fullAccessForwarder
    }

    private static let fullAccessForwarder: Void = {
        CFNotificationCenterAddObserver(
            CFNotificationCenterGetDarwinNotifyCenter(),
            nil,
            { _, _, _, _, _ in
                DispatchQueue.main.async {
                    NotificationCenter.default.post(name: KeyboardBridge.fullAccessReported, object: nil)
                }
            },
            fullAccessDarwinNotification as CFString,
            nil,
            .deliverImmediately
        )
    }()

    /// Whether Hex has the microphone, as the app last read it. Only the app sets this. A
    /// keyboard process keeps the permission it read when it started (iOS doesn't update it
    /// there), so a keyboard opened before Hex was allowed the microphone needs the app's word.
    var appMicrophoneAllowed: Bool {
        get {
            defaults?.bool(forKey: Key.appMicrophoneAllowed) ?? false
        }
        nonmutating set {
            guard newValue != appMicrophoneAllowed else { return }
            defaults?.set(newValue, forKey: Key.appMicrophoneAllowed)
        }
    }

    /// When the keyboard's own dictation last ended with nothing to insert (no speech, or too
    /// short). The keyboard stays quiet about it; onboarding's practice says it heard nothing.
    var lastEmptyDictationAt: TimeInterval? {
        get {
            let value = defaults?.double(forKey: Key.lastEmptyDictationAt) ?? 0
            return value > 0 ? value : nil
        }
        nonmutating set {
            if let newValue {
                defaults?.set(newValue, forKey: Key.lastEmptyDictationAt)
            } else {
                defaults?.removeObject(forKey: Key.lastEmptyDictationAt)
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
