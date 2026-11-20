import Foundation

/// The full keyboard's letters. Each language is its letter rows, its long-press accents,
/// and the spelling dictionary its autocorrect uses, matching Apple's keyboard for it;
/// another language is a new case with those filled in.
nonisolated enum KeyboardLanguage: String, CaseIterable, Sendable {
    case english = "en"
    case norwegian = "nb"
    case spanish = "es"
    case german = "de"

    /// Its own name, as Apple's keyboard list shows it.
    var displayName: String {
        switch self {
        case .english: "English"
        case .norwegian: "Norsk"
        case .spanish: "Español"
        case .german: "Deutsch"
        }
    }

    /// The three letter rows, lowercase. Rows 1 and 2 may hold 11 keys (Apple's Nordic and
    /// German layouts); row 3 sits between shift and delete.
    var letterRows: [[String]] {
        let rows = switch self {
        case .english: ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
        case .norwegian: ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"]
        case .spanish: ["qwertyuiop", "asdfghjklñ", "zxcvbnm"]
        case .german: ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"]
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
        }
    }

    /// `UITextChecker` language codes to try, best first.
    var spellCheckCandidates: [String] {
        switch self {
        case .english: ["en_US", "en"]
        case .norwegian: ["nb", "nb_NO", "no", "no_NO"]
        case .spanish: ["es", "es_ES", "es_MX"]
        case .german: ["de", "de_DE"]
        }
    }

    /// The first candidate the iPhone has a dictionary for, or nil: then autocorrect and
    /// suggestions stay off rather than "correcting" this language into another.
    func spellCheckLanguage(available: [String]) -> String? {
        spellCheckCandidates.first { available.contains($0) }
    }

    /// Whether the built-in English contractions and typo list apply.
    var usesEnglishFixes: Bool { self == .english }

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

    /// The layout for a language identifier ("nb-NO", "no", "nn_NO", "es-MX"), if any.
    static func matching(_ identifier: String) -> KeyboardLanguage? {
        let code = identifier.lowercased()
            .split(whereSeparator: { $0 == "-" || $0 == "_" })
            .first.map(String.init) ?? ""
        switch code {
        case "nb", "no", "nn": return .norwegian
        case "en": return .english
        case "es": return .spanish
        case "de": return .german
        default: return nil
        }
    }

    /// Automatic follows the dictation language: its layout, or English for a language
    /// without one (Hebrew, French). Before any dictation language is known, the first
    /// iPhone keyboard, then preferred language, that has a layout.
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
