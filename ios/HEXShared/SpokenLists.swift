import Foundation
import HexKeyboardCore

/// Settings › Dictation › Format lists, in the shared App Group so in-keyboard dictation reads
/// it too. On by default.
nonisolated struct DictationListSettings {
    static let enabledKey = "settings.dictationFormatLists"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Spoken lists ("first … second …", "bullet … bullet …") and line breaks ("new line",
    /// "new paragraph") are written as lines (`SpokenLists`), in English and Spanish.
    var isEnabled: Bool {
        get { defaults?.object(forKey: Self.enabledKey) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }
}

/// Dictated lists and line breaks as lines, with string rules and no model, in English and
/// Spanish; other languages are left as they are. Runs inside a style, after its fillers and
/// before its punctuation (`TranscriptFormatter.format(_:style:language:lists:)`), so each style
/// ends the lines its own way.
///
/// - "new line", "new paragraph" (desktop's Labs › Spoken formatting words; "nueva línea",
///   "nuevo párrafo", "salto de línea") become one or two line breaks, and the commas around
///   them go. Never at the very start, after "a", "the", "my", "una", … ("a new line of shoes"),
///   or before "of", "is", "de", … .
/// - Numbered lists: "first … second …" ("1st … 2nd …", "primero … segundo …",
///   "en primer lugar …"), "number one … number two …" ("step", "item", "point", "número",
///   "paso"), or "one, … two, …" ("uno, … dos, …", "1. … 2. …") become "1. …" lines. The
///   markers count up from one, there are at least two items, none is empty, and every
///   marker starts a clause: the start, after a mark (".", ",", ";", ":", …) or a line break,
///   or after "and"/"then"/"y"/"luego" that do. A bare number needs its comma ("one, …");
///   "finally" or "lastly" ("por último") ends a list of two or more.
/// - Bullets: "bullet", "bullet point", "next bullet", "next item", "dash" ("viñeta",
///   "guion", "siguiente elemento") said at least twice become "- …" lines, as the Notes style
///   already writes bullets (a hyphen reads in every app's plain text and is what Markdown and
///   Notes take for a list). "dash" and "guion" need a clause start; "bullet" may follow any
///   word but "a", "the", "to", … ("a bullet hit the wall").
/// - Text before the first marker is its own line, given a colon unless it ends a sentence;
///   a last item ends at its first sentence, and what follows is its own line. Items start
///   with a capital and lose a trailing comma, period, "and" or "or".
///
/// The rules and their words are the keyboard core's (`hk_dictation_spoken_lists`,
/// crates/keyboard/core/src/dictation/lists.rs).
nonisolated enum SpokenLists {
    static func format(_ text: String, language: String?) -> String {
        DictationCore.stage(text, language: language, hk_dictation_spoken_lists)
    }
}
