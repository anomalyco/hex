import Foundation
import HexKeyboardCore

/// Clean up what I say for Spanish, German, and Norwegian, which the English cleanup tagger
/// was never trained on. Run on Spanish, the tagger dropped negations ("Te digo que no puedo
/// ir" became "Te puedo ir"), so these languages get two word rules instead, each needing
/// the commas a pause leaves:
/// - the language's hesitation sounds go anywhere ("eh", "mmm"; `TranscriptFormatter`'s
///   list);
/// - a filler phrase set off by commas goes ("Es caro, o sea, no lo compro");
/// - a spoken correction ("a las 3, digo, a las 4") drops its marker and the words it
///   replaces, counted back from the nearest earlier word that matches the repair's first
///   word ("a"), or else is of its kind (a number, weekday, month, or today and tomorrow),
///   within the sentence and `maxReach` words. With no such word, the sentence stays as
///   said.
/// Like the tagger, it only removes words; `CleanupReconstruction` repairs the commas and
/// capitals next to a removal.
///
/// The rules and their words are the keyboard core's (crates/keyboard/core/src/dictation/
/// cleanup.rs, symbols.rs, `hk_dictation_cleanup_rules`, `hk_dictation_spoken_symbols`,
/// `hk_dictation_spoken_counts`).
nonisolated enum SpokenCleanupRules {
    /// How far back a correction looks for the words it replaces.
    static let maxReach = 8

    static func supports(_ language: String?) -> Bool {
        DictationCore.withLanguage(language) { language in
            DictationCore.flag(false) { hk_dictation_cleanup_supports(language, $0) }
        }
    }

    /// `text` with its fillers and spoken corrections removed, or `text` itself when
    /// nothing goes or the language has no rules.
    static func clean(_ text: String, language: String?) -> String {
        DictationCore.stage(text, language: language, hk_dictation_cleanup_rules)
    }

    /// Lowercased, without opening marks or trailing punctuation.
    static func bare(_ word: String) -> String {
        DictationCore.withText(word) { text in
            DictationCore.text(word.lowercased()) { hk_dictation_bare(text, $0) }
        }
    }

    // MARK: Spoken symbols

    /// English `text` with "@" for its spoken names, or `text` itself. "at the rate" (what
    /// India calls @) becomes "@" only before an address's domain ("gmail dot com",
    /// "gmail.com"), never in "at the rate of 5%" or "at the rate we're going"; "at sign"
    /// always does. Before a domain, the word before (with any "dot"-joined parts) and the
    /// domain join it as one lowercase address: "John at the rate Gmail dot com." becomes
    /// "john@gmail.com.", "dot" becoming "." only there. Elsewhere "at sign" is a lone "@"
    /// ("the @ key"). Words around it keep their spacing.
    static func spokenSymbols(in text: String, language: String?) -> String {
        let result = DictationCore.withText(text) { string in
            DictationCore.withLanguage(language) { language in
                DictationCore.call { hk_dictation_spoken_symbols(string, language, $0) }
            }
        }
        if let written = result.text { return written }
        if result.status == HK_DICTATION_OVERLAPPING_ADDRESSES { return overlappingAddresses(in: text) }
        assertionFailure("spokenSymbols: the keyboard core's dictation rules failed (status \(result.status))")
        return text
    }

    /// OWNER'S DECISION PENDING: what "a at sign b.com at sign c.com" (two addresses said
    /// back to back, the second's local part the first's domain) becomes. The Swift rules
    /// before the keyboard core trapped here (their edits overlapped and `text[cursor..<start]`
    /// had a lower bound past its upper bound), and so does this, on purpose, until the owner
    /// decides: this function is the one place that changes then. The core reports the case
    /// as `HK_DICTATION_OVERLAPPING_ADDRESSES`.
    static func overlappingAddresses(in text: String) -> String {
        preconditionFailure(
            "Range requires lowerBound <= upperBound: spokenSymbols' edits overlap (two addresses said back to back)"
        )
    }

    // MARK: Spoken counts

    /// English `text` with a spoken count in digits, or `text` itself, so a microphone test
    /// reads the same from every speech model: for "test, one, two, three" Parakeet writes
    /// "Test 1, 2, 3" and Apple's `SpeechTranscriber` "Test one, 2, 3." (macOS 27.2, `say`
    /// voices), and the cleanup tagger is sure a count in words is a self-correction (P(DELETE)
    /// over 0.98 on "one" and "two", which made "Test three." before `CleanupGuard`). A count is
    /// three numbers or more counting up from one, in words or digits, with only a comma or a
    /// space between them: "one two three" and "one, 2, 3" become "1, 2, 3". "123" (up to
    /// "123456789") right after "test" or "testing" and ending its clause, which Parakeet and
    /// `DictationTranscriber` write for a quick "testing one two three", becomes "1, 2, 3" too.
    /// Every other number stays as said ("Chapter one, two and three", "at 1, 2, or 3", "room
    /// 123", "test 123 passed").
    static func spokenCounts(in text: String, language: String?) -> String {
        DictationCore.stage(text, language: language, hk_dictation_spoken_counts)
    }
}
