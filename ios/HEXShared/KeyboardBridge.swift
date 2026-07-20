import Foundation

enum KeyboardDictationState: String, Codable {
    case offline
    case ready
    case recording
    case transcribing
    case failed
}

enum KeyboardCommandKind: String, Codable {
    case start
    case stop
    case cancel
}

struct KeyboardCommand: Codable, Equatable {
    let id: String
    let kind: KeyboardCommandKind
    let jobID: String
}

enum KeyboardLaunchRequest: Equatable {
    case startSession
    case startRecording(jobID: String)

    init?(url: URL) {
        guard url.scheme == "hex-dictation", url.host == "keyboard" else { return nil }

        switch url.path {
        case "/start":
            self = .startSession
        case "/record":
            guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
                  let jobID = components.queryItems?.first(where: { $0.name == "job" })?.value,
                  !jobID.isEmpty else { return nil }
            self = .startRecording(jobID: jobID)
        default:
            return nil
        }
    }

    var url: URL {
        switch self {
        case .startSession:
            return URL(string: "hex-dictation://keyboard/start")!
        case .startRecording(let jobID):
            var components = URLComponents()
            components.scheme = "hex-dictation"
            components.host = "keyboard"
            components.path = "/record"
            components.queryItems = [URLQueryItem(name: "job", value: jobID)]
            return components.url!
        }
    }
}

struct KeyboardSnapshot: Codable, Equatable {
    let state: KeyboardDictationState
    let heartbeat: TimeInterval
    let expiresAt: TimeInterval
    let jobID: String?
    let resultID: String?
    let transcript: String?
    let message: String?

    static let offline = KeyboardSnapshot(
        state: .offline,
        heartbeat: 0,
        expiresAt: 0,
        jobID: nil,
        resultID: nil,
        transcript: nil,
        message: nil
    )

    var isAvailable: Bool {
        isAvailable(at: Date().timeIntervalSince1970)
    }

    func isAvailable(at now: TimeInterval) -> Bool {
        return expiresAt > now && now - heartbeat < 3
    }
}

@MainActor
struct KeyboardBridge {
    static let appGroup = "group.com.kitlangton.hex.mobile"

    private enum Key {
        static let command = "keyboard.command"
        static let modelInstalled = "model.installed"
        static let snapshot = "keyboard.snapshot"
    }

    private let defaults: UserDefaults?
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init() {
        defaults = UserDefaults(suiteName: Self.appGroup)
    }

    func send(_ kind: KeyboardCommandKind, jobID: String) {
        let command = KeyboardCommand(
            id: UUID().uuidString,
            kind: kind,
            jobID: jobID
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

    func publish(_ snapshot: KeyboardSnapshot) {
        guard let data = try? encoder.encode(snapshot) else { return }
        defaults?.set(data, forKey: Key.snapshot)
    }

    func snapshot() -> KeyboardSnapshot {
        guard let data = defaults?.data(forKey: Key.snapshot) else { return .offline }
        return (try? decoder.decode(KeyboardSnapshot.self, from: data)) ?? .offline
    }
}
