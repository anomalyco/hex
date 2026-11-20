import Foundation

nonisolated enum DictationStyle: String, Codable, CaseIterable, Sendable {
    case polished
    case casual
    case notes
    case verbatim

    var label: String {
        switch self {
        case .polished: "Polished"
        case .casual: "Casual"
        case .notes: "Notes"
        case .verbatim: "Verbatim"
        }
    }

    var shortHint: String {
        switch self {
        case .polished: "Clean punctuation and filler trim"
        case .casual: "Lowercase texting, no trailing period"
        case .notes: "Spoken bullets and line breaks"
        case .verbatim: "Exact words from the model"
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

    /// Joins dictated text cleanly with existing text around the insertion cursor.
    static func insertionText(
        for formatted: String,
        beforeContext: String?,
        afterContext: String?
    ) -> String {
        guard !formatted.isEmpty else { return "" }
        var output = formatted

        if let before = beforeContext, let lastChar = before.last {
            let needsLeadingSpace = !lastChar.isWhitespace
                && lastChar != "("
                && lastChar != "["
                && lastChar != "{"
                && lastChar != "\""
                && lastChar != "“"
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

    private static func isClosingPunctuation(_ character: Character?) -> Bool {
        guard let character else { return false }
        return ".,!?;:)]}”".contains(character)
    }

    /// The hesitation sounds a style trims, per language. English "er" is a word in
    /// Norwegian, Danish, Swedish ("er" = is) and German, and "um" is one in Portuguese, so
    /// each language trims only its own; a language without a list trims nothing.
    static func fillerPattern(for language: String?) -> String? {
        let code = language.flatMap { Locale(identifier: $0).language.languageCode?.identifier } ?? "en"
        switch code {
        case "en":
            return "um+|uh+|er+|ah+"
        case "nb", "nn", "no", "da", "sv":
            return "e+h+m*|ø+h+m*|ö+h+m*|hm+"
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
        if let last = normalized.last, !".!?…\"”)]".contains(last) {
            normalized.append(".")
        }
        return normalized
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

    private static func capitalizeSentenceStarts(in text: String) -> String {
        var chars = Array(text)
        var capitalizeNext = true
        for index in chars.indices {
            let ch = chars[index]
            if capitalizeNext && ch.isLetter {
                for upper in String(ch).uppercased() {
                    chars[index] = upper
                    break
                }
                capitalizeNext = false
            } else if ".!?".contains(ch) || ch == "\n" {
                capitalizeNext = true
            } else if !ch.isWhitespace && ch != "\"" && ch != "“" && ch != "(" {
                capitalizeNext = false
            }
        }
        return String(chars)
    }

    private static func capitalizeFirstLetter(_ text: String) -> String {
        guard let index = text.firstIndex(where: \.isLetter) else { return text }
        var result = text
        let upper = String(result[index]).uppercased()
        result.replaceSubrange(index...index, with: upper)
        return result
    }
}
