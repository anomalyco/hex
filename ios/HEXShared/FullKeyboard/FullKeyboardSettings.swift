import Foundation

/// The full keyboard's switches in the shared App Group.
///
/// Without Full Access the keyboard cannot read the App Group, so `isEnabled` reads
/// `false` there and the keyboard keeps today's setup card.
@MainActor
struct FullKeyboardSettings {
    static let launchArgument = "--experiment-full-keyboard"
    static let emojiLaunchArgument = "--experiment-emoji"

    private enum Key {
        static let fullKeyboard = "experiments.fullKeyboard"
        static let autocorrect = "settings.keyboardAutocorrect"
        static let spatialAutocorrect = "experiments.spatialAutocorrect"
        static let keyPopups = "settings.keyboardKeyPopups"
        static let haptics = "settings.keyboardHaptics"
        static let emoji = "experiments.emoji"
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

    /// Experiment: autocorrect reads your taps with a language model (`TypingDecoder`) instead
    /// of `UITextChecker`'s one-edit guesses, in English and Spanish. Off by default.
    var spatialAutocorrect: Bool {
        get { defaults?.bool(forKey: Key.spatialAutocorrect) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.spatialAutocorrect) }
    }

    /// The enlarged letter that pops up over a pressed key. On by default.
    var keyPopups: Bool {
        get { defaults?.object(forKey: Key.keyPopups) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.keyPopups) }
    }

    /// A light tap under your finger for each key press. On by default.
    var haptics: Bool {
        get { defaults?.object(forKey: Key.haptics) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.haptics) }
    }

    /// Experiment: an emoji key whose panel suggests emoji for what you just typed, from
    /// what the words mean and what you use. Off by default.
    var emoji: Bool {
        get { defaults?.bool(forKey: Key.emoji) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.emoji) }
    }

    /// Simulator scenes pass `--experiment-full-keyboard` (and `--experiment-emoji`) to turn
    /// the experiments on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
        if arguments.contains(Self.emojiLaunchArgument) {
            emoji = true
        }
    }
}
