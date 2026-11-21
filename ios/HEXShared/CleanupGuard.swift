import Foundation

/// Clean up what I say's safety net after the tagger: a word goes only when the tagger is sure
/// and its removal is a known disfluency. The tagger alone deleted every hyphenated name
/// ("Tell Hye-jin I said thanks." became "Tell thanks."), "maybe" in "later, maybe after
/// dinner", and clean sentences' words it had seen as reparanda in training.
///
/// The tagger's P(DELETE) marks runs of words (spans) to remove. A span goes when it is:
/// - a spoken self-correction: something said, then a cue ("Tuesday, no wait,", "John, I
///   mean"), or a hesitation before the same thing said again differently ("15 euros, er,"
///   before "50 euros", "3, uh," before "4"), with every word above `repairFloor`;
/// - a repeat: the span is said again right after it ("I I", "the, the office", "w- we");
/// - a false start: a fragment between a clause start and a pause ("Did you, have you"),
///   only of function words or with every word above `sureFloor`;
/// - otherwise only its fillers go: um and uh, and "like", "you know", "I mean" set off by
///   commas, or without commas above `sureFloor` where "like" can't be the verb.
///
/// Outside a self-correction it never removes a protected word: a hyphenated compound
/// (Jean-Luc, follow-up), a word with a digit, a capitalized word mid-sentence (likely a
/// name), a custom vocabulary or learned term, or a content adverb (maybe, actually,
/// really), which also stays when repeated for emphasis ("really, really").
nonisolated enum CleanupGuard {
    /// P(DELETE) above which the tagger marks a word (the threshold it shipped with).
    static let deleteThreshold = 0.98
    /// Every word of a self-correction must be above this.
    static let repairFloor = 0.995
    /// A false start with content words, or a filler without commas, must be above this.
    static let sureFloor = 0.999

    private static let pureFillers: Set<String> = [
        "um", "uh", "er", "erm", "ah", "hmm", "umm", "uhh", "uhm", "mm", "hm", "eh", "mhm", "uh-huh",
    ]
    private static let cues: [[String]] = [
        ["actually"], ["no"], ["wait"], ["sorry"], ["i", "mean"], ["i", "meant"], ["rather"], ["make", "that"],
        ["make", "it"], ["scratch", "that"], ["correction"], ["oops"], ["my", "bad"], ["nope"], ["hold", "on"],
        ["change", "that", "to"], ["let", "me", "correct", "that"], ["let", "me", "rephrase", "that"],
        ["strike", "that"], ["delete", "that"], ["never", "mind"], ["i'm", "sorry"],
    ]
    private static let adverbs: Set<String> = ["maybe", "actually", "really", "probably", "definitely", "perhaps"]
    /// "It was like super expensive": "like" after a form of be is a filler...
    private static let be: Set<String> = [
        "was", "is", "are", "were", "it's", "that's", "be", "been", "being", "i'm", "you're", "he's", "she's",
        "we're", "they're", "am", "so",
    ]
    /// ...unless what follows starts a noun phrase: "it was like a dream".
    private static let nounStarts: Set<String> = [
        "a", "an", "the", "this", "that", "my", "your", "his", "her", "our", "their", "its", "i", "you", "he",
        "she", "we", "they", "it", "me", "him", "them", "us",
    ]
    /// Words a false start may be made of at the tagger's own threshold.
    private static let functionWords: Set<String> = [
        "a", "an", "the", "to", "of", "for", "with", "at", "in", "on", "from", "by", "about", "my", "your",
        "his", "her", "our", "their", "i", "we", "you", "he", "she", "they", "it", "can", "could", "would",
        "will", "should", "do", "does", "did", "is", "are", "was", "were", "be", "have", "has", "had", "and",
        "or", "but", "if", "that", "because", "so", "just", "want", "need", "think", "get", "got", "let's",
        "i'm", "we're", "you're", "it's", "there's", "i'll", "we'll", "i'd", "this", "me", "him", "them",
        "us", "some", "gonna", "wanna", "go", "going", "what", "when", "where", "how", "why", "which", "who",
    ]
    /// After these "like" is the verb ("I like it", "would like to"), never a filler.
    private static let likeVerbBefore: Set<String> = [
        "i", "you", "we", "they", "he", "she", "would", "i'd", "you'd", "we'd", "they'd", "to", "do",
        "don't", "did", "didn't", "does", "doesn't", "really", "also", "just", "will", "i'll", "we'll",
        "might", "may", "people", "kids", "who", "not", "you'll", "they'll", "can", "could", "should",
    ]

    /// The word as the tagger sees it: a hyphen between letters is dropped. The tokenizer splits
    /// "Hye-jin" into "hye", "-", "jin", exactly what a stutter fragment and the next word look
    /// like ("w- we"), which training always deleted; "Hyejin" reads as one name. Only the
    /// model's input changes, never the text.
    static func modelWord(_ word: String) -> String {
        let characters = Array(word)
        guard characters.contains("-") else { return word }
        var result = ""
        for (index, character) in characters.enumerated() {
            if character == "-", index > 0, index + 1 < characters.count,
               characters[index - 1].isLetter, characters[index + 1].isLetter {
                continue
            }
            result.append(character)
        }
        return result
    }

    /// Which words to delete: `probabilities` holds P(DELETE) per word of `words`, and
    /// `protectedTerms` (from `protectedTerms(_:)`) are custom vocabulary and learned terms that
    /// only a self-correction or a repeat may remove.
    static func deletes(_ words: [String], probabilities: [Double], protectedTerms: Set<String> = []) -> [Bool] {
        let count = min(words.count, probabilities.count)
        let bares = words.map(bare)
        var out = Array(repeating: false, count: words.count)
        var i = 0
        while i < count {
            guard probabilities[i] > deleteThreshold else {
                i += 1
                continue
            }
            var j = i
            while j < count, probabilities[j] > deleteThreshold {
                j += 1
            }
            let span = Array(i..<j)
            let spanBares = span.map { bares[$0] }
            let sure = span.map { probabilities[$0] }.min() ?? 0
            let isProtected = span.map { isProtectedWord(words, bares, at: $0, terms: protectedTerms) }
            let cueWords = cueMask(spanBares)
            let fillers = spanBares.map { pureFillers.contains($0) || $0.isEmpty }

            var repair = sure > repairFloor && cueWords.contains(true)
                && zip(cueWords, fillers).contains { !$0 && !$1 }
            if !repair, fillers.contains(true), j < count {
                let content = zip(span, fillers).filter { !$0.1 }.map(\.0)
                let next = bares[j..<min(count, j + 4)]
                if let last = content.last,
                   (content.allSatisfy { hasDigit(words[$0]) } && hasDigit(words[j])) || next.contains(bares[last]) {
                    repair = sure > repairFloor
                }
            }
            if repair {
                for k in span { out[k] = true }
                i = j
                continue
            }

            let after = Array(bares[j..<count])
            let length = span.count
            let isRepeat = Array(after.prefix(length)) == spanBares
                || (!spanBares[0].isEmpty && after.first == spanBares[0])
                || (length == 1 && words[i].hasSuffix("-") && !spanBares[0].isEmpty
                    && after.first?.hasPrefix(spanBares[0]) == true)
            if isRepeat {
                for (k, protected) in zip(span, isProtected) where !(protected && adverbs.contains(bares[k])) {
                    out[k] = true
                }
                i = j
                continue
            }

            if !isProtected.contains(true) {
                let startsClause = i == 0 || endsWithAny(words[i - 1], [".", "?", "!", ","])
                let falseStart = startsClause && j < count && endsWithPause(words[j - 1])
                    && (sure > sureFloor || spanBares.allSatisfy { functionWords.contains($0) })
                if falseStart {
                    for k in span { out[k] = true }
                    i = j
                    continue
                }
            }
            for index in span.indices where isFiller(
                index, span: span, spanBares: spanBares, words: words, bares: bares, probabilities: probabilities
            ) {
                out[span[index]] = true
            }
            i = j
        }
        return out
    }

    /// Custom vocabulary and learned terms, word by word, as `deletes` compares them.
    static func protectedTerms(_ terms: [String]) -> Set<String> {
        Set(terms.flatMap { $0.split(whereSeparator: \.isWhitespace).map { bare(String($0)) } }.filter { $0.count >= 2 })
    }

    // MARK: Words

    private static func isFiller(
        _ index: Int, span: [Int], spanBares: [String], words: [String], bares: [String], probabilities: [Double]
    ) -> Bool {
        let k = span[index]
        let word = spanBares[index]
        let previous = k > 0 ? words[k - 1] : ""
        let previousBare = k > 0 ? bares[k - 1] : ""
        let nextInSpan = index + 1 < span.count ? spanBares[index + 1] : nil
        let previousInSpan = index > 0 ? spanBares[index - 1] : nil
        if pureFillers.contains(word) { return true }
        if words[k].hasSuffix("-"), k + 1 < words.count, !word.isEmpty, bares[k + 1].hasPrefix(word) {
            return true // "w- we"
        }
        let sure = probabilities[k] > sureFloor
        switch word {
        case "like":
            let next = k + 1 < words.count ? bares[k + 1] : ""
            let commas = words[k].hasSuffix(",") || previous.hasSuffix(",")
            return commas || (be.contains(previousBare) && !nounStarts.contains(next) && !next.isEmpty)
                || (sure && !likeVerbBefore.contains(previousBare))
        case "know" where previousInSpan == "you":
            let you = span[index - 1]
            return words[k].hasSuffix(",") || (you > 0 && words[you - 1].hasSuffix(","))
                || (sure && probabilities[you] > sureFloor)
        case "you" where nextInSpan == "know":
            let know = span[index + 1]
            return words[know].hasSuffix(",") || previous.hasSuffix(",")
                || (sure && probabilities[know] > sureFloor)
        case "i" where nextInSpan == "mean":
            let mean = span[index + 1]
            return words[mean].hasSuffix(",") || (sure && probabilities[mean] > sureFloor)
        case "mean" where previousInSpan == "i":
            return words[k].hasSuffix(",") || (sure && probabilities[span[index - 1]] > sureFloor)
        default:
            return false
        }
    }

    private static func isProtectedWord(_ words: [String], _ bares: [String], at index: Int, terms: Set<String>) -> Bool {
        let word = words[index]
        if hasInnerHyphen(word) || hasDigit(word) { return true }
        if terms.contains(bares[index]) || adverbs.contains(bares[index]) { return true }
        return isCapitalizedMidSentence(words, bares, at: index)
    }

    private static func isCapitalizedMidSentence(_ words: [String], _ bares: [String], at index: Int) -> Bool {
        guard index > 0, let first = words[index].first(where: isWordCharacter), first.isUppercase else { return false }
        if endsWithAny(words[index - 1], [".", "?", "!", "\u{2026}", ":", "\""]) { return false }
        let word = bares[index]
        return word != "i" && !word.hasPrefix("i'")
    }

    private static func cueMask(_ bares: [String]) -> [Bool] {
        var mask = Array(repeating: false, count: bares.count)
        for start in bares.indices {
            for cue in cues where start + cue.count <= bares.count && Array(bares[start..<(start + cue.count)]) == cue {
                for k in start..<(start + cue.count) { mask[k] = true }
            }
        }
        return mask
    }

    /// Lowercased, without leading punctuation (but "$", "#") or trailing punctuation (but "%").
    static func bare(_ word: String) -> String {
        let characters = Array(word.lowercased())
        var start = 0
        var end = characters.count
        while start < end, !(isWordCharacter(characters[start]) || characters[start] == "$" || characters[start] == "#") {
            start += 1
        }
        while end > start, !(isWordCharacter(characters[end - 1]) || characters[end - 1] == "%") {
            end -= 1
        }
        return String(characters[start..<end])
    }

    private static func isWordCharacter(_ character: Character) -> Bool {
        character.isLetter || character.isNumber || character == "_"
    }

    private static func hasDigit(_ word: String) -> Bool {
        word.contains(where: \.isNumber)
    }

    private static func hasInnerHyphen(_ word: String) -> Bool {
        let characters = Array(word)
        guard characters.count >= 3 else { return false }
        return (1..<(characters.count - 1)).contains {
            characters[$0] == "-" && characters[$0 - 1].isLetter && characters[$0 + 1].isLetter
        }
    }

    private static func endsWithAny(_ word: String, _ marks: [Character]) -> Bool {
        guard let last = word.last else { return false }
        return marks.contains(last)
    }

    private static func endsWithPause(_ word: String) -> Bool {
        word.hasSuffix(",") || word.hasSuffix("...") || word.hasSuffix(";")
            || word.hasSuffix("\u{2026}") || word.hasSuffix("\u{2014}") || word.hasSuffix("\u{2013}")
    }
}
