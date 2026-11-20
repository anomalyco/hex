import Foundation

/// Settings › Dictation language and Experiments › Full keyboard › Keyboard layout, in the
/// App Group so the keyboard reads them and can switch the language.
nonisolated struct LanguageSettings {
    private enum Key {
        static let dictation = "settings.dictationLanguage"
        static let resolvedDictation = "settings.dictationLanguage.resolved"
        static let automaticDictation = "settings.dictationLanguage.automatic"
        static let recent = "settings.dictationLanguage.recent"
        static let supported = "settings.dictationLanguage.supported"
        static let keyboardLayout = "settings.keyboardLayout"
        static let speechModuleFailed = "settings.dictationLanguage.speechModuleFailed"
        static let unavailable = "settings.dictationLanguage.unavailable"
    }

    static let maxRecent = 5
    /// How long a language whose speech model wouldn't install stays set aside before Hex
    /// tries it again: Apple's downloads that fail tend to keep failing for days.
    static let setAsideDuration: TimeInterval = 3 * 24 * 60 * 60

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

    /// The full keyboard's letters, or nil for Automatic (follow the dictation language).
    var keyboardLayout: KeyboardLanguage? {
        get { defaults?.string(forKey: Key.keyboardLayout).flatMap(KeyboardLanguage.init(rawValue:)) }
        nonmutating set { defaults?.set(newValue?.rawValue, forKey: Key.keyboardLayout) }
    }

    /// The language dictation uses, when known: the one set, else what Automatic picks,
    /// else the last one it ran in.
    var effectiveDictationLanguage: String? {
        dictationLanguage ?? automaticDictationLanguage ?? resolvedDictationLanguage
    }

    /// `SpeechTranscriber`'s model for this language (BCP 47) wouldn't install, so dictation
    /// uses `DictationTranscriber`'s for a while.
    func markSpeechModuleFailed(_ identifier: String, at now: Date = Date()) {
        mark(identifier, key: Key.speechModuleFailed, at: now)
    }

    func speechModuleFailed(_ identifier: String, now: Date = Date()) -> Bool {
        isMarked(identifier, key: Key.speechModuleFailed, now: now)
    }

    /// Neither module's model for this language (BCP 47) would install while online, so
    /// Automatic passes over it for a while. An explicit choice still tries it.
    func markUnavailable(_ identifier: String, at now: Date = Date()) {
        mark(identifier, key: Key.unavailable, at: now)
    }

    func isUnavailable(_ identifier: String, now: Date = Date()) -> Bool {
        isMarked(identifier, key: Key.unavailable, now: now)
    }

    /// A language that installed after all.
    func clearUnavailable(_ identifier: String) {
        let key = Locale(identifier: identifier).identifier(.bcp47)
        guard var marks = defaults?.dictionary(forKey: Key.unavailable) as? [String: Double],
              marks.removeValue(forKey: key) != nil else { return }
        defaults?.set(marks, forKey: Key.unavailable)
    }

    private func mark(_ identifier: String, key: String, at now: Date) {
        var marks = (defaults?.dictionary(forKey: key) as? [String: Double]) ?? [:]
        marks = marks.filter { now.timeIntervalSince1970 - $0.value < Self.setAsideDuration }
        marks[Locale(identifier: identifier).identifier(.bcp47)] = now.timeIntervalSince1970
        defaults?.set(marks, forKey: key)
    }

    private func isMarked(_ identifier: String, key: String, now: Date) -> Bool {
        guard let marks = defaults?.dictionary(forKey: key) as? [String: Double],
              let at = marks[Locale(identifier: identifier).identifier(.bcp47)] else { return false }
        return now.timeIntervalSince1970 - at < Self.setAsideDuration
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
    /// `speech` and `dictation` return each module's equivalent locale, or nil. Automatic
    /// passes over a language `skip` names (one whose model wouldn't install here), so it
    /// lands on the next working one; an explicit choice is tried as it is.
    static func pick(
        explicit: String?,
        candidates: [String],
        speech: (Locale) async -> Locale?,
        dictation: (Locale) async -> Locale?,
        skip: (Locale) -> Bool = { _ in false }
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
                if skip(found) { continue }
                return SpeechLanguage(locale: found, module: .speech)
            }
            if let found = await dictation(candidate), !skip(found) {
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
