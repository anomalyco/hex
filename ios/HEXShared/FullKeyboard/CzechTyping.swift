import Foundation

/// The Czech layout's ´ and ˇ keys, as the system Czech keyboard was observed to apply them
/// (iOS 27.2 simulator, real touches in Messages, every letter followed by each key, black box).
/// Neither is a dead key: each accents the letter right before it, as Greek's tonos key does,
/// and after anything else (another letter, an accented one, a space, nothing) types itself.
/// ´ gives á ć é í ĺ ń ó ŕ ś ú ý ź; ˇ gives č ď ě ľ ň ř š ť ž, and ů after u; the ring ˚, held on
/// ˇ, gives ů and å. Capitals take theirs (Á, Č, Ů). Delete takes the accented letter whole.
nonisolated enum CzechTyping {
    static let acuteKey = "\u{00B4}"
    static let caronKey = "\u{02C7}"
    static let ringKey = "\u{02DA}"

    private static let marks: [String: [Character: Character]] = [
        acuteKey: [
            "a": "á", "c": "ć", "e": "é", "i": "í", "l": "ĺ", "n": "ń", "o": "ó", "r": "ŕ", "s": "ś",
            "u": "ú", "y": "ý", "z": "ź",
        ],
        caronKey: [
            "c": "č", "d": "ď", "e": "ě", "l": "ľ", "n": "ň", "r": "ř", "s": "š", "t": "ť", "z": "ž",
            "u": "ů",
        ],
        ringKey: ["a": "å", "u": "ů"],
    ]

    /// The letter `before` ends with, accented by the mark `text` types after it; nil when
    /// the key types the mark itself.
    static func accenting(_ text: String, after before: String?) -> String? {
        guard let table = marks[text], let last = before?.last else { return nil }
        let lower = Character(last.lowercased())
        guard let accented = table[lower] else { return nil }
        return last.isUppercase ? accented.uppercased() : String(accented)
    }
}

/// The apostrophe key of Apple's French keyboards, after n on France's "AZERTY – French" and
/// ending row 2 on Canada's "QWERTY – French" and Switzerland's "QWERTZ – French", as Apple's
/// were observed to apply it (iOS 27.2 simulator, iPhone 17 Pro and SE, real touches in a text
/// view with smart quotes on and off, black box; the same on all three). Right after a letter its keycap shows ’, a tap types the
/// apostrophe (’ where the field curls quotes, ' where it doesn't, as the 123 page's ' key does)
/// and a hold offers ' ’ ‘ ` (typed as they are); after anything else (a space, a digit, an
/// apostrophe, nothing) it shows ´, types ´ (U+00B4) itself, never as a dead key ("´e" stays
/// two characters), and a hold offers ´ ˋ ˆ ¨. It is no letter: the decoder, Adaptive touch,
/// next-letter odds and swipe typing read letters only, so none of them sees it.
nonisolated enum FrenchTyping {
    static let key = "\u{00B4}"

    /// Whether the key types the apostrophe after `before`: right after a letter.
    static func typesApostrophe(after before: String?) -> Bool {
        before?.last?.isLetter == true
    }
}

@MainActor
extension FullKeyboardEngine {
    /// The layout's own keys' typing rules: Greek's tonos key and final sigma (`GreekTyping`),
    /// Czech's ´ and ˇ (`CzechTyping`), French's apostrophe key (`FrenchTyping`). True when
    /// the key already did all it does; else `text` is what to type.
    func typeLayoutRules(_ text: inout String) -> Bool {
        switch layout.language {
        case .greek: return typeGreek(&text)
        case .czech: return typeCzech(text)
        case .french, .frenchCanadian, .frenchSwiss:
            if typesFrenchApostrophe(text) { text = "'" }
            return false
        default: return false
        }
    }

    /// Whether `character` is French's apostrophe key and types the apostrophe here, right
    /// after a letter (`FrenchTyping`).
    func typesFrenchApostrophe(_ character: String) -> Bool {
        layout.language.isFrench && character == FrenchTyping.key && FrenchTyping.typesApostrophe(after: typingContext)
    }

    /// The key a held `character` offers options for: French's apostrophe key after a letter
    /// holds the 123 page's ', else itself.
    func heldBase(_ character: String) -> String {
        typesFrenchApostrophe(character) ? "'" : character
    }

    /// Czech: a held ˇ's ˇ or ˚ accents the letter before it, as a tapped ˇ does. True when it
    /// did; else the held key's choice goes in as it is.
    func typeHeldMark(_ chosen: String) -> Bool {
        guard layout.language == .czech else { return false }
        var mark = chosen
        return typeLayoutRules(&mark)
    }

    /// ´, ˇ or ˚ after a letter it accents: puts the accented letter in its place. False
    /// otherwise, and the mark is typed as itself.
    private func typeCzech(_ text: String) -> Bool {
        guard let accented = CzechTyping.accenting(text, after: target.documentContextBeforeInput) else { return false }
        rememberTypingContext(before: textBefore.map { String($0.dropLast(1)) }, inserted: accented)
        target.deleteBackward()
        target.insertText(accented)
        didEdit()
        logState("typed")
        return true
    }
}
