import Foundation

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

    /// The share for a lowercase word: from the mapped list (`HexWordList`, read in place), or the
    /// table tests give.
    private let share: @Sendable (String) -> UInt8?

    init(percentages: [String: UInt8]) {
        share = { percentages[$0] }
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

    /// The list memory-mapped and searched in place (`HexWordList`), as every language's
    /// `.capitals` is: no table of its 55,000 words in memory.
    init?(url: URL) {
        guard let list = HexWordList(url: url) else { return nil }
        share = { word in
            // The builder writes every word lowercase, from a lowercase-only model.
            guard let percent = list.value(of: word), (Self.sometimesShare...100).contains(percent) else { return nil }
            return UInt8(percent)
        }
    }

    /// The share of the word's corpus occurrences written with a capital (50–100), or nil when
    /// the list doesn't know the word.
    func percent(of word: String) -> UInt8? {
        share(word.lowercased())
    }

    /// Whether `word`, all lowercase, takes a capital: a noun written with one at least
    /// `alwaysShare` percent of the time anywhere, or `sometimesShare` percent of the time
    /// after an article or determiner. Nil when the list doesn't know the word and false when
    /// it knows it but no determiner stands before it, so autocorrect asks the dictionary for
    /// both as before.
    func capitalises(_ word: String, after previous: String?) -> Bool? {
        guard word.count >= 2, word == word.lowercased(),
              let share = percent(of: word), share >= Self.sometimesShare else { return nil }
        if share >= Self.alwaysShare { return true }
        return previous.map { Self.determiners.contains($0) } ?? false
    }

    /// Articles, determiners, their contractions and possessives, after which German writes
    /// the noun with a capital ("das haus", "im haus", "mein haus"), so a word the books write
    /// both ways is a noun here ("das leben", never "zu leben"). "in" is in: "in Ruhe", "in
    /// Frage", and a verb rarely follows it. "ihr" is left out: it is the possessive
    /// "her/their", but as the pronoun "you" (plural) it comes before a verb far more often
    /// ("wollt ihr essen"). "ohne" is out too: "ohne essen" is a verb, "without eating".
    static let determiners: Set<String> = [
        "der", "die", "das", "den", "dem", "des",
        "ein", "eine", "einen", "einem", "einer", "eines",
        "kein", "keine", "keinen", "keinem", "keiner", "keines",
        "mein", "meine", "meinen", "meinem", "meiner", "meines",
        "dein", "deine", "deinen", "deinem", "deiner", "deines",
        "sein", "seine", "seinen", "seinem", "seiner", "seines",
        "ihre", "ihren", "ihrem", "ihrer", "ihres",
        "unser", "unsere", "unseren", "unserem", "unserer", "unseres",
        "euer", "eure", "euren", "eurem", "eurer", "eures",
        "dies", "dieser", "diese", "dieses", "diesem", "diesen",
        "jeder", "jede", "jedes", "jedem", "jeden",
        "im", "in", "am", "zum", "zur", "vom", "beim", "ins", "ans",
    ]
}
