import Foundation

/// Rewrites words a transcript likely misheard so they use the vocabulary's spelling.
///
/// Matching ignores case, whitespace, a single hyphen, and apostrophes. It never
/// changes words outside a matched span, so the surrounding transcript stays exactly
/// as transcribed. A word that carries an apostrophe of its own only matches a
/// term that also has one, and several transcript words are joined only when the
/// term reads as a compound, which keeps ordinary speech from matching by accident.
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

    private struct Candidate {
        let key: String
        let hasApostrophe: Bool
        let possessive: String?
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
        var spellings: [String: Spelling] = [:]
        for term in terms {
            let key = normalize(term)
            guard key.count >= 2, spellings[key] == nil else { continue }
            spellings[key] = Spelling(
                text: term,
                hasApostrophe: term.contains(where: isApostrophe),
                isCompound: looksCompound(term)
            )
        }
        guard !spellings.isEmpty, !text.isEmpty else { return text }

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
                spellings: spellings
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
        spellings: [String: Spelling]
    ) -> Match? {
        let longest = min(maxWindowLength, words.count - index)
        for wordCount in stride(from: longest, through: 1, by: -1) {
            guard canJoinWindow(from: index, count: wordCount, words: words, text: text) else {
                continue
            }

            for candidate in candidates(at: index, count: wordCount, words: words) {
                guard let spelling = spellings[candidate.key],
                      accepts(spelling, for: candidate, wordCount: wordCount) else {
                    continue
                }
                return Match(
                    wordCount: wordCount,
                    spelling: spelling.text,
                    possessive: candidate.possessive
                )
            }
        }
        return nil
    }

    /// The whole window key, plus the same key with a trailing possessive folded
    /// back onto its stem so "claude's" can still match "Claude".
    private static func candidates(at index: Int, count: Int, words: [Word]) -> [Candidate] {
        var key = ""
        var hasApostrophe = false
        for wordIndex in index..<(index + count) {
            key += words[wordIndex].key
            hasApostrophe = hasApostrophe || words[wordIndex].hasApostrophe
        }

        var result = [Candidate(key: key, hasApostrophe: hasApostrophe, possessive: nil)]

        let lastWord = words[index + count - 1]
        if let stemKey = lastWord.stemKey, let possessive = lastWord.possessive {
            var stemWindowKey = ""
            var stemHasApostrophe = lastWord.stemHasApostrophe
            for wordIndex in index..<(index + count - 1) {
                stemWindowKey += words[wordIndex].key
                stemHasApostrophe = stemHasApostrophe || words[wordIndex].hasApostrophe
            }
            stemWindowKey += stemKey
            result.append(
                Candidate(
                    key: stemWindowKey,
                    hasApostrophe: stemHasApostrophe,
                    possessive: possessive
                )
            )
        }
        return result
    }

    private static func accepts(
        _ spelling: Spelling,
        for candidate: Candidate,
        wordCount: Int
    ) -> Bool {
        // "she'll" must never match "Shell"; a possessive is handled by its stem
        // candidate, which carries the apostrophe separately.
        if candidate.hasApostrophe && !spelling.hasApostrophe {
            return false
        }
        // Joining words is only safe when the term itself reads as one word.
        if wordCount > 1 && !spelling.isCompound {
            return false
        }
        return true
    }

    /// A window of several words only spans spaces, tabs, or a single hyphen, and
    /// never includes a one-letter word that could glue ordinary speech together.
    private static func canJoinWindow(
        from index: Int,
        count: Int,
        words: [Word],
        text: String
    ) -> Bool {
        guard count > 1 else { return true }

        for wordIndex in index..<(index + count) where words[wordIndex].key.count < 2 {
            return false
        }

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
