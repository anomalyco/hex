import Foundation

/// The keyboard's quick dictation-language switch: which languages it offers and the names
/// it shows for them. The choice is Settings › Dictation language (`LanguageSettings`).
nonisolated enum DictationLanguageSwitch {
    static let maxChoices = 5

    /// Up to five languages, best first: the one set now, the ones picked recently, Hindi while
    /// Experiments › Hinglish is on (shown as Hinglish), then the iPhone's preferred languages.
    /// Each is matched to a language Apple Speech has (`supported`, BCP 47) when that list is
    /// known, and dropped when it has none.
    static func choices(
        current: String?,
        recent: [String],
        preferred: [String],
        supported: [String],
        hinglish: Bool = false
    ) -> [String] {
        var result: [String] = []
        let offered = hinglish ? [HinglishSettings.language] : []
        for identifier in [current].compactMap({ $0 }) + recent + offered + preferred {
            guard let match = supportedMatch(for: identifier, in: supported),
                  !result.contains(match) else { continue }
            result.append(match)
            if result.count == maxChoices { break }
        }
        return result
    }

    /// The supported locale for an identifier: the same region, else the language's usual
    /// region ("es" is Spain's), else any. Nynorsk maps to Bokmål. With no list, itself.
    static func supportedMatch(for identifier: String, in supported: [String]) -> String? {
        let wanted = Locale(identifier: identifier)
        guard !supported.isEmpty else { return wanted.identifier(.bcp47) }
        var code = wanted.language.languageCode?.identifier
        if code == "nn" || code == "no" { code = "nb" }
        let candidates = supported.filter { Locale(identifier: $0).language.languageCode?.identifier == code }
        guard !candidates.isEmpty, let code else { return nil }
        if let region = wanted.region?.identifier,
           let exact = candidates.first(where: { Locale(identifier: $0).region?.identifier == region }) {
            return exact
        }
        let usual = Locale.Language(identifier: Locale.Language(identifier: code).maximalIdentifier).region?.identifier
        return candidates.first { Locale(identifier: $0).region?.identifier == usual } ?? candidates.first
    }

    /// The language's own short name: "English", "Norsk", "Español", "Deutsch", "עברית",
    /// "Русский", and "Srpski" in its script for Serbian (Latin). Hindi is "Hinglish" while
    /// Experiments › Hinglish writes it in Latin letters.
    static func nativeName(for identifier: String) -> String {
        nativeName(for: identifier, hinglish: HinglishSettings())
    }

    static func nativeName(for identifier: String, hinglish: HinglishSettings) -> String {
        if let name = hinglish.name(for: identifier) { return name }
        let language = Locale(identifier: identifier).language
        let code = language.languageCode?.identifier ?? identifier
        switch code {
        case "nb", "no": return "Norsk"
        case "nn": return "Nynorsk"
        default: break
        }
        let own = Locale(identifier: language.script.map { "\(code)-\($0.identifier)" } ?? code)
        guard let name = own.localizedString(forLanguageCode: code) else { return identifier }
        return name.prefix(1).uppercased(with: own) + name.dropFirst()
    }

    /// Native names for a list, with the region code only where two share a language:
    /// "English (US)", "English (GB)", "Norsk".
    static func nativeNames(for identifiers: [String], hinglish: HinglishSettings = HinglishSettings()) -> [String: String] {
        var perLanguage: [String: Int] = [:]
        for identifier in identifiers {
            perLanguage[nativeName(for: identifier, hinglish: hinglish), default: 0] += 1
        }
        var names: [String: String] = [:]
        for identifier in identifiers {
            let name = nativeName(for: identifier, hinglish: hinglish)
            if perLanguage[name, default: 0] > 1, let region = Locale(identifier: identifier).region?.identifier {
                names[identifier] = "\(name) (\(region))"
            } else {
                names[identifier] = name
            }
        }
        return names
    }
}
