// The dictation parity oracle: a copy of ios/HEXShared/CleanupTagger (words) and DictationLanguage (isEnglish).swift at dev 93db6ad7 (unchanged
// since d6efeddb), its types renamed with an "Oracle" suffix, so the Rust port in
// crates/keyboard/core/src/dictation/ is checked against today's Swift (DictationParityTests).
// Only the UserDefaults settings struct is left out. Never edit the rules here.
import Foundation

nonisolated enum CleanupTaggerOracle {
    /// The transcript's whitespace-separated words.
    static func words(in text: String) -> [String] {
        text.split(whereSeparator: \.isWhitespace).map(String.init)
    }
}

nonisolated enum SpeechLanguagePickerOracle {
    /// Whether a language identifier is English, for the English-only steps (the cleanup
    /// tagger). nil (unknown) counts as English, as before languages existed.
    static func isEnglish(_ identifier: String?) -> Bool {
        guard let identifier else { return true }
        return Locale(identifier: identifier).language.languageCode?.identifier == "en"
    }
}
