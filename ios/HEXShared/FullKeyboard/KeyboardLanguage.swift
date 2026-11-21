import Foundation

/// The full keyboard's letters. Each language is its letter rows, its long-press accents,
/// and the spelling dictionary its autocorrect uses, matching Apple's keyboard for it;
/// another language is a new case with those filled in. Italian and Portuguese type on the
/// English letters (Apple's keyboards for them are QWERTY) with their own accents and
/// dictionary. French is Apple's AZERTY, as on its French (France) and (Belgique) keyboards,
/// Canadian French QWERTY, as on Apple's French (Canada), and Swiss French QWERTZ, as on
/// Apple's French (Suisse). Russian is Apple's ЙЦУКЕН (ё on a held е, ъ on a
/// held ь); Ukrainian is the standard Ukrainian ЙЦУКЕН (ДСТУ 4862:2007) in the twelve-key
/// rows the iPhone's own Ukrainian keyboard shows (ї, є, ʼ and ґ on keys of their own, the
/// other apostrophes on a held ʼ, and no ы, э, ъ or ё); Serbian (Latin) is Apple's QWERTZ option for it, which on iPhone has no keys of
/// its own for č ć đ š ž: they are held on c, d, s and z, as on Apple's.
nonisolated enum KeyboardLanguage: String, CaseIterable, Sendable {
    case english = "en"
    case norwegian = "nb"
    case spanish = "es"
    case german = "de"
    case italian = "it"
    case french = "fr"
    case frenchCanadian = "fr-CA"
    case frenchSwiss = "fr-CH"
    case portuguese = "pt"
    case russian = "ru"
    case ukrainian = "uk"
    case serbianLatin = "sr-Latn"

    /// Its own name, as Apple's keyboard list shows it.
    var displayName: String {
        switch self {
        case .english: "English"
        case .norwegian: "Norsk"
        case .spanish: "Español"
        case .german: "Deutsch"
        case .italian: "Italiano"
        case .french: "Français (France)"
        case .frenchCanadian: "Français (Canada)"
        case .frenchSwiss: "Français (Suisse)"
        case .portuguese: "Português"
        case .russian: "Русский"
        case .ukrainian: "Українська"
        case .serbianLatin: "Srpski"
        }
    }

    /// Its name on the space bar: French without its region ("Français"), as only one French
    /// layout is usually chosen (`KeyboardLanguageList.spaceLabel` adds it back otherwise).
    var shortName: String {
        switch self {
        case .french, .frenchCanadian, .frenchSwiss: "Français"
        default: displayName
        }
    }

    /// The language its dictionary is for: Apple's three French layouts are one language.
    var baseLanguage: String {
        String(rawValue.prefix { $0 != "-" })
    }

    /// Latin letters, so another Latin-script language can be autocorrected on its keys
    /// (bilingual autocorrect). Russian's and Ukrainian's are Cyrillic.
    var isLatin: Bool { self != .russian && self != .ukrainian }

    /// The Apple Speech language (BCP 47) to dictate in while this layout types (Settings ›
    /// Keyboard › Languages › Switch dictation too): the iPhone's own region where it has one
    /// for the language (English on a British iPhone is en-GB), else the language's usual
    /// one, among `supported` (what Apple Speech transcribes here; empty when not listed yet).
    /// Nil when Apple Speech has none of it (Serbian).
    func dictationLanguage(supported: [String], preferred: [String]) -> String? {
        func key(_ identifier: String) -> String {
            identifier.replacingOccurrences(of: "_", with: "-").lowercased()
        }
        let usual = switch self {
        case .english: "en-US"
        case .norwegian: "nb-NO"
        case .spanish: "es-ES"
        case .german: "de-DE"
        case .italian: "it-IT"
        case .french: "fr-FR"
        case .frenchCanadian: "fr-CA"
        case .frenchSwiss: "fr-CH"
        case .portuguese: "pt-BR"
        case .russian: "ru-RU"
        case .ukrainian: "uk-UA"
        case .serbianLatin: "sr-Latn-RS"
        }
        // A French layout keeps its own region: Canadian French dictates fr-CA.
        let ownRegion = self == .frenchCanadian || self == .frenchSwiss
        let mine = ownRegion ? [] : preferred.filter { Self.matching($0)?.baseLanguage == baseLanguage }
        let candidates = mine + [usual]
        guard !supported.isEmpty else {
            return self == .serbianLatin ? nil : candidates.first
        }
        let offered = Dictionary(supported.map { (key($0), $0) }, uniquingKeysWith: { a, _ in a })
        for candidate in candidates {
            if let found = offered[key(candidate)] { return found }
        }
        if ownRegion { return nil }
        return supported.first { Self.matching($0)?.baseLanguage == baseLanguage }
    }

    /// The three letter rows, lowercase. Rows 1 and 2 may hold 11 keys (Apple's Nordic,
    /// German and Russian layouts) or 12 (Ukrainian); row 3 sits between shift and delete.
    var letterRows: [[String]] {
        let rows = switch self {
        case .english, .italian, .frenchCanadian, .portuguese: ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
        // Apple's French keyboards also hold a ´ dead key (AZERTY's row 3, the end of the
        // others' row 2), which its French and English keyboard leaves out (a TestFlight
        // tester's screenshot). Hex leaves it out too: accents are on a held letter.
        case .french: ["azertyuiop", "qsdfghjklm", "wxcvbn"]
        case .frenchSwiss: ["qwertzuiop", "asdfghjkl", "yxcvbnm"]
        case .norwegian: ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"]
        case .spanish: ["qwertyuiop", "asdfghjklñ", "zxcvbnm"]
        case .german: ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"]
        case .russian: ["йцукенгшщзх", "фывапролджэ", "ячсмитьбю"]
        // The standard Ukrainian ЙЦУКЕН (ДСТУ 4862:2007) in the rows the iPhone's own Ukrainian
        // keyboard shows (iOS 27.2 simulator): ї after х, є and the apostrophe ʼ (U+02BC, the
        // one Ukrainian spelling uses) after ж, ґ after ю, so shift and delete are one letter
        // wide.
        case .ukrainian: ["йцукенгшщзхї", "фівапролджєʼ", "ячсмитьбюґ"]
        case .serbianLatin: ["qwertzuiop", "asdfghjkl", "yxcvbnm"]
        }
        return rows.map { $0.map(String.init) }
    }

    /// Letters held for their accents, base letter first, where this language's keyboard
    /// differs from `AccentCatalog`'s shared table: letters with their own key drop out of
    /// the vowels' lists.
    var accentOverrides: [String: [String]] {
        switch self {
        case .english:
            [:]
        case .norwegian:
            // Æ and Ø offer the Swedish and German Ä and Ö.
            [
                "a": ["a", "à", "á", "â", "ä", "ã", "ā"],
                "o": ["o", "ô", "ö", "ò", "ó", "œ", "ō", "õ"],
                "æ": ["æ", "ä"],
                "ø": ["ø", "ö"],
            ]
        case .spanish:
            // Spanish accents first; ª and º with A and O.
            [
                "a": ["a", "á", "à", "ä", "â", "ã", "å", "ą", "æ", "ā", "ª"],
                "e": ["e", "é", "è", "ë", "ê", "ę", "ė", "ē"],
                "i": ["i", "í", "ï", "ì", "î", "į", "ī"],
                "o": ["o", "ó", "ò", "ö", "ô", "õ", "ø", "œ", "ō", "º"],
                "u": ["u", "ú", "ü", "ù", "û", "ū"],
                "n": ["n", "ñ", "ń"],
            ]
        case .german:
            // ß stays on a held S (the shared table).
            [
                "a": ["a", "à", "á", "â", "æ", "ã", "å", "ā"],
                "o": ["o", "ô", "ò", "ó", "œ", "ø", "ō", "õ"],
                "u": ["u", "û", "ù", "ú", "ū"],
            ]
        case .italian:
            // Grave accents first, as Italian writes them (però, più); the shared table
            // already leads with à, è (then é, for perché) and ì.
            [
                "o": ["o", "ò", "ó", "ô", "ö", "õ", "œ", "ø", "ō"],
                "u": ["u", "ù", "ú", "û", "ü", "ū"],
            ]
        case .french, .frenchCanadian, .frenchSwiss:
            // Apple's French lists (TextInput_fr), base letter first, as its popup shows them.
            [
                "e": ["e", "é", "è", "ê", "ë", "ę", "ė", "ē"],
                "a": ["a", "à", "â", "ª", "æ", "á", "ä", "ã", "å", "ā"],
                "i": ["i", "î", "ï", "ì", "í", "į", "ī"],
                "o": ["o", "ô", "œ", "º", "ö", "ò", "ó", "õ", "ø", "ō"],
                "u": ["u", "û", "ù", "ü", "ú", "ū"],
            ]
        case .portuguese:
            [
                "a": ["a", "á", "ã", "â", "à", "ä", "å", "æ", "ā", "ª"],
                "e": ["e", "é", "ê", "è", "ë", "ę", "ė", "ē"],
                "i": ["i", "í", "ì", "î", "ï", "į", "ī"],
                "o": ["o", "ó", "õ", "ô", "ò", "ö", "œ", "ø", "ō", "º"],
                "u": ["u", "ú", "ü", "ù", "û", "ū"],
            ]
        case .russian:
            // Apple's Russian lists (TextInput_ru), base letter first: ё on е, ъ on ь, and
            // the letters of other Cyrillic languages. Apple's combining-macron forms (а̄ е̄ о̄ с̄)
            // are left out.
            [
                "а": ["а", "ӕ", "ӑ", "ӓ"],
                "г": ["г", "ғ", "ҕ", "ӷ", "ґ"],
                "е": ["е", "ё", "ӗ", "є", "ѣ"],
                "ж": ["ж", "җ", "ӝ", "ӂ", "ѥ"],
                "з": ["з", "ҙ", "ӟ", "ӡ"],
                "и": ["и", "і", "ӥ", "ӣ"],
                "й": ["й", "ї", "ј", "ѵ"],
                "к": ["к", "ҡ", "қ", "ҟ", "ѯ", "ҝ"],
                "л": ["л", "ӆ"],
                "н": ["н", "ң", "ҥ", "ӈ"],
                "о": ["о", "ө", "ӧ", "ѡ"],
                "п": ["п", "ԥ", "ѱ"],
                "с": ["с", "ҫ"],
                "т": ["т", "ҭ"],
                "у": ["у", "ү", "ў", "ӱ", "ӳ", "ұ", "ӯ"],
                "ф": ["ф", "ѳ"],
                "х": ["х", "ҳ", "һ"],
                "ц": ["ц", "ҵ"],
                "ч": ["ч", "ҷ", "ӌ", "ӵ", "ҹ"],
                "ш": ["ш", "ҽ", "ҿ"],
                "ы": ["ы", "ӹ"],
                "ь": ["ь", "ъ", "ӏ", "ѫ"],
                "э": ["э", "ӭ", "ә"],
                "ю": ["ю", "ҩ", "ѭ"],
                "я": ["я", "џ", "ѧ"],
            ]
        case .ukrainian:
            // As the system keyboard's long-press options were observed (iOS 27.2 simulator):
            // only ʼ offers anything, the other apostrophes, base first; ґ, ї and є have keys
            // of their own. The hryvnia, which the system keyboard's currency key shows, is held
            // on $ (Hex's numbers page is the same in every language).
            [
                "ʼ": ["ʼ", "'", "’", "‘", "`"],
                "$": ["$", "₴", "€", "£", "¥", "¢"],
            ]
        case .serbianLatin:
            // Apple's Serbian (Latin) lists (TextInput_sr), base letter first.
            [
                "c": ["c", "č", "ć", "ç"],
                "d": ["d", "đ"],
                "s": ["s", "š", "ß", "ś"],
                "z": ["z", "ž", "ź", "ż"],
            ]
        }
    }

    /// `UITextChecker` language codes to try, best first.
    var spellCheckCandidates: [String] {
        switch self {
        case .english: ["en_US", "en"]
        case .norwegian: ["nb", "nb_NO", "no", "no_NO"]
        case .spanish: ["es", "es_ES", "es_MX"]
        case .german: ["de", "de_DE"]
        case .italian: ["it", "it_IT"]
        case .french: ["fr", "fr_FR", "fr_CA"]
        case .frenchCanadian: ["fr_CA", "fr", "fr_FR"]
        case .frenchSwiss: ["fr_CH", "fr", "fr_FR"]
        // Brazil's spelling first, Portugal's where only it is installed.
        case .portuguese: ["pt_BR", "pt", "pt_PT"]
        case .russian: ["ru", "ru_RU"]
        case .ukrainian: ["uk", "uk_UA"]
        // Latin only: a Cyrillic `sr` dictionary would reject every Latin word.
        case .serbianLatin: ["sr_Latn", "sr_Latn_RS", "sr-Latn"]
        }
    }

    /// The first candidate the iPhone has a dictionary for, or nil: then autocorrect and
    /// suggestions stay off rather than "correcting" this language into another.
    func spellCheckLanguage(available: [String]) -> String? {
        spellCheckCandidates.first { available.contains($0) }
    }

    /// The `UITextChecker` dictionary for a dictation language ("nb-NO", "fr-FR", "es-419"),
    /// or nil when the iPhone has none for it.
    static func spellCheckLanguage(forDictation identifier: String, available: [String]) -> String? {
        if let code = matching(identifier)?.spellCheckLanguage(available: available) {
            return code
        }
        let underscored = identifier.replacingOccurrences(of: "-", with: "_")
        let base = underscored.split(separator: "_").first.map(String.init) ?? underscored
        return [underscored, base].first { available.contains($0) }
            ?? available.first { $0.hasPrefix(base + "_") }
    }

    /// Whether the built-in English contractions and typo list apply.
    var usesEnglishFixes: Bool { self == .english }

    /// Two-letter words common enough that autocorrect fixes a two-letter non-word one
    /// neighbouring key from one of them ("tk" -> "to"), commonest first. Empty where no list
    /// has been chosen yet: then two-letter words are left alone, as before.
    var commonTwoLetterWords: [String] {
        switch self {
        case .english:
            ["to", "of", "in", "it", "is", "be", "as", "at", "so", "we", "he", "by", "or", "on",
             "do", "if", "me", "my", "up", "an", "go", "no", "us", "am"]
        case .spanish:
            ["de", "la", "el", "en", "no", "se", "lo", "es", "un", "su", "me", "te", "ya", "mi",
             "tu", "yo", "le", "si", "al", "ha", "ni"]
        case .italian:
            ["di", "il", "la", "in", "un", "si", "le", "lo", "ha", "ho", "ma", "da", "mi", "ci",
             "ti", "se", "ne", "no", "io", "tu", "su", "al"]
        case .norwegian, .german, .french, .frenchCanadian, .frenchSwiss, .portuguese, .russian,
             .ukrainian, .serbianLatin:
            []
        }
    }

    /// Each letter's (row, column) for autocorrect's neighbouring-key cost, with rows 2
    /// and 3 shifted as the keys are.
    var keyCoordinates: [Character: (Double, Double)] {
        var map: [Character: (Double, Double)] = [:]
        // AZERTY's row 2 runs edge to edge, and its row 3 starts under e (W under E).
        // Ukrainian's rows 1 and 2 are both twelve keys, and its row 3 starts under ц.
        let offsets = switch self {
        case .french: [0.0, 0.0, 2.0]
        case .ukrainian: [0.0, 0.0, 1.0]
        default: [0.0, 0.5, 1.5]
        }
        for (row, letters) in letterRows.enumerated() {
            for (column, letter) in letters.enumerated() {
                guard let character = letter.first else { continue }
                map[character] = (Double(row), Double(column) + offsets[row])
            }
        }
        return map
    }

    /// The layout for a language identifier ("nb-NO", "no", "nn_NO", "es-MX", "sr-Latn-RS"),
    /// if any. Serbian only in Latin script: Hex has no Cyrillic Serbian layout. French follows
    /// Apple's keyboard for the region: QWERTY in Canada, QWERTZ in Switzerland, else AZERTY.
    static func matching(_ identifier: String) -> KeyboardLanguage? {
        let parts = identifier.lowercased()
            .split(whereSeparator: { $0 == "-" || $0 == "_" })
            .map(String.init)
        let code = parts.first ?? ""
        switch code {
        case "nb", "no", "nn": return .norwegian
        case "en": return .english
        case "es": return .spanish
        case "de": return .german
        case "it": return .italian
        case "fr":
            let region = parts.dropFirst()
            if region.contains("ca") { return .frenchCanadian }
            if region.contains("ch") { return .frenchSwiss }
            return .french
        case "pt": return .portuguese
        case "ru": return .russian
        case "uk": return .ukrainian
        case "sr": return parts.contains("latn") ? .serbianLatin : nil
        default: return nil
        }
    }

    /// The layouts of the iPhone's own keyboards, then of its preferred languages, in that
    /// order and once each: what Settings › Keyboard › Languages offers first.
    static func suggested(inputModes: [String], preferred: [String]) -> [KeyboardLanguage] {
        var seen = Set<KeyboardLanguage>()
        return (inputModes + preferred).compactMap(matching).filter { seen.insert($0).inserted }
    }

    /// Automatic follows the dictation language: its layout, or English for a language
    /// without one (Hebrew, Greek). Before any dictation language is known, the first
    /// iPhone keyboard, then preferred language, that has a layout. Hex's own input mode
    /// says nothing here: its `primaryLanguage` is the Info.plist's en-US, whatever you type.
    static func automatic(
        dictationLanguage: String?,
        inputModes: [String],
        preferredLanguages: [String]
    ) -> KeyboardLanguage {
        if let dictationLanguage {
            return matching(dictationLanguage) ?? .english
        }
        for identifier in inputModes + preferredLanguages {
            if let language = matching(identifier) {
                return language
            }
        }
        return .english
    }
}

/// Settings › Keyboard › Languages as the keys use it: the languages the space bar switches
/// between, in order, and the one typing now.
nonisolated struct KeyboardLanguageList: Equatable, Sendable {
    let languages: [KeyboardLanguage]
    let current: KeyboardLanguage

    /// `chosen` empty is Automatic: one language, the dictation language's (`automatic`).
    /// `stored` is the language last switched to, kept only while it is still chosen; else
    /// the first one.
    static func resolve(
        chosen: [KeyboardLanguage],
        stored: KeyboardLanguage?,
        automatic: @autoclosure () -> KeyboardLanguage
    ) -> KeyboardLanguageList {
        var seen = Set<KeyboardLanguage>()
        let languages = chosen.filter { seen.insert($0).inserted }
        guard let first = languages.first else {
            let only = automatic()
            return KeyboardLanguageList(languages: [only], current: only)
        }
        let current = stored.flatMap { languages.contains($0) ? $0 : nil } ?? first
        return KeyboardLanguageList(languages: languages, current: current)
    }

    /// Two or more: the space bar names the language and switches it.
    var switches: Bool { languages.count > 1 }

    /// The language a swipe along the space bar goes to: `step` 1 the next, -1 the one
    /// before, round the list.
    func switched(by step: Int) -> KeyboardLanguage {
        guard let index = languages.firstIndex(of: current), !languages.isEmpty else { return current }
        let count = languages.count
        return languages[((index + step) % count + count) % count]
    }

    /// The space bar's label while it switches: the current language's short name, with
    /// its region only when another layout of it is chosen too ("Français (Canada)").
    var spaceLabel: String? {
        guard switches else { return nil }
        let twins = languages.filter { $0.baseLanguage == current.baseLanguage }.count
        return twins > 1 ? current.displayName : current.shortName
    }

    /// Bilingual autocorrect's second language while `current` types: the first other chosen
    /// language with Latin letters, another dictionary than `current`'s (French and
    /// Canadian French are one), and, like `current`, a dictionary on this iPhone
    /// (`available` is `UITextChecker.availableLanguages`). Nil for Cyrillic, or with one
    /// language: a layout without a dictionary (Serbian) would otherwise see every word as a
    /// typo of the other language.
    func partner(available: [String]) -> KeyboardLanguage? {
        guard current.isLatin, current.spellCheckLanguage(available: available) != nil else { return nil }
        return languages.first {
            $0.isLatin && $0.baseLanguage != current.baseLanguage
                && $0.spellCheckLanguage(available: available) != nil
        }
    }
}
