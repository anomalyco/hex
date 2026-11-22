// The dictation parity oracle: a copy of ios/HEXShared/SpokenCleanupRules.swift at dev 93db6ad7 (unchanged
// since d6efeddb), its types renamed with an "Oracle" suffix, so the Rust port in
// crates/keyboard/core/src/dictation/ is checked against today's Swift (DictationParityTests).
// Only the UserDefaults settings struct is left out. Never edit the rules here.
import Foundation

/// Clean up what I say for Spanish, German, and Norwegian, which the English cleanup tagger
/// was never trained on. Run on Spanish, the tagger dropped negations ("Te digo que no puedo
/// ir" became "Te puedo ir"), so these languages get two word rules instead, each needing
/// the commas a pause leaves:
/// - the language's hesitation sounds go anywhere ("eh", "mmm"; `TranscriptFormatterOracle`'s
///   list);
/// - a filler phrase set off by commas goes ("Es caro, o sea, no lo compro");
/// - a spoken correction ("a las 3, digo, a las 4") drops its marker and the words it
///   replaces, counted back from the nearest earlier word that matches the repair's first
///   word ("a"), or else is of its kind (a number, weekday, month, or today and tomorrow),
///   within the sentence and `maxReach` words. With no such word, the sentence stays as
///   said.
/// Like the tagger, it only removes words; `CleanupReconstructionOracle` repairs the commas and
/// capitals next to a removal.
nonisolated enum SpokenCleanupRulesOracle {
    /// How far back a correction looks for the words it replaces.
    static let maxReach = 8

    struct Rules {
        /// Removed between commas, or at a sentence start before a comma.
        let fillers: [[String]]
        /// Start a correction when set off by commas ("3, digo, 4"), or by a sentence end
        /// and a comma ("3. Digo, 4").
        let markers: [[String]]
        /// Markers clear enough to count without commas ("3 mejor dicho 4").
        let plainMarkers: [[String]]
        /// Words of one kind can replace each other ("martes, perdón, miércoles").
        let kinds: [Set<String>]
        /// Words that lose their comma when the filler after them goes ("Y, este, fuimos").
        let noComma: Set<String>

        /// The index in `kinds` of a bare word, with any word holding a digit a number.
        func kind(of word: String) -> Int? {
            if word.contains(where: \.isNumber) { return kinds.count }
            return kinds.firstIndex { $0.contains(word) }
        }
    }

    static func rules(for language: String?) -> Rules? {
        let code = language.flatMap { Locale(identifier: $0).language.languageCode?.identifier }
        switch code {
        case "es": return spanish
        case "de": return german
        case "nb", "nn", "no": return norwegian
        default: return nil
        }
    }

    static func supports(_ language: String?) -> Bool {
        rules(for: language) != nil
    }

    /// `text` with its fillers and spoken corrections removed, or `text` itself when
    /// nothing goes or the language has no rules.
    static func clean(_ text: String, language: String?) -> String {
        guard let rules = rules(for: language) else { return text }
        let words = CleanupTaggerOracle.words(in: text)
        var cleaned = removingFillers(words, rules: rules, language: language)
        cleaned = resolvingCorrections(cleaned, rules: rules)
        return cleaned == words ? text : cleaned.joined(separator: " ")
    }

    // MARK: Fillers

    private static func removingFillers(_ input: [String], rules: Rules, language: String?) -> [String] {
        var words = input
        var deletes = Array(repeating: false, count: words.count)
        let hesitation = TranscriptFormatterOracle.fillerPattern(for: language).flatMap {
            try? NSRegularExpression(pattern: "^(?:\($0))$", options: .caseInsensitive)
        }
        var index = 0
        while index < words.count {
            if let hesitation, isHesitation(words[index], hesitation) {
                deletes[index] = true
                index += 1
                continue
            }
            guard let phrase = rules.fillers.first(where: { matches($0, words, at: index) }) else {
                index += 1
                continue
            }
            let end = index + phrase.count - 1
            let opens = index == 0 || isPause(words[index - 1]) || endsSentence(words[index - 1])
            guard opens, words[end].hasSuffix(","), end + 1 < words.count, !hasOpeningMark(words[index]) else {
                index += 1
                continue
            }
            if index > 0, words[index - 1].hasSuffix(","), rules.noComma.contains(bare(words[index - 1])) {
                words[index - 1].removeLast()
            }
            for position in index...end {
                deletes[position] = true
            }
            index = end + 1
        }
        guard deletes.contains(true) else { return input }
        return CleanupTaggerOracle.words(in: CleanupReconstructionOracle.reconstruct(words, deletes: deletes))
    }

    /// A sound like "eh", unless it asks or exclaims ("¿eh?", "¡Eh, tú!").
    private static func isHesitation(_ word: String, _ pattern: NSRegularExpression) -> Bool {
        guard !hasOpeningMark(word), let last = word.last, last != "?", last != "!" else { return false }
        let bare = bare(word)
        return pattern.firstMatch(in: bare, range: NSRange(bare.startIndex..., in: bare)) != nil
    }

    // MARK: Corrections

    private struct Correction {
        /// The first replaced word, the marker's last word, and the repair's first word.
        let start: Int
        let markerEnd: Int
        let repair: Int
    }

    private static func resolvingCorrections(_ input: [String], rules: Rules) -> [String] {
        var words = input
        var from = 0
        // Each correction removes words, so this ends; the cap only guards against a mistake.
        for _ in 0..<words.count {
            guard let found = nextCorrection(in: words, from: from, rules: rules) else { break }
            words[found.repair] = taking(words[found.start], into: words[found.repair])
            var deletes = Array(repeating: false, count: words.count)
            for position in found.start...found.markerEnd {
                deletes[position] = true
            }
            words = CleanupTaggerOracle.words(in: CleanupReconstructionOracle.reconstruct(words, deletes: deletes))
            from = found.start + 1
        }
        return words
    }

    private static func nextCorrection(in words: [String], from: Int, rules: Rules) -> Correction? {
        guard from < words.count else { return nil }
        for markerStart in max(1, from)..<words.count {
            for marker in rules.markers where matches(marker, words, at: markerStart) {
                let markerEnd = markerStart + marker.count - 1
                let repair = markerEnd + 1
                guard repair < words.count, !hasOpeningMark(words[markerStart]) else { continue }
                let before = words[markerStart - 1]
                let setOff = (isPause(before) || endsSentence(before)) && words[markerEnd].hasSuffix(",")
                let plain = rules.plainMarkers.contains(marker)
                    && !endsSentence(before)
                    && !isPause(words[markerEnd])
                    && !endsSentence(words[markerEnd])
                guard setOff || plain else { continue }
                let first = bare(words[repair])
                guard !first.isEmpty else { continue }

                // The sentence the replaced words are in, at most `maxReach` words back.
                var low = markerStart - 1
                while low > 0, !endsSentence(words[low - 1]), markerStart - low < maxReach {
                    low -= 1
                }
                let candidates = (low..<markerStart).reversed()
                var start = candidates.first { bare(words[$0]) == first }
                if start == nil, let kind = rules.kind(of: first) {
                    start = candidates.first { rules.kind(of: bare(words[$0])) == kind }
                }
                guard let start else { continue }
                return Correction(start: start, markerEnd: markerEnd, repair: repair)
            }
        }
        return nil
    }

    /// The repair takes the place of the words it corrects: its first word takes the first
    /// replaced word's capital and opening mark ("¿El martes, perdón, el miércoles?" becomes
    /// "¿El miércoles?").
    private static func taking(_ replaced: String, into repair: String) -> String {
        let marks = String(replaced.prefix { openingMarks.contains($0) })
        var result = repair
        if !marks.isEmpty, !hasOpeningMark(result) {
            result = marks + result
        }
        let source = replaced.dropFirst(marks.count).first
        guard let source, source.isLetter,
              let index = result.firstIndex(where: \.isLetter),
              !result[result.index(after: index)...].contains(where: \.isUppercase) else {
            return result
        }
        let letter = String(result[index])
        let cased = source.isUppercase ? letter.uppercased() : letter.lowercased()
        result.replaceSubrange(index...index, with: cased)
        return result
    }

    // MARK: Words

    private static let openingMarks: Set<Character> = ["¿", "¡", "\"", "“", "«", "(", "„"]
    private static let closingMarks: Set<Character> = [",", ".", ";", ":", "!", "?", "…", "\"", "”", "»", ")"]

    /// Lowercased, without opening marks or trailing punctuation.
    static func bare(_ word: String) -> String {
        var scalars = Substring(word)
        while let first = scalars.first, openingMarks.contains(first) {
            scalars = scalars.dropFirst()
        }
        while let last = scalars.last, closingMarks.contains(last) {
            scalars = scalars.dropLast()
        }
        return scalars.lowercased()
    }

    private static func matches(_ phrase: [String], _ words: [String], at index: Int) -> Bool {
        guard index + phrase.count <= words.count else { return false }
        for (offset, part) in phrase.enumerated() {
            let word = words[index + offset]
            // Inside a phrase only a comma may follow a word ("no, espera,").
            if offset < phrase.count - 1, let last = word.last, last != ",", closingMarks.contains(last) {
                return false
            }
            if bare(word) != part { return false }
        }
        return true
    }

    private static func isPause(_ word: String) -> Bool {
        guard let last = word.last else { return false }
        return last == "," || last == ";" || last == ":"
    }

    private static func endsSentence(_ word: String) -> Bool {
        var trimmed = Substring(word)
        while let last = trimmed.last, "\"”»)".contains(last) {
            trimmed = trimmed.dropLast()
        }
        guard let last = trimmed.last else { return false }
        return ".!?…".contains(last)
    }

    private static func hasOpeningMark(_ word: String) -> Bool {
        word.first.map(openingMarks.contains) ?? false
    }

    // MARK: Spoken symbols

    /// English `text` with "@" for its spoken names, or `text` itself. "at the rate" (what
    /// India calls @) becomes "@" only before an address's domain ("gmail dot com",
    /// "gmail.com"), never in "at the rate of 5%" or "at the rate we're going"; "at sign"
    /// always does. Before a domain, the word before (with any "dot"-joined parts) and the
    /// domain join it as one lowercase address: "John at the rate Gmail dot com." becomes
    /// "john@gmail.com.", "dot" becoming "." only there. Elsewhere "at sign" is a lone "@"
    /// ("the @ key"). Words around it keep their spacing.
    static func spokenSymbols(in text: String, language: String?) -> String {
        guard SpeechLanguagePickerOracle.isEnglish(language), text.range(of: "at", options: .caseInsensitive) != nil else {
            return text
        }
        let tokens = splittingGluedDomains(symbolTokens(in: text), in: text)
        var edits: [(range: Range<String.Index>, text: String)] = []
        var index = 0
        while index < tokens.count {
            guard let marker = symbolMarker(tokens, at: index) else {
                index += 1
                continue
            }
            let next = marker.end + 1
            if marker.ambiguous, next < tokens.count, bare(tokens[next].word) == "of" {
                index = next
                continue
            }
            if let domain = addressDomain(tokens, from: next) {
                var start = index
                var local: [String] = []
                if let last = tokens[safe: index - 1], let part = localPart(last.word) {
                    local = [part]
                    start = index - 1
                    // "john dot smith at the rate…": the parts before join with dots too.
                    while start >= 2, tokens[start - 1].word.lowercased() == "dot",
                          let part = localPart(tokens[start - 2].word) {
                        local.insert(part, at: 0)
                        start -= 2
                    }
                }
                let address = (local.joined(separator: ".") + "@" + domain.labels.joined(separator: ".")).lowercased()
                edits.append((tokens[start].range.lowerBound..<tokens[domain.end].range.upperBound, address + domain.trailing))
                index = domain.end + 1
            } else if !marker.ambiguous, bare(tokens[safe: next]?.word ?? "") != "language" {
                // "good at sign language" is no symbol.
                let last = tokens[marker.end].word
                let trailing = String(last.reversed().prefix { closingMarks.contains($0) }.reversed())
                edits.append((tokens[index].range.lowerBound..<tokens[marker.end].range.upperBound, "@" + trailing))
                index = marker.end + 1
            } else {
                index = marker.end + 1
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

    private struct SymbolToken {
        let word: String
        let range: Range<String.Index>
    }

    private static func symbolTokens(in text: String) -> [SymbolToken] {
        var tokens: [SymbolToken] = []
        var start: String.Index?
        for index in text.indices {
            if text[index].isWhitespace {
                if let begin = start {
                    tokens.append(SymbolToken(word: String(text[begin..<index]), range: begin..<index))
                    start = nil
                }
            } else if start == nil {
                start = index
            }
        }
        if let begin = start {
            tokens.append(SymbolToken(word: String(text[begin...]), range: begin..<text.endIndex))
        }
        return tokens
    }

    /// Apple's `SpeechTranscriber` glues a spoken domain to the word before it ("John at the
    /// rategmail.com.", "Priya at signyahoo.com."): this splits "rate" or "sign" off again.
    private static func splittingGluedDomains(_ tokens: [SymbolToken], in text: String) -> [SymbolToken] {
        var result: [SymbolToken] = []
        for (index, token) in tokens.enumerated() {
            let lower = token.word.lowercased()
            let before = tokens[safe: index - 1]?.word.lowercased()
            let prefix: String? = if before == "the", tokens[safe: index - 2]?.word.lowercased() == "at",
                                     lower.hasPrefix("rate") {
                "rate"
            } else if before == "at", lower.hasPrefix("sign") {
                "sign"
            } else {
                nil
            }
            guard let prefix, isDomain(String(token.word.dropFirst(prefix.count))) else {
                result.append(token)
                continue
            }
            let cut = text.index(token.range.lowerBound, offsetBy: prefix.count)
            result.append(SymbolToken(word: String(token.word.prefix(prefix.count)), range: token.range.lowerBound..<cut))
            result.append(SymbolToken(word: String(token.word.dropFirst(prefix.count)), range: cut..<token.range.upperBound))
        }
        return result
    }

    /// A written domain, maybe with punctuation after it: "gmail.com.", "co.in".
    private static func isDomain(_ word: String) -> Bool {
        let core = String(word.reversed().drop { closingMarks.contains($0) }.reversed())
        let labels = core.split(separator: ".", omittingEmptySubsequences: false).map(String.init)
        guard labels.count >= 2, let top = labels.last, top.count >= 2, top.allSatisfy(\.isLetter) else { return false }
        return labels.allSatisfy(isDomainLabel)
    }

    /// "at the rate" (ambiguous: also plain English) or "at sign" / "at-sign" at `index`; the
    /// words inside may not carry punctuation, the last may ("at sign.").
    private static func symbolMarker(_ tokens: [SymbolToken], at index: Int) -> (end: Int, ambiguous: Bool)? {
        func word(_ offset: Int) -> String? { tokens[safe: index + offset]?.word.lowercased() }
        if bare(tokens[index].word) == "at-sign" { return (index, false) }
        guard word(0) == "at" else { return nil }
        if let sign = word(1), bare(sign) == "sign" { return (index + 1, false) }
        if word(1) == "the", let rate = word(2), bare(rate) == "rate" { return (index + 2, true) }
        return nil
    }

    /// A domain spoken from `index`: labels joined by "dot" or already by ".", at least two,
    /// ending in a top-level domain of letters ("com", "co dot in"). Punctuation after the
    /// last label (a sentence's full stop) is kept apart.
    private static func addressDomain(
        _ tokens: [SymbolToken],
        from index: Int
    ) -> (labels: [String], end: Int, trailing: String)? {
        var labels: [String] = []
        var position = index
        var trailing = ""
        while let token = tokens[safe: position] {
            let core = String(token.word.reversed().drop { closingMarks.contains($0) }.reversed())
            let parts = core.split(separator: ".", omittingEmptySubsequences: false).map(String.init)
            guard !core.isEmpty, parts.allSatisfy(isDomainLabel) else { return nil }
            labels += parts
            trailing = String(token.word.dropFirst(core.count))
            guard trailing.isEmpty,
                  tokens[safe: position + 1]?.word.lowercased() == "dot",
                  let following = tokens[safe: position + 2],
                  isDomainLabel(String(following.word.reversed().drop { closingMarks.contains($0) }.reversed())
                      .split(separator: ".").first.map(String.init) ?? "") else { break }
            position += 2
        }
        guard labels.count >= 2, let top = labels.last, top.count >= 2, top.allSatisfy(\.isLetter) else { return nil }
        return (labels, position, trailing)
    }

    private static func isDomainLabel(_ label: String) -> Bool {
        !label.isEmpty && label.count <= 63 && label.first != "-" && label.last != "-"
            && label.allSatisfy { ($0.isLetter || $0.isNumber || $0 == "-") && $0.isASCII }
    }

    /// The word before "@" when it can start an address: letters, digits, "." "_" "-" "+",
    /// with no punctuation after it.
    private static func localPart(_ word: String) -> String? {
        guard !word.isEmpty, let last = word.last, last.isLetter || last.isNumber,
              word.first.map({ $0.isLetter || $0.isNumber }) == true,
              word.allSatisfy({ ($0.isLetter || $0.isNumber || "._-+".contains($0)) && $0.isASCII }) else { return nil }
        return word
    }

    // MARK: Spoken counts

    /// The English number words a count is made of.
    private static let countWords: [String: Int] = [
        "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
        "eleven": 11, "twelve": 12, "thirteen": 13, "fourteen": 14, "fifteen": 15, "sixteen": 16, "seventeen": 17,
        "eighteen": 18, "nineteen": 19, "twenty": 20,
    ]

    /// English `text` with a spoken count in digits, or `text` itself, so a microphone test
    /// reads the same from every speech model: for "test, one, two, three" Parakeet writes
    /// "Test 1, 2, 3" and Apple's `SpeechTranscriber` "Test one, 2, 3." (macOS 27.2, `say`
    /// voices), and the cleanup tagger is sure a count in words is a self-correction (P(DELETE)
    /// over 0.98 on "one" and "two", which made "Test three." before `CleanupGuard`). A count is
    /// three numbers or more counting up from one, in words or digits, with only a comma or a
    /// space between them: "one two three" and "one, 2, 3" become "1, 2, 3". "123" (up to
    /// "123456789") right after "test" or "testing" and ending its clause, which Parakeet and
    /// `DictationTranscriber` write for a quick "testing one two three", becomes "1, 2, 3" too.
    /// Every other number stays as said ("Chapter one, two and three", "at 1, 2, or 3", "room
    /// 123", "test 123 passed").
    static func spokenCounts(in text: String, language: String?) -> String {
        guard SpeechLanguagePickerOracle.isEnglish(language) else { return text }
        let tokens = symbolTokens(in: text)
        var edits: [(range: Range<String.Index>, text: String)] = []
        var index = 0
        while index < tokens.count {
            if let digits = joinedCount(tokens, at: index) {
                edits.append((tokens[index].range, digits))
                index += 1
                continue
            }
            var count: [(value: Int, trailing: String)] = []
            while let token = tokens[safe: index + count.count], let number = countNumber(token.word),
                  number.value == count.count + 1 {
                count.append(number)
                // Only a comma or a space comes between two numbers of a count.
                guard number.trailing.isEmpty || number.trailing == "," else { break }
            }
            // "zero, one, two, three" and "twenty one two three" are no count from one.
            let previous = tokens[safe: index - 1].map { bare($0.word) } ?? ""
            let followsNumber = countNumber(previous) != nil || previous == "zero"
                || (!previous.isEmpty && previous.allSatisfy(\.isNumber))
            guard count.count >= 3, !followsNumber else {
                index += 1
                continue
            }
            let last = index + count.count - 1
            let written = count.enumerated().map { offset, number in
                "\(number.value)" + (offset == count.count - 1 ? number.trailing : ",")
            }
            edits.append((tokens[index].range.lowerBound..<tokens[last].range.upperBound, written.joined(separator: " ")))
            index = last + 1
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

    /// A count's number and the marks after it ("two," is 2 and ","): one of `countWords`, or 1
    /// to 20 in digits.
    private static func countNumber(_ word: String) -> (value: Int, trailing: String)? {
        let core = String(word.reversed().drop { closingMarks.contains($0) }.reversed())
        let trailing = String(word.dropFirst(core.count))
        if let value = countWords[core.lowercased()] { return (value, trailing) }
        guard core.count <= 2, core.first != "0", core.allSatisfy({ $0.isASCII && $0.isNumber }),
              let value = Int(core) else { return nil }
        return (value, trailing)
    }

    /// "123." after "test" or "testing" as "1, 2, 3.", when nothing but its marks or the end
    /// follows it: "test 123 passed" keeps its number.
    private static func joinedCount(_ tokens: [SymbolToken], at index: Int) -> String? {
        guard let before = tokens[safe: index - 1], ["test", "testing"].contains(bare(before.word)),
              !endsSentence(before.word) else { return nil }
        let word = tokens[index].word
        let core = String(word.reversed().drop { closingMarks.contains($0) }.reversed())
        let trailing = word.dropFirst(core.count)
        guard core.count >= 3, "123456789".hasPrefix(core), !trailing.isEmpty || index == tokens.count - 1 else {
            return nil
        }
        return core.map(String.init).joined(separator: ", ") + trailing
    }

    // MARK: Languages

    private static let spanish = Rules(
        fillers: [["este"], ["o", "sea"]],
        markers: [["digo"], ["perdón"], ["perdon"], ["no"], ["no", "espera"], ["no", "mejor"], ["mejor", "dicho"]],
        plainMarkers: [["mejor", "dicho"], ["no", "mejor"]],
        kinds: [
            ["dos", "tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve", "diez", "once", "doce"],
            ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"],
            ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre",
             "setiembre", "octubre", "noviembre", "diciembre"],
            ["hoy", "mañana", "ayer", "anoche"],
        ],
        noComma: ["y", "o", "pero", "que", "a", "de", "en", "con", "para", "por", "el", "la", "los", "las",
                  "un", "una", "mi", "tu", "su", "porque", "si"]
    )

    private static let german = Rules(
        fillers: [],
        markers: [["nein"], ["nein", "warte"], ["ich", "meine"], ["besser", "gesagt"], ["sorry"],
                  ["entschuldigung"]],
        plainMarkers: [["besser", "gesagt"]],
        kinds: [
            ["zwei", "drei", "vier", "fünf", "sechs", "sieben", "acht", "neun", "zehn", "elf", "zwölf"],
            ["montag", "dienstag", "mittwoch", "donnerstag", "freitag", "samstag", "sonntag"],
            ["januar", "jänner", "februar", "märz", "april", "mai", "juni", "juli", "august", "september",
             "oktober", "november", "dezember"],
            ["heute", "morgen", "gestern", "übermorgen"],
        ],
        noComma: ["und", "oder", "aber", "dass", "der", "die", "das", "ein", "eine", "mit", "zu", "in",
                  "an", "auf", "für", "von", "ich", "du", "er", "sie", "wir"]
    )

    private static let norwegian = Rules(
        fillers: [],
        markers: [["nei"], ["nei", "vent"], ["jeg", "mener"], ["rettere", "sagt"], ["unnskyld"], ["sorry"]],
        plainMarkers: [["rettere", "sagt"]],
        kinds: [
            ["to", "tre", "fire", "fem", "seks", "sju", "syv", "åtte", "ni", "ti", "elleve", "tolv"],
            ["mandag", "tirsdag", "onsdag", "torsdag", "fredag", "lørdag", "søndag"],
            ["januar", "februar", "mars", "april", "mai", "juni", "juli", "august", "september",
             "oktober", "november", "desember"],
        ],
        noComma: ["og", "eller", "men", "at", "jeg", "du", "han", "hun", "vi", "en", "et", "den", "det",
                  "på", "i", "til", "med", "for", "som"]
    )
}

private extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
