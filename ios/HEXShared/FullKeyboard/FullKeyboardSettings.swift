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
    static let swipeLaunchArgument = "--experiment-swipe"
    /// Simulator scenes pass `--keyboard dictation-bar` (or `full`) to pick the keyboard.
    static let keyboardLaunchArgument = "--keyboard"

    private enum Key {
        static let fullKeyboard = "experiments.fullKeyboard"
        static let autocorrect = "settings.keyboardAutocorrect"
        static let autoCapitalization = "settings.keyboardAutoCapitalization"
        static let spatialAutocorrect = "experiments.spatialAutocorrect"
        static let keyPopups = "settings.keyboardKeyPopups"
        static let haptics = "settings.keyboardHaptics"
        static let suggestSwearWords = "settings.keyboardSuggestSwearWords"
        static let emoji = "experiments.emoji"
        static let swipeTyping = "experiments.swipeTyping"
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

    /// Settings › Keyboard › Auto-capitalization, as Apple's switch of the same name. On by
    /// default: shift turns itself on where the field asks (the start of a field or sentence,
    /// after "." "!" "?"). Off, Hex never adds a capital you didn't type: shift stays off
    /// whatever the field's `autocapitalizationType`, and autocorrect and swipe typing keep
    /// words lowercase ("i", "i'm", "monday") unless they're your own spelling. Dictation
    /// keeps its own Lowercase setting.
    var autoCapitalization: Bool {
        get { defaults?.object(forKey: Key.autoCapitalization) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.autoCapitalization) }
    }

    /// Cautious autocorrect on space. On by default.
    var autocorrect: Bool {
        get { defaults?.object(forKey: Key.autocorrect) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.autocorrect) }
    }

    /// Settings › Keyboard › Spatial autocorrect: autocorrect reads your taps with a language
    /// model (`TypingDecoder`) instead of `UITextChecker`'s one-edit guesses, in English and
    /// Spanish. On by default. Stored as `experiments.spatialAutocorrect`, from when it was an
    /// experiment (off by default) that only its switch ever wrote: unset or true is on and
    /// false off, so testers who never touched it get it and those who turned it off keep it off.
    var spatialAutocorrect: Bool {
        get { defaults?.object(forKey: Key.spatialAutocorrect) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.spatialAutocorrect) }
    }

    /// Suggestions and autocorrect may offer swear words (`OffensiveWords`); slurs never.
    /// Off by default, like Apple's keyboard.
    var suggestSwearWords: Bool {
        get { defaults?.bool(forKey: Key.suggestSwearWords) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.suggestSwearWords) }
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

    /// Experiment: slide through a word's letters to type it, on the English and Spanish
    /// layouts (`SwipeTyping`). Off by default.
    var swipeTyping: Bool {
        get { defaults?.bool(forKey: Key.swipeTyping) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.swipeTyping) }
    }

    /// Simulator scenes pass `--experiment-full-keyboard` or `--keyboard full|dictation-bar`
    /// to pick the keyboard, `--experiment-emoji` to turn the emoji experiment on, and
    /// `--experiment-swipe` for Swipe typing.
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
        if arguments.contains(Self.swipeLaunchArgument) {
            swipeTyping = true
        }
    }
}
