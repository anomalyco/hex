import Foundation

/// The steps between the speech model and the text Hex inserts, the same wherever Hex
/// dictates: `DictationController` (the app, the keyboard's warm session and cold start, the
/// Action Button, Control Center and Siri) and in-keyboard dictation
/// (`KeyboardLocalDictation`), so a style applies on every path.
///
/// In order: learned fixes, custom vocabulary, Clean up what I say
/// (`CleanupTagger.cleanIfEnabled`, which guards its own memory in the keyboard), `refine`
/// (Clean up with Apple Intelligence, only in the app), translation, the style
/// (`TranscriptFormatter`), then the spellings again. The style is string rules with no
/// model, so in-keyboard dictation needs no more memory to run it.
nonisolated struct DictationPipeline: Sendable {
    var style: DictationStyle
    /// The dictation language (BCP 47).
    var language: String
    /// Custom vocabulary's terms; empty while it is off.
    var vocabulary: [String] = []
    /// Learn how I type's dictation fixes; empty while it is off.
    var corrections: [DictationCorrection] = []

    /// Each step's text, for Record typing logs.
    struct Stages: Equatable, Sendable {
        /// After learned fixes.
        var fixed: String
        /// After custom vocabulary.
        var spelled: String
        /// After Clean up what I say.
        var tagged: String
        /// After `refine`.
        var cleaned: String
        var translated: String?
        /// What Hex inserts.
        var formatted: String
    }

    /// `translationSource` is the dictation language as Apple Speech names it; nil reads
    /// `language`.
    @MainActor
    func run(
        _ heard: String,
        translationSource: Locale.Language? = nil,
        refine: ((String) async -> String)? = nil
    ) async -> Stages {
        let fixed = DictationCorrections.apply(corrections, to: heard)
        let spelled = VocabularySpelling.apply(vocabulary, to: fixed)
        let tagged = await CleanupTagger.cleanIfEnabled(spelled, style: style, language: language)
        let cleaned = await refine?(tagged) ?? tagged
        let translated = await DictationTranslator.translateIfEnabled(
            cleaned,
            source: translationSource ?? Locale(identifier: language).language
        )
        // A translation is formatted in the language it is now in.
        let formatLanguage = translated == nil ? language : DictationTranslationSettings().targetLanguage
        return Stages(
            fixed: fixed,
            spelled: spelled,
            tagged: tagged,
            cleaned: cleaned,
            translated: translated,
            formatted: format(translated ?? cleaned, language: formatLanguage)
        )
    }

    /// `text` in the style, then the vocabulary's and learned fixes' spellings again (a style
    /// can lowercase a term or capitalize a sentence start: "iPhone" -> "IPhone").
    func format(_ text: String, language: String?) -> String {
        let styled = TranscriptFormatter.format(text, style: style, language: language)
        return VocabularySpelling.apply(vocabulary + DictationCorrections.spellings(corrections), to: styled)
    }
}
