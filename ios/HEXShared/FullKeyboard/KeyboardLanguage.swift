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
/// Romanian is QWERTY too, its own ă â î ș ț held on a, i, s and t (the comma-below ș and ț, not
/// the cedilla ş ţ), as on Apple's iPhone keyboard; Apple's Romanian layout with keys of its own
/// for them is the iPad's. Greek matches the standard Greek layout and the system keyboard's
/// observed long-press options, in the iPhone's rows (no ; or ς keys, the tonos key ending row
/// 1; its typing rules are `GreekTyping`).
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
    case romanian = "ro"
    case russian = "ru"
    case ukrainian = "uk"
    case greek = "el"
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
        case .romanian: "Română"
        case .russian: "Русский"
        case .ukrainian: "Українська"
        case .greek: "Ελληνικά"
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
    /// (bilingual autocorrect). Russian's and Ukrainian's are Cyrillic, Greek's Greek.
    var isLatin: Bool { self != .russian && self != .ukrainian && self != .greek }

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
        case .romanian: "ro-RO"
        case .russian: "ru-RU"
        case .ukrainian: "uk-UA"
        case .greek: "el-GR"
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

    /// The three letter rows of the language's own layout (its `defaultLetterLayout`), lowercase.
    var letterRows: [[String]] { letterRows(defaultLetterLayout) }

    /// The three letter rows on `letters` (Settings › Keyboard › Languages › Layout), lowercase,
    /// or on the language's own layout where it doesn't offer `letters`. Rows 1 and 2 may hold
    /// 11 keys (Apple's Nordic, German and Russian layouts) or 12 (Ukrainian); row 3 sits
    /// between shift and delete, except on Dvorak, whose delete ends row 1.
    func letterRows(_ letters: KeyboardLetterLayout) -> [[String]] {
        let rows: [String] = switch offered(letters) {
        case .own: ownLetterRows
        case .qwerty: ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
        case .qwertz: ["qwertzuiop", "asdfghjkl", "yxcvbnm"]
        case .azerty: ["azertyuiop", "qsdfghjklm", "wxcvbn"]
        case .dvorak: ["pyfgcrl", "aoeuidhtns", "qjkxbmwvz"]
        }
        return rows.map { $0.map(String.init) }
    }

    /// The layouts Settings › Keyboard › Languages › Layout offers for this language, its own
    /// (today's, `defaultLetterLayout`) first, then the rest in the order Apple's Keyboard
    /// Layout list shows them (iOS 27.0 simulator, Settings › General › Keyboard › Keyboards ›
    /// Add New Keyboard, black box). Apple's French keyboards also list "AZERTY – French",
    /// "QWERTY – French" and "QWERTZ – French", which differ from the plain ones only by the ´
    /// dead key Hex leaves out, so French offers the three plain ones. One layout: no choice.
    var letterLayouts: [KeyboardLetterLayout] {
        switch self {
        case .english: [.qwerty, .azerty, .qwertz, .dvorak]
        case .german: [.own, .qwertz, .qwerty, .azerty]
        case .spanish: [.own, .qwerty, .azerty, .qwertz]
        case .french: [.azerty, .qwerty, .qwertz]
        case .frenchCanadian: [.qwerty, .azerty, .qwertz]
        case .frenchSwiss: [.qwertz, .azerty, .qwerty]
        case .italian, .portuguese: [.qwerty, .azerty, .qwertz]
        case .polish, .romanian: [.qwerty, .qwertz, .azerty]
        // Apple lists QWERTY first for Serbian (Latin) and adds it as QWERTY; Hex keeps the
        // QWERTZ it always typed on as the default.
        case .serbianLatin: [.qwertz, .qwerty, .azerty]
        case .norwegian, .russian, .ukrainian, .greek: [.own]
        }
    }

    /// The layout the language types on until another is chosen: the one Hex always had.
    var defaultLetterLayout: KeyboardLetterLayout { letterLayouts[0] }

    /// `letters` where this language offers it, else its own layout.
    func offered(_ letters: KeyboardLetterLayout?) -> KeyboardLetterLayout {
        guard let letters, letterLayouts.contains(letters) else { return defaultLetterLayout }
        return letters
    }

    /// What Settings names `letters` for this language: Apple's names, and for a language's
    /// own layout with letters of its own, its arrangement and the language ("QWERTZ – Deutsch").
    func layoutName(_ letters: KeyboardLetterLayout) -> String {
        switch letters {
        case .qwerty: "QWERTY"
        case .qwertz: "QWERTZ"
        case .azerty: "AZERTY"
        case .dvorak: "Dvorak"
        case .own:
            switch self {
            case .german: "QWERTZ – \(displayName)"
            case .spanish, .norwegian: "QWERTY – \(displayName)"
            default: displayName
            }
        }
    }

    private var ownLetterRows: [String] {
        switch self {
        case .english, .italian, .frenchCanadian, .portuguese, .polish, .romanian:
            ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
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
        // The standard Greek layout in the rows the iPhone's own Greek keyboard shows (iOS 27.2
        // simulator): no ; or ς keys (; is on the 123 page, ς on a held σ), so row 1 is nine
        // keys ending in the tonos key ´, over row 2's nine.
        case .greek: ["ερτυθιοπ\u{00B4}", "ασδφγηξκλ", "ζχψωβνμ"]
        case .serbianLatin: ["qwertzuiop", "asdfghjkl", "yxcvbnm"]
        }
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
        case .romanian:
            // Romanian's own letters right after the base: ă and â on A, î on I, ș on S and ț on
            // T (comma-below, never the cedilla ş ţ); every other letter holds nothing. Its held
            // ß comes out ẞ in capitals, as every layout's does (Apple's uppercase S leaves it out).
            [
                "a": ["a", "ă", "â", "à", "á", "ä", "æ", "ã", "å", "ā"],
                "i": ["i", "î", "ï", "ì", "í", "į", "ī"],
                "s": ["s", "ș", "ß", "ś", "š"],
                "t": ["t", "ț"],
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
        case .greek:
            // Each vowel's tonos form (ι and υ also with diaeresis, and with both), ς on σ and
            // the kai sign ϗ on κ; in capitals Σ holds nothing and Ι, Υ only Ί Ϊ, Ύ Ϋ
            // (`AccentCatalog`). Every other letter, and the tonos key, holds nothing.
            [
                "ε": ["ε", "έ"],
                "υ": ["υ", "ύ", "ϋ", "ΰ"],
                "ι": ["ι", "ΐ", "ϊ", "ί"],
                "ο": ["ο", "ό"],
                "α": ["α", "ά"],
                "σ": ["σ", "ς"],
                "η": ["η", "ή"],
                "κ": ["κ", "ϗ"],
                "ω": ["ω", "ώ"],
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

    /// `letterHolds` on `letters`. A plain layout leaves out the keys the language's own has
    /// for its letters (German's ä ö ü, Spanish's ñ), and Apple's keyboard then holds each right
    /// after its base letter (iOS 27.0 simulator, German QWERTZ and QWERTY: A Ä À Á…, O Ö Ô…,
    /// U Ü Û…; Spanish QWERTY: N Ñ Ń), so Hex's does too. Every other hold stays the language's.
    func letterHolds(on letters: KeyboardLetterLayout) -> [String: [String]] {
        var holds = letterHolds
        let shown = Set(letterRows(letters).joined())
        for letter in ownLetterRows.joined().map(String.init) where !shown.contains(letter) {
            let base = String(letter.decomposedStringWithCanonicalMapping.unicodeScalars.prefix(1).map(Character.init))
            guard base != letter, shown.contains(base) else { continue }
            var list = holds[base] ?? [base]
            list.removeAll { $0 == letter }
            list.insert(letter, at: 1)
            holds[base] = list
        }
        return holds
    }

    /// The 123 and #+= keys whose held options on this language's system keyboard differ from
    /// English's (`AccentCatalog`), observed the same way. The pages themselves are English's in
    /// every language but Greek (`KeyboardLayout.characterRows`), so Ukrainian's ₴, second on the
    /// system keyboard's held $, is held there.
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
        case .romanian:
            // Its currency key is €, held for € $ £ ¥ ₩ ₽ as Polish's is, and a held " offers
            // Romanian's order.
            [
                "€": ["€", "$", "£", "¥", "₩", "₽"],
                "\"": ["\"", "„", "”", "»", "«", "“"],
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
        case .greek:
            // Its currency key is $, held as English's is; a held " offers French's order.
            [
                "\"": ["\"", "»", "«", "”", "“", "„"],
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
        case .romanian: ["ro", "ro_RO"]
        case .russian: ["ru", "ru_RU"]
        case .ukrainian: ["uk", "uk_UA"]
        case .greek: ["el", "el_GR"]
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
             .romanian, .russian, .ukrainian, .greek, .serbianLatin:
            []
        }
    }

    /// Each letter's (row, column) for autocorrect's neighbouring-key cost on `letters`, with
    /// rows shifted as the keys are.
    func keyCoordinates(_ letters: KeyboardLetterLayout? = nil) -> [Character: (Double, Double)] {
        var map: [Character: (Double, Double)] = [:]
        let letters = offered(letters)
        // AZERTY's row 2 runs edge to edge, and its row 3 starts under e (W under E).
        // Ukrainian's rows 1 and 2 are both twelve keys, and its row 3 starts under ц; Greek's
        // are both nine (row 1 ending in the tonos key), and its row 3 starts under σ.
        // Dvorak's row 2 runs from the edge, its row 1 ends at delete (P over U's left half) and
        // its row 3 starts after shift (Q under O's right half).
        let offsets = switch letters {
        case .azerty: [0.0, 0.0, 2.0]
        case .dvorak: [1.85, 0.0, 1.33]
        case .own where self == .ukrainian || self == .greek: [0.0, 0.0, 1.0]
        default: [0.0, 0.5, 1.5]
        }
        for (row, letters) in letterRows(letters).enumerated() {
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
        case "ro": return .romanian
        case "ru": return .russian
        case "uk": return .ukrainian
        case "el": return .greek
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
    /// without one (Hebrew). Before any dictation language is known, the first
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

/// How a language's letters are arranged (Settings › Keyboard › Languages › Layout), as
/// Apple's Keyboard Layout list offers them for it. The plain ones hold only the 26 letters, in
/// every language (iOS 27.0 simulator: the same rows, bottom row and 123 pages as the language's
/// own layout, which is English's QWERTY, French's AZERTY, Swiss French's and Serbian's QWERTZ);
/// a language's other letters are then held on their base letter (`letterHolds(on:)`).
nonisolated enum KeyboardLetterLayout: String, CaseIterable, Sendable {
    /// The language's own keys, with letters of their own (German's ü ö ä, Spanish's ñ).
    case own
    case qwerty
    case qwertz
    case azerty
    /// Apple's iPhone Dvorak (English): P Y F G C R L and delete, A O E U I D H T N S edge to
    /// edge, shift and Q J K X B M W V Z, on keys a little narrower so that row fits.
    case dvorak
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

    /// The language whose letters and long presses the keys show while `current` types. Apple
    /// types English and Spanish on one bilingual keyboard laid out as the first of the two
    /// (iOS 27.2 simulator, black box: Spanish added to the English keyboard is QWERTY with ñ
    /// on a held n and English's long presses; English added to the Spanish keyboard keeps its
    /// ñ key and Spanish's). So with both chosen, both type on whichever comes first, and the
    /// space bar switches the language they autocorrect and dictate in; any other language
    /// types on its own.
    var layout: KeyboardLanguage {
        let bilingual: Set<KeyboardLanguage> = [.english, .spanish]
        guard bilingual.contains(current), bilingual.isSubset(of: languages) else { return current }
        return languages.first(where: bilingual.contains) ?? current
    }

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
