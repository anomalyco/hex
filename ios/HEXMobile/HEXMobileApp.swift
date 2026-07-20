import SwiftUI

@main
struct HEXMobileApp: App {
    @Environment(\.scenePhase) private var scenePhase
    @State private var dictation = DictationController()
    @State private var openedFromKeyboard = false

    var body: some Scene {
        WindowGroup {
            ContentView(
                dictation: dictation,
                openedFromKeyboard: openedFromKeyboard
            )
                .onOpenURL { url in
                    guard let request = KeyboardLaunchRequest(url: url) else { return }
                    openedFromKeyboard = true
                    switch request {
                    case .startSession:
                        dictation.startKeyboardSessionFromKeyboard()
                    case .startRecording(let jobID):
                        dictation.startKeyboardRecordingFromKeyboard(jobID: jobID)
                    }
                }
                .onChange(of: scenePhase) { _, phase in
                    if phase == .background {
                        openedFromKeyboard = false
                    }
                }
        }
    }
}
