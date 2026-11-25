import Foundation

/// Labs › Two-language dictation, in the shared App Group so in-keyboard dictation
/// reads it too. Off by default. While it is on, Apple Speech reads one recording in the
/// language dictation resolved and in the first other keyboard language whose model is
/// already installed, and keeps the more confident reading for each phrase
/// (`AppleSpeechTwoLanguage`, `TwoLanguageMerger`).
nonisolated struct TwoLanguageDictationSettings {
    static let enabledKey = "experiments.twoLanguageDictation"
    static let launchArgument = "--experiment-two-language-dictation"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Self.enabledKey) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }

    /// Simulator scenes pass `--experiment-two-language-dictation` to turn the experiment on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
    }
}
