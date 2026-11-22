import Foundation

/// How hard the keys' haptic tap is (Settings › Keyboard › Strength). Medium is meant to match
/// Apple's keyboard (testers found Hex's, a 0.65 light impact, too weak beside it).
nonisolated enum KeyHapticStrength: String, CaseIterable, Sendable {
    case light
    case medium
    case strong
}

/// Settings › Keyboard › Auto-start, while the dictation bar is chosen: whether the bar starts
/// dictating by itself as it shows, so Dictate needs no tap.
nonisolated enum DictationBarAutoStart: String, CaseIterable, Sendable {
    case never
    /// Only when you switch to Hex from another keyboard in the same field
    /// (`KeyboardSwitchGuess`), not when a field opens with Hex already chosen.
    case whenSwitching
    case always
}

/// Settings › Experiments › Korean keyboard (두벌식), read where the pure language helpers need
/// it before a keyboard exists (Settings' Languages list, Automatic). Stored as
/// `experiments.koreanKeyboard`, off by default like every experiment.
nonisolated enum KoreanKeyboardExperiment {
    static let key = "experiments.koreanKeyboard"

    static var isEnabled: Bool {
        UserDefaults(suiteName: KeyboardBridge.appGroup)?.bool(forKey: key) ?? false
    }

    /// The flag in a store a caller already owns (LanguageSettings' injected defaults), so a
    /// test's own suite decides without touching the App Group.
    static func isEnabled(in defaults: UserDefaults?) -> Bool {
        guard let defaults else { return isEnabled }
        return defaults.object(forKey: key) as? Bool ?? false
    }
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
    /// Simulator scenes pass `--number-row on` (or `off`) for Settings › Keyboard › Number row.
    static let numberRowLaunchArgument = "--number-row"
    /// Simulator scenes pass `--join-unfolded-keyboard on` (or `off`) for Settings ›
    /// Experiments › Join the keyboard when unfolded.
    static let joinUnfoldedKeyboardLaunchArgument = "--join-unfolded-keyboard"

    private enum Key {
        static let fullKeyboard = "experiments.fullKeyboard"
        static let autocorrect = "settings.keyboardAutocorrect"
        static let autoCapitalization = "settings.keyboardAutoCapitalization"
        static let spatialAutocorrect = "experiments.spatialAutocorrect"
        static let germanSpatialAutocorrect = "experiments.germanSpatialAutocorrect"
        static let koreanKeyboard = KoreanKeyboardExperiment.key
        static let keyPopups = "settings.keyboardKeyPopups"
        static let haptics = "settings.keyboardHaptics"
        static let hapticStrength = "settings.keyboardHapticStrength"
        static let suggestSwearWords = "settings.keyboardSuggestSwearWords"
        static let emoji = "experiments.emoji"
        static let emojiLettersUnderEmojiKey = "experiments.emojiLettersUnderEmojiKey"
        static let swipeTyping = "experiments.swipeTyping"
        static let swipeDeleteLetterByLetter = "experiments.swipeDeleteLetterByLetter"
        static let twoThumbSelection = "experiments.twoThumbSelection"
        static let hideWordSuggestions = "experiments.hideWordSuggestions"
        static let hexKeyOnLeft = "settings.keyboardHexKeyLeft"
        static let numberRow = "settings.keyboardNumberRow"
        static let dictationBarAutoStart = "settings.dictationBarAutoStart"
        static let joinKeyboardWhenUnfolded = "experiments.joinKeyboardWhenUnfolded"
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

    /// Settings › Experiments › Korean keyboard (두벌식): adds 한국어 to Settings › Keyboard ›
    /// Languages, typed on the standard two-set layout and composed as Hangul while the run is
    /// in the field (`HangulComposer`). Off by default: 한국어 isn't offered, Automatic never
    /// picks it, and a stored "ko" is ignored.
    var koreanKeyboard: Bool {
        get { defaults?.object(forKey: Key.koreanKeyboard) as? Bool ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.koreanKeyboard) }
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

    /// Settings › Experiments › Hide word suggestions: the strip above the keys stops suggesting
    /// words (typing and next-word suggestions, swipe readings, the finished word's emoji, and
    /// the chip that keeps what you typed while a correction is still ahead), while the Hex key,
    /// Undo, status, notices and the Paste chip stay. Autocorrect still corrects on space, and
    /// the chip that puts back what a correction replaced stays after Delete; holding a strip
    /// word to add it to Custom vocabulary needs a suggestion, so it goes. Off by default.
    /// Stored as `experiments.hideWordSuggestions`; off, the strip suggests exactly as it did.
    var hideWordSuggestions: Bool {
        get { defaults?.object(forKey: Key.hideWordSuggestions) as? Bool ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.hideWordSuggestions) }
    }

    /// Settings › Keyboard › Swipe typing: slide through a word's letters to type it, on the
    /// English and Spanish layouts (`SwipeTyping`). On by default. Stored as
    /// `experiments.swipeTyping`, from when it was an experiment (off by default): unset or
    /// true is on and false off, so testers who turned it off keep it off.
    var swipeTyping: Bool {
        get { defaults?.object(forKey: Key.swipeTyping) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.swipeTyping) }
    }

    /// Settings › Experiments › Delete swiped words letter by letter: delete right after a swipe
    /// acts as anywhere else and takes one character, instead of the whole swiped word and the
    /// space the swipe added before it (`FullKeyboardEngine.deleteBackward`). The swipe's record
    /// clears with that delete, as with any delete, so what remains types like ordinary text.
    /// Off by default, like every experiment. Stored as
    /// `experiments.swipeDeleteLetterByLetter`; off, delete takes the whole word back as before.
    var deletesSwipedWordsLetterByLetter: Bool {
        get { defaults?.object(forKey: Key.swipeDeleteLetterByLetter) as? Bool ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.swipeDeleteLetterByLetter) }
    }

    /// Settings › Keyboard › Hex key: Right (false, the default) or Left. Left puts the Hex key
    /// at the strip's leading end, for people who type with their left thumb; everything
    /// tied to it (the recording bar's ✕ and ✓, its menu, the Full Access hint) follows.
    var hexKeyOnLeft: Bool {
        get { defaults?.bool(forKey: Key.hexKeyOnLeft) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.hexKeyOnLeft) }
    }

    /// Settings › Keyboard › Number row: 1 to 0 in a row of their own above the letters on an
    /// iPhone, on every page but 123 (whose top row is the same digits). Off by default, where
    /// the keys stay exactly on Apple's rows; on, the keyboard grows upward by the row and every
    /// row below stays where it was (`KeyboardMetrics.withNumberRow`).
    var numberRow: Bool {
        get { defaults?.bool(forKey: Key.numberRow) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.numberRow) }
    }

    /// Settings › Experiments › Join the keyboard when unfolded: on the Duo unfolded and held
    /// upright, Hex normally splits its keyboard as Apple's does there, since an extension
    /// can't read Apple's Split Keyboard preference. On, it draws the joined keyboard instead,
    /// at the same row heights and unfoldedTall's side inset. Off by default. Stored as
    /// `experiments.joinKeyboardWhenUnfolded`.
    var joinsKeyboardWhenUnfolded: Bool {
        get { defaults?.bool(forKey: Key.joinKeyboardWhenUnfolded) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.joinKeyboardWhenUnfolded) }
    }

    /// Settings › Keyboard › Auto-start: when the dictation bar starts dictating by itself.
    /// When switching by default.
    var dictationBarAutoStart: DictationBarAutoStart {
        get {
            defaults?.string(forKey: Key.dictationBarAutoStart).flatMap(DictationBarAutoStart.init(rawValue:))
                ?? .whenSwitching
        }
        nonmutating set { defaults?.set(newValue.rawValue, forKey: Key.dictationBarAutoStart) }
    }

    /// Where the full keyboard's Hex key sits for `hexKeyOnLeft`.
    var hexKeyPlacement: HexKeyPlacement {
        hexKeyOnLeft ? .stripLeading : .stripTrailing
    }

    /// Simulator scenes pass `--experiment-full-keyboard` or `--keyboard full|dictation-bar`
    /// to pick the keyboard, `--experiment-emoji` to turn Emoji on, `--experiment-swipe` for
    /// Swipe typing (both on by default; the arguments override a stored off),
    /// `--experiment-emoji-letters` for ABC under the emoji key,
    /// `--hex-key left|right` for the Hex key's side, `--number-row on|off` for the number row,
    /// and `--join-unfolded-keyboard on|off` for the joined keyboard on the Duo unfolded.
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
        if let index = arguments.firstIndex(of: Self.numberRowLaunchArgument), index + 1 < arguments.count {
            switch arguments[index + 1] {
            case "on": numberRow = true
            case "off": numberRow = false
            default: break
            }
        }
        if let index = arguments.firstIndex(of: Self.joinUnfoldedKeyboardLaunchArgument), index + 1 < arguments.count {
            switch arguments[index + 1] {
            case "on": joinsKeyboardWhenUnfolded = true
            case "off": joinsKeyboardWhenUnfolded = false
            default: break
            }
        }
    }
}
