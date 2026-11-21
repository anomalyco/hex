import Foundation

/// The keyboard's quick dictation-language switch: which languages it offers and the names
/// it shows for them. The choice is Settings › Dictation language (`LanguageSettings`).
nonisolated enum DictationLanguageSwitch {
    static let maxChoices = 5

    /// Up to five languages, best first: the one set now, the ones picked recently, Hindi while
    /// Experiments › Hinglish is on (shown as Hinglish), then the iPhone's preferred languages.
    /// Each is matched to a language Apple Speech has (`supported`, BCP 47) when that list is
    /// known, and dropped when it has none. One variant per language: the first of es-US and
    /// es-ES wins, so no two chips mean the same language. Languages taken off the switch
    /// (`removed`, as it listed them) stay off, except the one set now. `region` is the
    /// iPhone's Region, which Spanish without one of its own follows.
    static func choices(
        current: String?,
        recent: [String],
        preferred: [String],
        supported: [String],
        hinglish: Bool = false,
        removed: [String] = [],
        region: String? = Locale.current.region?.identifier
    ) -> [String] {
        var result: [String] = []
        var languages = Set<String>()
        if let current, let match = supportedMatch(for: current, in: supported, region: region) {
            languages.insert(languageCode(of: match) ?? match)
            result.append(match)
        }
        func offer(_ identifier: String) {
            guard result.count < maxChoices,
                  let match = supportedMatch(for: identifier, in: supported, region: region),
                  !removed.contains(match),
                  languages.insert(languageCode(of: match) ?? match).inserted else { return }
            result.append(match)
        }
        let offered = hinglish ? [HinglishSettings.language] : []
        for identifier in recent + offered + preferred {
            guard result.count < maxChoices else { break }
            offer(identifier)
        }
        return result
    }

    /// The keyboard's More languages: every other language Apple Speech has, the iPhone's
    /// preferred ones first (with their other regions), then the rest by name.
    static func more(
        choices: [String],
        preferred: [String],
        supported: [String],
        hinglish: HinglishSettings = HinglishSettings(),
        region: String? = Locale.current.region?.identifier
    ) -> [String] {
        guard !supported.isEmpty else { return [] }
        var first: [String] = []
        for identifier in preferred {
            guard let match = supportedMatch(for: identifier, in: supported, region: region),
                  !choices.contains(match), !first.contains(match) else { continue }
            first.append(match)
        }
        let codes = preferred.compactMap { Locale(identifier: $0).language.languageCode?.identifier }
        let rest = supported.filter { !choices.contains($0) && !first.contains($0) }
        let names = nativeNames(for: supported, hinglish: hinglish)
        func byName(_ a: String, _ b: String) -> Bool {
            (names[a] ?? a).localizedStandardCompare(names[b] ?? b) == .orderedAscending
        }
        func rank(_ id: String) -> Int? {
            Locale(identifier: id).language.languageCode.flatMap { codes.firstIndex(of: $0.identifier) }
        }
        let regions = rest.filter { rank($0) != nil }.sorted { a, b in
            rank(a) == rank(b) ? byName(a, b) : rank(a)! < rank(b)!
        }
        return first + regions + rest.filter { rank($0) == nil }.sorted(by: byName)
    }

    /// The supported locale for an identifier: the same region, else the language's usual
    /// region ("es" is Spain's), else any. Nynorsk maps to Bokmål. With no list, itself.
    /// Spanish follows `spanishCandidates`: a Latin American, United States or other region
    /// Apple doesn't offer is the Americas' (es-US, then es-MX) before Spain's, and plain
    /// Spanish is whichever of those the iPhone's `region` calls for.
    static func supportedMatch(
        for identifier: String,
        in supported: [String],
        region: String? = Locale.current.region?.identifier
    ) -> String? {
        let wanted = Locale(identifier: identifier)
        guard !supported.isEmpty else { return wanted.identifier(.bcp47) }
        var code = wanted.language.languageCode?.identifier
        if code == "nn" || code == "no" { code = "nb" }
        let candidates = supported.filter { Locale(identifier: $0).language.languageCode?.identifier == code }
        guard !candidates.isEmpty, let code else { return nil }
        if code == "es" {
            for fallback in spanishCandidates(for: identifier, region: region) {
                let wantedRegion = Locale(identifier: fallback).region?.identifier
                if let exact = candidates.first(where: { Locale(identifier: $0).region?.identifier == wantedRegion }) {
                    return exact
                }
            }
            return candidates.first
        }
        if let region = wanted.region?.identifier,
           let exact = candidates.first(where: { Locale(identifier: $0).region?.identifier == region }) {
            return exact
        }
        let usual = Locale.Language(identifier: Locale.Language(identifier: code).maximalIdentifier).region?.identifier
        return candidates.first { Locale(identifier: $0).region?.identifier == usual } ?? candidates.first
    }

    /// The iPhone's regions whose Spanish Apple Speech calls Latin American: es-US serves the
    /// United States, Puerto Rico and Latin America, and Mexico has es-MX.
    private static let latinAmericanSpanishRegions: Set<String> = [
        "419", "AR", "BO", "CL", "CO", "CR", "CU", "DO", "EC", "SV", "GT", "HN", "NI", "PA",
        "PY", "PE", "PR", "US", "UY", "VE",
    ]

    /// The Spanish variant the iPhone's Region dictates in when nothing else names one: es-MX
    /// for Mexico, es-US for the United States, Puerto Rico and Latin America, else es-ES.
    /// `region` is `Locale.current.region` where the caller has not been handed one.
    static func spanish(forRegion region: String?) -> String {
        let code = region?.uppercased()
        if code == "MX" { return "es-MX" }
        if let code, latinAmericanSpanishRegions.contains(code) { return "es-US" }
        return "es-ES"
    }

    /// Apple's Spanish locales for an identifier, best first: its own (Apple may have it), then
    /// es-US and es-MX for a region that is neither Spain's nor Equatorial Guinea's, and Spain's
    /// es-ES. Plain Spanish (no region) starts from the iPhone's Region's variant.
    static func spanishCandidates(
        for identifier: String,
        region: String? = Locale.current.region?.identifier
    ) -> [String] {
        let wanted = Locale(identifier: identifier).region?.identifier
        var result: [String] = []
        if let wanted {
            result = ["es-\(wanted)"]
            if wanted == "ES" || wanted == "GQ" {
                result.append("es-ES")
            } else {
                result += ["es-US", "es-MX", "es-ES"]
            }
        } else {
            result = [spanish(forRegion: region), "es-US", "es-MX", "es-ES"]
        }
        var seen = Set<String>()
        return result.filter { seen.insert(Locale(identifier: $0).identifier(.bcp47)).inserted }
    }

    /// The language an identifier belongs to, with Apple's aliases: Nynorsk and Norwegian are
    /// Bokmål, and a script (Srpski's Latin) stays out of it.
    static func languageCode(of identifier: String) -> String? {
        let code = Locale(identifier: identifier).language.languageCode?.identifier
        return code == "nn" || code == "no" ? "nb" : code
    }

    /// The language's own short name: "English", "Norsk", "Español", "Deutsch", "עברית",
    /// "Русский", and "Srpski" in its script for Serbian (Latin). Hindi is "Hinglish" while
    /// Experiments › Hinglish writes it in Latin letters. `supported` (what Apple Speech has
    /// here) adds the region where it offers more than one of the language, as Apple's own
    /// keyboards do: "Español (US)" while it also has Spain's, "English (GB)"; Norwegian stays
    /// "Norsk" because it has only one.
    static func nativeName(for identifier: String) -> String {
        nativeName(for: identifier, supported: [])
    }

    static func nativeName(for identifier: String, hinglish: HinglishSettings) -> String {
        nativeName(for: identifier, supported: [], hinglish: hinglish)
    }

    static func nativeName(
        for identifier: String,
        supported: [String],
        hinglish: HinglishSettings = HinglishSettings()
    ) -> String {
        nativeName(for: identifier, among: supported, hinglish: hinglish)
    }

    /// Native names for a list, with the region code only where two share a language:
    /// "English (US)", "English (GB)", "Norsk". `supported` is Apple Speech's list here, so a
    /// language with another region it doesn't show still names the one in the list.
    static func nativeNames(for identifiers: [String], hinglish: HinglishSettings = HinglishSettings()) -> [String: String] {
        nativeNames(for: identifiers, supported: [], hinglish: hinglish)
    }

    static func nativeNames(
        for identifiers: [String],
        supported: [String],
        hinglish: HinglishSettings = HinglishSettings()
    ) -> [String: String] {
        let variants = supported.isEmpty ? identifiers : supported
        var names: [String: String] = [:]
        for identifier in identifiers {
            names[identifier] = nativeName(for: identifier, among: variants, hinglish: hinglish)
        }
        return names
    }

    private static func nativeName(
        for identifier: String,
        among variants: [String],
        hinglish: HinglishSettings
    ) -> String {
        let name = nativeLanguageName(for: identifier, hinglish: hinglish)
        guard showsRegion(for: identifier, among: variants),
              let region = Locale(identifier: identifier).region?.identifier else { return name }
        return "\(name) (\(region))"
    }

    private static func nativeLanguageName(for identifier: String, hinglish: HinglishSettings) -> String {
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

    /// Whether Apple Speech has more than one region of the language `identifier` belongs to.
    private static func showsRegion(for identifier: String, among supported: [String]) -> Bool {
        let code = languageCode(of: identifier) ?? identifier
        var regions = Set<String>()
        for other in supported {
            guard languageCode(of: other) == code,
                  let region = Locale(identifier: other).region?.identifier else { continue }
            regions.insert(region)
        }
        return regions.count > 1
    }
}
