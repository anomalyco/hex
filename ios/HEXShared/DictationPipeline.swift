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

/// Settings › Dictation › End with a period, in the shared App Group so in-keyboard dictation
/// reads it too. On by default. Off, no line ends with a full stop, though "?" and "!" stay
/// (`TranscriptFormatter.format(_:style:language:lists:endsWithPeriod:)`).
nonisolated struct DictationPeriodSettings {
    static let enabledKey = "settings.dictationEndWithPeriod"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.object(forKey: Self.enabledKey) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }
}

/// Experiments › Hinglish (Hindi in Latin letters), in the shared App Group so in-keyboard
/// dictation reads it too. Off by default. While it is on, Hindi dictation is written in Latin
/// letters (`HinglishRomanizer`) and Hindi is called Hinglish in the language menus.
nonisolated struct HinglishSettings {
    static let enabledKey = "experiments.hinglish"
    static let launchArgument = "--experiment-hinglish"
    /// Hindi's name in the Hex key's language menu and Settings › Dictation language while on.
    static let languageName = "Hinglish"
    /// The language it is for: Apple Speech's Hindi, also what the keyboard offers to switch to.
    static let language = "hi-IN"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Self.enabledKey) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }

    /// "Hinglish" for Hindi while the experiment is on; nil for other languages, or while off.
    func name(for language: String) -> String? {
        isEnabled && Self.isHindi(language) ? Self.languageName : nil
    }

    static func isHindi(_ language: String?) -> Bool {
        guard let language else { return false }
        return Locale(identifier: language).language.languageCode?.identifier == "hi"
    }

    /// Simulator scenes pass `--experiment-hinglish` to turn the experiment on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
    }
}

/// Experiments › Greeklish (Greek in Latin letters), in the shared App Group so in-keyboard
/// dictation reads it too. Off by default. While it is on, Greek dictation is written in Latin
/// letters (`GreeklishRomanizer`) and Greek is called Greeklish in the language menus.
nonisolated struct GreeklishSettings {
    static let enabledKey = "experiments.greeklish"
    static let launchArgument = "--experiment-greeklish"
    /// Greek's name in the Hex key's language menu and Settings › Dictation language while on.
    static let languageName = "Greeklish"
    /// The language it is for: Apple Speech's Greek, also what the keyboard offers to switch to.
    static let language = "el-GR"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Self.enabledKey) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }

    /// "Greeklish" for Greek while the experiment is on; nil for other languages, or while off.
    func name(for language: String) -> String? {
        isEnabled && Self.isGreek(language) ? Self.languageName : nil
    }

    static func isGreek(_ language: String?) -> Bool {
        guard let language else { return false }
        return Locale(identifier: language).language.languageCode?.identifier == "el"
    }

    /// Simulator scenes pass `--experiment-greeklish` to turn the experiment on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
    }
}

/// The steps between the speech model and the text Hex inserts, the same wherever Hex
/// dictates: `DictationController` (the app, the keyboard's warm session and cold start, the
/// Action Button, Control Center and Siri) and in-keyboard dictation
/// (`KeyboardLocalDictation`), so a style and Lowercase apply on every path.
///
/// In order: Hinglish (Hindi in Latin letters, while that experiment is on) or Greeklish
/// (Greek in Latin letters, while that one is on), learned fixes, custom vocabulary, Numbers as
/// digits and Clean up what I say (`CleanupTagger.cleanIfEnabled`, which guards its own memory
/// in the keyboard),
/// `refine` (Clean up with Apple Intelligence, only in the app), translation, the style
/// (`TranscriptFormatter`, with Format lists' `SpokenLists` and Spoken punctuation's
/// `SpokenPunctuation` inside it), the spellings again,
/// Lowercase, then French spacing (`FrenchSpacing`, in every style but Verbatim). The style,
/// Lowercase and French spacing are string rules with no model, so in-keyboard dictation needs
/// no more memory to run them.
nonisolated struct DictationPipeline: Sendable {
    var style: DictationStyle
    /// The dictation language (BCP 47).
    var language: String
    /// Custom vocabulary's terms; empty while it is off.
    var vocabulary: [String] = []
    /// Learn how I type's dictation fixes; empty while it is off.
    var corrections: [DictationCorrection] = []
    var lowercase = false
    /// Settings › Dictation › Format lists (`SpokenLists`), inside the style.
    var formatLists = false
    /// Settings › Dictation › Spoken punctuation (`SpokenPunctuation`), inside the style.
    var spokenPunctuation = false
    /// Settings › Dictation › Numbers as digits (`SpokenNumbers`), before Clean up what I say's
    /// tagger, in every style but Verbatim.
    var numbersAsDigits = false
    /// Experiments › Hinglish: Hindi is written in Latin letters before every other step, so
    /// learned fixes, vocabulary and the style see the Latin words.
    var hinglish = false
    /// Experiments › Greeklish: Greek is written in Latin letters there too.
    var greeklish = false
    /// Settings › Dictation › End with a period; off, no line ends with a full stop.
    var endsWithPeriod = true
    /// Translate what I say's translator; nil is Apple's. Tests stand in for it.
    var translationEngine: TranslationEngine?

    /// Each step's text, for Record typing logs.
    struct Stages: Equatable, Sendable {
        /// What the speech model heard, in Latin letters for Hinglish or Greeklish: what
        /// learning compares the inserted text with.
        var heard: String
        /// After learned fixes.
        var fixed: String
        /// After custom vocabulary.
        var spelled: String
        /// After Clean up what I say.
        var tagged: String
        /// After `refine`.
        var cleaned: String
        var translated: String?
        /// Why it went in as spoken while Translate what I say was on.
        var translationFailure: TranslationFailure?
        /// What Hex inserts.
        var formatted: String
    }

    /// `translationSource` is the dictation language as Apple Speech names it; nil reads
    /// `language`.
    @MainActor
    func run(
        _ spoken: String,
        translationSource: Locale.Language? = nil,
        refine: ((String) async -> String)? = nil
    ) async -> Stages {
        let translation = DictationTranslationSettings()
        let romanizesHindi = hinglish && HinglishSettings.isHindi(language)
        let romanizesGreek = greeklish && GreeklishSettings.isGreek(language)
        // Apple translates Hindi from Devanagari and Greek from Greek letters, not the
        // experiments' Latin: a dictation that may go into another language keeps its script,
        // romanized only if it goes in as spoken.
        let translatesLatin = (romanizesHindi && translation.isEnabled
            && !HinglishSettings.isHindi(translation.targetLanguage))
            || (romanizesGreek && translation.isEnabled
                && !GreeklishSettings.isGreek(translation.targetLanguage))
        let heard = romanizesHindi ? HinglishRomanizer.romanize(spoken)
            : romanizesGreek ? GreeklishRomanizer.romanize(spoken)
            : spoken
        let fixed = DictationCorrections.apply(corrections, to: translatesLatin ? spoken : heard)
        let spelled = SoundAlikeSpelling.apply(vocabulary, to: VocabularySpelling.apply(vocabulary, to: fixed), language: language)
        let tagged = await CleanupTagger.cleanIfEnabled(
            spelled,
            style: style,
            language: language,
            protectedTerms: DictationCorrections.recognitionHints(vocabulary: vocabulary, corrections: corrections),
            numbers: numbersAsDigits
        )
        let refined = await refine?(tagged) ?? tagged
        let outcome = await DictationTranslator.translate(
            refined,
            source: translationSource ?? Locale(identifier: language).language,
            settings: translation,
            engine: translationEngine
        )
        let translated = outcome.text
        let cleaned = translatesLatin && translated == nil
            ? (romanizesGreek ? GreeklishRomanizer.romanize(refined) : HinglishRomanizer.romanize(refined))
            : refined
        // A translation is formatted in the language it is now in.
        let formatLanguage = translated == nil ? language : translation.targetLanguage
        return Stages(
            heard: heard,
            fixed: fixed,
            spelled: spelled,
            tagged: tagged,
            cleaned: cleaned,
            translated: translated,
            translationFailure: outcome.failure,
            formatted: format(translated ?? cleaned, language: formatLanguage)
        )
    }

    /// `text` in the style, then the vocabulary's and learned fixes' spellings again (a style
    /// can lowercase a term or capitalize a sentence start: "iPhone" -> "IPhone"), then
    /// Lowercase, with Unicode's context-aware mapping, so a word-final Σ becomes ς ("ΟΔΟΣ" ->
    /// "οδος"), where Swift's `lowercased()` always gives σ, then, in French, the spaces before
    /// `? ! ; :` and inside `« »` that the styles take out (`FrenchSpacing`; Verbatim keeps the
    /// model's).
    func format(_ text: String, language: String?) -> String {
        let styled = TranscriptFormatter.format(
            text,
            style: style,
            language: language,
            lists: formatLists,
            punctuation: spokenPunctuation,
            endsWithPeriod: endsWithPeriod
        )
        let spelled = VocabularySpelling.apply(vocabulary + DictationCorrections.spellings(corrections), to: styled)
        let cased = lowercase ? spelled.lowercased(with: nil) : spelled
        return style == .verbatim ? cased : FrenchSpacing.apply(cased, language: language)
    }
}
