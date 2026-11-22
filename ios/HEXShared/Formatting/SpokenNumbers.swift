import Foundation

/// Settings › Dictation › Numbers as digits, in the shared App Group so in-keyboard dictation
/// reads it too. On by default.
nonisolated struct DictationNumberSettings {
    static let enabledKey = "settings.dictationNumbersAsDigits"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Numbers said in words are written in digits (`SpokenNumbers`), in English.
    var isEnabled: Bool {
        get { defaults?.object(forKey: Self.enabledKey) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }
}

/// English numbers said in words written in digits, as Apple's `SpeechTranscriber` writes them,
/// with string rules and no model, so every speech model reads the same. Parakeet V2 switches a
/// whole sentence into words now and then ("The ratio is five point five.", "november twenty
/// second"), most often in a real voice: in Ryan's cloned voice it wrote words where Apple wrote
/// digits in 53 of 80 sentences, against 11 of 80 in macOS voices (macOS 27.2.1, FluidAudio
/// 0.15.5, `experiments/2026-11-22-hex-numbers`). Runs in `CleanupTagger.cleanIfEnabled` after
/// `SpokenCleanupRules.spokenCounts` and before the tagger, in every style but Verbatim. Text
/// that already has digits keeps them as written ("2,400", "$4.99", "7:15").
///
/// What becomes digits:
/// - decimals ("five point five" 5.5, "zero point two" 0.2, "three point one point four" 3.1.4,
///   "one point two million" 1.2 million), percentages ("twenty percent" 20%);
/// - numbers from ten up and compounds ("twenty five" 25, "three hundred and twelve" 312,
///   "fifteen thousand" 15,000; commas from 10,000, as Apple writes "8000" and "15,000");
/// - years ("nineteen ninety nine", "twenty twenty six");
/// - money with "dollars" ("five dollars" $5, "fifteen dollars and forty cents" $15.40);
/// - two to nine right before a plural noun or a unit ("two dogs", "four options", "six foot"),
///   and "one" only before a unit ("one mile"), as a decimal, a percentage or money;
/// - ranges ending in a plural noun or unit ("five to ten minutes"), times with "am", "pm" or
///   "o'clock" ("six am", "seven fifteen pm" 7:15), ordinals in dates ("November twenty second"
///   November 22nd, "the fourth of July"), three or more spoken digits ("four seven two nine"
///   4729, "oh four seven two" 0472 mid-sentence; ten as 555-123-4567), and "minus" or
///   "negative" before a number (-5, -$40).
///
/// All of a number goes into digits or none of it: "a hundred and fifty" is 150, "one point twenty
/// one" 1.21, "one in ten" 1 in 10, "five foot six" 5 foot 6, never "a hundred and 50".
///
/// What stays as said: a number the engine capitalised as a name, mid-sentence or with a capital
/// inside, before another capitalised word ("Twenty One Pilots", "Fifty Shades", "Ten Thousand
/// Villages"); custom vocabulary and learned words (`protectedTerms`), the person's fix for
/// names the engine writes in lower case ("five guys"); arithmetic ("ten minus five is five");
/// and, for now, "one" alone ("one of them", "no one"), times and prices without
/// a marker ("three thirty", "four ninety nine", "twelve fifty"), ranges and scores without a
/// unit ("eight to twelve", "three to one"), fractions ("two thirds", "one and a half"),
/// ordinals outside dates ("finished first", "the twenty first floor", "a second"), small
/// numbers before anything but a plural noun or unit ("at eight", "GPT five"), and any run of
/// number words that could be read two ways ("one thirty", "seven fifteen"). Other languages are
/// left as they are.
nonisolated enum SpokenNumbers {
    /// `protectedTerms` (custom vocabulary and learned words) stay as said wherever they match,
    /// whatever their case: someone who adds "Five Guys" or "Seven Eleven" keeps it.
    static func digits(in text: String, language: String?, protectedTerms: [String] = []) -> String {
        guard SpeechLanguagePicker.isEnglish(language), !text.isEmpty else { return text }
        let tokens = protect(tokenize(text), terms: protectedTerms)
        guard tokens.contains(where: { isNumberish($0) || isOrdinal($0.word) }) else { return text }
        var edits: [(range: Range<String.Index>, text: String)] = []
        var index = 0
        while index < tokens.count {
            switch match(tokens, at: index) {
            case let .edit(first, last, written):
                edits.append((tokens[first].range.lowerBound..<tokens[last].range.upperBound, written))
                index = last + 1
            case let .skip(last):
                index = max(last, index) + 1
            case .none:
                index += 1
            }
        }
        guard !edits.isEmpty else { return text }
        var result = ""
        var cursor = text.startIndex
        for edit in edits {
            result += text[cursor..<edit.range.lowerBound] + edit.text
            cursor = edit.range.upperBound
        }
        return result + text[cursor...]
    }

    // MARK: Tokens

    /// A word between spaces: the marks before it, the word, the marks after it.
    private struct Token {
        let range: Range<String.Index>
        let lead: String
        let core: String
        let trail: String
        let hasDigit: Bool
        /// First in the text, or after "." "!" "?" ":" or a line break.
        let startsSentence: Bool
        /// Inside a protected term.
        var protected = false
        var word: String { core.lowercased() }
        /// Nothing but the word: no mark after it ends the number.
        var open: Bool { trail.isEmpty }
    }

    private static func tokenize(_ text: String) -> [Token] {
        var tokens: [Token] = []
        var index = text.startIndex
        while index < text.endIndex {
            guard !text[index].isWhitespace else {
                index = text.index(after: index)
                continue
            }
            var end = index
            while end < text.endIndex, !text[end].isWhitespace { end = text.index(after: end) }
            let chunk = text[index..<end]
            let startsSentence: Bool = {
                guard let previous = tokens.last else { return true }
                if text[previous.range.upperBound..<index].contains(where: \.isNewline) { return true }
                return previous.trail.last.map { ".!?…:".contains($0) } ?? false
            }()
            let lead = chunk.prefix { !$0.isLetter && !$0.isNumber }
            let rest = chunk.dropFirst(lead.count)
            let core = String(rest.reversed().drop { !$0.isLetter && !$0.isNumber }.reversed())
            tokens.append(Token(
                range: index..<end,
                lead: String(lead),
                core: core,
                trail: String(rest.dropFirst(core.count)),
                hasDigit: chunk.contains(where: \.isNumber),
                startsSentence: startsSentence
            ))
            index = end
        }
        return tokens
    }

    /// Marks the tokens of every match of a term, compared word by word without case or marks
    /// ("seven-eleven" matches "Seven Eleven").
    private static func protect(_ tokens: [Token], terms: [String]) -> [Token] {
        func words(_ text: String) -> [String] {
            text.lowercased().split { !$0.isLetter && !$0.isNumber }.map(String.init)
        }
        var flat: [(word: String, token: Int)] = []
        for (index, token) in tokens.enumerated() {
            for word in words(token.core) { flat.append((word, index)) }
        }
        var result = tokens
        for term in terms {
            let wanted = words(term)
            guard !wanted.isEmpty, wanted.count <= flat.count,
                  wanted.contains(where: { kind($0) != nil || ordinals[$0] != nil || $0 == "oh" }) else { continue }
            for start in 0...(flat.count - wanted.count)
            where (0..<wanted.count).allSatisfy({ flat[start + $0].word == wanted[$0] }) {
                for offset in 0..<wanted.count { result[flat[start + offset].token].protected = true }
            }
        }
        return result
    }

    private enum Outcome {
        /// Tokens `first` through `last` become `text`.
        case edit(Int, Int, String)
        /// Tokens up to `last` stay as said.
        case skip(Int)
        case none
    }

    // MARK: Words

    private enum Kind {
        case digit(Int), teen(Int), tens(Int), tensUnit(Int), hundred, big(Int)
    }

    private static let digitWords: [String: Int] = [
        "zero": 0, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6, "seven": 7, "eight": 8, "nine": 9,
    ]
    private static let teenWords: [String: Int] = [
        "ten": 10, "eleven": 11, "twelve": 12, "thirteen": 13, "fourteen": 14, "fifteen": 15, "sixteen": 16,
        "seventeen": 17, "eighteen": 18, "nineteen": 19,
    ]
    private static let tensWords: [String: Int] = [
        "twenty": 20, "thirty": 30, "forty": 40, "fifty": 50, "sixty": 60, "seventy": 70, "eighty": 80, "ninety": 90,
    ]
    private static let bigWords: [String: Int] = [
        "thousand": 1_000, "million": 1_000_000, "billion": 1_000_000_000, "trillion": 1_000_000_000_000,
    ]
    private static let ordinals: [String: Int] = [
        "first": 1, "second": 2, "third": 3, "fourth": 4, "fifth": 5, "sixth": 6, "seventh": 7, "eighth": 8,
        "ninth": 9, "tenth": 10, "eleventh": 11, "twelfth": 12, "thirteenth": 13, "fourteenth": 14,
        "fifteenth": 15, "sixteenth": 16, "seventeenth": 17, "eighteenth": 18, "nineteenth": 19,
        "twentieth": 20, "thirtieth": 30, "fortieth": 40, "fiftieth": 50, "sixtieth": 60, "seventieth": 70,
        "eightieth": 80, "ninetieth": 90, "hundredth": 100, "thousandth": 1_000,
    ]
    private static let months: Set<String> = [
        "january", "february", "march", "april", "may", "june", "july", "august", "september", "october",
        "november", "december",
    ]
    /// Units after a number, singular or short: "six foot", "five K", "ten GB".
    private static let units: Set<String> = [
        "k", "km", "kg", "mph", "kph", "gb", "mb", "tb", "kb", "lb", "lbs", "oz", "ml", "mg", "cm", "mm", "ft",
        "mile", "foot", "inch", "kilo", "kilogram", "gram", "pound", "ounce", "degree", "liter", "litre",
        "gallon", "meter", "metre", "kilometer", "kilometre",
    ]
    /// The units "one" becomes 1 before ("one mile"); not "foot" ("one foot in the door").
    private static let oneUnits: Set<String> = [
        "km", "kg", "mph", "gb", "mb", "tb", "kb", "lb", "oz", "mile", "inch", "kilo", "kilogram", "gram",
        "pound", "ounce", "degree", "liter", "litre", "gallon", "meter", "metre", "kilometer", "kilometre",
    ]
    private static let irregularPlurals: Set<String> = [
        "people", "children", "men", "women", "feet", "teeth", "mice", "geese", "cattle",
    ]
    /// Words that may stand between a number and its plural noun: "two more minutes".
    private static let bridges: Set<String> = [
        "more", "other", "extra", "new", "old", "big", "small", "little", "different", "separate",
        "additional", "whole", "full", "main", "quick", "short", "long", "free", "good", "great", "bad",
        "major", "minor", "key", "young", "large", "huge", "tiny",
    ]
    /// Words ending in "s" that aren't plural nouns, or after which a number stays a word: verbs
    /// ("two equals"), fractions ("two thirds"), decades ("the nineteen nineties").
    private static let notPlural: Set<String> = [
        "this", "his", "hers", "its", "yes", "less", "unless", "perhaps", "always", "towards", "besides",
        "whereas", "across", "plus", "minus", "thus", "was", "has", "does", "goes", "gets", "says", "seems",
        "means", "makes", "equals", "times", "ours", "yours", "theirs", "sometimes", "lets", "afterwards",
        "nowadays", "looks", "sounds", "needs", "wants", "takes", "comes", "works", "feels", "stays",
        "becomes", "remains", "keeps", "tells", "gives", "knows", "thinks",
    ]
    private static let fractions: Set<String> = [
        "half", "halves", "third", "thirds", "quarter", "quarters", "fourths", "fifths", "sixths", "sevenths",
        "eighths", "ninths", "tenths",
    ]
    private static let decades: Set<String> = [
        "twenties", "thirties", "forties", "fifties", "sixties", "seventies", "eighties", "nineties",
        "hundreds", "thousands", "millions", "billions", "dozen", "dozens",
    ]

    private static func kind(_ word: String) -> Kind? {
        if let value = digitWords[word] { return .digit(value) }
        if let value = teenWords[word] { return .teen(value) }
        if let value = tensWords[word] { return .tens(value) }
        if word == "hundred" { return .hundred }
        if let value = bigWords[word] { return .big(value) }
        // "twenty-five", as some models write it.
        let parts = word.split(separator: "-")
        if parts.count == 2, let tens = tensWords[String(parts[0])], let unit = digitWords[String(parts[1])], unit > 0 {
            return .tensUnit(tens + unit)
        }
        return nil
    }

    private static func isNumberish(_ token: Token) -> Bool {
        !token.hasDigit && !token.protected && kind(token.word) != nil
    }

    private static func isOrdinal(_ word: String) -> Bool {
        if ordinals[word] != nil { return true }
        let parts = word.split(separator: "-")
        return parts.count == 2 && tensWords[String(parts[0])] != nil && ordinals[String(parts[1])] != nil
    }

    /// A single digit in a run or after "point": "oh" too.
    private static func spokenDigit(_ token: Token) -> Int? {
        guard !token.hasDigit, !token.protected else { return nil }
        return token.word == "oh" ? 0 : digitWords[token.word]
    }

    // MARK: Matching

    private static func match(_ t: [Token], at i: Int) -> Outcome {
        let token = t[i]
        guard !token.hasDigit, !token.protected else { return .none }
        // "November twenty second", "July fourth": "May" and "March" only with a capital.
        if months.contains(token.word), token.open,
           !["may", "march"].contains(token.word) || token.core.first?.isUppercase == true,
           let day = ordinal(t, at: i + 1) {
            return .edit(i + 1, day.end, t[i + 1].lead + day.text + t[day.end].trail)
        }
        // "the fourth of July".
        if token.word == "the", token.open, let day = ordinal(t, at: i + 1), t[day.end].open,
           day.end + 2 < t.count, t[day.end + 1].word == "of", t[day.end + 1].open,
           months.contains(t[day.end + 2].word) {
            return .edit(i + 1, day.end, t[i + 1].lead + day.text)
        }
        if token.word == "minus" || token.word == "negative", token.open, !followsNumber(t, i) {
            if case let .edit(first, last, written) = number(t, at: i + 1, forced: true), first == i + 1 {
                return .edit(i, last, token.lead + "-" + written)
            }
            return .none
        }
        if let run = digitRun(t, at: i) { return run }
        return number(t, at: i, forced: false)
    }

    /// "a hundred" or "a thousand" that a number goes on from: "a hundred and fifty" is 150.
    private static func leadsScale(_ t: [Token], _ i: Int) -> Bool {
        t[i].word == "a" && t[i].open && i + 2 < t.count && t[i + 1].open && !t[i + 1].protected
            && !t[i + 1].hasDigit && ["hundred", "thousand"].contains(t[i + 1].word)
    }

    /// A number the engine capitalised mid-sentence, or with a capital on a later word even at a
    /// sentence start: with a capitalised word after it, a name ("Twenty One Pilots", "Fifty
    /// Shades", "Eleven Labs", "Ten Thousand Villages"). "Twenty people" at a sentence start
    /// can't be told from a title, so it is a number.
    private static func isName(_ t: [Token], from i: Int, to end: Int) -> Bool {
        func capitalised(_ token: Token) -> Bool {
            token.core.first?.isUppercase == true && token.core != token.core.uppercased()
        }
        guard t[i].core.first?.isUppercase == true, end + 1 < t.count, t[end + 1].core.first?.isUppercase == true,
              !t[i].startsSentence || (i < end && t[(i + 1)...end].contains(where: capitalised)) else { return false }
        let next = t[end + 1].word
        return next != "i" && !next.hasPrefix("i'") && !next.hasPrefix("i’")
    }

    private static func followsNumber(_ t: [Token], _ i: Int) -> Bool {
        guard i > 0, t[i - 1].open else { return false }
        return t[i - 1].hasDigit || isNumberish(t[i - 1])
    }

    /// A day said as an ordinal, 1st to 31st: "third", "twenty second", "thirty-first".
    private static func ordinal(_ t: [Token], at j: Int) -> (text: String, end: Int)? {
        guard j < t.count, !t[j].hasDigit, !t[j].protected else { return nil }
        let word = t[j].word
        var value: Int?
        var end = j
        if let single = ordinals[word] {
            value = single
        } else if let tens = tensWords[word], t[j].open, j + 1 < t.count, !t[j + 1].hasDigit, !t[j + 1].protected,
                  let unit = ordinals[t[j + 1].word], unit < 10 {
            value = tens + unit
            end = j + 1
        } else {
            let parts = word.split(separator: "-")
            if parts.count == 2, let tens = tensWords[String(parts[0])], let unit = ordinals[String(parts[1])], unit < 10 {
                value = tens + unit
            }
        }
        guard let value, (1...31).contains(value) else { return nil }
        return ("\(value)\(suffix(value))", end)
    }

    private static func suffix(_ value: Int) -> String {
        if (11...13).contains(value % 100) { return "th" }
        switch value % 10 {
        case 1: return "st"
        case 2: return "nd"
        case 3: return "rd"
        default: return "th"
        }
    }

    /// Three or more digits said one by one: "four seven two nine" 4729, "nine one one" 911,
    /// and ten (with or without commas between groups of 3, 3 and 4) as 555-123-4567.
    private static func digitRun(_ t: [Token], at i: Int) -> Outcome? {
        // "oh" leads a run only mid-sentence and in lower case ("the code is oh four seven two"),
        // never "Oh, one more thing"; a run right after such an "Oh" stays in words.
        let leadingOh = t[i].word == "oh"
        if leadingOh, t[i].startsSentence || t[i].core != "oh" || !t[i].open { return nil }
        if !leadingOh, i > 0, t[i - 1].word == "oh", t[i - 1].open { return .skip(clusterEnd(t, from: i)) }
        guard let first = spokenDigit(t[i]), !followsNumber(t, i) else { return nil }
        var digits = [first]
        var groups: [Int] = []
        var group = 1
        var end = i
        while end + 1 < t.count, let next = spokenDigit(t[end + 1]), t[end].open || t[end].trail == "," {
            if !t[end].open {
                groups.append(group)
                group = 0
            }
            digits.append(next)
            group += 1
            end += 1
        }
        groups.append(group)
        guard digits.count >= 3 else { return nil }
        if leadingOh, groups.count > 1 { return nil }
        if t[end].open, end + 1 < t.count {
            // "seven oh five am" is a time.
            if isMeridiem(t[end + 1]) { return nil }
            if isNumberish(t[end + 1]) || t[end + 1].hasDigit { return .skip(end) }
        }
        let joined = digits.map(String.init).joined()
        if digits.count == 10, groups == [10] || groups == [3, 3, 4] {
            let phone = Array(joined)
            return .edit(i, end, t[i].lead + String(phone[0..<3]) + "-" + String(phone[3..<6]) + "-"
                + String(phone[6...]) + t[end].trail)
        }
        // A list said with commas ("five, six, seven") isn't one number.
        guard groups.count == 1 else { return nil }
        return .edit(i, end, t[i].lead + joined + t[end].trail)
    }

    // MARK: Numbers

    private struct Cardinal {
        var value: Int
        var end: Int
        var words: Int
        var isYear = false
        var hasScale = false
        /// "five million": the value and word of the last scale when it ends the number.
        var bigScale: (value: Int, word: String)?
        var bigScales = 0
        var ambiguous = false
        /// The value when the number is one digit word ("two").
        var single: Int?
    }

    private enum Last { case none, unit, teen, tens, hundred, big, and }

    /// "nineteen ninety nine", "twenty twenty six", "nineteen oh five".
    private static func year(_ t: [Token], at i: Int) -> Cardinal? {
        guard t[i].open, i + 1 < t.count, !t[i + 1].hasDigit else { return nil }
        let century: Int
        switch t[i].word {
        case "nineteen": century = 19
        case "twenty": century = 20
        default: return nil
        }
        let j = i + 1
        var rest: Int?
        var end = j
        func digit(after k: Int) -> Int? {
            guard t[k].open, k + 1 < t.count, !t[k + 1].hasDigit, case let .digit(value)? = kind(t[k + 1].word),
                  value > 0 else { return nil }
            return value
        }
        switch kind(t[j].word) {
        case let .teen(value)?:
            rest = value
        case let .tens(value)?:
            rest = value
            if let unit = digit(after: j) {
                rest = value + unit
                end = j + 1
            }
        case let .tensUnit(value)?:
            rest = value
        default:
            if t[j].word == "oh", let unit = digit(after: j) {
                rest = unit
                end = j + 1
            }
        }
        guard let rest else { return nil }
        return Cardinal(value: century * 100 + rest, end: end, words: end - i + 1, isYear: true)
    }

    private static func cardinal(_ t: [Token], at i: Int) -> Cardinal? {
        guard i < t.count, isNumberish(t[i]) || leadsScale(t, i) else { return nil }
        var parsed: Cardinal
        if let year = year(t, at: i) {
            parsed = year
        } else {
            let fromA = leadsScale(t, i)
            var total = 0
            var current = fromA ? 1 : 0
            var last = fromA ? Last.unit : Last.none
            var words = fromA ? 1 : 0
            var end = i
            var j = fromA ? i + 1 : i
            var hasScale = false
            var lastBig: (value: Int, word: String)?
            var bigScales = 0
            var ambiguous = false
            var zero = false
            scan: while j < t.count {
                let token = t[j]
                if token.hasDigit { break }
                if token.word == "and", last == .hundred || last == .big, token.open, j + 1 < t.count,
                   isNumberish(t[j + 1]), additive(kind(t[j + 1].word)) {
                    last = .and
                    j += 1
                    continue
                }
                guard let found = kind(token.word) else { break }
                let adds: Set<Last> = [.none, .hundred, .big, .and]
                switch found {
                case let .digit(value):
                    if value == 0 {
                        guard words == 0 else { ambiguous = true; break scan }
                        zero = true
                    } else {
                        guard adds.contains(last) || last == .tens else { ambiguous = true; break scan }
                        current += value
                    }
                    last = .unit
                case let .teen(value), let .tens(value), let .tensUnit(value):
                    guard adds.contains(last) else { ambiguous = true; break scan }
                    current += value
                    if case .tens = found { last = .tens } else if case .teen = found { last = .teen } else { last = .unit }
                case .hundred:
                    guard [.unit, .teen, .tens].contains(last), (1...99).contains(current) else {
                        ambiguous = words > 0
                        break scan
                    }
                    current *= 100
                    last = .hundred
                    hasScale = true
                case let .big(value):
                    guard [.unit, .teen, .tens, .hundred].contains(last), current > 0,
                          lastBig.map({ value < $0.value }) ?? true else {
                        ambiguous = words > 0
                        break scan
                    }
                    total += current * value
                    current = 0
                    last = .big
                    hasScale = true
                    lastBig = (value, token.word)
                    bigScales += 1
                }
                words += 1
                end = j
                j += 1
                if zero || !token.open { break }
            }
            guard words > 0 else { return nil }
            // "a hundred" alone stays: "a hundred percent", "a hundred dollars".
            if fromA, end <= i + 1 { return nil }
            parsed = Cardinal(value: total + current, end: end, words: words, hasScale: hasScale)
            parsed.bigScale = last == .big ? lastBig : nil
            parsed.bigScales = bigScales
            parsed.ambiguous = ambiguous
            if words == 1, case let .digit(value)? = kind(t[i].word) { parsed.single = value }
        }
        // Another number word right after, or before, makes it two numbers or a time: "three
        // thirty", "four ninety nine", "twenty twenty five hundred".
        if t[parsed.end].open, parsed.end + 1 < t.count, isNumberish(t[parsed.end + 1]) { parsed.ambiguous = true }
        if followsNumber(t, i), !t[i - 1].hasDigit { parsed.ambiguous = true }
        // The rest of a number we didn't take whole: "… hundred and fifty", "… point twenty one".
        if i >= 2, t[i - 1].open, t[i - 2].open, !t[i - 2].hasDigit, !t[i - 2].protected,
           (t[i - 1].word == "and" && (t[i - 2].word == "hundred" || bigWords[t[i - 2].word] != nil))
            || (t[i - 1].word == "point" && kind(t[i - 2].word) != nil) {
            parsed.ambiguous = true
        }
        return parsed
    }

    private static func additive(_ kind: Kind?) -> Bool {
        switch kind {
        case let .digit(value)?: value > 0
        case .teen?, .tens?, .tensUnit?: true
        default: false
        }
    }

    /// The last token of the run of number words from `i`.
    private static func clusterEnd(_ t: [Token], from i: Int) -> Int {
        var j = i
        while j + 1 < t.count, t[j].open, isNumberish(t[j + 1]) || t[j + 1].word == "point" { j += 1 }
        return j
    }

    private static func written(_ number: Cardinal) -> String {
        if number.isYear { return String(number.value) }
        if let big = number.bigScale, number.bigScales == 1, big.value >= 1_000_000, number.value % big.value == 0 {
            return "\(number.value / big.value) \(big.word)"
        }
        return grouped(number.value)
    }

    /// Commas from 10,000 up: Apple writes "8000" and "2400" but "15,000".
    private static func grouped(_ value: Int) -> String {
        let plain = String(value)
        guard value >= 10_000 else { return plain }
        var result = ""
        for (offset, character) in plain.reversed().enumerated() {
            if offset > 0, offset % 3 == 0 { result.append(",") }
            result.append(character)
        }
        return String(result.reversed())
    }

    /// "point five", "point one point four" after a number, and a scale after one decimal
    /// ("one point two million").
    private static func decimal(_ t: [Token], after number: Cardinal, from i: Int) -> (text: String, end: Int, ambiguous: Bool)? {
        guard !number.isYear, number.bigScale == nil, t[number.end].open, number.end + 2 < t.count,
              t[number.end + 1].word == "point", t[number.end + 1].open, !t[number.end + 1].hasDigit else { return nil }
        // "at one point", "that one point".
        if number.value == 1, number.words == 1, i > 0,
           ["at", "some", "that", "this", "any", "the", "one", "a"].contains(t[i - 1].word) { return nil }
        var text = String(number.value)
        var end = number.end
        var points = 0
        var j = number.end + 1
        while j + 1 < t.count, t[end].open, t[j].word == "point", t[j].open, !t[j].hasDigit {
            var digits = ""
            var k = j + 1
            if let pair = twoDigits(t, at: k) {
                // "one point twenty one" is 1.21.
                digits = pair.text
                end = pair.end
                k = pair.end + 1
            } else {
                while k < t.count, let digit = spokenDigit(t[k]) {
                    digits += String(digit)
                    end = k
                    k += 1
                    if !t[end].open { break }
                }
            }
            guard !digits.isEmpty else { break }
            text += "." + digits
            points += 1
            j = k
        }
        guard points > 0 else { return nil }
        if points == 1, t[end].open, end + 1 < t.count, let big = bigWords[t[end + 1].word], big >= 1_000_000,
           !t[end + 1].hasDigit {
            text += " " + t[end + 1].word
            end += 1
        }
        let ambiguous = t[end].open && end + 1 < t.count && isNumberish(t[end + 1])
        return (text, end, ambiguous)
    }

    /// "twenty one", "fifteen", "forty" after "point".
    private static func twoDigits(_ t: [Token], at k: Int) -> (text: String, end: Int)? {
        guard k < t.count, !t[k].hasDigit, !t[k].protected else { return nil }
        switch kind(t[k].word) {
        case let .teen(value)?, let .tensUnit(value)?:
            return (String(value), k)
        case let .tens(value)?:
            if t[k].open, k + 1 < t.count, !t[k + 1].hasDigit, !t[k + 1].protected,
               case let .digit(unit)? = kind(t[k + 1].word), unit > 0 {
                return (String(value + unit), k + 1)
            }
            return (String(value), k)
        default:
            return nil
        }
    }

    private static func number(_ t: [Token], at i: Int, forced: Bool) -> Outcome {
        guard i < t.count, isNumberish(t[i]) || leadsScale(t, i) else { return .none }
        if let time = clockTime(t, at: i) { return time }
        guard let number = cardinal(t, at: i) else { return .none }
        if number.ambiguous { return .skip(clusterEnd(t, from: i)) }
        var text = written(number)
        var end = number.end
        var isDecimal = false
        if let point = decimal(t, after: number, from: i) {
            if point.ambiguous { return .skip(clusterEnd(t, from: point.end)) }
            text = point.text
            end = point.end
            isDecimal = true
        }
        let lead = t[i].lead
        let next = end + 1
        let open = t[end].open && next < t.count
        // Digits right beside it: the model already wrote this number its way.
        if open, t[next].hasDigit { return .skip(end) }
        if i > 0, t[i - 1].open, t[i - 1].hasDigit { return .skip(end) }
        if !forced, isName(t, from: i, to: end) { return .skip(end) }
        let plain = !isDecimal && !number.isYear && !number.hasScale
        // "ten minus five is five": arithmetic stays as said, whole.
        if open, !forced, ["minus", "plus", "times", "over", "equals"].contains(t[next].word), t[next].open,
           next + 1 < t.count, isNumberish(t[next + 1]) {
            var last = clusterEnd(t, from: next + 1)
            // …and its result: "ten times two is twenty".
            if t[last].open, last + 2 < t.count, ["is", "equals", "makes"].contains(t[last + 1].word), t[last + 1].open,
               isNumberish(t[last + 2]) {
                last = clusterEnd(t, from: last + 2)
            }
            return .skip(last)
        }
        // "one in ten people", "eight out of ten", "nine times out of ten": with the second
        // number in digits, the first goes too.
        if open, !forced, plain, let second = ratioSecond(t, after: end),
           case let .edit(first, _, _) = Self.number(t, at: second, forced: false), first == second {
            return .edit(i, end, lead + text)
        }
        // "five foot six": inches after a height in digits.
        if !forced, plain, number.value <= 11, i >= 2, ["foot", "feet"].contains(t[i - 1].word), t[i - 1].open,
           t[i - 2].open, t[i - 2].hasDigit || height(t, at: i - 2) {
            return .edit(i, end, lead + text + t[end].trail)
        }
        if open {
            let word = t[next].word
            if word == "percent" { return .edit(i, next, lead + text + "%" + t[next].trail) }
            if word == "per", t[next].open, next + 1 < t.count, t[next + 1].word == "cent" {
                return .edit(i, next + 1, lead + text + "%" + t[next + 1].trail)
            }
            if word == "dollar" || word == "dollars" {
                return money(t, at: i, amount: text, isDecimal: isDecimal, dollars: next)
            }
            // "twenty first": an ordinal, outside dates for now.
            if isOrdinal(word) { return .skip(next) }
            if plain, (1...12).contains(number.value), isMeridiem(t[next]) || word == "o'clock" || word == "o’clock" {
                return .edit(i, end, lead + text)
            }
            if !forced, plain, number.value < 100, let range = range(t, number, at: i, end: end, text: lead + text) {
                return range
            }
            if isCountNoun(t, next) {
                if forced || !plain || number.value >= 10 { return .edit(i, end, lead + text) }
                if let single = number.single {
                    let possessive = i > 0 && ["my", "your", "his", "her", "our", "their"].contains(t[i - 1].word)
                    if single >= 2, !(word == "cents" && possessive) { return .edit(i, end, lead + text) }
                    if single == 1, oneUnits.contains(word) { return .edit(i, end, lead + text) }
                }
                return .skip(end)
            }
            if fractions.contains(word) || decades.contains(word) { return .skip(end) }
            if word == "and", next + 2 < t.count, t[next + 1].word == "a", fractions.contains(t[next + 2].word) {
                return .skip(end)
            }
        }
        if forced || !plain || number.value >= 10 { return .edit(i, end, lead + text + t[end].trail) }
        return .skip(end)
    }

    /// Where the second number of "one in ten", "eight out of ten", "nine times out of ten" starts.
    private static func ratioSecond(_ t: [Token], after end: Int) -> Int? {
        func word(_ k: Int) -> String? { k < t.count && t[k].open ? t[k].word : nil }
        var k = end + 1
        if word(k) == "times" { k += 1 }
        if word(k) == "in", k == end + 1 {
            k += 1
        } else if word(k) == "out", word(k + 1) == "of" {
            k += 2
        } else {
            return nil
        }
        return k < t.count && isNumberish(t[k]) ? k : nil
    }

    /// The feet of a height said in words: "five" in "five foot six".
    private static func height(_ t: [Token], at k: Int) -> Bool {
        guard let feet = cardinal(t, at: k), !feet.ambiguous, feet.end == k, let value = feet.single else { return false }
        return value >= 2
    }

    /// "five to ten minutes", "two or three days", "between five and ten minutes": the first
    /// number goes into digits with the second, which its own rule writes, only when a plural
    /// noun or unit follows. Without one it may be a score, a time or "nine to five", so both
    /// stay as said. "one or two" stays too.
    private static func range(_ t: [Token], _ first: Cardinal, at i: Int, end: Int, text: String) -> Outcome? {
        let link = end + 1
        let word = t[link].word
        let between = word == "and" && i > 0 && t[i - 1].word == "between"
        guard ["to", "through", "or", "past"].contains(word) || between, t[link].open,
              let second = cardinal(t, at: link + 1), !second.ambiguous else { return nil }
        var secondEnd = second.end
        var secondPlain = !second.isYear && !second.hasScale && second.value < 100
        if let point = decimal(t, after: second, from: link + 1) {
            secondEnd = point.end
            secondPlain = false
        }
        // "from zero to one hundred": a second number that is plainly a number takes the first.
        guard secondPlain else {
            // "two to three hundred" and "one or two hundred" mean hundreds: both stay as said.
            let endsWithScale = t[secondEnd].word == "hundred" || bigWords[t[secondEnd].word] != nil
            if word == "past" || (endsWithScale && (word == "or" || first.value >= 2)) { return .skip(secondEnd) }
            return .edit(i, end, text)
        }
        let after = secondEnd + 1 < t.count && t[secondEnd].open ? t[secondEnd + 1].word : ""
        let percent = after == "percent" || (after == "per" && secondEnd + 2 < t.count && t[secondEnd + 2].word == "cent")
        let unitAfter = t[secondEnd].open && (isCountNoun(t, secondEnd + 1) || percent)
        if word == "past" || !unitAfter || (word == "or" && first.single == 1) { return .skip(secondEnd) }
        return .edit(i, end, text)
    }

    /// "seven fifteen pm" as 7:15, "seven oh five am" as 7:05: the marker stays as written.
    private static func clockTime(_ t: [Token], at i: Int) -> Outcome? {
        guard i + 2 < t.count, t[i].open, t[i + 1].open, !t[i + 1].hasDigit else { return nil }
        let hour: Int
        switch kind(t[i].word) {
        case let .digit(value)? where value > 0: hour = value
        case let .teen(value)? where value <= 12: hour = value
        default: return nil
        }
        var minutes: Int?
        var end = i + 1
        let unit: Int? = {
            guard i + 2 < t.count, !t[i + 2].hasDigit, case let .digit(value)? = kind(t[i + 2].word), value > 0 else {
                return nil
            }
            return value
        }()
        switch kind(t[i + 1].word) {
        case let .teen(value)?:
            minutes = value
        case let .tens(value)? where value <= 50:
            minutes = value
            if let unit {
                minutes = value + unit
                end = i + 2
            }
        case let .tensUnit(value)? where value < 60:
            minutes = value
        default:
            if t[i + 1].word == "oh", let unit {
                minutes = unit
                end = i + 2
            }
        }
        guard let minutes, t[end].open, end + 1 < t.count, isMeridiem(t[end + 1]) else { return nil }
        return .edit(i, end, t[i].lead + "\(hour):" + (minutes < 10 ? "0" : "") + "\(minutes)")
    }

    private static func isMeridiem(_ token: Token) -> Bool {
        let plain = token.word.replacingOccurrences(of: ".", with: "")
        return !token.hasDigit && (plain == "am" || plain == "pm")
    }

    /// "$5", "$15.40" (with "and forty cents"), "$1.2 million", "$5.50".
    private static func money(_ t: [Token], at i: Int, amount: String, isDecimal: Bool, dollars: Int) -> Outcome {
        var written = amount
        var end = dollars
        if isDecimal, let dot = written.firstIndex(of: "."), written.distance(from: dot, to: written.endIndex) == 2 {
            written += "0"
        }
        if !isDecimal, !written.contains(" "), t[dollars].open {
            var j = dollars + 1
            if j < t.count, t[j].word == "and", t[j].open { j += 1 }
            if let cents = cardinal(t, at: j), !cents.ambiguous, !cents.isYear, !cents.hasScale,
               (1...99).contains(cents.value), t[cents.end].open, cents.end + 1 < t.count,
               ["cents", "cent"].contains(t[cents.end + 1].word) {
                written += "." + (cents.value < 10 ? "0" : "") + "\(cents.value)"
                end = cents.end + 1
            }
        }
        return .edit(i, end, t[i].lead + "$" + written + t[end].trail)
    }

    /// A plural noun or a unit: "dogs", "people", "more minutes", "foot", "K".
    private static func isCountNoun(_ t: [Token], _ k: Int) -> Bool {
        guard k < t.count, !t[k].hasDigit, !t[k].core.isEmpty else { return false }
        let word = t[k].word
        if units.contains(word) { return true }
        if bridges.contains(word), t[k].open { return isPlural(t, k + 1) }
        return isPlural(t, k)
    }

    private static func isPlural(_ t: [Token], _ k: Int) -> Bool {
        guard k < t.count, !t[k].hasDigit, t[k].core.first?.isLowercase == true,
              t[k].core.allSatisfy(\.isLetter) else { return false }
        let word = t[k].word
        if irregularPlurals.contains(word) { return true }
        guard word.count >= 3, word.hasSuffix("s"), !notPlural.contains(word), !fractions.contains(word),
              !decades.contains(word) else { return false }
        return !["ss", "us", "is"].contains { word.hasSuffix($0) }
    }
}
