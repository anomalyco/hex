import Foundation

/// Settings › Dictation › Lowercase, in the shared App Group. Off by default.
nonisolated struct DictationLowercaseSettings {
    private enum Key {
        static let enabled = "settings.dictationLowercase"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Everything dictation writes is lowercased after its style, custom vocabulary included,
    /// as desktop's Lowercase step does (`str::to_lowercase` on the final text).
    var isEnabled: Bool {
        get { defaults?.bool(forKey: Key.enabled) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.enabled) }
    }
}

/// The steps between the speech model and the text Hex inserts, the same wherever Hex
/// dictates: `DictationController` (the app, the keyboard's warm session and cold start, the
/// Action Button, Control Center and Siri) and in-keyboard dictation
/// (`KeyboardLocalDictation`), so a style and Lowercase apply on every path.
///
/// In order: learned fixes, custom vocabulary, Clean up what I say
/// (`CleanupTagger.cleanIfEnabled`, which guards its own memory in the keyboard), `refine`
/// (Clean up with Apple Intelligence, only in the app), translation, the style
/// (`TranscriptFormatter`), the spellings again, then Lowercase. The style and Lowercase are
/// string rules with no model, so in-keyboard dictation needs no more memory to run them.
nonisolated struct DictationPipeline: Sendable {
    var style: DictationStyle
    /// The dictation language (BCP 47).
    var language: String
    /// Custom vocabulary's terms; empty while it is off.
    var vocabulary: [String] = []
    /// Learn how I type's dictation fixes; empty while it is off.
    var corrections: [DictationCorrection] = []
    var lowercase = false

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
    /// can lowercase a term or capitalize a sentence start: "iPhone" -> "IPhone"), then
    /// Lowercase.
    func format(_ text: String, language: String?) -> String {
        let styled = TranscriptFormatter.format(text, style: style, language: language)
        let spelled = VocabularySpelling.apply(vocabulary + DictationCorrections.spellings(corrections), to: styled)
        return lowercase ? spelled.lowercased() : spelled
    }
}
