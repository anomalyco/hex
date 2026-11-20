import Foundation

/// Settings › Dictation language, in the App Group so the keyboard reads it and can switch it.
nonisolated struct LanguageSettings {
    private enum Key {
        static let dictation = "settings.dictationLanguage"
        static let resolvedDictation = "settings.dictationLanguage.resolved"
        static let automaticDictation = "settings.dictationLanguage.automatic"
        static let recent = "settings.dictationLanguage.recent"
        static let supported = "settings.dictationLanguage.supported"
    }

    static let maxRecent = 5

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// A BCP 47 identifier such as `nb-NO`, or nil for Automatic.
    var dictationLanguage: String? {
        get { defaults?.string(forKey: Key.dictation) }
        nonmutating set { defaults?.set(newValue, forKey: Key.dictation) }
    }

    /// The language Apple Speech last resolved for dictation (the app or the keyboard), so
    /// translation can follow it without asking Speech again.
    var resolvedDictationLanguage: String? {
        get { defaults?.string(forKey: Key.resolvedDictation) }
        nonmutating set { defaults?.set(newValue, forKey: Key.resolvedDictation) }
    }

    /// What Automatic picks on this iPhone, when the app or keyboard last worked it out.
    var automaticDictationLanguage: String? {
        get { defaults?.string(forKey: Key.automaticDictation) }
        nonmutating set { defaults?.set(newValue, forKey: Key.automaticDictation) }
    }

    /// Languages picked lately, newest first, for the keyboard's quick switch.
    var recentDictationLanguages: [String] {
        get { defaults?.stringArray(forKey: Key.recent) ?? [] }
        nonmutating set { defaults?.set(Array(newValue.prefix(Self.maxRecent)), forKey: Key.recent) }
    }

    /// Every language Apple Speech transcribes here (BCP 47), as last listed, so the
    /// keyboard can offer only those without asking Speech.
    var supportedDictationLanguages: [String] {
        get { defaults?.stringArray(forKey: Key.supported) ?? [] }
        nonmutating set { defaults?.set(newValue, forKey: Key.supported) }
    }

    /// The language dictation is set to or last ran in, when known.
    var effectiveDictationLanguage: String? {
        dictationLanguage ?? resolvedDictationLanguage
    }

    /// A choice made in Hex or the keyboard: the setting, and the front of the recents.
    func chooseDictationLanguage(_ identifier: String?) {
        dictationLanguage = identifier
        if let identifier {
            recentDictationLanguages = [identifier] + recentDictationLanguages.filter { $0 != identifier }
        }
    }
}

/// The Apple Speech module that transcribes a language. `SpeechTranscriber` is Apple's newer
/// model and Hex's default; `DictationTranscriber` (the model behind the iOS keyboard's
/// dictation) covers languages it lacks, such as Norwegian, Danish, and Swedish.
nonisolated enum SpeechModuleKind: String, Codable, Sendable {
    case speech
    case dictation
}

nonisolated struct SpeechLanguage: Equatable, Sendable {
    let locale: Locale
    let module: SpeechModuleKind

    var identifier: String { locale.identifier(.bcp47) }
    var languageCode: String? { locale.language.languageCode?.identifier }
}

/// One entry in Settings › Dictation language.
nonisolated struct SpeechLanguageOption: Equatable, Identifiable, Sendable {
    /// BCP 47, such as `nb-NO`.
    let id: String
    let name: String
}

/// Which language and Apple Speech module a dictation uses.
nonisolated enum SpeechLanguagePicker {
    /// What Automatic tries, in order: the iPhone's preferred languages, its own locale, then
    /// US English. Norwegian Nynorsk also tries Bokmål, the only Norwegian Apple transcribes.
    static func automaticCandidates(preferred: [String], current: String) -> [String] {
        var result: [String] = []
        var seen = Set<String>()
        func add(_ identifier: String) {
            let key = Locale(identifier: identifier).identifier(.bcp47)
            if seen.insert(key).inserted {
                result.append(identifier)
            }
        }
        for identifier in preferred + [current, "en-US"] {
            add(identifier)
            if Locale(identifier: identifier).language.languageCode?.identifier == "nn" {
                add("nb-NO")
            }
        }
        return result
    }

    /// An explicit choice wins: `SpeechTranscriber` when it has that language, else
    /// `DictationTranscriber`. Automatic takes the first candidate either module has,
    /// preferring `SpeechTranscriber` for the same language, so a Norwegian iPhone gets
    /// Norwegian from `DictationTranscriber` rather than English from `SpeechTranscriber`.
    /// `speech` and `dictation` return each module's equivalent locale, or nil.
    static func pick(
        explicit: String?,
        candidates: [String],
        speech: (Locale) async -> Locale?,
        dictation: (Locale) async -> Locale?
    ) async -> SpeechLanguage? {
        if let explicit, !explicit.isEmpty {
            let wanted = Locale(identifier: explicit)
            if let found = await speech(wanted), sameLanguage(found, wanted) {
                return SpeechLanguage(locale: found, module: .speech)
            }
            if let found = await dictation(wanted) {
                return SpeechLanguage(locale: found, module: .dictation)
            }
        }
        for identifier in candidates {
            let candidate = Locale(identifier: identifier)
            if let found = await speech(candidate) {
                return SpeechLanguage(locale: found, module: .speech)
            }
            if let found = await dictation(candidate) {
                return SpeechLanguage(locale: found, module: .dictation)
            }
        }
        return nil
    }

    /// The same language and, when both name one, the same region.
    private static func sameLanguage(_ a: Locale, _ b: Locale) -> Bool {
        guard a.language.languageCode == b.language.languageCode else { return false }
        guard let ra = a.region, let rb = b.region else { return true }
        return ra == rb
    }

    /// "Norwegian Bokmål (Norway)" in the iPhone's own language.
    static func displayName(for identifier: String, in display: Locale = .current) -> String {
        display.localizedString(forIdentifier: identifier)
            ?? Locale(identifier: identifier).localizedString(forIdentifier: identifier)
            ?? identifier
    }

    /// Short names for a list of locales: the language alone where it has one region in the
    /// list ("Norwegian Bokmål"), the full name where it has several ("English (Canada)").
    static func shortNames(for identifiers: [String], in display: Locale = .current) -> [String: String] {
        var regionsPerLanguage: [String: Int] = [:]
        for identifier in identifiers {
            let code = Locale(identifier: identifier).language.languageCode?.identifier ?? identifier
            regionsPerLanguage[code, default: 0] += 1
        }
        var names: [String: String] = [:]
        for identifier in identifiers {
            let code = Locale(identifier: identifier).language.languageCode?.identifier ?? identifier
            if regionsPerLanguage[code] == 1, let language = display.localizedString(forLanguageCode: code) {
                names[identifier] = language.prefix(1).uppercased() + language.dropFirst()
            } else {
                names[identifier] = displayName(for: identifier, in: display)
            }
        }
        return names
    }

    /// Whether a language identifier is English, for the English-only steps (the cleanup
    /// tagger). nil (unknown) counts as English, as before languages existed.
    static func isEnglish(_ identifier: String?) -> Bool {
        guard let identifier else { return true }
        return Locale(identifier: identifier).language.languageCode?.identifier == "en"
    }
}
