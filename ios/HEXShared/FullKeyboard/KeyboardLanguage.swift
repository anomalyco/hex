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
/// its own for č ć đ š ž: they are held on c, d, s and z, as on Apple's. Polish types on the
/// English letters (the standard Polish layout is QWERTY) with ą ć ę ł ń ó ś ź ż on held keys.
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
    case polish = "pl"
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
        case .polish: "Polski"
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
        case .polish: "pl-PL"
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
        case .english, .italian, .frenchCanadian, .portuguese, .polish: ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
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

    /// Letter keys held for more letters, as the system keyboard's long-press options for this
    /// language were observed (iOS 27.2 simulator: every key held for about 3 s with a real
    /// touch, the callout read from the accessibility tree). Base letter first, then the others
    /// as its callout shows them, read from the base's side: left to right on a left-hand key,
    /// right to left on a right-hand one, as Hex's callout opens the other way there and so
    /// shows them in the same left-to-right order. A letter not listed holds nothing, as on the
    /// system keyboard.
    var letterHolds: [String: [String]] {
        switch self {
        case .english:
            [
                "w": ["w", "ŵ"],
                "e": ["e", "ë", "é", "è", "ê", "ě", "ẽ", "ē", "ė", "ę"],
                "r": ["r", "ř"],
                "t": ["t", "ț", "ť", "þ"],
                "y": ["y", "ý", "ŷ", "ÿ"],
                "u": ["u", "ū", "ǔ", "û", "ù", "ú", "ü", "ũ", "ű", "ů", "ų"],
                "i": ["i", "ǐ", "î", "ì", "í", "ï", "ĩ", "ī", "ı", "į"],
                "o": ["o", "ô", "ò", "ó", "ö", "ǒ", "œ", "ø", "õ", "ō", "ő"],
                "a": ["a", "à", "á", "â", "ä", "ǎ", "æ", "ã", "å", "ā", "ă", "ą"],
                "s": ["s", "ß", "ş", "ș", "ś", "š"],
                "d": ["d", "ď", "ð"],
                "g": ["g", "ğ", "ġ"],
                "h": ["h", "ħ"],
                "k": ["k", "ķ"],
                "l": ["l", "ł", "ļ", "ľ"],
                "z": ["z", "ź", "ž", "ż"],
                "c": ["c", "ç", "ć", "č", "ċ"],
                "n": ["n", "ñ", "ń", "ņ", "ň"],
            ]
        case .norwegian:
            [
                "e": ["e", "ë", "è", "é", "ê", "ė", "ę", "ē", "ẹ", "ǝ", "ẽ"],
                "t": ["t", "ŧ", "ț", "ṱ", "ṯ", "þ"],
                "y": ["y", "ẏ", "ȳ", "ŷ", "ý", "ÿ", "ỳ", "ỹ", "ẙ", "ɏ"],
                "u": ["u", "ů", "ū", "ü", "û", "ù", "ú", "ũ", "ụ"],
                "i": ["i", "ï", "í", "ì", "î", "ĩ", "ī", "ị"],
                "o": ["o", "õ", "ó", "ô", "ò", "ö", "œ", "ō", "ȯ", "ọ", "ő"],
                "a": ["a", "à", "ä", "á", "â", "ã", "ā", "ạ"],
                "s": ["s", "š", "ŝ", "ṣ", "ș"],
                "d": ["d", "đ", "ð", "ḓ", "ď"],
                "g": ["g", "ǥ", "ǧ", "ģ"],
                "k": ["k", "ķ", "ḵ", "ǩ"],
                "l": ["l", "ł"],
                "ø": ["ø", "ö"],
                "æ": ["æ", "ä"],
                "z": ["z", "ž", "ʒ", "ǯ"],
                "c": ["c", "č", "ç", "ĉ"],
                "n": ["n", "ñ", "ň", "ṋ", "ṉ", "ŋ"],
            ]
        case .spanish:
            [
                "e": ["e", "é", "è", "ë", "ê", "ę", "ė", "ē"],
                "u": ["u", "ú", "ü", "ù", "û", "ū"],
                "i": ["i", "í", "ï", "ì", "î", "į", "ī"],
                "o": ["o", "ö", "ó", "ò", "ô", "õ", "ø", "œ", "ō", "º"],
                "a": ["a", "á", "à", "ä", "â", "ã", "å", "ą", "æ", "ā", "ª"],
                "s": ["s", "š"],
                "d": ["d", "đ"],
                "l": ["l", "l·l"],
                "c": ["c", "ç", "ć", "č"],
                "n": ["n", "ń"],
            ]
        case .german:
            [
                "e": ["e", "é", "è", "ê", "ë", "ė"],
                "u": ["u", "û", "ù", "ú", "ū"],
                "i": ["i", "î", "ï", "í", "ī", "ì"],
                "o": ["o", "ô", "ò", "ó", "õ", "œ", "ø", "ō"],
                "a": ["a", "à", "á", "â", "æ", "ã", "å", "ā"],
                "s": ["s", "ß", "ś", "š"],
                "y": ["y", "ÿ"],
                "c": ["c", "ç", "ć", "č"],
                "n": ["n", "ñ", "ń"],
            ]
        case .italian:
            [
                "e": ["e", "é", "è", "ê", "ə", "ę", "ë", "ē", "ė"],
                "u": ["u", "ù", "ú", "û", "ü", "ū"],
                "i": ["i", "ì", "í", "î", "ï", "į", "ī"],
                "o": ["o", "ô", "ò", "ó", "ö", "õ", "œ", "ø", "ō", "º"],
                "a": ["a", "à", "á", "â", "ä", "æ", "ã", "å", "ā", "ª"],
                "s": ["s", "ß", "ś", "š"],
                "c": ["c", "ç", "ć", "č"],
            ]
        case .french, .frenchCanadian, .frenchSwiss:
            [
                "a": ["a", "à", "â", "æ", "á", "ä", "ã", "å", "ā", "ª"],
                "e": ["e", "é", "è", "ê", "ë", "ę", "ė", "ē"],
                "y": ["y", "ÿ"],
                "u": ["u", "û", "ù", "ü", "ú", "ū"],
                "i": ["i", "î", "ï", "ì", "í", "į", "ī"],
                "o": ["o", "ö", "ô", "œ", "ò", "ó", "õ", "ø", "ō", "º"],
                "c": ["c", "ç", "ć", "č"],
                "n": ["n", "ñ", "ń"],
            ]
        case .portuguese:
            [
                "e": ["e", "é", "ê", "è", "ę", "ė", "ē", "ë"],
                "u": ["u", "ú", "ü", "ù", "û", "ū"],
                "i": ["i", "í", "î", "ì", "ï", "į", "ī"],
                "o": ["o", "ô", "ó", "õ", "ò", "ö", "œ", "ø", "ō", "º"],
                "a": ["a", "á", "ã", "à", "â", "ä", "å", "æ", "ª"],
                "c": ["c", "ç"],
                "n": ["n", "ñ"],
            ]
        case .polish:
            // Polish's own letter right after the base (ą ć ę ł ń ó ś ż, then ź on Z); U, I and
            // Y hold nothing.
            [
                "e": ["e", "ę", "è", "é", "ê", "ë", "ė", "ē"],
                "o": ["o", "ó", "ô", "ö", "ò", "õ", "œ", "ø", "ō"],
                "a": ["a", "ą", "à", "á", "â", "ä", "æ", "ã", "å", "ā"],
                "s": ["s", "ś", "ß", "š"],
                "l": ["l", "ł"],
                "z": ["z", "ż", "ź", "ž"],
                "c": ["c", "ć", "ç", "č"],
                "n": ["n", "ń", "ñ"],
            ]
        case .russian:
            [
                "й": ["й", "ї", "ј", "ѵ"],
                "ц": ["ц", "ҵ"],
                "у": ["у", "ү", "ў", "ӱ", "ӳ", "ұ", "ӯ"],
                "к": ["к", "ҡ", "қ", "ҟ", "ѯ", "ҝ"],
                "е": ["е", "ё", "ӗ", "є", "ѣ", "е̄"],
                "н": ["н", "ң", "ҥ", "ӈ", "ᵸ"],
                "г": ["г", "ґ", "ӷ", "ҕ", "ғ"],
                "ш": ["ш", "ҿ", "ҽ"],
                "з": ["з", "ҙ", "ӟ", "ӡ"],
                "х": ["х", "ҳ", "һ"],
                "ф": ["ф", "ѳ"],
                "ы": ["ы", "ӹ"],
                "а": ["а", "ӕ", "ӑ", "ӓ", "а̄"],
                "п": ["п", "ԥ", "ѱ"],
                "о": ["о", "ѡ", "о̄", "ӧ", "ө"],
                "л": ["л", "ӆ"],
                "ж": ["ж", "җ", "ӝ", "ӂ", "ѥ"],
                "э": ["э", "ӭ", "ә"],
                "я": ["я", "џ", "ѧ"],
                "ч": ["ч", "ҷ", "ӌ", "ӵ", "ҹ"],
                "с": ["с", "ҫ", "с̄"],
                "и": ["и", "і", "ӥ", "ӣ"],
                "т": ["т", "ҭ"],
                "ь": ["ь", "ъ", "ӏ", "ѫ"],
                "ю": ["ю", "ҩ", "ѭ"],
            ]
        case .ukrainian:
            [
                "ʼ": ["ʼ", "'", "’", "‘", "`"],
            ]
        case .serbianLatin:
            [
                "z": ["z", "ż", "ź", "ž"],
                "s": ["s", "š", "ß", "ś"],
                "d": ["d", "đ"],
                "c": ["c", "č", "ć", "ç"],
            ]
        }
    }

    /// The 123 and #+= keys whose held options on this language's system keyboard differ from
    /// English's (`AccentCatalog`), observed the same way. The pages themselves are English's in
    /// every language, so Ukrainian's ₴, second on the system keyboard's held $, is held there.
    var symbolHolds: [String: [String]] {
        switch self {
        case .english, .serbianLatin:
            [:]
        case .norwegian:
            [
                "$": ["$", "₽", "₩", "¥", "£", "€", "¢", "kr"],
                "\"": ["\"", "«", "»", "“", "”", "„"],
                "•": ["•", "°"],
            ]
        case .spanish, .italian:
            [
                "1": ["1", "1º", "1ª"],
                "2": ["2", "2º", "2ª"],
                "3": ["3", "3º", "3ª"],
                "4": ["4", "4º", "4ª"],
                "5": ["5", "5º", "5ª"],
                "6": ["6", "6ª", "6º"],
                "7": ["7", "7ª", "7º"],
                "8": ["8", "8ª", "8º"],
                "9": ["9", "9º", "9ª"],
                "0": ["0", "0º", "°", "0ª"],
            ]
        case .german:
            [
                "\"": ["\"", "„", "“", "”", "«", "»"],
                "'": ["'", "’", "‚", "‹", "›", "‘", "`"],
            ]
        case .french, .frenchCanadian, .frenchSwiss:
            [
                "\"": ["\"", "»", "«", "”", "“", "„"],
                ".": [".", "…", "·"],
            ]
        case .portuguese:
            [
                "1": ["1", "1º", "1ª"],
                "2": ["2", "2º", "2ª"],
                "3": ["3", "3º", "3ª"],
                "4": ["4", "4º", "4ª"],
                "5": ["5", "5º", "5ª"],
                "6": ["6", "6ª", "6º"],
                "7": ["7", "7ª", "7º"],
                "8": ["8", "8ª", "8º"],
                "9": ["9", "9º", "9ª"],
            ]
        case .polish:
            // Its currency key is € (no zł), held for € $ £ ¥ ₩ ₽; Hex's 123 page keeps $.
            [
                "€": ["€", "$", "£", "¥", "₩", "₽"],
            ]
        case .russian:
            [
                "#": ["#", "№"],
            ]
        case .ukrainian:
            [
                "$": ["$", "₽", "₩", "¥", "£", "€", "¢", "₴"],
                "#": ["#", "№"],
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
        case .polish: ["pl", "pl_PL"]
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
        case .norwegian, .german, .french, .frenchCanadian, .frenchSwiss, .portuguese, .polish,
             .russian, .ukrainian, .serbianLatin:
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
        case "pl": return .polish
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
