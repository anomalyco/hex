import Foundation

/// The full keyboard's switches in the shared App Group.
///
/// Without Full Access an iPhone may not let the keyboard read the App Group (the simulator
/// does), so every switch's default must work there: the full keyboard, which types
/// without Full Access.
@MainActor
struct FullKeyboardSettings {
    static let launchArgument = "--experiment-full-keyboard"
    static let emojiLaunchArgument = "--experiment-emoji"
    /// Simulator scenes pass `--keyboard dictation-bar` (or `full`) to pick the keyboard.
    static let keyboardLaunchArgument = "--keyboard"

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

    /// Settings › Keyboard: Full (true) or Dictation bar (false). The full keyboard (letters,
    /// numbers, symbols, autocorrect and the Hex key) by default; the dictation bar (space,
    /// delete, return and Dictate) only once it was chosen. Stored as
    /// `experiments.fullKeyboard`, from when the full keyboard was an experiment: unset or
    /// true is the full keyboard and false the dictation bar, so testers who never touched
    /// the experiment get the full keyboard and those who turned it off keep the bar.
    var isEnabled: Bool {
        get { defaults?.object(forKey: Key.fullKeyboard) as? Bool ?? true }
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

    /// Simulator scenes pass `--experiment-full-keyboard` or `--keyboard full|dictation-bar`
    /// to pick the keyboard, and `--experiment-emoji` to turn the emoji experiment on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
        if let index = arguments.firstIndex(of: Self.keyboardLaunchArgument), index + 1 < arguments.count {
            switch arguments[index + 1] {
            case "full": isEnabled = true
            case "dictation-bar": isEnabled = false
            default: break
            }
        }
        if arguments.contains(Self.emojiLaunchArgument) {
            emoji = true
        }
    }
}
