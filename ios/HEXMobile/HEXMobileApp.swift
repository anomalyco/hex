import SwiftUI
import UIKit

@main
struct HEXMobileApp: App {
    @Environment(\.scenePhase) private var scenePhase
    @State private var dictation = DictationController.shared
    @State private var openedFromKeyboard = Self.initialOpenedFromKeyboard
    @State private var showSetup = SettingsSnapshotFixture.showsSetupSheet
    @State private var openPage: SettingsPage?
    /// Counts the keyboard's Open Hex taps from the Hex key menu: each one shows Home.
    @State private var homeRequests = 0

    init() {
        FullKeyboardSettings().applyLaunchArguments()
        TypingLogSettings().turnOffOnce()
        TypingLogSettings().applyLaunchArguments()
        HinglishSettings().applyLaunchArguments()
        GreeklishSettings().applyLaunchArguments()
        TwoLanguageDictationSettings().applyLaunchArguments()
        VoiceIsolationSettings().applyLaunchArguments()
        MultilingualParakeetSettings().applyLaunchArguments()
        KeyboardParakeetSettings().applyLaunchArguments()
        SettingsSnapshotFixture.applyDefaults()
        HexInbox.sweep()
    }

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
            } else if let fullKeyboardScene = FullKeyboardSnapshotStage.requestedScene {
                FullKeyboardSnapshotStage(scene: fullKeyboardScene)
            } else if let experimentsScene = ExperimentsSnapshotStage.requestedScene {
                ExperimentsSnapshotStage(scene: experimentsScene)
            } else if let hostedScene = HostedKeyboardConversationStage.requestedScene {
                HostedKeyboardConversationStage(scene: hostedScene, dictation: dictation)
            } else {
                ContentView(
                    dictation: dictation,
                    openedFromKeyboard: $openedFromKeyboard,
                    showSetup: $showSetup,
                    openPage: $openPage,
                    homeRequests: homeRequests
                )
                .onOpenURL { url in
                    // A Hex words file from AirDrop, Files or Open in Hex: its review sheet.
                    if HexInbox.open(url) { return }
                    guard let request = KeyboardLaunchRequest(url: url) else { return }
                    if case .setup = request {
                        openedFromKeyboard = false
                        showSetup = true
                    } else if case .translationSettings = request {
                        openedFromKeyboard = false
                        openPage = .dictation
                    } else if case .home = request {
                        openedFromKeyboard = false
                        showSetup = false
                        homeRequests += 1
                    } else if case .pasteSettings = request {
                        openedFromKeyboard = false
                        openPage = .keyboard
                        if let settingsURL = URL(string: UIApplication.openSettingsURLString) {
                            UIApplication.shared.open(settingsURL)
                        }
                    } else {
                        openedFromKeyboard = true
                    }
                    dictation.handleKeyboardLaunch(request)
                }
                .onChange(of: scenePhase) { _, phase in
                    if phase == .background {
                        openedFromKeyboard = false
                        dictation.appMovedToBackground()
                    } else if phase == .active {
                        dictation.endStaleLiveActivities()
                        dictation.appBecameActive()
                        // Settings › Dictation › Speech model in keyboard: picks its download up.
                        dictation.startKeyboardParakeetDownloadIfWanted()
                    }
                }
            }
        }
    }
}
