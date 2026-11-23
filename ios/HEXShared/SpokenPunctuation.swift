import Foundation
import HexKeyboardCore

/// Settings › Dictation › Spoken punctuation, in the shared App Group so in-keyboard dictation
/// reads it too. On by default.
nonisolated struct DictationPunctuationSettings {
    static let enabledKey = "settings.dictationSpokenPunctuation"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Punctuation said by name ("comma", "question mark", "open quote") is written as the mark
    /// (`SpokenPunctuation`), in English, Spanish, French, German, Italian and Portuguese.
    var isEnabled: Bool {
        get { defaults?.object(forKey: Self.enabledKey) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }
}

/// Punctuation said by name written as the mark, with string rules and no model, in English,
/// Spanish, French, German, Italian and Portuguese; other languages are left as they are. Runs
/// inside a style, after its fillers and Format lists (`SpokenLists`, so "dash … dash …" stays a
/// list and "comma, new line" keeps its comma) and before the style's own punctuation
/// (`TranscriptFormatter.format(_:style:language:lists:punctuation:endsWithPeriod:)`), so each
/// style capitalizes and ends sentences its own way; French spacing comes after (`FrenchSpacing`).
///
/// Apple's `DictationTranscriber` already writes the marks (macOS 27.2.1, `say` voices: "That is
/// amazing!", "Here is the plan: we leave at noon", "(after lunch)", "#Blessed"), so its text has no
/// names left. `SpeechTranscriber` writes only some ("comma", "question mark", French "point" and
/// "virgule", German "Komma" and "Fragezeichen") and leaves the rest as words, often with commas
/// around them ("I came semicolon, I saw", "open Paren after lunch close, Paren"); Parakeet writes
/// every name as words ("noon, comma, see you soon, period", "tonight question mark?"). A mark the
/// model already wrote beside a name is replaced, never doubled.
///
/// - Marks attach to the word before them ("noon, see you"), a dash is spaced ("good – really
///   good"), "hyphen", "apostrophe" and "arroba" ("arobase", "chiocciola", "At-Zeichen") join
///   their words ("well-known", "John's", "ana@gmail"; English "at sign" is
///   `SpokenCleanupRules.spokenSymbols`'s), "hashtag" and "dollar sign" go on the next word
///   ("#blessed"), and a word after ".", "?" or "!" starts with a capital.
/// - Quotes and brackets open and close: “ ” (« » in French, „ “ in German), ( ). A closing name
///   counts only while one is open, and a bare "quote" ("comillas", "guillemets",
///   "Anführungszeichen", "virgolette", "aspas") opens only with a closing name later, never in
///   "quote unquote".
/// - Guards keep prose. A name is no command right after a determiner, a preposition, a number
///   or a word that makes it a noun ("a period of time", "the comma is", "a dash of salt", "his
///   colon", "trial period", "Oxford comma", "ein wichtiger Punkt"), right before a word that
///   continues the noun ("question mark tattoo", "colon cancer", "period of", "punto de vista"),
///   or inside a compound ("comma-separated"). The names that are everyday nouns ("period",
///   "point", "Punkt", "punto", "ponto", "question mark", "dash", "smiley", "dos puntos") need
///   command-like placement as well: the end of the line, a mark after them, or a next word that
///   starts a clause ("I", "see", "thanks", "nos", "à", a capital outside German). A colon needs
///   words after it.
///
/// The rules and their words are the keyboard core's (`hk_dictation_spoken_punctuation`,
/// crates/keyboard/core/src/dictation/punctuation).
nonisolated enum SpokenPunctuation {
    static func format(_ text: String, language: String?) -> String {
        DictationCore.stage(text, language: language, hk_dictation_spoken_punctuation)
    }
}
