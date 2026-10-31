import SwiftUI

@main
struct HEXMobileApp: App {
    @Environment(\.scenePhase) private var scenePhase
    @State private var dictation = DictationController.shared
    @State private var openedFromKeyboard = Self.initialOpenedFromKeyboard
    @State private var showSetup = false

    private static var initialOpenedFromKeyboard: Bool {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else {
            return false
        }
        return args[idx + 1].hasPrefix("handoff-")
    }

    var body: some Scene {
        WindowGroup {
            if ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil {
                Color.clear
            } else if let keyboardScene = KeyboardSnapshotStage.requestedScene {
                KeyboardSnapshotStage(scene: keyboardScene)
            } else {
                ContentView(
                    dictation: dictation,
                    openedFromKeyboard: $openedFromKeyboard,
                    showSetup: $showSetup
                )
                .onOpenURL { url in
                    guard let request = KeyboardLaunchRequest(url: url) else { return }
                    if case .setup = request {
                        openedFromKeyboard = false
                        showSetup = true
                    } else {
                        openedFromKeyboard = true
                    }
                    dictation.handleKeyboardLaunch(request)
                }
                .onChange(of: scenePhase) { _, phase in
                    if phase == .background {
                        dictation.noteSceneDidEnterBackground()
                        openedFromKeyboard = false
                    }
                }
            }
        }
    }
}
