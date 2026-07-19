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
                    guard url.scheme == "hex-dictation",
                          url.host == "keyboard",
                          url.path == "/start" else { return }
                    openedFromKeyboard = true
                    dictation.startKeyboardSessionFromKeyboard()
                }
                .onChange(of: scenePhase) { _, phase in
                    if phase == .background {
                        openedFromKeyboard = false
                    }
                }
        }
    }
}
