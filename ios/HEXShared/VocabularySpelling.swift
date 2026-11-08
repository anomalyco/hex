import Foundation

/// Rewrites words a transcript likely misheard so they use the vocabulary's spelling.
///
/// Matching ignores case, whitespace, a single hyphen, and apostrophes. It never
/// changes words outside a matched span, so the surrounding transcript stays exactly
/// as transcribed.
nonisolated enum VocabularySpelling {
    private static let maxWindowLength = 4

    private struct Word {
        let range: Range<String.Index>
        let key: String
        let possessive: String?
        let stemKey: String?
    }

    private struct Match {
        let wordCount: Int
        let spelling: String
        let possessive: String?
    }

    static func apply(_ terms: [String], to text: String) -> String {
        var spellings: [String: String] = [:]
        for term in terms {
            let key = normalize(term)
            guard key.count >= 2, spellings[key] == nil else { continue }
            spellings[key] = term
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
        spellings: [String: String]
    ) -> Match? {
        let longest = min(maxWindowLength, words.count - index)
        for wordCount in stride(from: longest, through: 1, by: -1) {
            guard canJoinAll(from: index, count: wordCount, words: words, text: text) else {
                continue
            }

            var key = ""
            for wordIndex in index..<(index + wordCount) {
                key += words[wordIndex].key
            }

            if let spelling = spellings[key] {
                return Match(wordCount: wordCount, spelling: spelling, possessive: nil)
            }

            let lastWord = words[index + wordCount - 1]
            if let stemKey = lastWord.stemKey,
               let possessive = lastWord.possessive {
                var stemWindowKey = ""
                for wordIndex in index..<(index + wordCount - 1) {
                    stemWindowKey += words[wordIndex].key
                }
                stemWindowKey += stemKey

                if let spelling = spellings[stemWindowKey] {
                    return Match(
                        wordCount: wordCount,
                        spelling: spelling,
                        possessive: possessive
                    )
                }
            }
        }
        return nil
    }

    /// A window only crosses whitespace or a single hyphen, never other punctuation.
    private static func canJoinAll(
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
            if !(separator == "-" || separator.allSatisfy(\.isWhitespace)) {
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
        let word = String(substring)
        let key = normalize(word)
        guard !key.isEmpty else { return }

        let possessiveInfo = possessive(in: word)
        words.append(
            Word(
                range: substring.startIndex..<substring.endIndex,
                key: key,
                possessive: possessiveInfo?.suffix,
                stemKey: possessiveInfo?.stemKey
            )
        )
    }

    /// Splits a trailing English possessive so the stem can match a term.
    private static func possessive(in word: String) -> (suffix: String, stemKey: String)? {
        for suffix in ["'s", "’s"] where word.hasSuffix(suffix) {
            let stem = String(word.dropLast(suffix.count))
            let stemKey = normalize(stem)
            guard !stemKey.isEmpty else { return nil }
            return (suffix, stemKey)
        }
        return nil
    }

    private static func isWordCharacter(_ character: Character) -> Bool {
        character.isLetter || character.isNumber || character == "'" || character == "’"
    }

    private static func normalize(_ text: String) -> String {
        text.lowercased().filter { $0.isLetter || $0.isNumber }
    }
}
