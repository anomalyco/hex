import Foundation

/// Rewrites words a transcript likely misheard so they use the vocabulary's spelling.
///
/// Matching works across case, spaces, tabs, a single hyphen, and possessive
/// apostrophes, and never changes anything outside a matched span. The listed
/// spelling is kept exactly, even at a sentence start. A term that spells its own
/// boundaries ("iPhone 7", "X-ray") matches those boundaries exactly, and a
/// compound term ("WeChat") can also match the plain phrase ("we chat"). A word
/// that carries its own apostrophe only matches a term that has one.
nonisolated enum VocabularySpelling {
    private static let maxWindowLength = 4
    private static let quoteMarks: Set<Character> = ["'", "’", "‘", "\"", "“", "”", "`"]

    private struct Spelling {
        let text: String
        let hasApostrophe: Bool
        let isCompound: Bool
    }

    private struct Word {
        let range: Range<String.Index>
        let key: String
        let hasApostrophe: Bool
        let possessive: String?
        let stemKey: String?
        let stemHasApostrophe: Bool
    }

    private struct Match {
        let wordCount: Int
        let spelling: String
        let possessive: String?
    }

    /// The letters and digits that decide whether two terms are the same word.
    /// Settings and the editor use this so a term is judged the same way everywhere.
    static func normalizedKey(_ term: String) -> String {
        normalize(term)
    }

    static func apply(_ terms: [String], to text: String) -> String {
        var glued: [String: Spelling] = [:]
        var phrases: [[String]: Spelling] = [:]
        for term in terms {
            let key = normalize(term)
            guard key.count >= 2, glued[key] == nil else { continue }
            let spelling = Spelling(
                text: term,
                hasApostrophe: term.contains(where: isApostrophe),
                isCompound: looksCompound(term)
            )
            glued[key] = spelling

            let wordKeys = termWordKeys(term)
            if wordKeys.count > 1 {
                phrases[wordKeys] = spelling
            }
        }
        guard !glued.isEmpty, !text.isEmpty else { return text }

        let tokens = words(in: text)
        guard !tokens.isEmpty else { return text }

        var output = ""
        output.reserveCapacity(text.count)
        var copiedThrough = text.startIndex
        var index = 0

        while index < tokens.count {
            guard let matched = match(
                startingAt: index,
                words: tokens,
                text: text,
                glued: glued,
                phrases: phrases
            ) else {
                index += 1
                continue
            }

            let lastWord = index + matched.wordCount - 1
            output.append(contentsOf: text[copiedThrough..<tokens[index].range.lowerBound])
            output.append(matched.spelling)
            if let possessive = matched.possessive {
                output.append(possessive)
            }
            copiedThrough = tokens[lastWord].range.upperBound
            index = lastWord + 1
        }

        output.append(contentsOf: text[copiedThrough...])
        return output
    }

    private static func match(
        startingAt index: Int,
        words: [Word],
        text: String,
        glued: [String: Spelling],
        phrases: [[String]: Spelling]
    ) -> Match? {
        let longest = min(maxWindowLength, words.count - index)
        for wordCount in stride(from: longest, through: 1, by: -1) {
            guard canJoinWindow(from: index, count: wordCount, words: words, text: text) else {
                continue
            }

            let window = words[index..<(index + wordCount)]
            let keys = window.map(\.key)
            let hasApostrophe = window.contains(where: \.hasApostrophe)
            let lastWord = words[index + wordCount - 1]

            var stemKeys: [String]?
            var stemHasApostrophe = false
            if let stemKey = lastWord.stemKey {
                var candidate = keys
                candidate[candidate.count - 1] = stemKey
                stemKeys = candidate
                stemHasApostrophe = window.dropLast().contains(where: \.hasApostrophe)
                    || lastWord.stemHasApostrophe
            }

            // A term that spells its own boundaries ("iPhone 7", "X-ray") matches
            // them exactly, even when one of them is a single letter.
            if wordCount > 1,
               let spelling = phrases[keys],
               accepts(spelling, hasApostrophe: hasApostrophe, needsCompound: false) {
                return Match(wordCount: wordCount, spelling: spelling.text, possessive: nil)
            }

            if wordCount > 1,
               let stemKeys,
               let possessive = lastWord.possessive,
               let spelling = phrases[stemKeys],
               accepts(
                   spelling,
                   hasApostrophe: stemHasApostrophe,
                   needsCompound: false
               ) {
                return Match(
                    wordCount: wordCount,
                    spelling: spelling.text,
                    possessive: possessive
                )
            }

            // Only a glued match remains, and that never spans a one-letter word
            // because ordinary speech would match by accident.
            guard wordCount == 1 || !keys.contains(where: { $0.count < 2 }) else {
                continue
            }

            if let spelling = glued[keys.joined()],
               accepts(
                   spelling,
                   hasApostrophe: hasApostrophe,
                   needsCompound: wordCount > 1
               ) {
                return Match(wordCount: wordCount, spelling: spelling.text, possessive: nil)
            }

            if let stemKeys,
               let possessive = lastWord.possessive,
               let spelling = glued[stemKeys.joined()],
               accepts(
                   spelling,
                   hasApostrophe: stemHasApostrophe,
                   needsCompound: wordCount > 1
               ) {
                return Match(
                    wordCount: wordCount,
                    spelling: spelling.text,
                    possessive: possessive
                )
            }
        }
        return nil
    }

    private static func accepts(
        _ spelling: Spelling,
        hasApostrophe: Bool,
        needsCompound: Bool
    ) -> Bool {
        // "she'll" must never match "Shell"; a possessive is handled by its stem
        // candidate, which carries the apostrophe separately.
        if hasApostrophe && !spelling.hasApostrophe {
            return false
        }
        // Gluing ordinary words together is only safe for a compound term.
        if needsCompound && !spelling.isCompound {
            return false
        }
        return true
    }

    /// A window of several words only spans spaces, tabs, or a single hyphen.
    private static func canJoinWindow(
        from index: Int,
        count: Int,
        words: [Word],
        text: String
    ) -> Bool {
        guard count > 1 else { return true }

        for wordIndex in (index + 1)..<(index + count) {
            let separator = text[
                words[wordIndex - 1].range.upperBound..<words[wordIndex].range.lowerBound
            ]
            let isSpaceOrTab = separator.allSatisfy { $0 == " " || $0 == "\t" }
            if separator != "-" && !isSpaceOrTab {
                return false
            }
        }
        return true
    }

    private static func words(in text: String) -> [Word] {
        var words: [Word] = []
        var wordStart: String.Index?

        for index in text.indices {
            let character = text[index]
            if isWordCharacter(character) {
                if wordStart == nil {
                    wordStart = index
                }
            } else if let start = wordStart {
                appendWord(text[start..<index], to: &words)
                wordStart = nil
            }
        }

        if let start = wordStart {
            appendWord(text[start...], to: &words)
        }
        return words
    }

    private static func appendWord(_ substring: Substring, to words: inout [Word]) {
        var start = substring.startIndex
        var end = substring.endIndex
        while start < end, quoteMarks.contains(substring[start]) {
            start = substring.index(after: start)
        }
        while start < end, quoteMarks.contains(substring[substring.index(before: end)]) {
            end = substring.index(before: end)
        }
        guard start < end else { return }

        let word = String(substring[start..<end])
        let key = normalize(word)
        guard !key.isEmpty else { return }

        let possessiveInfo = possessive(in: word)
        words.append(
            Word(
                range: start..<end,
                key: key,
                hasApostrophe: word.contains(where: isApostrophe),
                possessive: possessiveInfo?.suffix,
                stemKey: possessiveInfo?.stemKey,
                stemHasApostrophe: possessiveInfo?.stemHasApostrophe ?? false
            )
        )
    }

    /// Splits a trailing English possessive so the stem can match a term.
    private static func possessive(
        in word: String
    ) -> (suffix: String, stemKey: String, stemHasApostrophe: Bool)? {
        for suffix in ["'s", "’s"] where word.hasSuffix(suffix) {
            let stem = String(word.dropLast(suffix.count))
            let stemKey = normalize(stem)
            guard !stemKey.isEmpty else { return nil }
            return (suffix, stemKey, stem.contains(where: isApostrophe))
        }
        return nil
    }

    private static func isWordCharacter(_ character: Character) -> Bool {
        character.isLetter || character.isNumber || isApostrophe(character)
    }

    private static func isApostrophe(_ character: Character) -> Bool {
        character == "'" || character == "’"
    }

    /// Splits a term on its own spaces and hyphens so each word keeps its key.
    /// This lets "iPhone 7", "Plan B", and "X-ray" match one-letter words exactly
    /// where the term puts them, without letting glued matches span one-letter words.
    private static func termWordKeys(_ term: String) -> [String] {
        var keys: [String] = []
        var current = ""
        for character in term {
            guard character.isWhitespace || character == "-" else {
                current.append(character)
                continue
            }
            if !current.isEmpty {
                keys.append(normalize(current))
                current = ""
            }
        }
        if !current.isEmpty {
            keys.append(normalize(current))
        }
        return keys.filter { !$0.isEmpty }
    }

    /// A compound is a term people say as one word: it has a space, a hyphen, a
    /// digit, or a capital after a lowercase letter ("OpenCode", "iPhone 17").
    private static func looksCompound(_ term: String) -> Bool {
        var previous: Character?
        for character in term {
            if character.isWhitespace || character == "-" || character.isNumber {
                return true
            }
            if let previous, previous.isLowercase, character.isUppercase {
                return true
            }
            previous = character
        }
        return false
    }

    private static func normalize(_ text: String) -> String {
        text.lowercased().filter { $0.isLetter || $0.isNumber }
    }
}
