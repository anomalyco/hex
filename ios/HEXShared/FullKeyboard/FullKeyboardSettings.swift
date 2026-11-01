import Foundation

/// The full keyboard's switches in the shared App Group.
///
/// Without Full Access the keyboard cannot read the App Group, so `isEnabled` reads
/// `false` there and the keyboard keeps today's setup card.
@MainActor
struct FullKeyboardSettings {
    static let launchArgument = "--experiment-full-keyboard"

    private enum Key {
        static let fullKeyboard = "experiments.fullKeyboard"
        static let autocorrect = "settings.keyboardAutocorrect"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Experiment: Hex shows a full QWERTY keyboard instead of the dictation bar. Off by default.
    var isEnabled: Bool {
        get { defaults?.bool(forKey: Key.fullKeyboard) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.fullKeyboard) }
    }

    /// Cautious autocorrect on space. On by default.
    var autocorrect: Bool {
        get { defaults?.object(forKey: Key.autocorrect) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.autocorrect) }
    }

    /// Simulator scenes pass `--experiment-full-keyboard` to turn the experiment on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
    }
}
