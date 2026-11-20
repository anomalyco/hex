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
        case .polished: String(localized: "Clean punctuation and filler trim")
        case .casual: String(localized: "Lowercase texting, no trailing period")
        case .notes: String(localized: "Spoken bullets and line breaks")
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
    /// `language` (BCP 47) picks the fillers a style trims; nil is English.
    static func format(_ raw: String, style: DictationStyle, language: String? = nil) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "" }
        let fillers = fillerPattern(for: language)

        switch style {
        case .verbatim:
            return collapseHorizontalSpaces(in: trimmed)

        case .polished:
            let cleaned = stripStandaloneFillers(in: trimmed, fillers: fillers)
            return polishSentences(cleaned)

        case .casual:
            let cleaned = stripStandaloneFillers(in: trimmed, fillers: fillers)
            var lowered = collapseHorizontalSpaces(in: cleaned).lowercased()
            // Keep standalone "i" and "i'm"/"i'll"/"i've"/"i'd" capitalized naturally, or pure casual lowercase?
            // Pure texting style: lowercase start and drop single-sentence trailing period, keep ? and !.
            lowered = fixSpacingBeforePunctuation(in: lowered)
            if lowered.hasSuffix("."), !lowered.dropLast().contains(where: { ".!?".contains($0) }) {
                lowered.removeLast()
            }
            return lowered.trimmingCharacters(in: .whitespacesAndNewlines)

        case .notes:
            let cleaned = stripStandaloneFillers(in: trimmed, fillers: fillers)
            return formatNotes(cleaned)
        }
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
            return "um+|uh+|er+|ah+"
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

    private static func polishSentences(_ text: String) -> String {
        guard !text.isEmpty else { return "" }
        var normalized = fixSpacingBeforePunctuation(in: collapseHorizontalSpaces(in: text))
        normalized = capitalizeSentenceStarts(in: normalized)
        // A lone address ("john@gmail.com") is no sentence to end.
        let loneAddress = normalized.contains("@") && !normalized.contains(where: \.isWhitespace)
        if let last = normalized.last, !".!?…\"”)]".contains(last), !loneAddress {
            normalized.append(".")
        }
        return normalized
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

    private static func formatNotes(_ text: String) -> String {
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
        let lines = working
            .components(separatedBy: "\n")
            .map { line -> String in
                let trimmedLine = collapseHorizontalSpaces(in: line)
                if trimmedLine.hasPrefix("- ") {
                    let body = String(trimmedLine.dropFirst(2)).trimmingCharacters(in: .whitespaces)
                    return body.isEmpty ? "" : "- " + capitalizeFirstLetter(fixSpacingBeforePunctuation(in: body))
                }
                return capitalizeFirstLetter(fixSpacingBeforePunctuation(in: trimmedLine))
            }
        return lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
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
