import Foundation

/// The full keyboard's letters. Each language is its letter rows, its long-press accents,
/// and the spelling dictionary its autocorrect uses, matching Apple's keyboard for it;
/// another language is a new case with those filled in. Italian, French and Portuguese
/// type on the English letters (Apple's Italian and Portuguese keyboards are QWERTY; Hex
/// has no AZERTY yet, so French is QWERTY too, as on Apple's Canadian French keyboard)
/// with their own accents and dictionary. Russian is Apple's ЙЦУКЕН (ё on a held е, ъ on a
/// held ь); Serbian (Latin) is Apple's QWERTZ option for it, which on iPhone has no keys of
/// its own for č ć đ š ž: they are held on c, d, s and z, as on Apple's.
nonisolated enum KeyboardLanguage: String, CaseIterable, Sendable {
    case english = "en"
    case norwegian = "nb"
    case spanish = "es"
    case german = "de"
    case italian = "it"
    case french = "fr"
    case portuguese = "pt"
    case russian = "ru"
    case serbianLatin = "sr-Latn"

    /// Its own name, as Apple's keyboard list shows it.
    var displayName: String {
        switch self {
        case .english: "English"
        case .norwegian: "Norsk"
        case .spanish: "Español"
        case .german: "Deutsch"
        case .italian: "Italiano"
        case .french: "Français"
        case .portuguese: "Português"
        case .russian: "Русский"
        case .serbianLatin: "Srpski"
        }
    }

    /// The three letter rows, lowercase. Rows 1 and 2 may hold 11 keys (Apple's Nordic,
    /// German and Russian layouts); row 3 sits between shift and delete.
    var letterRows: [[String]] {
        let rows = switch self {
        case .english, .italian, .french, .portuguese: ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
        case .norwegian: ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"]
        case .spanish: ["qwertyuiop", "asdfghjklñ", "zxcvbnm"]
        case .german: ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"]
        case .russian: ["йцукенгшщзх", "фывапролджэ", "ячсмитьбю"]
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
        case .french:
            [
                "e": ["e", "é", "è", "ê", "ë", "ę", "ė", "ē"],
                "a": ["a", "à", "â", "æ", "á", "ä", "ã", "å", "ā"],
                "i": ["i", "î", "ï", "ì", "í", "į", "ī"],
                "o": ["o", "ô", "œ", "ö", "ò", "ó", "õ", "ø", "ō"],
                "u": ["u", "ù", "û", "ü", "ú", "ū"],
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
        // Brazil's spelling first, Portugal's where only it is installed.
        case .portuguese: ["pt_BR", "pt", "pt_PT"]
        case .russian: ["ru", "ru_RU"]
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
        case .norwegian, .german, .french, .portuguese, .russian, .serbianLatin:
            []
        }
    }

    /// Each letter's (row, column) for autocorrect's neighbouring-key cost, with rows 2
    /// and 3 shifted as the keys are.
    var keyCoordinates: [Character: (Double, Double)] {
        var map: [Character: (Double, Double)] = [:]
        let offsets = [0.0, 0.5, 1.5]
        for (row, letters) in letterRows.enumerated() {
            for (column, letter) in letters.enumerated() {
                guard let character = letter.first else { continue }
                map[character] = (Double(row), Double(column) + offsets[row])
            }
        }
        return map
    }

    /// The layout for a language identifier ("nb-NO", "no", "nn_NO", "es-MX", "sr-Latn-RS"),
    /// if any. Serbian only in Latin script: Hex has no Cyrillic Serbian layout.
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
        case "fr": return .french
        case "pt": return .portuguese
        case "ru": return .russian
        case "sr": return parts.contains("latn") ? .serbianLatin : nil
        default: return nil
        }
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
