import Foundation

/// Experiment (Learn each language separately): what the keys and dictation learn stays with
/// the language it was typed or dictated in. Corrections and dictation fixes are tagged with
/// their language either way, so turning it on needs nothing learned again; off, one shared
/// list and one shared learning, exactly as before.
nonisolated enum LanguageLearning {
    /// The code a dictation's fixes are kept under: its base language, so English heard in
    /// the US and in the UK stays one list ("en" for "en-US" and "en-GB").
    static func dictationCode(_ identifier: String?) -> String? {
        guard let identifier else { return nil }
        return Locale(identifier: identifier).language.languageCode?.identifier ?? identifier
    }
}

extension TypingLearning {
    /// `derive(from:)` over the corrections typed in `language` and the ones never tagged;
    /// nil derives from all of them, as before.
    static func derive(from corrections: [TypingJournalCorrection], language: String?) -> TypingLearning {
        guard let language else { return derive(from: corrections) }
        return derive(from: corrections.filter { $0.language == nil || $0.language == language })
    }
}
