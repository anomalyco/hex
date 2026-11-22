// The dictation parity oracle: a copy of ios/HEXShared/SpokenLists.swift at dev 93db6ad7 (unchanged
// since d6efeddb), its types renamed with an "Oracle" suffix, so the Rust port in
// crates/keyboard/core/src/dictation/ is checked against today's Swift (DictationParityTests).
// Only the UserDefaults settings struct is left out. Never edit the rules here.
import Foundation

/// Dictated lists and line breaks as lines, with string rules and no model, in English and
/// Spanish; other languages are left as they are. Runs inside a style, after its fillers and
/// before its punctuation (`TranscriptFormatterOracle.format(_:style:language:lists:)`), so each style
/// ends the lines its own way.
///
/// - "new line", "new paragraph" (desktop's Labs › Spoken formatting words; "nueva línea",
///   "nuevo párrafo", "salto de línea") become one or two line breaks, and the commas around
///   them go. Never at the very start, after "a", "the", "my", "una", … ("a new line of shoes"),
///   or before "of", "is", "de", … .
/// - Numbered lists: "first … second …" ("1st … 2nd …", "primero … segundo …",
///   "en primer lugar …"), "number one … number two …" ("step", "item", "point", "número",
///   "paso"), or "one, … two, …" ("uno, … dos, …", "1. … 2. …") become "1. …" lines. The
///   markers count up from one, there are at least two items, none is empty, and every
///   marker starts a clause: the start, after a mark (".", ",", ";", ":", …) or a line break,
///   or after "and"/"then"/"y"/"luego" that do. A bare number needs its comma ("one, …");
///   "finally" or "lastly" ("por último") ends a list of two or more.
/// - Bullets: "bullet", "bullet point", "next bullet", "next item", "dash" ("viñeta",
///   "guion", "siguiente elemento") said at least twice become "- …" lines, as the Notes style
///   already writes bullets (a hyphen reads in every app's plain text and is what Markdown and
///   Notes take for a list). "dash" and "guion" need a clause start; "bullet" may follow any
///   word but "a", "the", "to", … ("a bullet hit the wall").
/// - Text before the first marker is its own line, given a colon unless it ends a sentence;
///   a last item ends at its first sentence, and what follows is its own line. Items start
///   with a capital and lose a trailing comma, period, "and" or "or".
nonisolated enum SpokenListsOracle {
    static func format(_ text: String, language: String?) -> String {
        guard let table = Table.forLanguage(language), !text.isEmpty else { return text }
        return listing(breaking(text, table), table)
    }

    // MARK: Words

    private struct Token {
        let text: String
        let isWord: Bool
        /// Lowercased, accents and "ñ" folded: "Viñeta" reads "vineta".
        let folded: String
    }

    /// Alternating words (letters, digits, apostrophes) and the separators between them.
    private static func tokenize(_ text: String) -> [Token] {
        var tokens: [Token] = []
        var current = ""
        var currentIsWord: Bool?
        func flush() {
            guard let isWord = currentIsWord else { return }
            let folded = isWord ? current.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil) : current
            tokens.append(Token(text: current, isWord: isWord, folded: folded))
        }
        for character in text {
            let isWord = character.isLetter || character.isNumber || character == "'" || character == "’"
            if currentIsWord == isWord {
                current.append(character)
            } else {
                flush()
                current = String(character)
                currentIsWord = isWord
            }
        }
        flush()
        return tokens
    }

    private static func joined(_ tokens: ArraySlice<Token>) -> String {
        tokens.map(\.text).joined()
    }

    /// Marks that end a clause, so a marker after them starts one.
    private static let clauseBoundaries: Set<Character> = [".", ",", ";", ":", "!", "?", "…", "—", "–", "\n", "(", "¿", "¡"]

    private struct Words {
        let tokens: [Token]
        let table: Table

        func isWord(_ index: Int) -> Bool { tokens.indices.contains(index) && tokens[index].isWord }

        func spacesOnly(_ index: Int) -> Bool {
            tokens.indices.contains(index) && !tokens[index].isWord
                && tokens[index].text.allSatisfy { $0 == " " || $0 == "\t" }
        }

        /// Where `words` said one after another (spaces between) starting at `index` end.
        func phrase(_ words: [String], at index: Int) -> Int? {
            var position = index
            for (number, word) in words.enumerated() {
                if number > 0 {
                    guard spacesOnly(position) else { return nil }
                    position += 1
                }
                guard isWord(position), tokens[position].folded == word else { return nil }
                position += 1
            }
            return position
        }

        /// The word right after the separator at `end`, when only spaces come between.
        func nextWord(after end: Int) -> String? {
            spacesOnly(end) && isWord(end + 1) ? tokens[end + 1].folded : nil
        }

        /// The word right before `index`, when only spaces come between.
        func previousWord(before index: Int) -> String? {
            index >= 2 && spacesOnly(index - 1) && isWord(index - 2) ? tokens[index - 2].folded : nil
        }

        /// Whether the word at `index` starts a clause: the text's start, after a mark or a
        /// line break, or after connectors ("and then") that start one.
        func startsClause(_ index: Int) -> Bool {
            var position = index
            while position >= 2 {
                if tokens[position - 1].text.contains(where: clauseBoundaries.contains) { return true }
                guard table.connectors.contains(tokens[position - 2].folded) else { return false }
                position -= 2
            }
            return true
        }

        /// The separator after the word ending at `end`, "" at the end of the text.
        func separator(at end: Int) -> String {
            tokens.indices.contains(end) && !tokens[end].isWord ? tokens[end].text : ""
        }

        /// A number said as a word or written in digits ("two", "dos", "2"), 1 to 20.
        func number(at index: Int) -> Int? {
            guard isWord(index) else { return nil }
            let word = tokens[index].folded
            if let value = table.cardinals[word] { return value }
            guard word.count <= 2, word.allSatisfy(\.isASCII), let value = Int(word), (1...20).contains(value) else { return nil }
            return value
        }
    }

    // MARK: Line breaks

    private static func breaking(_ text: String, _ table: Table) -> String {
        let words = Words(tokens: tokenize(text), table: table)
        let tokens = words.tokens
        var out = ""
        var index = 0
        var sawWord = false
        while index < tokens.count {
            if tokens[index].isWord, sawWord, let (breaks, end) = lineBreak(at: index, words) {
                while let last = out.last, last == " " || last == "\t" || last == "," || last == ";" {
                    out.removeLast()
                }
                out += breaks
                index = end
                if index < tokens.count, !tokens[index].isWord {
                    out += tokens[index].text.drop { " \t,.;:".contains($0) }
                    index += 1
                }
                continue
            }
            sawWord = sawWord || tokens[index].isWord
            out += tokens[index].text
            index += 1
        }
        return out
    }

    private static func lineBreak(at index: Int, _ words: Words) -> (String, Int)? {
        for (phrase, breaks) in words.table.breaks {
            guard let end = words.phrase(phrase, at: index) else { continue }
            if let previous = words.previousWord(before: index), words.table.determiners.contains(previous) { return nil }
            if let next = words.nextWord(after: end), words.table.breakFollowers.contains(next) { return nil }
            return (breaks, end)
        }
        return nil
    }

    // MARK: Lists

    private enum Family: Equatable {
        case ordinal
        case cardinal
        case keyword(String)
        case bullet
    }

    private struct Marker {
        let family: Family
        /// The item's number; 0 for "finally".
        let value: Int
        let start: Int
        let end: Int
    }

    private static func markers(_ words: Words) -> [Marker] {
        var found: [Marker] = []
        var index = 0
        while index < words.tokens.count {
            if words.isWord(index), let marker = marker(at: index, words) {
                found.append(marker)
                index = marker.end
            } else {
                index += 1
            }
        }
        return found
    }

    private static func marker(at index: Int, _ words: Words) -> Marker? {
        let table = words.table
        let word = words.tokens[index].folded

        for (phrase, needsMark) in table.finals {
            guard let end = words.phrase(phrase, at: index), words.startsClause(index) else { continue }
            let after = words.separator(at: end)
            if needsMark, !after.hasPrefix(","), !after.hasPrefix(":") { continue }
            return Marker(family: .ordinal, value: 0, start: index, end: end)
        }
        for (phrase, value) in table.ordinalPhrases {
            guard let end = words.phrase(phrase, at: index), words.startsClause(index) else { continue }
            return Marker(family: .ordinal, value: value, start: index, end: end)
        }
        if table.keywords.contains(word), words.spacesOnly(index + 1), let value = words.number(at: index + 2),
           words.startsClause(index) {
            return Marker(family: .keyword(word), value: value, start: index, end: index + 3)
        }
        for (phrase, strict) in table.bullets {
            guard let end = words.phrase(phrase, at: index) else { continue }
            let previous = words.previousWord(before: index)
            let starts = words.startsClause(index)
                || (!strict && !(previous.map(table.determiners.contains) ?? false))
            if !starts { return nil }
            if let next = words.nextWord(after: end), table.bulletFollowers.contains(next) { return nil }
            return Marker(family: .bullet, value: 1, start: index, end: end)
        }
        if let value = table.ordinal(word), words.startsClause(index) {
            if words.spacesOnly(index + 1), words.phrase(table.ofAll, at: index + 2) != nil {
                return Marker(family: .ordinal, value: value, start: index, end: index + 1 + 2 * table.ofAll.count)
            }
            if let next = words.nextWord(after: index + 1), table.ordinalFollowers.contains(next) { return nil }
            return Marker(family: .ordinal, value: value, start: index, end: index + 1)
        }
        if let value = words.number(at: index), words.startsClause(index) {
            // A bare number counts only with its mark and a space after: "one, eggs", "1. eggs",
            // never "1:30" or "1.5".
            let separator = words.separator(at: index + 1)
            guard let mark = separator.first, ",:.)".contains(mark),
                  separator.dropFirst().first?.isWhitespace ?? (index + 2 >= words.tokens.count) else { return nil }
            return Marker(family: .cardinal, value: value, start: index, end: index + 1)
        }
        return nil
    }

    /// The lists among `markers` in order: numbered runs counting up from 1, and two bullets or
    /// more.
    private static func lists(_ markers: [Marker]) -> [[Marker]] {
        var runs: [[Marker]] = []
        for (position, first) in markers.enumerated() {
            if first.family == .bullet {
                let run = markers[position...].filter { $0.family == .bullet }
                if run.count >= 2 { runs.append(Array(run)) }
                continue
            }
            guard first.value == 1 else { continue }
            var run = [first]
            for marker in markers[(position + 1)...] where marker.family == first.family {
                if marker.value == run.count + 1 {
                    run.append(marker)
                } else if marker.value == 0, run.count >= 2 {
                    run.append(marker)
                    break
                }
            }
            if run.count >= 2 { runs.append(run) }
        }
        return runs
    }

    private static func listing(_ text: String, _ table: Table) -> String {
        let words = Words(tokens: tokenize(text), table: table)
        for run in lists(markers(words)) {
            // A run with an empty item ("one, two, three, go") is no list; a later one may be.
            if let formatted = format(run, words) { return formatted }
        }
        return text
    }

    private static func format(_ run: [Marker], _ words: Words) -> String? {
        let tokens = words.tokens
        let table = words.table
        guard let last = run.last else { return nil }

        // The last item ends at its first sentence (or line); what follows is its own line.
        var itemsEnd = tokens.count
        var tailBreak = "\n"
        var sawWord = false
        for index in last.end..<tokens.count {
            if tokens[index].isWord {
                sawWord = true
                continue
            }
            guard sawWord, index + 1 < tokens.count else { continue }
            let separator = tokens[index].text
            if separator.contains("\n") || endsSentence(separator, after: tokens[index - 1].folded) {
                itemsEnd = index
                tailBreak = separator.contains("\n\n") ? "\n\n" : "\n"
                break
            }
        }

        var items: [String] = []
        for (number, marker) in run.enumerated() {
            let end = number + 1 < run.count ? run[number + 1].start : itemsEnd
            var raw = joined(tokens[marker.end..<end])
            // The last item keeps its sentence's mark ("?", "!") but not what follows.
            if number + 1 == run.count, itemsEnd < tokens.count {
                let separator = tokens[itemsEnd].text
                raw += separator.prefix { !$0.isWhitespace }
            }
            let item = cleanItem(raw, table)
            guard !item.isEmpty else { return nil }
            items.append(item)
        }

        var output = ""
        let leadIn = joined(tokens[..<run[0].start])
        let leadTrimmed = String(leadIn.reversed().drop { $0 == " " || $0 == "\t" }.reversed())
        if leadTrimmed.hasSuffix("\n") {
            output = leadTrimmed
        } else {
            var lead = stripTrailingConnectors(leadIn.trimmingCharacters(in: .whitespacesAndNewlines), table)
            while let mark = lead.last, ",;-–—".contains(mark) {
                lead.removeLast()
                lead = lead.trimmingCharacters(in: .whitespaces)
            }
            if let mark = lead.last {
                if mark.isLetter || mark.isNumber { lead += ":" }
                output = lead + "\n"
            }
        }

        let isBullet = run[0].family == .bullet
        output += items.enumerated()
            .map { isBullet ? "- " + $0.element : "\($0.offset + 1). " + $0.element }
            .joined(separator: "\n")

        if itemsEnd < tokens.count {
            let tail = joined(tokens[(itemsEnd + 1)...]).trimmingCharacters(in: .whitespacesAndNewlines)
            if !tail.isEmpty {
                output += tailBreak + listing(tail, table)
            }
        }
        return output
    }

    private static let abbreviations: Set<String> = [
        "mr", "mrs", "ms", "dr", "st", "jr", "sr", "prof", "vs", "etc", "inc", "ltd", "approx", "sra", "dra", "ud", "uds",
    ]

    /// Whether `separator` ends a sentence after `word`: ".", "!", "?" or "…" then a space,
    /// never after "Dr" or an initial.
    private static func endsSentence(_ separator: String, after word: String) -> Bool {
        guard let markIndex = separator.firstIndex(where: { ".!?…".contains($0) }),
              separator[separator.index(after: markIndex)...].first?.isWhitespace == true else { return false }
        if separator[markIndex] == "." && (abbreviations.contains(word) || (word.count == 1 && word.first?.isLetter == true)) {
            return false
        }
        return true
    }

    /// An item's text: no leading marks, no trailing comma, period, "and" or "or", and a capital
    /// first letter (never on "iPhone" or an address).
    private static func cleanItem(_ raw: String, _ table: Table) -> String {
        var item = String(raw.drop { " \t\n,:.;)-–—".contains($0) })
        var previous = ""
        while previous != item {
            previous = item
            item = item.trimmingCharacters(in: .whitespacesAndNewlines)
            if !item.hasSuffix("..."), let mark = item.last, ",;:.".contains(mark) {
                item.removeLast()
            }
            item = stripTrailingConnectors(item, table)
        }
        return capitalizingFirstWord(item)
    }

    /// `text` without the connectors ("and", "y luego") a next item's marker leaves at its end.
    private static func stripTrailingConnectors(_ text: String, _ table: Table) -> String {
        var text = text
        while let space = text.lastIndex(where: { $0 == " " || $0 == "," }) {
            let last = text[text.index(after: space)...].folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
            guard table.connectors.contains(last) || table.alternatives.contains(last) else { break }
            text = String(text[..<space]).trimmingCharacters(in: .whitespaces)
            while text.last == "," { text.removeLast() }
        }
        return text
    }

    private static func capitalizingFirstWord(_ text: String) -> String {
        guard let first = text.firstIndex(where: \.isLetter) else { return text }
        let word = text[first...].prefix { !$0.isWhitespace }
        if word.contains("@") || word.dropFirst().contains(where: \.isUppercase) { return text }
        var result = text
        result.replaceSubrange(first...first, with: String(text[first]).uppercased())
        return result
    }

    // MARK: Words per language

    private struct Table {
        /// Phrases that break a line.
        let breaks: [([String], String)]
        /// Words before a phrase that make it a noun, not a command: "a new line", "the bullet".
        let determiners: Set<String>
        /// Words after "new line" that make it a noun: "new line of shoes".
        let breakFollowers: Set<String>
        let connectors: Set<String>
        /// "or", stripped from an item's end like a connector.
        let alternatives: Set<String>
        let ordinals: [String: Int]
        /// Ordinal suffixes written after digits: "1st", "2nd".
        let digitOrdinalSuffixes: [String]
        let ordinalPhrases: [([String], Int)]
        /// Words right after an ordinal that keep it prose: "first time", "second guess".
        let ordinalFollowers: Set<String>
        /// "first of all".
        let ofAll: [String]
        /// "finally", "lastly"; the flag asks for a comma after ("last, …").
        let finals: [([String], Bool)]
        let cardinals: [String: Int]
        /// "number one", "step one".
        let keywords: Set<String>
        /// Bullet phrases, longest first; the flag asks for a clause start.
        let bullets: [([String], Bool)]
        /// Words right after "bullet" that keep it prose: "bullet points", "bullet train".
        let bulletFollowers: Set<String>

        func ordinal(_ word: String) -> Int? {
            if let value = ordinals[word] { return value }
            for suffix in digitOrdinalSuffixes where word.hasSuffix(suffix) {
                let digits = word.dropLast(suffix.count)
                if !digits.isEmpty, digits.count <= 2, digits.allSatisfy(\.isASCII), let value = Int(digits),
                   (1...20).contains(value) {
                    return value
                }
            }
            return nil
        }

        static func forLanguage(_ language: String?) -> Table? {
            let code = language.flatMap { Locale(identifier: $0).language.languageCode?.identifier } ?? "en"
            switch code {
            case "en": return english
            case "es": return spanish
            default: return nil
            }
        }

        static let english = Table(
            breaks: [(["new", "paragraph"], "\n\n"), (["new", "line"], "\n"), (["line", "break"], "\n")],
            determiners: [
                "a", "an", "the", "this", "that", "these", "those", "my", "your", "his", "her", "its", "our", "their",
                "one", "another", "every", "each", "any", "some", "no", "to", "of", "in", "on", "with", "for", "at",
                "by", "from", "silver", "magic", "rubber", "plastic", "stray", "whole", "entire",
            ],
            breakFollowers: [
                "of", "items", "item", "is", "was", "are", "were", "has", "had", "will", "would", "that", "which",
                "for", "from", "cook", "cooks", "worker", "workers", "manager", "managers", "up", "breaks",
            ],
            connectors: ["and", "then", "also", "plus"],
            alternatives: ["or"],
            ordinals: [
                "first": 1, "firstly": 1, "second": 2, "secondly": 2, "third": 3, "thirdly": 3, "fourth": 4,
                "fourthly": 4, "fifth": 5, "fifthly": 5, "sixth": 6, "seventh": 7, "eighth": 8, "ninth": 9,
                "tenth": 10,
            ],
            digitOrdinalSuffixes: ["st", "nd", "rd", "th"],
            ordinalPhrases: [],
            ordinalFollowers: [
                "time", "times", "place", "thing", "things", "one", "ones", "day", "days", "week", "weeks", "month",
                "months", "year", "years", "half", "class", "person", "people", "hand", "floor", "grade", "round",
                "prize", "guess", "guessed", "guessing", "nature", "rate", "aid", "base", "cousin", "cousins", "date",
                "name", "impression", "choice", "language", "lady", "world", "edition", "gear", "love", "sight",
                "glance", "mate", "chance", "try", "attempt", "look", "night", "kiss", "draft", "opinion", "wave",
                "quarter", "season", "episode", "chapter", "page", "row", "team", "degree", "letter", "word", "words",
                "course", "act", "part", "period", "hour", "minute", "thought", "thoughts", "off", "up", "to", "in",
                "on", "at", "and", "or", "is", "was", "came", "come", "served", "last", "of", "string", "fiddle",
                "semester", "term", "born", "child", "son", "daughter", "baby", "wife", "husband", "job", "line",
                "generation", "step", "steps", "officer", "lieutenant", "amendment", "street", "avenue", "best",
                "most", "largest", "biggest",
            ],
            ofAll: ["of", "all"],
            finals: [(["last", "but", "not", "least"], false), (["finally"], false), (["lastly"], false), (["last"], true)],
            cardinals: [
                "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6, "seven": 7, "eight": 8, "nine": 9,
                "ten": 10,
            ],
            keywords: ["number", "step", "item", "point"],
            bullets: [
                (["bullet", "point"], false), (["next", "bullet"], false), (["next", "item"], false),
                (["bullet"], false), (["dash"], true),
            ],
            bulletFollowers: [
                "points", "proof", "proofs", "train", "trains", "wound", "wounds", "hole", "holes", "journal",
                "journals", "casing", "casings", "shell", "shells", "ant", "ants", "is", "was", "hit", "hits",
                "went", "flew", "of", "in", "list", "lists",
            ]
        )

        static let spanish = Table(
            breaks: [
                (["nuevo", "parrafo"], "\n\n"), (["nueva", "linea"], "\n"), (["salto", "de", "linea"], "\n"),
            ],
            determiners: [
                "un", "una", "el", "la", "los", "las", "este", "esta", "estos", "estas", "ese", "esa", "eso", "mi",
                "mis", "tu", "tus", "su", "sus", "nuestra", "nuestro", "otra", "otro", "cada", "de", "del", "con",
                "en", "para", "por", "al", "a",
            ],
            breakFollowers: ["de", "del", "es", "fue", "que", "para", "y"],
            connectors: ["y", "e", "luego", "despues", "tambien"],
            alternatives: ["o", "u"],
            ordinals: [
                "primero": 1, "primera": 1, "primeramente": 1, "segundo": 2, "segunda": 2, "tercero": 3,
                "tercera": 3, "cuarto": 4, "cuarta": 4, "quinto": 5, "quinta": 5, "sexto": 6, "sexta": 6,
                "septimo": 7, "septima": 7, "octavo": 8, "octava": 8, "noveno": 9, "novena": 9, "decimo": 10,
                "decima": 10,
            ],
            digitOrdinalSuffixes: ["º", "ª"],
            ordinalPhrases: [
                (["en", "primer", "lugar"], 1), (["en", "segundo", "lugar"], 2), (["en", "tercer", "lugar"], 3),
                (["en", "cuarto", "lugar"], 4), (["en", "quinto", "lugar"], 5),
            ],
            ordinalFollowers: [
                "vez", "veces", "lugar", "dia", "dias", "plato", "piso", "mano", "grado", "premio", "parte", "nombre",
                "ano", "anos", "mes", "semana", "momento", "paso", "pasos", "hijo", "hija", "ministro", "ministra",
                "actor", "plano", "mundo", "tiempo", "ciclo", "curso", "nivel", "puesto", "de", "del", "en", "que",
                "es", "fue", "y", "o", "a", "con", "amor", "intento", "capitulo", "temporada", "clase", "fila",
            ],
            ofAll: ["de", "todo"],
            finals: [
                (["en", "ultimo", "lugar"], false), (["por", "ultimo"], false), (["finalmente"], false),
            ],
            cardinals: [
                "uno": 1, "una": 1, "dos": 2, "tres": 3, "cuatro": 4, "cinco": 5, "seis": 6, "siete": 7, "ocho": 8,
                "nueve": 9, "diez": 10,
            ],
            keywords: ["numero", "paso"],
            bullets: [
                (["siguiente", "vineta"], false), (["siguiente", "elemento"], false), (["vineta"], false),
                (["guion"], true),
            ],
            bulletFollowers: ["de", "del", "es", "fue", "que"]
        )
    }
}
