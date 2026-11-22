import AppIntents
import Foundation

/// Action Button, Back Tap, Siri, and Shortcuts entry point.
/// Conforms to `AudioRecordingIntent` + `LiveActivityIntent` so iOS permits
/// starting or stopping background microphone dictation with a Live Activity.
nonisolated struct DictateWithHexIntent: AudioRecordingIntent, LiveActivityIntent {
    static let title: LocalizedStringResource = "Dictate with Hex"
    static let description = IntentDescription(
        "Press once to record, again to transcribe on device and copy the text."
    )

    init() {}

    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        let output = await HexSystemCommandBus.shared.dispatch(.toggleDictation) ?? ""
        return .result(value: output)
    }
}

nonisolated struct WarmHexKeyboardIntent: AppIntent {
    static let title: LocalizedStringResource = "Warm Hex Keyboard"
    static let description = IntentDescription(
        "Lets the keyboard dictate without opening Hex."
    )
    @available(iOS 26.0, *)
    static let supportedModes: IntentModes = .foreground

    init() {}

    func perform() async throws -> some IntentResult {
        await HexSystemCommandBus.shared.dispatch(.warmSession)
        return .result()
    }
}

nonisolated struct HexShortcutsProvider: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: DictateWithHexIntent(),
            phrases: [
                "Dictate with \(.applicationName)",
                "Start \(.applicationName) dictation",
            ],
            shortTitle: "Dictate",
            systemImageName: "mic.fill"
        )
        AppShortcut(
            intent: WarmHexKeyboardIntent(),
            phrases: [
                "Warm \(.applicationName) keyboard",
                "Start \(.applicationName) session",
            ],
            shortTitle: "Warm keyboard",
            systemImageName: "keyboard"
        )
    }
}
