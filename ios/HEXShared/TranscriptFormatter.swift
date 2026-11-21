import Foundation

nonisolated enum DictationStyle: String, Codable, CaseIterable, Sendable {
    case polished
    case casual
    case notes
    case verbatim

    var label: String {
        switch self {
        case .polished: String(localized: "Polished")
        case .casual: String(localized: "Casual")
        case .notes: String(localized: "Notes")
        case .verbatim: String(localized: "Verbatim")
        }
    }

    var shortHint: String {
        switch self {
        case .polished: String(localized: "Full sentences, fillers trimmed")
        case .casual: String(localized: "Texting: lowercase start, no final period")
        case .notes: String(localized: "Short lines, a list for several items")
        case .verbatim: String(localized: "Exact words from the model")
        }
    }

    var next: DictationStyle {
        let all = Self.allCases
        guard let index = all.firstIndex(of: self) else { return .polished }
        return all[(index + 1) % all.count]
    }
}

nonisolated enum TranscriptFormatter {
    /// `text` in `style`. `language` (BCP 47) picks the fillers a style trims and the
    /// English-only rules ("I", Casual's interjections, Notes' connectives); nil is English.
    /// - Verbatim: as the speech model wrote it.
    /// - Polished: full sentences: fillers trimmed, each sentence capitalized ("I" too) and
    ///   ended, and English "gonna", "wanna", "gotta", "kinda" and "lemme" written out.
    /// - Casual: texting: fillers trimmed, sentences may start lowercase (never an acronym
    ///   or a name with inner capitals), English "i", no comma after an opening "yeah" or
    ///   "ok", and no final period on a short message (one line, up to three sentences).
    /// - Notes: concise lines: spoken "bullet" and "new line" markers, else several
    ///   sentences become a list (and a list after a colon becomes bullets), with no
    ///   opening "so", "yeah" or "and then" and no final periods.
    ///
    /// `lists` (Settings › Dictation › Format lists) writes spoken lists and line breaks as
    /// lines (`SpokenLists`) after the fillers and before the style's punctuation, in every
    /// style but Verbatim, which keeps the model's exact words.
    static func format(_ raw: String, style: DictationStyle, language: String? = nil, lists: Bool = false) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "" }
        let fillers = fillerPattern(for: language)
        let english = SpeechLanguagePicker.isEnglish(language)

        switch style {
        case .verbatim:
            return collapseHorizontalSpaces(in: trimmed)

        case .polished:
            let cleaned = listed(stripStandaloneFillers(in: trimmed, fillers: fillers), lists, language)
            return polishSentences(cleaned, english: english)

        case .casual:
            let cleaned = listed(stripStandaloneFillers(in: trimmed, fillers: fillers), lists, language)
            return casualMessage(cleaned, english: english)

        case .notes:
            let cleaned = listed(stripStandaloneFillers(in: trimmed, fillers: fillers), lists, language)
            return formatNotes(cleaned, english: english)
        }
    }

    private static func listed(_ text: String, _ lists: Bool, _ language: String?) -> String {
        lists ? SpokenLists.format(text, language: language) : text
    }

    /// Joins dictated text cleanly with existing text around the insertion cursor: one space
    /// between it and a word before it, and, mid-sentence, a lowercase first word
    /// (`continuingSentence`). `isWord` says whether a lowercase word is in the dictation
    /// language's dictionary; nil leaves the case alone.
    static func insertionText(
        for formatted: String,
        beforeContext: String?,
        afterContext: String?,
        isWord: ((String) -> Bool)? = nil
    ) -> String {
        guard !formatted.isEmpty else { return "" }
        var output = formatted

        if let isWord, !startsSentence(after: beforeContext) {
            output = continuingSentence(output, isWord: isWord)
        }

        if beforeContext?.last?.isWhitespace ?? true {
            // Never a second space after the one already there.
            output = String(output.drop { $0 == " " || $0 == "\t" })
        } else if let lastChar = beforeContext?.last {
            let needsLeadingSpace = !"([{\"“‘«¿¡".contains(lastChar)
                && !output.hasPrefix(" ")
                && !output.hasPrefix("\n")
                && !isClosingPunctuation(output.first)
            if needsLeadingSpace {
                output = " " + output
            }
        }

        if let after = afterContext, let firstChar = after.first {
            let needsTrailingSpace = !firstChar.isWhitespace
                && !isClosingPunctuation(firstChar)
                && !output.hasSuffix("\n")
                && !output.hasSuffix(" ")
            if needsTrailingSpace {
                output += " "
            }
        }

        return output
    }

    /// Whether text typed before the cursor leaves the next words at a sentence start: none
    /// (or only spaces), a line break, or `.` `!` `?` `…` (closing quotes or brackets
    /// after them count too).
    static func startsSentence(after before: String?) -> Bool {
        guard let before else { return true }
        for character in before.reversed() {
            if character.isNewline { return true }
            if character.isWhitespace || "\"”’')]»".contains(character) { continue }
            return ".!?…。".contains(character)
        }
        return true
    }

    /// German and Luxembourgish capitalize every noun, and their dictionaries accept nouns in
    /// lowercase, so a dictation there keeps its case rather than lose a noun's capital.
    static func lowercasesContinuations(in language: String) -> Bool {
        let code = Locale(identifier: language).language.languageCode?.identifier ?? language
        return code != "de" && code != "lb"
    }

    /// Lowercases the first letter of words that continue a sentence, as typing would. Keeps
    /// "I" and its contractions, acronyms ("NASA"), words with capitals inside ("McDonald's"),
    /// and words whose lowercase form `isWord` doesn't know, which are likely names.
    static func continuingSentence(_ text: String, isWord: (String) -> Bool) -> String {
        guard let first = text.first, first.isUppercase else { return text }
        let word = String(text.prefix { $0.isLetter || $0 == "'" || $0 == "’" })
        if word == "I" || word.hasPrefix("I'") || word.hasPrefix("I’") { return text }
        let letters = word.filter(\.isLetter)
        if letters.count >= 2, letters == letters.uppercased() { return text }
        if letters.dropFirst().contains(where: \.isUppercase) { return text }
        guard isWord(word.lowercased()) else { return text }
        return first.lowercased() + text.dropFirst()
    }

    private static func isClosingPunctuation(_ character: Character?) -> Bool {
        guard let character else { return false }
        return ".,!?;:)]}”".contains(character)
    }

    /// The hesitation sounds a style trims, per language. English "er" is a word in
    /// Norwegian, Danish, Swedish ("er" = is) and German, and "um" is one in Portuguese, so
    /// each language trims only its own; a language without a list trims nothing. Spanish
    /// trims "eh", "em" and "mmm" (never "e", which is "and"), German "äh", "ähm" and "öhm"
    /// (never "eh", which is "anyway").
    static func fillerPattern(for language: String?) -> String? {
        let code = language.flatMap { Locale(identifier: $0).language.languageCode?.identifier } ?? "en"
        switch code {
        case "en":
            // Desktop's list (`src/dictation/filler_words.rs`): um, umm, uh, uhh, erm, er, ah.
            return "um+|uh+|er+m*|ah+"
        case "nb", "nn", "no", "da", "sv":
            return "e+h+m*|ø+h+m*|ö+h+m*|hm+"
        case "es":
            return "e+h+m*|e+m+|m{2,}|hm+"
        case "de":
            return "ä+h+m*|ö+h+m*|hm+"
        default:
            return nil
        }
    }

    private static func stripStandaloneFillers(in text: String, fillers: String?) -> String {
        guard let fillers else { return collapseHorizontalSpaces(in: text) }
        let pattern = #"(?i)(?:^|(?<=[\s,]))(?:"# + fillers + #")(?=(?:[\s,.!?]|$)),?\s*"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return text }
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        let stripped = regex.stringByReplacingMatches(in: text, range: range, withTemplate: " ")
        return collapseHorizontalSpaces(in: stripped)
    }

    private static func polishSentences(_ text: String, english: Bool) -> String {
        guard !text.isEmpty else { return "" }
        var normalized = fixSpacingBeforePunctuation(in: collapseHorizontalSpaces(in: text))
        if english {
            normalized = expandingInformalContractions(in: normalized)
        }
        normalized = capitalizeSentenceStarts(in: normalized)
        if english {
            normalized = capitalizingPronounI(in: normalized)
        }
        // Every paragraph ends as a sentence does, a Chinese or Japanese one with "。".
        return normalized
            .components(separatedBy: "\n")
            .map(endingSentence)
            .joined(separator: "\n")
    }

    /// `line` with a full stop unless it already ends a sentence (or a list's colon), is
    /// empty, or is a lone address ("john@gmail.com"), which is no sentence to end.
    private static func endingSentence(_ line: String) -> String {
        guard let last = line.last, !sentenceClosers.contains(last) else { return line }
        if line.contains("@"), !line.contains(where: \.isWhitespace) { return line }
        return line + (usesIdeographicFullStop(last) ? "。" : ".")
    }

    /// What may end a sentence already: its mark, or a closing quote or bracket after one.
    private static let sentenceClosers: Set<Character> = [
        ".", "!", "?", "…", "\"", "”", ")", "]", ":", "。", "！", "？", "」", "』", "）",
    ]

    /// Marks that end a sentence. A full stop counts only before a space, a line break, or the
    /// end ("gmail.com" and "e.g." end nothing), the ideographic ones anywhere.
    private static func endsSentence(_ chars: [Character], at index: Int) -> Bool {
        let ch = chars[index]
        if "。！？".contains(ch) { return true }
        guard ".!?…".contains(ch) else { return false }
        return index + 1 == chars.count || chars[index + 1].isWhitespace
    }

    /// Chinese and Japanese end a sentence with "。", not ".".
    private static func usesIdeographicFullStop(_ character: Character) -> Bool {
        guard let scalar = character.unicodeScalars.first?.value else { return false }
        return (0x4E00...0x9FFF).contains(scalar) // CJK unified ideographs
            || (0x3400...0x4DBF).contains(scalar) // extension A
            || (0x3040...0x30FF).contains(scalar) // hiragana and katakana
            || (0xF900...0xFAFF).contains(scalar) // compatibility ideographs
    }

    /// What iOS 27's speech models write for casual speech, and what Polished writes instead.
    private static let informalContractions: [String: String] = [
        "gonna": "going to", "wanna": "want to", "gotta": "got to", "kinda": "kind of", "lemme": "let me",
    ]

    /// "I'm gonna call" -> "I'm going to call", "Gonna rain." -> "Going to rain.". Never inside
    /// quotes (he said “gonna win”) or on a capitalized word mid-sentence, which is a name or
    /// a title ("Gonna Fly Now"); a capitalized one starting a sentence keeps its capital.
    private static func expandingInformalContractions(in text: String) -> String {
        let lowered = text.lowercased()
        guard informalContractions.keys.contains(where: lowered.contains) else { return text }
        var output = ""
        var word = ""
        var inStraightQuote = false
        var curlyQuotes = 0
        func flush() {
            defer { word = "" }
            let lower = word.lowercased()
            guard let expansion = informalContractions[lower], !inStraightQuote, curlyQuotes == 0 else {
                output += word
                return
            }
            if word == lower {
                output += expansion
            } else if word == lower.prefix(1).uppercased() + lower.dropFirst(), startsSentenceOrItem(after: output) {
                output += expansion.prefix(1).uppercased() + expansion.dropFirst()
            } else {
                output += word
            }
        }
        for character in text {
            if character.isLetter || character.isNumber || character == "'" || character == "’" {
                word.append(character)
                continue
            }
            flush()
            if character == "\"" {
                inStraightQuote.toggle()
            } else if character == "“" {
                curlyQuotes += 1
            } else if character == "”" {
                curlyQuotes = max(0, curlyQuotes - 1)
            }
            output.append(character)
        }
        flush()
        return output
    }

    /// Whether the next word starts a sentence, or a bulleted item ("- ").
    private static func startsSentenceOrItem(after text: String) -> Bool {
        if startsSentence(after: text) { return true }
        let line = text.split(separator: "\n", omittingEmptySubsequences: false).last ?? ""
        let marker = line.trimmingCharacters(in: .whitespaces)
        return marker == "-" || marker == "•"
    }

    /// English "i", "i'm", "i'll", "i've", "i'd" as "I", "I'm", ...; never "i.e." or an address.
    private static func capitalizingPronounI(in text: String) -> String {
        recasingPronounI(in: text, from: "i", to: "I")
    }

    /// The English pronoun "I" (alone or in "I'm", "I'll", "I've", "I'd") written as `to`.
    private static func recasingPronounI(in text: String, from: String, to: String) -> String {
        guard text.contains(from) else { return text }
        let pattern = #"(?<![\p{L}\p{N}@.'’])"# + from + #"(?=(?:['’](?:m|ll|ve|d))?(?![\p{L}\p{N}@'’]|\.\p{L}))"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return text }
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        return regex.stringByReplacingMatches(in: text, range: range, withTemplate: to)
    }

    // MARK: Casual

    /// Interjections that open a text without their comma in Casual ("yeah sounds good").
    private static let casualInterjections: Set<String> = [
        "yeah", "yes", "yep", "yup", "no", "nope", "nah", "ok", "okay", "oh", "hey", "hi", "hello",
        "so", "well", "sure", "alright", "cool", "thanks",
    ]

    private static func casualMessage(_ text: String, english: Bool) -> String {
        var message = fixSpacingBeforePunctuation(in: collapseHorizontalSpaces(in: text))
        if english {
            message = droppingInterjectionCommas(in: message)
        }
        message = lowercasingSentenceStarts(in: message)
        if english {
            message = recasingPronounI(in: message, from: "I", to: "i")
        }
        // A short message drops its final full stop; a question or an exclamation keeps its mark.
        let chars = Array(message)
        let sentences = chars.indices.filter { endsSentence(chars, at: $0) }.count
        if !message.contains("\n"), sentences <= 3, let last = message.last, last == "." || last == "。" {
            message.removeLast()
        }
        return message.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// "Yeah, sounds good. Ok, see you." -> "Yeah sounds good. Ok see you.": an interjection at
    /// a sentence start loses its comma.
    private static func droppingInterjectionCommas(in text: String) -> String {
        var words = text.components(separatedBy: " ")
        var atStart = true
        for index in words.indices {
            let word = words[index]
            if atStart, word.hasSuffix(","), index + 1 < words.count,
               casualInterjections.contains(String(word.dropLast()).lowercased()) {
                words[index].removeLast()
            }
            let chars = Array(word)
            atStart = word.contains("\n") || (!chars.isEmpty && endsSentence(chars + [" "], at: chars.count - 1))
        }
        return words.joined(separator: " ")
    }

    /// Lowercases the first letter of each sentence, as people text. Keeps acronyms ("NASA",
    /// though "OK" becomes "ok"), words with capitals inside ("iPhone", "McDonald's"), and
    /// addresses.
    private static func lowercasingSentenceStarts(in text: String) -> String {
        var chars = Array(text)
        var atStart = true
        var index = 0
        while index < chars.count {
            // A list item ("1. Eggs", "- Eggs") keeps its capital.
            if index == 0 || chars[index - 1].isNewline, let marker = listMarkerLength(chars, at: index) {
                index += marker
                atStart = false
                continue
            }
            let ch = chars[index]
            if atStart, ch.isLetter {
                var end = index
                while end < chars.count, chars[end].isLetter || chars[end] == "'" || chars[end] == "’" {
                    end += 1
                }
                let word = String(chars[index..<end])
                if !keepsCapital(word), !isAddress(chars, at: index) {
                    let lowered = Array(word == "OK" ? "ok" : word.prefix(1).lowercased() + word.dropFirst())
                    chars.replaceSubrange(index..<end, with: lowered)
                    end = index + lowered.count
                }
                atStart = false
                index = end
                continue
            }
            if ch.isNewline || endsSentence(chars, at: index) {
                atStart = true
            } else if !ch.isWhitespace, !"\"“'‘(¿¡”’)]".contains(ch) {
                atStart = false
            }
            index += 1
        }
        return String(chars)
    }

    /// The length of a list marker ("1. ", "- ") at `index`, if one starts there.
    private static func listMarkerLength(_ chars: [Character], at index: Int) -> Int? {
        if index + 1 < chars.count, chars[index] == "-" || chars[index] == "•", chars[index + 1] == " " { return 2 }
        var end = index
        while end < chars.count, end - index < 2, chars[end].isASCII, chars[end].isNumber { end += 1 }
        guard end > index, end + 1 < chars.count, chars[end] == ".", chars[end + 1] == " " else { return nil }
        return end + 2 - index
    }

    private static func keepsCapital(_ word: String) -> Bool {
        if word == "OK" { return false }
        let letters = word.filter(\.isLetter)
        if letters.count >= 2, letters == letters.uppercased() { return true }
        return letters.dropFirst().contains(where: \.isUppercase)
    }

    /// Whether the word at `index` is an address ("john@gmail.com"), whose case stays.
    private static func isAddress(_ chars: [Character], at index: Int) -> Bool {
        var end = index
        while end < chars.count, !chars[end].isWhitespace {
            if chars[end] == "@" { return true }
            end += 1
        }
        return false
    }

    // MARK: Notes

    private static func formatNotes(_ text: String, english: Bool) -> String {
        guard !text.isEmpty else { return "" }
        var working = text
        let replacements: [(String, String)] = [
            (#"(?i)\bnew paragraph\b[,.]?\s*"#, "\n\n"),
            (#"(?i)\bnew line\b[,.]?\s*"#, "\n"),
            (#"(?i)(?:^|\s+)(?:bullet point|next bullet|bullet)\b[:,-]?\s*"#, "\n- "),
        ]
        for (pattern, template) in replacements {
            if let regex = try? NSRegularExpression(pattern: pattern) {
                let range = NSRange(working.startIndex..<working.endIndex, in: working)
                working = regex.stringByReplacingMatches(in: working, range: range, withTemplate: template)
            }
        }
        var lines = working.components(separatedBy: "\n")
        // Without spoken markers, one dictation of several items becomes a list.
        if lines.count == 1 {
            let line = fixSpacingBeforePunctuation(in: collapseHorizontalSpaces(in: lines[0]))
            let sentences = noteSentences(in: line)
            if sentences.count >= 2 {
                lines = sentences.map { "- " + $0 }
            } else if let list = colonList(in: line, english: english) {
                lines = [list.intro + ":"] + list.items.map { "- " + $0 }
            }
        }
        return lines
            .map { line -> String in
                let trimmedLine = collapseHorizontalSpaces(in: line)
                if trimmedLine.hasPrefix("- ") {
                    let body = String(trimmedLine.dropFirst(2)).trimmingCharacters(in: .whitespaces)
                    let note = conciseNote(body, english: english)
                    return note.isEmpty ? "" : "- " + note
                }
                return conciseNote(trimmedLine, english: english)
            }
            .joined(separator: "\n")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// One note: no opening "so," "okay," "and" or "then" (English), no final full stop, and
    /// a capital first letter.
    private static func conciseNote(_ line: String, english: Bool) -> String {
        var note = fixSpacingBeforePunctuation(in: line)
        if english {
            note = droppingConnectives(from: note)
        }
        if let last = note.last, last == "." || last == "。", !note.hasSuffix("...") {
            note.removeLast()
        }
        return capitalizeFirstLetter(note.trimmingCharacters(in: .whitespaces))
    }

    /// Words that open a spoken note but add nothing to a written one.
    private static let noteConnectives: [String] = [
        "and then", "and also", "and", "also", "then", "plus", "so", "okay", "ok", "yeah", "oh", "well", "anyway",
        "alright", "basically", "first", "firstly", "second", "secondly", "third", "thirdly", "next",
        "finally", "lastly",
    ]
    /// Of those, the ones that may also open a note without a comma after them.
    private static let bareNoteConnectives: Set<String> = ["and then", "and also", "and", "also", "then", "plus"]

    private static func droppingConnectives(from line: String) -> String {
        var note = line
        var changed = true
        while changed {
            changed = false
            let lowered = note.lowercased()
            for connective in noteConnectives where lowered.hasPrefix(connective) {
                let rest = note.dropFirst(connective.count)
                let next: Substring
                if rest.hasPrefix(", ") {
                    next = rest.dropFirst(2)
                } else if rest.hasPrefix(" "), bareNoteConnectives.contains(connective) {
                    next = rest.dropFirst()
                } else {
                    continue
                }
                // Never the whole note, and never into a closing mark ("And?").
                guard let first = next.first, first.isLetter || first.isNumber else { continue }
                note = String(next)
                changed = true
                break
            }
        }
        return note
    }

    /// Abbreviations whose full stop ends no sentence ("Dr. Patel").
    private static let abbreviations: Set<String> = [
        "mr", "mrs", "ms", "dr", "st", "jr", "sr", "prof", "vs", "etc", "inc", "ltd", "approx", "e.g", "i.e",
    ]

    /// `line`'s sentences, each with its mark; one sentence or none returns `[line]`.
    private static func noteSentences(in line: String) -> [String] {
        let chars = Array(line)
        var sentences: [String] = []
        var start = 0
        for index in chars.indices where endsSentence(chars, at: index) {
            if chars[index] == "." {
                var wordStart = index
                while wordStart > start, !chars[wordStart - 1].isWhitespace { wordStart -= 1 }
                let word = String(chars[wordStart..<index]).lowercased()
                // "Dr." or an initial ("J. K.") ends nothing.
                if abbreviations.contains(word) || (word.count == 1 && word.first?.isLetter == true) { continue }
            }
            let sentence = String(chars[start...index]).trimmingCharacters(in: .whitespaces)
            if !sentence.isEmpty { sentences.append(sentence) }
            start = index + 1
        }
        let rest = String(chars[start...]).trimmingCharacters(in: .whitespaces)
        if !rest.isEmpty { sentences.append(rest) }
        return sentences.isEmpty ? [line] : sentences
    }

    /// "Shopping list: eggs, milk and bread." -> ("Shopping list", ["eggs", "milk", "bread"]):
    /// three or more short items (four words at most) after a colon. English splits the last
    /// "and" or "or" off too; other languages take commas only.
    private static func colonList(in line: String, english: Bool) -> (intro: String, items: [String])? {
        guard let colon = line.range(of: ": ") else { return nil }
        let intro = String(line[..<colon.lowerBound]).trimmingCharacters(in: .whitespaces)
        var body = String(line[colon.upperBound...]).trimmingCharacters(in: .whitespaces)
        guard !intro.isEmpty, !body.contains(": ") else { return nil }
        if let last = body.last, last == "." || last == "。" { body.removeLast() }
        var items = body.components(separatedBy: ", ").map { $0.trimmingCharacters(in: .whitespaces) }
        if english, let last = items.last {
            items.removeLast()
            var tail = last
            for joiner in ["and ", "or "] where tail.lowercased().hasPrefix(joiner) {
                tail = String(tail.dropFirst(joiner.count))
            }
            if let split = tail.range(of: " and ") ?? tail.range(of: " or ") {
                items.append(String(tail[..<split.lowerBound]))
                items.append(String(tail[split.upperBound...]))
            } else {
                items.append(tail)
            }
        }
        let wordCounts = items.map { $0.split(separator: " ").count }
        guard items.count >= 3, wordCounts.allSatisfy({ (1...4).contains($0) }),
              !items.contains(where: { item in item.contains(where: { ".!?。！？".contains($0) }) }) else { return nil }
        return (intro, items)
    }

    private static func collapseHorizontalSpaces(in text: String) -> String {
        let lines = text.components(separatedBy: "\n").map { line in
            line.replacingOccurrences(of: #"[ \t]+"#, with: " ", options: .regularExpression)
                .trimmingCharacters(in: .whitespaces)
        }
        return lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func fixSpacingBeforePunctuation(in text: String) -> String {
        var result = text.replacingOccurrences(
            of: #"\s+([,.;:!?])"#,
            with: "$1",
            options: .regularExpression
        )
        result = result.replacingOccurrences(
            of: #"^[,;:]+\s*"#,
            with: "",
            options: .regularExpression
        )
        return result.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Capitalizes the first letter of each sentence: at the start, after a line break, or
    /// after ".", "!" or "?" and a space. A full stop inside a word ("gmail.com", "e.g.")
    /// starts nothing, and an address keeps its case.
    private static func capitalizeSentenceStarts(in text: String) -> String {
        var chars = Array(text)
        var capitalizeNext = true
        var endedSentence = false
        for index in chars.indices {
            let ch = chars[index]
            if capitalizeNext && ch.isLetter {
                if !isAddress(chars, at: index) {
                    for upper in String(ch).uppercased() {
                        chars[index] = upper
                        break
                    }
                }
                capitalizeNext = false
            } else if ch == "\n" {
                capitalizeNext = true
                endedSentence = false
            } else if ".!?".contains(ch) {
                endedSentence = true
                capitalizeNext = false
            } else if ch.isWhitespace {
                if endedSentence {
                    capitalizeNext = true
                    endedSentence = false
                }
            } else if "\"”’')]".contains(ch), endedSentence {
                continue
            } else if !"\"“(¿¡".contains(ch) {
                capitalizeNext = false
                endedSentence = false
            }
        }
        return String(chars)
    }

    private static func capitalizeFirstLetter(_ text: String) -> String {
        guard let index = text.firstIndex(where: \.isLetter),
              !isAddress(Array(text), at: text.distance(from: text.startIndex, to: index)) else { return text }
        var result = text
        let upper = String(result[index]).uppercased()
        result.replaceSubrange(index...index, with: upper)
        return result
    }
}
