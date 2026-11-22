import Foundation

/// Experiments › Voice isolation, in the shared App Group so in-keyboard dictation
/// reads it too. Off by default. While it is on, the app records in
/// `AVAudioSession.Mode.voiceChat` instead of `.measurement`, so iOS's Mic Mode menu in
/// Control Center offers Voice Isolation while Hex records, and in-keyboard dictation asks
/// WebKit for echo cancellation and noise suppression. Off, both record exactly as before.
nonisolated struct VoiceIsolationSettings {
    static let enabledKey = "experiments.voiceIsolation"
    static let launchArgument = "--experiment-voice-isolation"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Self.enabledKey) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }

    /// Simulator scenes pass `--experiment-voice-isolation` to turn the experiment on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
    }
}
