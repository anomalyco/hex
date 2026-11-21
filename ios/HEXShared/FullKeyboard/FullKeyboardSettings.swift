import Foundation

/// How hard the keys' haptic tap is (Settings › Keyboard › Strength). Medium is meant to match
/// Apple's keyboard (testers found Hex's, a 0.65 light impact, too weak beside it).
nonisolated enum KeyHapticStrength: String, CaseIterable, Sendable {
    case light
    case medium
    case strong
}

/// The full keyboard's switches in the shared App Group.
///
/// Without Full Access an iPhone may not let the keyboard read the App Group (the simulator
/// does), so every switch's default must work there: the full keyboard, which types
/// without Full Access.
@MainActor
struct FullKeyboardSettings {
    static let launchArgument = "--experiment-full-keyboard"
    static let emojiLaunchArgument = "--experiment-emoji"
    static let emojiLettersLaunchArgument = "--experiment-emoji-letters"
    static let swipeLaunchArgument = "--experiment-swipe"
    static let twoThumbSelectionLaunchArgument = "--experiment-two-thumb-selection"
    /// Simulator scenes pass `--hex-key left` (or `right`) for Settings › Keyboard › Hex key.
    static let hexKeyLaunchArgument = "--hex-key"
    /// Simulator scenes pass `--keyboard dictation-bar` (or `full`) to pick the keyboard.
    static let keyboardLaunchArgument = "--keyboard"

    private enum Key {
        static let fullKeyboard = "experiments.fullKeyboard"
        static let autocorrect = "settings.keyboardAutocorrect"
        static let autoCapitalization = "settings.keyboardAutoCapitalization"
        static let spatialAutocorrect = "experiments.spatialAutocorrect"
        static let germanSpatialAutocorrect = "experiments.germanSpatialAutocorrect"
        static let keyPopups = "settings.keyboardKeyPopups"
        static let haptics = "settings.keyboardHaptics"
        static let hapticStrength = "settings.keyboardHapticStrength"
        static let suggestSwearWords = "settings.keyboardSuggestSwearWords"
        static let emoji = "experiments.emoji"
        static let emojiLettersUnderEmojiKey = "experiments.emojiLettersUnderEmojiKey"
        static let swipeTyping = "experiments.swipeTyping"
        static let twoThumbSelection = "experiments.twoThumbSelection"
        static let hexKeyOnLeft = "settings.keyboardHexKeyLeft"
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

    /// Settings › Experiments › German spatial autocorrect: lets the Deutsch layout read your
    /// taps with the German model Android ships (`TypingDecoder`), as English and Spanish
    /// already do. Off by default, like every experiment: a keyboard without it types German
    /// exactly as before (the dictionary engine, and no word list for the letter odds, the
    /// strip or a bilingual partner). Stored as `experiments.germanSpatialAutocorrect`.
    var germanSpatialAutocorrect: Bool {
        get { defaults?.object(forKey: Key.germanSpatialAutocorrect) as? Bool ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.germanSpatialAutocorrect) }
    }

    /// The typing model the keyboard may read `language`'s taps with: its bundled model, with
    /// German's only while both Spatial autocorrect and the German experiment are on. English
    /// and Spanish never wait on the experiment. The keyboard asks this for the word list, the
    /// decoder and a bilingual partner, so one gate keeps a German model out of everything
    /// until it is chosen; `TypingDecoderModel.bundled` still loads any bundled file for tests.
    func typingModel(for language: KeyboardLanguage) -> TypingDecoderModel? {
        if language == .german, !(spatialAutocorrect && germanSpatialAutocorrect) { return nil }
        return TypingDecoderModel.bundled(language.rawValue)
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

    /// Settings › Keyboard › Strength, under Haptics: how hard each key's tap is. Medium, the
    /// default, is meant to feel like Apple's keyboard.
    var hapticStrength: KeyHapticStrength {
        get { defaults?.string(forKey: Key.hapticStrength).flatMap(KeyHapticStrength.init(rawValue:)) ?? .medium }
        nonmutating set { defaults?.set(newValue.rawValue, forKey: Key.hapticStrength) }
    }

    /// Settings › Keyboard › Emoji: an emoji key whose panel suggests emoji for what you just
    /// typed, from what the words mean and what you use. On by default. Stored as
    /// `experiments.emoji`, from when it was an experiment (off by default) that only its
    /// switch ever wrote: unset or true is on and false off, so testers who turned it off
    /// keep it off.
    var emoji: Bool {
        get { defaults?.object(forKey: Key.emoji) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.emoji) }
    }

    /// Settings › Experiments › ABC under the emoji key: the emoji panel's ABC sits where the
    /// emoji key that opened it was, so switching back to the letters needs no thumb move.
    /// Off by default; a panel with no emoji key behind it (the dictation bar's, a standalone
    /// panel) keeps ABC at the bar's leading end. Stored as
    /// `experiments.emojiLettersUnderEmojiKey`.
    var emojiLettersUnderEmojiKey: Bool {
        get { defaults?.object(forKey: Key.emojiLettersUnderEmojiKey) as? Bool ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.emojiLettersUnderEmojiKey) }
    }

    /// Settings › Experiments › Two-thumb selection: while the space bar is the trackpad, a
    /// second thumb selects — each thumb moves one end, shown in the strip, with Delete, Cut,
    /// Copy, Dictate and Cancel once both lift. Off by default. Stored as
    /// `experiments.twoThumbSelection`; off, a second finger is ignored as it always was.
    var twoThumbSelection: Bool {
        get { defaults?.object(forKey: Key.twoThumbSelection) as? Bool ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.twoThumbSelection) }
    }

    /// Settings › Keyboard › Swipe typing: slide through a word's letters to type it, on the
    /// English and Spanish layouts (`SwipeTyping`). On by default. Stored as
    /// `experiments.swipeTyping`, from when it was an experiment (off by default): unset or
    /// true is on and false off, so testers who turned it off keep it off.
    var swipeTyping: Bool {
        get { defaults?.object(forKey: Key.swipeTyping) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.swipeTyping) }
    }

    /// Settings › Keyboard › Hex key: Right (false, the default) or Left. Left puts the Hex key
    /// at the strip's leading end, for people who type with their left thumb; everything
    /// tied to it (the recording bar's ✕ and ✓, its menu, the Full Access hint) follows.
    var hexKeyOnLeft: Bool {
        get { defaults?.bool(forKey: Key.hexKeyOnLeft) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.hexKeyOnLeft) }
    }

    /// Where the full keyboard's Hex key sits for `hexKeyOnLeft`.
    var hexKeyPlacement: HexKeyPlacement {
        hexKeyOnLeft ? .stripLeading : .stripTrailing
    }

    /// Simulator scenes pass `--experiment-full-keyboard` or `--keyboard full|dictation-bar`
    /// to pick the keyboard, `--experiment-emoji` to turn Emoji on, `--experiment-swipe` for
    /// Swipe typing (both on by default; the arguments override a stored off),
    /// `--experiment-emoji-letters` for ABC under the emoji key, and
    /// `--hex-key left|right` for the Hex key's side.
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
        if arguments.contains(Self.emojiLettersLaunchArgument) {
            emojiLettersUnderEmojiKey = true
        }
        if arguments.contains(Self.swipeLaunchArgument) {
            swipeTyping = true
        }
        if arguments.contains(Self.twoThumbSelectionLaunchArgument) {
            twoThumbSelection = true
        }
        if let index = arguments.firstIndex(of: Self.hexKeyLaunchArgument), index + 1 < arguments.count {
            switch arguments[index + 1] {
            case "left": hexKeyOnLeft = true
            case "right": hexKeyOnLeft = false
            default: break
            }
        }
    }
}
