import Foundation
import HexKeyboardCore

/// German nouns' share of capitalised uses, read from `HEXKeyboard/TypingModel/de.capitals`
/// beside the German typing model. One `word<TAB>percent` line per word (lowercase, sorted,
/// written by `scripts/build-german-capitals.py`): the percent is how often the word is written
/// with a capital in the same Google Books corpus the model was counted from, so "haus" is 99
/// and "leben" — `Leben` the noun or `leben` the verb — is lower. The builder also caps a word
/// that is equally a common verb form or stem ("frage", "reise", "kauf") or a two-letter
/// abbreviation ("gl") at 94, so it takes a capital only after a determiner. German's model is
/// lowercase throughout and Apple's German dictionary takes "haus", "auto" and "zeit" lowercase
/// too, so this list is what knows they are nouns.
nonisolated struct GermanCapitals: Sendable {
    /// A word the books write with a capital this often is a noun wherever it stands.
    static let alwaysShare = 95
    /// From this share it is a noun after an article or determiner ("das leben") and whatever
    /// shares the spelling elsewhere ("zu leben", "ich frage").
    static let sometimesShare = 50

    /// The list as the keyboard core reads it (`hk_german_capitals_*`).
    let core: KeyboardCoreGermanCapitals

    /// A list of these shares, for tests: as the sidecar's lines.
    init(percentages: [String: UInt8]) {
        let lines = percentages.sorted { Array($0.key.utf8).lexicographicallyPrecedes(Array($1.key.utf8)) }
            .map { "\($0.key)\t\($0.value)\n" }.joined()
        // A list of no words is an empty file, which isn't one: it knows no word either way.
        core = KeyboardCoreGermanCapitals(bytes: Array(lines.utf8)) ?? KeyboardCoreGermanCapitals(bytes: Array("\n".utf8))!
    }

    /// The sidecar beside the German model (`HEXKeyboard/TypingModel/de.capitals`), read once
    /// per process; nil when the build has none. The app's keyboard preview finds it inside the
    /// embedded keyboard, as `TypingDecoderModel.bundled` finds the model.
    @MainActor static func bundled() -> GermanCapitals? {
        if didRead { return loaded }
        didRead = true
        var url = Bundle.main.url(forResource: "de", withExtension: "capitals")
        if url == nil, let plugins = Bundle.main.builtInPlugInsURL,
           let keyboard = Bundle(url: plugins.appendingPathComponent("HEXKeyboard.appex")) {
            url = keyboard.url(forResource: "de", withExtension: "capitals")
        }
        loaded = url.flatMap { GermanCapitals(url: $0) }
        return loaded
    }

    @MainActor private static var loaded: GermanCapitals?
    @MainActor private static var didRead = false

    /// The list memory-mapped and searched in place, as every language's `.capitals` is: no
    /// table of its 55,000 words in memory.
    init?(url: URL) {
        guard let core = KeyboardCoreGermanCapitals(url: url) else { return nil }
        self.core = core
    }

    /// The share of the word's corpus occurrences written with a capital (50–100), or nil when
    /// the list doesn't know the word.
    func percent(of word: String) -> UInt8? {
        let percent = hk_german_capitals_percent(core.pointer, word)
        return percent > 0 ? UInt8(percent) : nil
    }

    /// Whether `word`, all lowercase, takes a capital: a noun written with one at least
    /// `alwaysShare` percent of the time anywhere, or `sometimesShare` percent of the time
    /// after an article or determiner (the core's list of them). Nil when the list doesn't
    /// know the word and false when it knows it but no determiner stands before it, so
    /// autocorrect asks the dictionary for both as before.
    func capitalises(_ word: String, after previous: String?) -> Bool? {
        switch hk_german_capitals_capitalises(core.pointer, word, previous) {
        case 1: true
        case 0: false
        default: nil
        }
    }
}
