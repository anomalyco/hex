// The dictation parity oracle: a copy of ios/HEXShared/CleanupReconstruction.swift at dev 93db6ad7 (unchanged
// since d6efeddb), its types renamed with an "Oracle" suffix, so the Rust port in
// crates/keyboard/core/src/dictation/ is checked against today's Swift (DictationParityTests).
// Only the UserDefaults settings struct is left out. Never edit the rules here.
import Foundation

/// Rebuilds a transcript from the cleanup tagger's KEEP/DELETE labels: joins the kept words
/// and repairs punctuation and capitals left next to a deletion. It only removes characters
/// (commas, hesitation ellipses) or changes the case of a first letter; it never adds,
/// reorders, or changes words, names, or numbers.
///
/// A line-for-line port of the tagger's `js/reconstruct.js`, with JavaScript's string
/// semantics: a word's "first letter" is its first UTF-16 unit (an emoji or other astral
/// character is never cased), while the rest of a word is checked by code point.
nonisolated enum CleanupReconstructionOracle {
    private typealias Word = [Unicode.Scalar]

    /// Words that never keep a trailing comma when the filler after them is removed:
    /// "Can you, um, send" -> "Can you send".
    private static let noComma: Set<String> = [
        "a", "an", "the", "to", "of", "for", "with", "at", "in", "on", "from", "by", "about", "into", "onto", "my",
        "your", "his", "her", "our", "their", "its", "i", "we", "you", "he", "she", "they", "can",
        "could", "would", "will", "should", "shall", "may", "might", "must", "do", "does", "did", "is",
        "are", "was", "were", "be", "been", "am", "have", "has", "had", "and", "or", "but", "if", "that", "because",
        "than", "just", "gonna", "wanna", "want", "need", "think", "thinking", "guess", "get", "got", "let's", "i'm",
        "we're", "you're", "they're", "it's", "there's", "i'll", "we'll", "you'll", "i'd", "we'd", "i've", "we've",
        "really", "very", "so", "like", "isn't", "don't", "can't", "won't", "didn't", "this", "these", "those", "some",
        "any", "every", "what", "where", "when", "how", "which", "who", "also", "go", "going", "s",
    ]
    /// Function words a correction's repair may start with; lowercased when a deleted
    /// sentence end capitalized them: "at 5. Wait, no. At 6." -> "at 6."
    private static let lowerable: Set<String> = [
        "at", "on", "in", "the", "a", "an", "to", "for", "with", "from", "by", "and", "or", "but", "about", "around",
        "before", "after", "until", "into", "my", "your", "our", "their", "his", "her", "this", "that", "these", "those",
    ]
    private static let fillerWords: Set<String> = [
        "um", "uh", "er", "ah", "hmm", "umm", "uhh", "erm", "uhm", "mm", "like", "you", "know", "i", "mean",
    ]
    private static let trailing: Set<Unicode.Scalar> = [",", ".", "?", "!", ";", ":", "\u{2026}"]
    private static let pauses: Set<Unicode.Scalar> = [",", ";", ":"]
    private static let sentenceEnds: Set<Unicode.Scalar> = [".", "?", "!"]

    /// `words` are the transcript's whitespace-separated words; `deletes` has one flag per word.
    static func reconstruct(_ words: [String], deletes: [Bool]) -> String {
        var out: [Word] = []
        var pending: [Word] = []
        for (index, word) in words.enumerated() {
            var scalars = Array(word.unicodeScalars)
            if index < deletes.count, deletes[index] {
                pending.append(scalars)
                continue
            }
            if !pending.isEmpty {
                scalars = afterGap(&out, deleted: pending, next: scalars)
                pending.removeAll()
            }
            out.append(scalars)
        }
        if !pending.isEmpty, !out.isEmpty {
            atEnd(&out, deleted: pending)
        }
        return out.map(string).joined(separator: " ")
    }

    private static func afterGap(_ out: inout [Word], deleted: [Word], next: Word) -> Word {
        var b = next
        guard let last = out.last else { return capitalized(b) } // deletion at the very start
        var a = stripEllipsis(last) // hesitation "..." before a removed disfluency
        let term = terminal(deleted[deleted.count - 1])
        if a.last == ",", contains(noComma, bare(a)) {
            a.removeLast()
        }
        if !term.isEmpty, !endsSentence(a), isUpper(firstUnit(b)) {
            if contains(lowerable, bare(b)) {
                b = lowercasedFirst(b)
            } else if deleted.allSatisfy({ contains(fillerWords, bare($0)) }) {
                a = trimming(a, pauses) + term // "Thanks, um. See you" -> "Thanks. See you"
            }
        }
        out[out.count - 1] = a
        if a.isEmpty || endsSentence(a) {
            b = capitalized(b)
        }
        return b
    }

    private static func atEnd(_ out: inout [Word], deleted: [Word]) {
        var a = stripEllipsis(out[out.count - 1])
        if !endsSentence(a) {
            a = trimming(a, pauses) + terminal(deleted[deleted.count - 1])
        }
        out[out.count - 1] = a
    }

    // MARK: JavaScript string helpers

    /// `t[0]`: the first UTF-16 unit. An astral first character is half a surrogate pair
    /// in JavaScript, which is neither upper- nor lowercase, so it reads as nil here.
    private static func firstUnit(_ t: Word) -> Unicode.Scalar? {
        guard let first = t.first, first.value <= 0xFFFF else { return nil }
        return first
    }

    /// `ch === ch.toUpperCase() && ch !== ch.toLowerCase()`
    private static func isUpper(_ ch: Unicode.Scalar?) -> Bool {
        guard let ch else { return false }
        return same(ch.properties.uppercaseMapping, ch) && !same(ch.properties.lowercaseMapping, ch)
    }

    /// `ch === ch.toLowerCase() && ch !== ch.toUpperCase()`
    private static func isLower(_ ch: Unicode.Scalar?) -> Bool {
        guard let ch else { return false }
        return same(ch.properties.lowercaseMapping, ch) && !same(ch.properties.uppercaseMapping, ch)
    }

    private static func same(_ mapping: String, _ scalar: Unicode.Scalar) -> Bool {
        let scalars = mapping.unicodeScalars
        return scalars.count == 1 && scalars.first == scalar
    }

    private static func endsSentence(_ t: Word) -> Bool {
        guard let last = t.last, sentenceEnds.contains(last) else { return false }
        return !hasEllipsis(t)
    }

    private static func hasEllipsis(_ t: Word) -> Bool {
        t.count >= 3 && t[t.count - 3] == "." && t[t.count - 2] == "." && t[t.count - 1] == "."
    }

    /// The sentence-final mark (. ? !) a deleted word carries, ignoring ellipses.
    private static func terminal(_ t: Word) -> Word {
        endsSentence(t) ? [t[t.count - 1]] : []
    }

    /// `t.replace(/[,.?!;:…]+$/, "").toLowerCase()`
    private static func bare(_ t: Word) -> String {
        var end = t.count
        while end > 0, trailing.contains(t[end - 1]) {
            end -= 1
        }
        var result = ""
        for scalar in t[..<end] {
            result += scalar.properties.lowercaseMapping
        }
        return result
    }

    /// Every listed word is ASCII, and an ASCII string is canonically equivalent only to
    /// itself, so this is JavaScript's exact `Set.has`.
    private static func contains(_ set: Set<String>, _ word: String) -> Bool {
        word.utf8.allSatisfy { $0 < 0x80 } && set.contains(word)
    }

    private static func capitalized(_ t: Word) -> Word {
        guard let first = firstUnit(t), isLower(first), !t.dropFirst().contains(where: { isUpper($0) }) else {
            return t
        }
        return Array(first.properties.uppercaseMapping.unicodeScalars) + t.dropFirst()
    }

    private static func lowercasedFirst(_ t: Word) -> Word {
        guard let first = firstUnit(t) else { return t }
        return Array(first.properties.lowercaseMapping.unicodeScalars) + t.dropFirst()
    }

    private static func stripEllipsis(_ t: Word) -> Word {
        if hasEllipsis(t) { return Array(t.dropLast(3)) }
        if t.last == "\u{2026}" { return Array(t.dropLast()) }
        return t
    }

    private static func trimming(_ t: Word, _ set: Set<Unicode.Scalar>) -> Word {
        var end = t.count
        while end > 0, set.contains(t[end - 1]) {
            end -= 1
        }
        return Array(t[..<end])
    }

    private static func string(_ t: Word) -> String {
        var view = String.UnicodeScalarView()
        view.append(contentsOf: t)
        return String(view)
    }
}
