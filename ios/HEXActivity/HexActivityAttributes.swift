import ActivityKit
import AppIntents
import Foundation

nonisolated struct HexDictationActivityAttributes: ActivityAttributes {
    nonisolated enum Stage: String, Codable, Hashable, Sendable {
        case recording
        case transcribing
        case copied
    }

    nonisolated struct ContentState: Codable, Hashable, Sendable {
        var stage: Stage
        var startedAt: Date
        var subtitle: String
    }

    var sessionID: String
}

nonisolated enum HexSystemCommand: String, Codable, Sendable {
    case toggleDictation
    case stopDictation
    case cancelDictation
    case warmSession
}

/// Bridges Live Activity and Control Center intents back into the main app's controller.
actor HexSystemCommandBus {
    static let shared = HexSystemCommandBus()

    private var handler: (@MainActor @Sendable (HexSystemCommand) async -> String?)?

    func register(_ handler: @escaping @MainActor @Sendable (HexSystemCommand) async -> String?) {
        self.handler = handler
    }

    @discardableResult
    func dispatch(_ command: HexSystemCommand) async -> String? {
        guard let handler else { return nil }
        return await handler(command)
    }
}

nonisolated struct StopHexDictationIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "Finish dictation"
    static let description = IntentDescription("Stops recording, transcribes on device, and copies the text.")
    static let isDiscoverable = false

    init() {}

    func perform() async throws -> some IntentResult {
        await HexSystemCommandBus.shared.dispatch(.stopDictation)
        return .result()
    }
}

nonisolated struct CancelHexDictationIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "Cancel dictation"
    static let description = IntentDescription("Discards the active Hex recording.")
    static let isDiscoverable = false

    init() {}

    func perform() async throws -> some IntentResult {
        await HexSystemCommandBus.shared.dispatch(.cancelDictation)
        return .result()
    }
}
