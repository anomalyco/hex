import Foundation

/// Labs › Hinglish: Hindi dictation as people text it, in Latin letters ("kya kar rahe
/// ho", "meeting kal 5 baje hai"), never translated. Apple's \`SpeechTranscriber\` already writes
/// Hindi in Latin letters, English words in English (macOS and iOS 27: "meeting kal panch baje
/// hai"); \`DictationTranscriber\`, the module on iPhones without it, writes Devanagari, English
/// words included ("मीटिंग"). So each word is handled by its script:
/// - Devanagari: the lexicon's spelling when it has the word (common Hindi words, English
///   loanwords back in English: मीटिंग → meeting), else ICU's \`Devanagari-Latin\` after schwa
///   deletion (कमल → kamal, not kamala), reduced to casual spelling: no diacritics, aa, ee and
///   oo only where people stress them (aaj, baat, theek, bhookh), ch/chh, sh, w before a/o.
/// - Latin: a few of Apple's spellings changed to the usual ones (mei → main, hoo → hoon), and
///   the capitals Apple puts on common Hindi words mid-sentence lowered ("Ghar" → ghar);
///   other capitalized words, likely names, keep theirs.
/// - Numbers: Devanagari digits become 0–9, and a spoken hour before "baje" a digit
///   ("paanch baje" → "5 baje", "saadhe paanch baje" → "5:30 baje").
/// String rules and two small tables, no model, so in-keyboard dictation needs no more memory.
nonisolated enum HinglishRomanizer {
    static func romanize(_ text: String) -> String {
        var pieces: [Piece] = []
        var sentenceStart = true
        for run in runs(in: text) {
            switch run {
            case .devanagari(let word):
                // `DictationTranscriber` writes मैं (I) as में (in), which never starts a sentence.
                let meansI = sentenceStart && HinglishLexicon.key(word) == "में"
                pieces.append(.word(meansI ? "main" : romanizeDevanagari(word)))
                sentenceStart = false
            case .latin(let word):
                pieces.append(.word(normalizeLatin(word, sentenceStart: sentenceStart)))
                sentenceStart = false
            case .other(let other):
                pieces.append(.other(other))
                if other.contains(where: { ".!?\n".contains($0) }) { sentenceStart = true }
            }
        }
        return spokenHours(in: pieces).map(\.text).joined()
    }

    // MARK: Runs of one script

    private enum Run {
        case devanagari(String)
        case latin(String)
        case other(String)
    }

    private enum Piece {
        case word(String)
        case other(String)

        var text: String {
            switch self {
            case .word(let text), .other(let text): text
            }
        }
    }

    private enum Script { case devanagari, latin, other }

    private static func script(of scalar: Unicode.Scalar) -> Script {
        let value = scalar.value
        switch value {
        case 0x0964...0x0970: return .other // dandas, digits, abbreviation sign
        case 0x0900...0x097F, 0xA8E0...0xA8FF, 0x200C, 0x200D: return .devanagari
        default: break
        }
        if scalar.properties.isAlphabetic || scalar.properties.generalCategory == .nonspacingMark {
            return .latin
        }
        return .other
    }

    /// Splits \`text\` into words by script, with Devanagari digits as 0–9 and dandas as periods.
    /// An apostrophe between Latin letters stays in its word ("don't").
    private static func runs(in text: String) -> [Run] {
        var result: [Run] = []
        var current = String.UnicodeScalarView()
        var currentScript = Script.other
        func flush() {
            guard !current.isEmpty else { return }
            let string = String(current)
            switch currentScript {
            case .devanagari: result.append(.devanagari(string))
            case .latin: result.append(.latin(string))
            case .other: result.append(.other(string))
            }
            current = String.UnicodeScalarView()
        }
        let scalars = Array(text.unicodeScalars)
        for (index, scalar) in scalars.enumerated() {
            var kind = script(of: scalar)
            if kind == .other, scalar == "'" || scalar == "’", currentScript == .latin,
               index + 1 < scalars.count, script(of: scalars[index + 1]) == .latin {
                kind = .latin
            }
            if kind != currentScript { flush() }
            currentScript = kind
            switch scalar.value {
            case 0x0966...0x096F:
                current.append(Unicode.Scalar(UInt8(0x30 + scalar.value - 0x0966)))
            case 0x0964, 0x0965:
                current.append(".")
            default:
                current.append(scalar)
            }
        }
        flush()
        return result
    }

    // MARK: Devanagari

    static func romanizeDevanagari(_ word: String) -> String {
        let key = HinglishLexicon.key(word)
        if let known = HinglishLexicon.lookup(key) { return known }
        let scalars = Array(key.unicodeScalars)
        // An English plural: मिनिट्स → minutes.
        if scalars.count > 2, scalars[scalars.count - 2].value == 0x094D, scalars.last?.value == 0x0938,
           let base = HinglishLexicon.lookup(String(String.UnicodeScalarView(scalars.dropLast(2)))),
           HinglishLexicon.isLoanword(base) {
            return base + "s"
        }
        let marked = deletingSchwas(in: droppingNasalBeforeNasal(scalars))
        guard let latin = String(String.UnicodeScalarView(marked))
            .applyingTransform(StringTransform("Devanagari-Latin"), reverse: false) else { return word }
        return casualSpelling(of: latin)
    }

    private static func isConsonant(_ value: UInt32) -> Bool {
        (0x0915...0x0939).contains(value) || (0x0958...0x095F).contains(value) || (0x0978...0x097F).contains(value)
    }

    private static func isVowelSign(_ value: UInt32) -> Bool {
        (0x093A...0x093B).contains(value) || (0x093E...0x094C).contains(value)
            || (0x094E...0x094F).contains(value) || (0x0955...0x0957).contains(value)
            || (0x0962...0x0963).contains(value)
    }

    private static func isIndependentVowel(_ value: UInt32) -> Bool {
        (0x0904...0x0914).contains(value) || (0x0960...0x0961).contains(value) || (0x0972...0x0977).contains(value)
    }

    private static func isNasalOrVisarga(_ value: UInt32) -> Bool {
        (0x0901...0x0903).contains(value)
    }

    /// मैंने → मैने: an anusvara before न or म is that nasal already ("maine", not "mainne").
    private static func droppingNasalBeforeNasal(_ scalars: [Unicode.Scalar]) -> [Unicode.Scalar] {
        var result: [Unicode.Scalar] = []
        for (index, scalar) in scalars.enumerated() {
            if scalar.value == 0x0902, index + 1 < scalars.count,
               [0x0928, 0x092E, 0x0923].contains(scalars[index + 1].value) {
                continue
            }
            result.append(scalar)
        }
        return result
    }

    private struct Syllable {
        enum Vowel { case inherent, written, none }
        var start: Int
        /// After the consonant and its nukta, where a virama goes.
        var consonantEnd: Int
        var end: Int
        var isConsonant: Bool
        var vowel: Vowel
        var nasal = false
        /// The consonant ends a cluster (the one before it has a virama).
        var clustered = false
        var writtenVowel: UInt32?
    }

    private static func syllables(of scalars: [Unicode.Scalar]) -> [Syllable] {
        var result: [Syllable] = []
        var index = 0
        while index < scalars.count {
            let value = scalars[index].value
            var syllable: Syllable
            var end = index + 1
            if isConsonant(value) {
                if end < scalars.count, scalars[end].value == 0x093C { end += 1 }
                let consonantEnd = end
                var vowel = Syllable.Vowel.inherent
                var written: UInt32?
                if end < scalars.count {
                    let next = scalars[end].value
                    if next == 0x094D {
                        vowel = .none
                        end += 1
                    } else if isVowelSign(next) {
                        vowel = .written
                        written = next
                        end += 1
                    }
                }
                let clustered = result.last.map { $0.isConsonant && $0.vowel == .none } ?? false
                syllable = Syllable(
                    start: index, consonantEnd: consonantEnd, end: end, isConsonant: true,
                    vowel: vowel, clustered: clustered, writtenVowel: written
                )
            } else {
                syllable = Syllable(
                    start: index, consonantEnd: end, end: end, isConsonant: false,
                    vowel: isIndependentVowel(value) ? .written : .none, writtenVowel: value
                )
            }
            while syllable.end < scalars.count, isNasalOrVisarga(scalars[syllable.end].value) {
                syllable.nasal = true
                syllable.end += 1
            }
            result.append(syllable)
            index = syllable.end
        }
        return result
    }

    /// Hindi drops the inherent "a" at the end of a word, after a cluster too (कमल kamal, दोस्त
    /// dost) but for ra, ya or va (मित्र mitra, सत्य satya), and between a vowel and a consonant
    /// that has one (करना karna, समझना samajhna), but not after a cluster or a nasal (ज़िंदगी
    /// zindagi), nor on the first consonant. Marks each dropped one with a virama, right to
    /// left, so ICU reads it without the vowel.
    private static func deletingSchwas(in scalars: [Unicode.Scalar]) -> [Unicode.Scalar] {
        let parts = syllables(of: scalars)
        guard parts.count >= 2 else { return scalars }
        var dropped = Set<Int>()
        for index in stride(from: parts.count - 1, through: 1, by: -1) {
            let part = parts[index]
            guard part.isConsonant, part.vowel == .inherent, !part.nasal else { continue }
            if part.clustered {
                let keeps = [0x0930, 0x092F, 0x0935].contains(scalars[part.start].value)
                if index == parts.count - 1, !keeps { dropped.insert(index) }
                continue
            }
            let previous = parts[index - 1]
            guard previous.vowel != .none || !previous.isConsonant else { continue }
            if index == parts.count - 1 {
                // प्रिय stays "priya": a final ya after an i keeps its vowel.
                let afterI = previous.writtenVowel.map { [0x093F, 0x0940, 0x0907, 0x0908].contains($0) } ?? false
                if scalars[part.start].value == 0x092F, afterI { continue }
                dropped.insert(index)
            } else {
                let next = parts[index + 1]
                guard !previous.nasal, next.isConsonant, next.vowel != .none, !dropped.contains(index + 1) else {
                    continue
                }
                dropped.insert(index)
            }
        }
        guard !dropped.isEmpty else { return scalars }
        var result: [Unicode.Scalar] = []
        for (index, part) in parts.enumerated() {
            result.append(contentsOf: scalars[part.start..<part.consonantEnd])
            if dropped.contains(index) { result.append(Unicode.Scalar(0x094D)!) }
            result.append(contentsOf: scalars[part.consonantEnd..<part.end])
        }
        return result
    }

    private enum Sound {
        enum Vowel { case a, aa, i, ii, u, uu, e, o, ai, au, ri }
        case vowel(Vowel, nasal: Bool)
        case consonant(String)
        /// ICU's apostrophe between two vowels ("ga'ē").
        case gap
        case other(String)

        var isVowel: Bool {
            if case .vowel = self { return true }
            return false
        }

        var isConsonant: Bool {
            if case .consonant = self { return true }
            return false
        }
    }

    private static let consonants: [UInt32: String] = [
        0x1E6D: "t", 0x1E0D: "d", 0x1E47: "n", 0x1E45: "n", 0x00F1: "n", 0x015B: "sh", 0x1E63: "sh",
        0x1E5B: "d", 0x0071: "k", 0x0121: "g", 0x1E8F: "y", 0x1E35: "k", 0x1E96: "h", 0x1E25: "h",
        0x1E3B: "l", 0x1E3F: "m",
    ]

    /// ICU's transliteration as one reads it: its letters and vowels.
    private static func sounds(of latin: String) -> [Sound] {
        let scalars = Array(latin.precomposedStringWithCanonicalMapping.unicodeScalars)
        var result: [Sound] = []
        var index = 0
        func next(_ offset: Int = 1) -> UInt32? {
            index + offset < scalars.count ? scalars[index + offset].value : nil
        }
        func nasalize() {
            if case .vowel(let vowel, _) = result.last {
                result[result.count - 1] = .vowel(vowel, nasal: true)
            } else {
                result.append(.consonant("n"))
            }
        }
        while index < scalars.count {
            let value = scalars[index].value
            var step = 1
            switch value {
            case 0x61: // a, ai, au
                if next() == 0x69 {
                    result.append(.vowel(.ai, nasal: false)); step = 2
                } else if next() == 0x75 {
                    result.append(.vowel(.au, nasal: false)); step = 2
                } else {
                    result.append(.vowel(.a, nasal: false))
                }
            case 0x0101: result.append(.vowel(.aa, nasal: false))
            case 0x69: result.append(.vowel(.i, nasal: false))
            case 0x012B: result.append(.vowel(.ii, nasal: false))
            case 0x75: result.append(.vowel(.u, nasal: false))
            case 0x016B: result.append(.vowel(.uu, nasal: false))
            case 0x65, 0x0113, 0x0115, 0x00E6: result.append(.vowel(.e, nasal: false))
            case 0x6F, 0x014D, 0x014F: result.append(.vowel(.o, nasal: false))
            case 0x1E41: nasalize()
            case 0x6D where next() == 0x0310: // m̐
                nasalize(); step = 2
            case 0x72 where next() == 0x0325: // r̥, r̥̄
                result.append(.vowel(.ri, nasal: false))
                step = next(2) == 0x0304 ? 3 : 2
            case 0x63: // c, ch
                if next() == 0x68 {
                    result.append(.consonant("ch")); step = 2
                } else {
                    result.append(.consonant("c"))
                }
            case 0x6A where next() == 0x00F1: // jñ: "gyaan"
                result.append(.consonant("gy")); step = 2
            case 0x27, 0x2019:
                result.append(.gap)
            case 0x0300...0x036F:
                break // a mark ICU leaves (n̄)
            default:
                if let mapped = consonants[value] {
                    result.append(.consonant(mapped))
                } else if let scalar = Unicode.Scalar(value), scalar.properties.isAlphabetic {
                    result.append(.consonant(String(scalar)))
                } else if let scalar = Unicode.Scalar(value) {
                    result.append(.other(String(scalar)))
                }
            }
            index += step
        }
        return result
    }

    /// ICU's diacritics as people text Hindi.
    private static func casualSpelling(of latin: String) -> String {
        let sounds = sounds(of: latin)
        let vowelPositions = sounds.indices.filter { sounds[$0].isVowel }
        var output = ""
        for (index, sound) in sounds.enumerated() {
            switch sound {
            case .consonant(let letters):
                let following = index + 1 < sounds.count ? sounds[index + 1] : nil
                switch letters {
                case "c":
                    // च्च, च्छ: "bacche", "accha". छ is "ch" too, as people text it: kuch, chota.
                    if case .consonant(let after) = following, after == "c" || after == "ch" {
                        output += "c"
                    } else {
                        output += "ch"
                    }
                case "ch":
                    output += "ch"
                case "v":
                    // w before a and o: wala, jawab, woh; v before i and e: vivek, ved.
                    if case .vowel(let vowel, _) = following, [.i, .ii, .e, .ri].contains(vowel) {
                        output += "v"
                    } else {
                        output += "w"
                    }
                default:
                    output += letters
                }
            case .vowel(let vowel, let nasal):
                let position = vowelPositions.firstIndex(of: index)!
                let context = VowelContext(
                    initial: !sounds[..<index].contains { $0.isConsonant },
                    last: position == vowelPositions.count - 1,
                    closed: index + 1 < sounds.count && sounds[index + 1].isConsonant,
                    afterVowel: index > 0 && (sounds[index - 1].isVowel || isGapAfterVowel(sounds, index - 1)),
                    single: vowelPositions.count == 1,
                    nasal: nasal
                )
                output += spell(vowel, context)
            case .gap:
                // गए "gaye", चाहिए "chahiye", आएगा "aayega".
                if index > 0, index + 1 < sounds.count,
                   case .vowel(let before, _) = sounds[index - 1], [.a, .aa, .i, .ii].contains(before),
                   case .vowel(.e, _) = sounds[index + 1] {
                    output += "y"
                }
            case .other(let other):
                output += other
            }
        }
        return output.lowercased()
    }

    private static func isGapAfterVowel(_ sounds: [Sound], _ index: Int) -> Bool {
        guard case .gap = sounds[index], index > 0 else { return false }
        return sounds[index - 1].isVowel
    }

    private struct VowelContext {
        var initial: Bool
        var last: Bool
        /// A consonant follows in the word.
        var closed: Bool
        var afterVowel: Bool
        var single: Bool
        var nasal: Bool
    }

    private static func spell(_ vowel: Sound.Vowel, _ c: VowelContext) -> String {
        let n = c.nasal ? "n" : ""
        switch vowel {
        case .a: return "a" + n
        case .aa:
            if c.nasal { return c.last && !c.closed && !c.single ? "an" : "aan" } // kahan, haan, paanch
            // aaj, aap; baat, naam; dhanyawaad. Elsewhere "a": khana, tumhara, kya.
            if c.initial || (c.single && c.closed) || (c.last && c.closed) { return "aa" }
            return "a"
        case .i: return "i" + n
        case .ii:
            if c.nasal { return c.last && !c.closed ? "in" : "een" }
            return c.last && !c.closed || c.afterVowel ? "i" : "ee" // theek, peene; meri, koi
        case .u: return "u" + n
        case .uu:
            if c.afterVowel { return "u" + n } // jaunga, aaunga
            if c.nasal { return "oon" } // hoon
            return c.last && !c.closed ? "u" : "oo" // bhookh, zaroor; aalu
        case .e:
            if c.nasal { return c.last && !c.closed ? "ein" : "en" } // mein, chalein
            return "e"
        case .o: return "o" + n
        case .ai: return "ai" + n
        case .au: return "au" + n
        case .ri: return "ri"
        }
    }

    // MARK: Latin

    /// Apple's own Latin Hindi: its few unusual spellings changed, and its capitals on common
    /// Hindi words lowered mid-sentence. Other words are left as they are.
    static func normalizeLatin(_ word: String, sentenceStart: Bool) -> String {
        let lower = word.lowercased()
        let capitalized = word.first?.isUppercase == true && word.dropFirst().allSatisfy { !$0.isUppercase }
        if let usual = HinglishLexicon.latinSpellings[lower] {
            return sentenceStart && capitalized ? usual.prefix(1).uppercased() + usual.dropFirst() : usual
        }
        if !sentenceStart, capitalized, word.count > 1, HinglishLexicon.commonLatin.contains(lower) {
            return lower
        }
        return word
    }

    // MARK: Spoken hours

    private static let hours: [String: Int] = [
        "ek": 1, "do": 2, "teen": 3, "tin": 3, "char": 4, "chaar": 4, "panch": 5, "paanch": 5,
        "paach": 5, "chhe": 6, "chhah": 6, "che": 6, "cheh": 6, "chah": 6, "saat": 7, "sat": 7,
        "aath": 8, "ath": 8, "nau": 9, "das": 10, "dus": 10, "gyarah": 11, "gyaarah": 11,
        "barah": 12, "baarah": 12,
    ]
    private static let halfPast = ["saadhe", "sadhe", "saade", "sade"]
    private static let quarterPast = ["sawa", "sava", "savaa", "sawaa"]
    private static let quarterTo = ["paune", "pone", "paun"]
    private static let fixedHours = ["dedh": "1:30", "dhai": "2:30", "dhaai": "2:30", "dhaayi": "2:30"]

    /// "paanch baje" → "5 baje", "saadhe paanch baje" → "5:30 baje", "dedh baje" → "1:30 baje":
    /// an hour is written as digits, as people text times. Only before "baje"/"baj", so "kar do"
    /// stays words.
    private static func spokenHours(in pieces: [Piece]) -> [Piece] {
        var result = pieces
        let words = result.indices.filter { if case .word = result[$0] { return true }; return false }
        for (position, index) in words.enumerated() where position > 0 {
            let word = result[index].text.lowercased()
            guard word == "baje" || word == "baj" else { continue }
            let hourIndex = words[position - 1]
            guard isSpace(result, between: hourIndex, and: index) else { continue }
            let spoken = result[hourIndex].text.lowercased()
            if let fixed = fixedHours[spoken] {
                result[hourIndex] = .word(fixed)
                continue
            }
            guard let hour = hours[spoken] else { continue }
            var written = "\(hour)"
            if position >= 2 {
                let modifierIndex = words[position - 2]
                let modifier = result[modifierIndex].text.lowercased()
                if isSpace(result, between: modifierIndex, and: hourIndex) {
                    if halfPast.contains(modifier) {
                        written = "\(hour):30"
                    } else if quarterPast.contains(modifier) {
                        written = "\(hour):15"
                    } else if quarterTo.contains(modifier) {
                        written = "\(hour == 1 ? 12 : hour - 1):45"
                    }
                    if written.contains(":") {
                        for gap in modifierIndex..<hourIndex { result[gap] = .other("") }
                    }
                }
            }
            result[hourIndex] = .word(written)
        }
        return result
    }

    private static func isSpace(_ pieces: [Piece], between a: Int, and b: Int) -> Bool {
        guard b > a + 1 else { return false }
        return pieces[(a + 1)..<b].allSatisfy { piece in
            if case .other(let text) = piece { return text.allSatisfy(\.isWhitespace) }
            return false
        }
    }
}
