import Foundation

/// Transcripts built from the rules' own words (spoken punctuation, list markers, fillers,
/// self-corrections, counts, "at the rate"), French and other languages, mixed case,
/// emoji, unusual whitespace and scripts, decomposed accents, and a few very long ones.
enum ParityGenerator {
    static func transcript(_ random: inout DictationRandom) -> String {
        if random.chance(0.12) { return template(&random) }
        let count: Int
        switch random.below(100) {
        case 0..<70: count = 1 + random.below(12)
        case 70..<96: count = 13 + random.below(48)
        default: count = 200 + random.below(1300)
        }
        var text = random.chance(0.05) ? random.pick(edges) : ""
        for index in 0..<count {
            if index > 0 { text += separator(&random) }
            text += token(&random)
        }
        if random.chance(0.05) { text += random.pick(edges) }
        return text
    }

    /// Text around the insertion point.
    static func context(_ random: inout DictationRandom) -> String {
        random.chance(0.6) ? random.pick(contexts) : transcript(&random)
    }

    private static func token(_ random: inout DictationRandom) -> String {
        let word: String
        switch random.below(100) {
        case 0..<34: word = random.pick(ParityCorpus.vocabulary)
        case 34..<58: word = random.pick(common)
        case 58..<63: word = random.pick(fillers)
        case 63..<70: word = random.pick(numbers)
        case 70..<78: word = random.pick(marks)
        case 78..<83: word = random.pick(addresses)
        case 83..<86: word = random.pick(emoji)
        case 86..<91: word = exotic(&random)
        default: word = random.pick(names)
        }
        return cased(word, &random)
    }

    private static func cased(_ word: String, _ random: inout DictationRandom) -> String {
        var result: String
        switch random.below(100) {
        case 0..<60: result = word
        case 60..<75: result = word.prefix(1).uppercased() + word.dropFirst()
        case 75..<85: result = word.lowercased()
        case 85..<93: result = word.uppercased()
        default: result = word.map { random.chance(0.5) ? $0.uppercased() : String($0) }.joined()
        }
        if random.chance(0.05) { result = result.decomposedStringWithCanonicalMapping }
        return result
    }

    private static func separator(_ random: inout DictationRandom) -> String {
        let roll = random.below(1000)
        switch roll {
        case 0..<700: return " "
        case 700..<780: return ", "
        case 780..<830: return ". "
        case 830..<845: return "? "
        case 845..<855: return "! "
        case 855..<885: return "\n"
        case 885..<895: return "\n\n"
        case 895..<915: return "  "
        case 915..<925: return "\t"
        case 925..<945: return ""
        default: return random.pick(oddSeparators)
        }
    }

    private static func exotic(_ random: inout DictationRandom) -> String {
        let ranges: [ClosedRange<UInt32>] = [
            0x00A0...0x024F, 0x0300...0x036F, 0x0370...0x03FF, 0x0400...0x04FF, 0x0590...0x06FF, 0x0900...0x097F,
            0x0E00...0x0E7F, 0x1100...0x11FF, 0x13A0...0x13FF, 0x1E00...0x1FFF, 0x2000...0x206F, 0x2070...0x218F,
            0x2460...0x24FF, 0x3000...0x30FF, 0x4E00...0x4E2F, 0xAB70...0xABBF, 0xAC00...0xAC3F, 0xFB00...0xFB4F,
            0xFE00...0xFE2F, 0xFF00...0xFFEF, 0x10400...0x1044F, 0x1D400...0x1D7FF, 0x1F1E6...0x1F1FF,
            0x1F300...0x1F6FF, 0x1F900...0x1F9FF, 0xE0020...0xE007F, 0x0000...0x001F,
        ]
        var word = ""
        for _ in 0...random.below(3) {
            let range = random.pick(ranges)
            let value = range.lowerBound + UInt32(random.below(Int(range.upperBound - range.lowerBound) + 1))
            if let scalar = Unicode.Scalar(value) { word.unicodeScalars.append(scalar) }
        }
        return random.chance(0.5) ? random.pick(common) + word : word
    }

    private static func template(_ random: inout DictationRandom) -> String {
        let a = random.pick(common), b = random.pick(common), c = random.pick(common)
        let templates = [
            "\(a) at the rate \(b) dot com", "\(a) dot \(b) at the rate gmail dot co dot in.",
            "Email \(a) at sign \(b).com please", "test one two three", "Testing 123.", "test, one, 2, 3",
            "first \(a) second \(b) third \(c)", "First, \(a). Second, \(b). Finally, \(c).",
            "\(a) bullet \(b) bullet \(c)", "number one \(a) number two \(b)", "one, \(a), two, \(b), three, \(c)",
            "\(a), new line, \(b) new paragraph \(c)", "primero \(a) segundo \(b) por último \(c)",
            "a las 3, digo, a las 4", "el martes, perdón, el miércoles", "um 3, nein, um 4 Uhr",
            "på tirsdag, nei vent, torsdag", "\(a) comma \(b) period \(c) question mark",
            "open quote \(a) close quote \(b)", "\(a) open paren \(b) close paren", "\(a) dash \(b) hyphen \(c)",
            "\(a) apostrophe s \(b) hashtag \(c)", "point virgule \(a) deux points \(b) point d'interrogation",
            "ouvrez les guillemets \(a) fermez les guillemets", "Klammer auf \(a) Klammer zu Punkt",
            "punto y coma \(a) punto y aparte \(b)", "virgola \(a) punto e virgola \(b) punto a capo",
            "\(a) vírgula \(b) ponto final", "ça va ? Oui ! Voici : « \(a) »", "il est 10:30 ; voir https://x.fr/a?b",
            "um, so, \(a), uh, \(b)", "Yeah, ok, \(a). \(b)?", "Shopping list: \(a), \(b), and \(c).",
            "Dr. \(a) called. Call her back.", "\(a) gonna \(b), wanna \(c)?", "i think i'm \(a), i.e. \(b)",
            "\(a) smiley face \(b) winky", "Note: \(a)", "Gonna rain. He said “gonna win”.",
            "牛奶 ，然后 \(a)", "我用 iPhone 拍了", "如果天气好。", "买点水果 。 然后", "日本語 です \(a)",
            "\(a) at sign \(b).com at sign \(c).com", "mail \(a) at the rate gmail dot com at the rate yahoo dot com",
            "\(a) at sign gmail.com and \(b) at sign yahoo.com",
        ]
        return cased(random.pick(templates), &random)
    }

    private static let common = [
        "the", "a", "and", "then", "also", "or", "I", "you", "we", "it", "to", "of", "in", "on", "for", "with", "is",
        "was", "see", "call", "send", "milk", "eggs", "bread", "tomorrow", "today", "time", "list", "note", "yeah",
        "so", "okay", "ok", "well", "thanks", "hi", "hello", "meeting", "Sarah", "John", "Ravi", "gmail", "yahoo",
        "y", "o", "el", "la", "que", "de", "en", "por", "para", "hola", "gracias", "mañana", "sí", "no", "pero",
        "le", "les", "et", "je", "tu", "c'est", "d'accord", "très", "où", "à", "ça", "oui", "merci", "der", "die",
        "das", "und", "ich", "nicht", "für", "über", "Straße", "schließende", "il", "di", "che", "è", "perché",
        "o", "um", "não", "você", "obrigado", "og", "jeg", "på", "ikke", "æ", "ø", "å", "好的", "明天", "见", "。",
        "Ґанок", "і", "żółw", "kůň", "καλημέρα", "Όλα", "नमस्ते", "مرحبا", "שלום", "日本語", "한국어", "ÇA",
    ]
    private static let fillers = ["um", "umm", "uh", "uhh", "erm", "er", "ah", "ahh", "eh", "ehm", "em", "mmm",
                                  "hm", "hmm", "äh", "ähm", "öhm", "øh", "öh", "Um,", "UM", "Ehm,", "mm"]
    private static let numbers = ["1", "2", "3", "4", "10", "20", "21", "123", "1234", "12345", "1:30", "3:00",
                                  "2.5", "1.", "2)", "0", "05", "one", "two", "three", "four", "ten", "twenty",
                                  "zero", "uno", "dos", "tres", "zwei", "drei", "1st", "2nd", "3rd", "4th", "11th",
                                  "1º", "2ª", "１", "٣", "一", "三", "Ⅻ", "½", "²", "⑤", "३"]
    private static let marks = [",", ".", "?", "!", ";", ":", "…", "...", "-", "–", "—", "(", ")", "[", "]", "\"",
                                "“", "”", "‘", "’", "'", "«", "»", "„", "¿", "¡", "@", "#", "$", "/", "&", "*",
                                "。", "！", "？", "，", "」", "』", "）", "．", "｡", "•", "- ", ":-)", ";)", ":D", "`"]
    private static let addresses = ["john@gmail.com", "gmail", "dot", "com", "co", "in", "at", "the", "rate",
                                    "sign", "at-sign", "rategmail.com", "signyahoo.com.", "www.example.com",
                                    "https://x.io/a?b=1", "john.smith", "a_b-c+d", "co.in", "x@y", "@home"]
    private static let emoji = ["👍", "👍🏽", "🇫🇷", "❤️", "😀", "🧑‍💻", "1️⃣", "☺︎", "✌🏿", "🏳️‍🌈", "#️⃣"]
    private static let names = ["NASA", "iPhone", "McDonald's", "OK", "Ok", "I", "I'm", "I’ll", "i", "i'm", "i'll",
                                "i've", "i'd", "i.e.", "e.g.", "U.S.", "Dr.", "etc.", "gonna", "wanna", "gotta",
                                "kinda", "lemme", "Gonna", "LEMME", "Sarah's", "O'Brien", "J.", "K.", "iOS", "eBay",
                                "Dash", "Smiley", "Bullet", "Ravi", "Paris", "NYC", "ÉCOLE", "İstanbul", "ǅemal"]
    private static let edges = [" ", "  ", "\n", "\t", "\u{00A0}", "\u{3000}", " \n ", "\u{2028}", "\r\n", ", ",
                                "\u{301}", "\u{200D}", "\u{FEFF}", "\u{1F}", "\u{0B}"]
    private static let oddSeparators = ["\u{00A0}", "\u{202F}", "\u{3000}", "\u{2028}", "\u{2029}", "\r\n", "\r",
                                        " \u{301}", "\u{0B}", "\u{0C}", "\u{85}", "-", "/", "; ", ": ", "… ", ",",
                                        ".", " , ", " .", " ?", "\u{200B}", "\u{2009}", " - ", " — ", "\t\t", "\n \n"]
    private static let contexts = ["", " ", "Hello", "Hello ", "Hello.", "Hello. ", "and", "and ", "(", "“",
                                   "Note: ", "see (this)", "x\n", "Hi!” ", "\t", "¿", "Dijo «", " \u{301}", "Okay",
                                   ".", "world", " world", ")", "”", "'", "\u{2028}", "end.)", "Wait…", "OK? ",
                                   "我想", "，然后", "我到了。", "去超市", "「", "今天", "很好！", "iPhone", "한국어 "]
}
