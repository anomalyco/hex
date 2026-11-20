import Foundation

/// Clean up what I say for Spanish, German, and Norwegian, which the English cleanup tagger
/// was never trained on. Run on Spanish, the tagger dropped negations ("Te digo que no puedo
/// ir" became "Te puedo ir"), so these languages get two word rules instead, each needing
/// the commas a pause leaves:
/// - the language's hesitation sounds go anywhere ("eh", "mmm"; `TranscriptFormatter`'s
///   list, so in-keyboard dictation, which no style formats, loses them too);
/// - a filler phrase set off by commas goes ("Es caro, o sea, no lo compro");
/// - a spoken correction ("a las 3, digo, a las 4") drops its marker and the words it
///   replaces, counted back from the nearest earlier word that matches the repair's first
///   word ("a"), or else is of its kind (a number, weekday, month, or today and tomorrow),
///   within the sentence and `maxReach` words. With no such word, the sentence stays as
///   said.
/// Like the tagger, it only removes words; `CleanupReconstruction` repairs the commas and
/// capitals next to a removal.
nonisolated enum SpokenCleanupRules {
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
        let words = CleanupTagger.words(in: text)
        var cleaned = removingFillers(words, rules: rules, language: language)
        cleaned = resolvingCorrections(cleaned, rules: rules)
        return cleaned == words ? text : cleaned.joined(separator: " ")
    }

    // MARK: Fillers

    private static func removingFillers(_ input: [String], rules: Rules, language: String?) -> [String] {
        var words = input
        var deletes = Array(repeating: false, count: words.count)
        let hesitation = TranscriptFormatter.fillerPattern(for: language).flatMap {
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
        return CleanupTagger.words(in: CleanupReconstruction.reconstruct(words, deletes: deletes))
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
            words = CleanupTagger.words(in: CleanupReconstruction.reconstruct(words, deletes: deletes))
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
