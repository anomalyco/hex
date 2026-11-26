import Foundation

/// Settings › Dictation language and Settings › Keyboard › Languages, in the App Group so the
/// keyboard reads them and can switch the language.
nonisolated struct LanguageSettings {
    private enum Key {
        static let dictation = "settings.dictationLanguage"
        static let resolvedDictation = "settings.dictationLanguage.resolved"
        static let automaticDictation = "settings.dictationLanguage.automatic"
        static let recent = "settings.dictationLanguage.recent"
        static let removed = "settings.dictationLanguage.removed"
        static let supported = "settings.dictationLanguage.supported"
        /// One layout, or none for Automatic: the setting before Languages, still written
        /// with the first language for an older build.
        static let keyboardLayout = "settings.keyboardLayout"
        static let keyboardLanguages = "settings.keyboardLanguages"
        static let currentKeyboardLanguage = "settings.keyboardLanguages.current"
        static let letterLayouts = "settings.keyboardLanguages.layouts"
        static let dictationFollowsKeyboard = "settings.keyboardLanguages.switchDictation"
        static let speechModuleFailed = "settings.dictationLanguage.speechModuleFailed"
        static let unavailable = "settings.dictationLanguage.unavailable"
        static let keyboardNeedsHex = "settings.dictationLanguage.keyboardNeedsHex"
        /// Set with `keyboardNeedsHex` when Hex found Apple has no speech model for the
        /// language on this iPhone, rather than one the keyboard couldn't get.
        static let keyboardLanguageUnavailable = "settings.dictationLanguage.keyboardNeedsHex.unavailable"
        /// First failures, which set nothing aside until a second in a row.
        static let speechModuleStrikes = "settings.dictationLanguage.speechModuleStrikes"
        static let unavailableStrikes = "settings.dictationLanguage.unavailableStrikes"
        static let keyboardModelStrike = "settings.dictationLanguage.keyboardModelStrike"
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

    /// Languages taken off the keyboard's quick switch (as it listed them), so the iPhone's own
    /// languages stay off it too, until picked again.
    var removedDictationLanguages: [String] {
        get { defaults?.stringArray(forKey: Key.removed) ?? [] }
        nonmutating set { defaults?.set(newValue, forKey: Key.removed) }
    }

    /// The keyboard's quick switch (`DictationLanguageSwitch.choices`), for the keyboard and
    /// Settings › Dictation alike.
    func keyboardLanguageChoices(
        preferred: [String] = Locale.preferredLanguages,
        hinglish: Bool = HinglishSettings().isEnabled,
        greeklish: Bool = GreeklishSettings().isEnabled,
        region: String? = Locale.current.region?.identifier
    ) -> [String] {
        DictationLanguageSwitch.choices(
            current: dictationLanguage,
            recent: recentDictationLanguages,
            preferred: preferred,
            supported: supportedDictationLanguages,
            hinglish: hinglish,
            greeklish: greeklish,
            removed: removedDictationLanguages,
            region: region
        )
    }

    /// Takes a language off the keyboard's quick switch: out of the recents, and kept off
    /// while it is one of the iPhone's languages. The one set goes back to Automatic.
    func removeDictationLanguage(_ identifier: String) {
        let supported = supportedDictationLanguages
        func same(_ other: String) -> Bool {
            other == identifier || DictationLanguageSwitch.supportedMatch(for: other, in: supported) == identifier
        }
        recentDictationLanguages = recentDictationLanguages.filter { !same($0) }
        if !removedDictationLanguages.contains(identifier) {
            removedDictationLanguages.append(identifier)
        }
        if let current = dictationLanguage, same(current) {
            dictationLanguage = nil
        }
    }

    /// Every language Apple Speech transcribes here (BCP 47), as last listed, so the
    /// keyboard can offer only those without asking Speech.
    var supportedDictationLanguages: [String] {
        get { defaults?.stringArray(forKey: Key.supported) ?? [] }
        nonmutating set { defaults?.set(newValue, forKey: Key.supported) }
    }

    /// Settings › Keyboard › Languages: the full keyboard's languages in order, the space bar
    /// switching between them; empty is Automatic (the dictation language's layout). Until
    /// it is first written, the one layout chosen before (`settings.keyboardLayout`).
    var keyboardLanguages: [KeyboardLanguage] {
        get {
            let stored = defaults?.stringArray(forKey: Key.keyboardLanguages)
                ?? defaults?.string(forKey: Key.keyboardLayout).map { [$0] }
                ?? []
            var seen = Set<KeyboardLanguage>()
            let koreanEnabled = KoreanKeyboardExperiment.isEnabled(in: defaults)
            return stored.compactMap(KeyboardLanguage.init(rawValue:))
                .filter { seen.insert($0).inserted }
                // 한국어 is ignored while its experiment is off, even if it was stored before.
                .filter { $0 != .korean || koreanEnabled }
        }
        nonmutating set {
            var seen = Set<KeyboardLanguage>()
            let languages = newValue.filter { seen.insert($0).inserted }
            defaults?.set(languages.map(\.rawValue), forKey: Key.keyboardLanguages)
            defaults?.set(languages.first?.rawValue, forKey: Key.keyboardLayout)
        }
    }

    /// Settings › Keyboard › Languages › Layout: how each language arranges its letters
    /// (`KeyboardLetterLayout`), by language code. A language never set, or set to one it no
    /// longer offers, types on its own layout, as it always did.
    func letterLayout(for language: KeyboardLanguage) -> KeyboardLetterLayout {
        let stored = letterLayouts[language.rawValue].flatMap(KeyboardLetterLayout.init(rawValue:))
        return language.offered(stored)
    }

    func setLetterLayout(_ letters: KeyboardLetterLayout, for language: KeyboardLanguage) {
        var all = letterLayouts
        all[language.rawValue] = language.offered(letters).rawValue
        defaults?.set(all, forKey: Key.letterLayouts)
    }

    /// Every language's chosen layout as stored (code to `KeyboardLetterLayout` raw value).
    var letterLayouts: [String: String] {
        defaults?.dictionary(forKey: Key.letterLayouts) as? [String: String] ?? [:]
    }

    /// The language the space bar last switched to, so the keyboard comes back in it.
    var currentKeyboardLanguage: KeyboardLanguage? {
        get {
            guard let language = defaults?.string(forKey: Key.currentKeyboardLanguage)
                .flatMap(KeyboardLanguage.init(rawValue:)) else { return nil }
            let koreanEnabled = KoreanKeyboardExperiment.isEnabled(in: defaults)
            return language == .korean && !koreanEnabled ? nil : language
        }
        nonmutating set { defaults?.set(newValue?.rawValue, forKey: Key.currentKeyboardLanguage) }
    }

    /// Settings › Keyboard › Languages › Switch dictation too: switching the keyboard's language
    /// on the space bar switches the dictation language to it. Off by default.
    var dictationFollowsKeyboard: Bool {
        get { defaults?.bool(forKey: Key.dictationFollowsKeyboard) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.dictationFollowsKeyboard) }
    }

    /// What Automatic dictates in while the keyboard switches between two or more languages
    /// and Switch dictation too is on: the one the space bar shows now, in the iPhone's region
    /// where Apple Speech has it (`KeyboardLanguage.dictationLanguage`), so Spanish speech is
    /// never transcribed as English, nor English as Spanish. A variant picked by hand for that
    /// language's keyboard wins over the preferred languages, so swiping back to Español
    /// returns to the Spanish picked last. Nil with a dictation language picked by hand, with
    /// one keyboard language, and with Switch dictation too off: from iOS 26, where Settings
    /// offers that switch, dictation then stays on Automatic's own language (TestFlight build
    /// 150: with English and Deutsch ticked and the keyboard left on Deutsch, English speech
    /// came out as German). Before iOS 26 the switch and the Dictation language menu aren't
    /// offered, so Automatic still follows the keyboard there, except where the iPhone's own
    /// language has no layout to type it (Hebrew) and dictating is how it gets written.
    func keyboardDictationLanguage(
        preferred: [String] = Locale.preferredLanguages,
        region: String? = Locale.current.region?.identifier,
        osMajorVersion: Int = ProcessInfo.processInfo.operatingSystemVersion.majorVersion
    ) -> String? {
        guard dictationLanguage == nil else { return nil }
        let chosen = keyboardLanguages
        guard chosen.count > 1 else { return nil }
        let koreanEnabled = KoreanKeyboardExperiment.isEnabled(in: defaults)
        if !dictationFollowsKeyboard {
            if osMajorVersion >= 26 { return nil }
            if let own = automaticDictationLanguage ?? preferred.first,
               KeyboardLanguage.matching(own, koreanEnabled: koreanEnabled) == nil {
                return nil
            }
        }
        let list = KeyboardLanguageList.resolve(chosen: chosen, stored: currentKeyboardLanguage, automatic: .english)
        return dictationLanguage(for: list.current, preferred: preferred, region: region)
    }

    /// What a keyboard language dictates in on Automatic: the variant picked for that
    /// language's keyboard last (`recentDictationLanguages`) ahead of the preferred languages,
    /// so swiping back to Español returns to the Spanish picked by hand.
    func dictationLanguage(
        for language: KeyboardLanguage,
        preferred: [String] = Locale.preferredLanguages,
        region: String? = Locale.current.region?.identifier
    ) -> String? {
        var preferred = preferred
        let koreanEnabled = KoreanKeyboardExperiment.isEnabled(in: defaults)
        if let picked = recentDictationLanguages.first(where: {
            KeyboardLanguage.matching($0, koreanEnabled: koreanEnabled)?.baseLanguage == language.baseLanguage
        }) {
            preferred.insert(picked, at: 0)
        }
        return language.dictationLanguage(
            supported: supportedDictationLanguages,
            preferred: preferred,
            region: region
        )
    }

    /// The one definition of the language dictation is meant to be in: the language picked by
    /// hand, else the keyboard's language Automatic follows, else the iPhone's first preferred
    /// language.
    func wantedDictationLanguage(
        preferred: [String] = Locale.preferredLanguages
    ) -> String? {
        dictationLanguage ?? keyboardDictationLanguage(preferred: preferred) ?? preferred.first
    }

    /// The language dictation uses, when known: the one set, else the keyboard's it follows
    /// (`keyboardDictationLanguage`), else what Automatic picks, else the last one it ran in.
    var effectiveDictationLanguage: String? {
        dictationLanguage ?? keyboardDictationLanguage() ?? automaticDictationLanguage ?? resolvedDictationLanguage
    }

    /// `SpeechTranscriber`'s model for this language (BCP 47) wouldn't install, so dictation
    /// uses `DictationTranscriber`'s for a while.
    func markSpeechModuleFailed(_ identifier: String, at now: Date = Date()) {
        mark(identifier, key: Key.speechModuleFailed, at: now)
    }

    func speechModuleFailed(_ identifier: String, now: Date = Date()) -> Bool {
        isMarked(identifier, key: Key.speechModuleFailed, now: now)
    }

    /// How long the keyboard hands dictation to Hex after it couldn't get a model itself.
    static let keyboardNeedsHexDuration: TimeInterval = 24 * 60 * 60

    /// In-keyboard dictation couldn't get its speech model ready (an asset released or
    /// removed since Hex downloaded it, a download the keyboard couldn't finish), so a
    /// dictation tap says so with Open Hex, which downloads it in the foreground; or, with
    /// `unavailable`, Hex found Apple has no model for the language on this iPhone, which
    /// the tap says without Open Hex. Until a model installs (`SpeechModelInstall`), a
    /// language is chosen, or a day passes.
    func markKeyboardNeedsHex(at now: Date = Date(), unavailable: Bool = false) {
        defaults?.set(now.timeIntervalSince1970, forKey: Key.keyboardNeedsHex)
        defaults?.set(unavailable, forKey: Key.keyboardLanguageUnavailable)
    }

    /// `keyboardNeedsHex` is because Apple has no speech model for the language here.
    var keyboardLanguageUnavailable: Bool {
        defaults?.bool(forKey: Key.keyboardLanguageUnavailable) ?? false
    }

    func keyboardNeedsHex(now: Date = Date()) -> Bool {
        guard let at = defaults?.object(forKey: Key.keyboardNeedsHex) as? Double else { return false }
        return now.timeIntervalSince1970 - at < Self.keyboardNeedsHexDuration
    }

    func clearKeyboardNeedsHex() {
        if defaults?.object(forKey: Key.keyboardModelStrike) != nil {
            defaults?.removeObject(forKey: Key.keyboardModelStrike)
        }
        guard defaults?.object(forKey: Key.keyboardNeedsHex) != nil else { return }
        defaults?.removeObject(forKey: Key.keyboardNeedsHex)
        defaults?.removeObject(forKey: Key.keyboardLanguageUnavailable)
    }

    /// How long a first failure waits for a second before it no longer counts.
    static let keyboardStrikeWindow: TimeInterval = 6 * 60 * 60

    /// The keyboard couldn't get its speech model ready. True the second time in a row (no
    /// model in place since, within `keyboardStrikeWindow`): then it hands dictation to Hex.
    func noteKeyboardModelFailure(at now: Date = Date()) -> Bool {
        if let first = defaults?.object(forKey: Key.keyboardModelStrike) as? Double,
           now.timeIntervalSince1970 - first < Self.keyboardStrikeWindow {
            defaults?.removeObject(forKey: Key.keyboardModelStrike)
            return true
        }
        defaults?.set(now.timeIntervalSince1970, forKey: Key.keyboardModelStrike)
        return false
    }

    /// `SpeechTranscriber`'s model for this language failed to install: set aside
    /// (`markSpeechModuleFailed`) the second time in a row, within `setAsideDuration`.
    func noteSpeechModuleFailure(_ identifier: String, at now: Date = Date()) {
        if strike(identifier, key: Key.speechModuleStrikes, at: now) {
            markSpeechModuleFailed(identifier, at: now)
        }
    }

    /// Neither module's model installed while online: set aside (`markUnavailable`) the
    /// second time in a row, within `setAsideDuration`.
    func noteUnavailable(_ identifier: String, at now: Date = Date()) {
        if strike(identifier, key: Key.unavailableStrikes, at: now) {
            markUnavailable(identifier, at: now)
        }
    }

    /// A model installed for this language: its failures so far no longer count.
    func clearFailureStrikes(_ identifier: String, speechModule: Bool) {
        let key = Locale(identifier: identifier).identifier(.bcp47)
        for strikes in speechModule ? [Key.speechModuleStrikes, Key.unavailableStrikes] : [Key.unavailableStrikes] {
            guard var marks = defaults?.dictionary(forKey: strikes) as? [String: Double],
                  marks.removeValue(forKey: key) != nil else { continue }
            defaults?.set(marks, forKey: strikes)
        }
    }

    /// Records a first failure and returns false, or clears it and returns true for a second.
    private func strike(_ identifier: String, key: String, at now: Date) -> Bool {
        if isMarked(identifier, key: key, now: now) {
            var marks = (defaults?.dictionary(forKey: key) as? [String: Double]) ?? [:]
            marks.removeValue(forKey: Locale(identifier: identifier).identifier(.bcp47))
            defaults?.set(marks, forKey: key)
            return true
        }
        mark(identifier, key: key, at: now)
        return false
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

    /// A choice made in Hex or the keyboard: the setting, and the front of the recents. The
    /// other variants of that language leave the recents, so es-US and es-ES never both take
    /// a chip (`DictationLanguageSwitch.choices` offers one per language); Norwegian's
    /// nb/nn/no aliases are one language, as Hinglish's Hindi is one, and Chinese's variants
    /// stay apart (`DictationLanguageSwitch.variantKey`), so picking Mandarin's zh-CN leaves
    /// Cantonese's zh-HK in the recents.
    func chooseDictationLanguage(_ identifier: String?) {
        dictationLanguage = identifier
        clearKeyboardNeedsHex()
        if let identifier {
            let key = DictationLanguageSwitch.variantKey(of: identifier)
            recentDictationLanguages = [identifier] + recentDictationLanguages.filter {
                guard $0 != identifier else { return false }
                guard let key else { return true }
                return DictationLanguageSwitch.variantKey(of: $0) != key
            }
            let match = DictationLanguageSwitch.supportedMatch(for: identifier, in: supportedDictationLanguages)
            let removed = removedDictationLanguages
            if removed.contains(where: { $0 == identifier || $0 == match }) {
                removedDictationLanguages = removed.filter { $0 != identifier && $0 != match }
            }
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
    /// Spanish tries its variants (`DictationLanguageSwitch.spanishCandidates`) in order:
    /// es-US and es-MX before Spain's for a Latin American, United States or other region, and
    /// the iPhone's `region`'s variant when the preferred Spanish names none.
    static func automaticCandidates(
        preferred: [String],
        current: String,
        region: String? = Locale.current.region?.identifier
    ) -> [String] {
        var result: [String] = []
        var seen = Set<String>()
        func add(_ identifier: String) {
            let key = Locale(identifier: identifier).identifier(.bcp47)
            if seen.insert(key).inserted {
                result.append(identifier)
            }
        }
        for identifier in preferred + [current, "en-US"] {
            if Locale(identifier: identifier).language.languageCode?.identifier == "es" {
                for candidate in DictationLanguageSwitch.spanishCandidates(for: identifier, region: region) {
                    add(candidate)
                }
                continue
            }
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
    /// `speech` and `dictation` return each module's equivalent locale, or nil. An explicit
    /// Spanish choice tries its variants in order too (`spanishCandidates`), and a module's
    /// Spanish answer must name the region asked for, so a module resolving es-419 to Spain's
    /// is passed over for es-US. Automatic passes over a language `skip` names (one whose
    /// model wouldn't install here), so it lands on the next working one; an explicit choice
    /// is tried as it is. When every language either module has was set aside (an English-only
    /// iPhone whose English model failed once), Automatic tries the first of them again: nil
    /// means neither module has any of them, never that Hex gave up on the only one there is.
    static func pick(
        explicit: String?,
        candidates: [String],
        region: String? = Locale.current.region?.identifier,
        speech: (Locale) async -> Locale?,
        dictation: (Locale) async -> Locale?,
        skip: (Locale) -> Bool = { _ in false }
    ) async -> SpeechLanguage? {
        if let explicit, !explicit.isEmpty {
            let spanish = Locale(identifier: explicit).language.languageCode?.identifier == "es"
            let wanted = spanish
                ? DictationLanguageSwitch.spanishCandidates(for: explicit, region: region)
                : [explicit]
            for identifier in wanted {
                let candidate = Locale(identifier: identifier)
                if let found = await speech(candidate), sameLanguage(found, candidate) {
                    return SpeechLanguage(locale: found, module: .speech)
                }
                if let found = await dictation(candidate), !spanish || sameLanguage(found, candidate) {
                    return SpeechLanguage(locale: found, module: .dictation)
                }
            }
        }
        var setAside: SpeechLanguage?
        for identifier in candidates {
            let candidate = Locale(identifier: identifier)
            let spanish = Locale(identifier: identifier).language.languageCode?.identifier == "es"
            if let found = await speech(candidate), !spanish || sameLanguage(found, candidate) {
                let language = SpeechLanguage(locale: found, module: .speech)
                guard skip(found) else { return language }
                setAside = setAside ?? language
                continue
            }
            if let found = await dictation(candidate), !spanish || sameLanguage(found, candidate) {
                let language = SpeechLanguage(locale: found, module: .dictation)
                guard skip(found) else { return language }
                setAside = setAside ?? language
            }
        }
        return setAside
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

    /// Short names in the app's language, as the keyboard names languages in their own
    /// (`DictationLanguageSwitch.named`): the language alone where `supported` (Apple
    /// Speech's list here; else `identifiers`) has one region of it ("Norwegian Bokmål"), with
    /// its region code where it has several ("English (US)", "French (FR)").
    static func shortNames(
        for identifiers: [String],
        among supported: [String] = [],
        in display: Locale = .current
    ) -> [String: String] {
        let variants = supported.isEmpty ? identifiers : supported
        var names: [String: String] = [:]
        for identifier in identifiers {
            let code = Locale(identifier: identifier).language.languageCode?.identifier ?? identifier
            guard let language = display.localizedString(forLanguageCode: code) else {
                names[identifier] = displayName(for: identifier, in: display)
                continue
            }
            let name = language.prefix(1).uppercased() + language.dropFirst()
            names[identifier] = DictationLanguageSwitch.named(name, for: identifier, among: variants)
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
