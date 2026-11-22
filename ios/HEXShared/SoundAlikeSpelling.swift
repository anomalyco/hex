import Foundation

/// Custom vocabulary's sound-alike spellings, English only: a transcribed name that sounds
/// like a listed term and is spelled nearly the same takes the listed spelling ("Stacy" →
/// "Stacey", "John" → "Jon", "Sarah's" → "Sara's"). Speech engines write a name's commonest
/// spelling, so `VocabularySpelling`'s exact match never reached a list entry spelled any
/// other way. Runs after it, on every engine.
///
/// A term qualifies if it is one word of at least three letters that starts with a capital
/// (a name or a product, never a lowercase word like "kubectl"). A transcript word is a
/// candidate only if the engine capitalized it and it isn't itself listed. It takes a term
/// when, after the interchangeable spellings `folded` evens out, it is spelled the same
/// ("Catherine" and "Katherine" both read "katherine"). Inside a sentence, where only a name
/// keeps a capital, and not right after another capitalized word (a title such as "A Study in
/// Scarlet"), it may instead sound the same by `SoundKey` and be at most two edits away
/// ("Jeff" → "Geoff", "Versal" → "Vercel"). Two terms that fit equally well leave the
/// word as heard. Self-contained, so it ports to the shared core as it is.
nonisolated enum SoundAlikeSpelling {
    private static let maxEdits = 2

    static func apply(_ terms: [String], to text: String, language: String) -> String {
        guard language.lowercased().hasPrefix("en"), !text.isEmpty else { return text }
        let listed = Set(terms.map(letters))
        let candidates = terms.compactMap(Term.init)
        guard !candidates.isEmpty else { return text }

        var output = ""
        var copied = text.startIndex
        let all = words(in: text)
        for (index, word) in all.enumerated() {
            let opensSentence = startsSentence(word.range.lowerBound, in: text)
            let followsCapital = index > 0 && !opensSentence
                && all[index - 1].stem.first?.isUppercase == true
                && !startsSentence(all[index - 1].range.lowerBound, in: text)
            guard let term = match(word, candidates: candidates, listed: listed, soundsAlike: !opensSentence && !followsCapital)
            else { continue }
            output.append(contentsOf: text[copied..<word.range.lowerBound])
            output.append(term)
            output.append(word.possessive)
            copied = word.range.upperBound
        }
        output.append(contentsOf: text[copied...])
        return output
    }

    private struct Term {
        let text: String
        let letters: String
        let folded: String
        let key: SoundKey

        init?(_ term: String) {
            let trimmed = term.trimmingCharacters(in: .whitespaces)
            guard trimmed.count >= 3, trimmed.allSatisfy(\.isLetter),
                  trimmed.first?.isUppercase == true else { return nil }
            text = trimmed
            letters = SoundAlikeSpelling.letters(trimmed)
            guard letters.count >= 3 else { return nil }
            folded = SoundAlikeSpelling.folded(letters)
            key = SoundKey(letters)
        }
    }

    private struct Word {
        let range: Range<String.Index>
        let stem: String
        let possessive: String
    }

    private static func match(_ word: Word, candidates: [Term], listed: Set<String>, soundsAlike: Bool) -> String? {
        let stem = word.stem
        guard stem.count >= 3, stem.allSatisfy(\.isLetter), stem.first?.isUppercase == true,
              !(stem.count > 1 && stem == stem.uppercased()) else { return nil }
        let heard = letters(stem)
        guard !listed.contains(heard) else { return nil }
        let heardFolded = folded(heard)
        let heardKey = SoundKey(heard)

        // Rank: spelled the same once folded (0), else sounds the same (1 + edits).
        var best: (term: String, rank: Int)?
        var tied = false
        for term in candidates {
            let rank: Int
            if term.folded == heardFolded {
                rank = 0
            } else if soundsAlike, term.key == heardKey,
                      let edits = editDistance(heard, term.letters, limit: maxEdits) {
                rank = 1 + edits
            } else {
                continue
            }
            if let current = best, rank >= current.rank {
                if rank == current.rank, term.text != current.term { tied = true }
                continue
            }
            best = (term.text, rank)
            tied = false
        }
        return tied ? nil : best?.term
    }

    // MARK: - Spelling

    /// Lowercase ASCII letters, diacritics removed.
    static func letters(_ word: String) -> String {
        String(word.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: nil)
            .lowercased().filter { ("a"..."z").contains($0) })
    }

    /// Evens out spellings English names write interchangeably: "ph" is "f", "ck" and a hard
    /// "c" are "k", "chr" is "kr", a soft "c" is "s", "ae" is "e", a leading "ai" is "a",
    /// a final "y", "ey", "ie", "ee" or "i" is "i", any other "y" is "i", a silent "h"
    /// (after a vowel before a consonant or the end, or after g, k, r, d, b, j or w) goes,
    /// and doubled letters are single.
    static func folded(_ word: String) -> String {
        var s = Array(letters(word))
        func replace(_ from: String, _ to: String) {
            let f = Array(from), t = Array(to)
            var out: [Character] = []
            var i = 0
            while i < s.count {
                if i + f.count <= s.count, Array(s[i..<(i + f.count)]) == f {
                    out += t
                    i += f.count
                } else {
                    out.append(s[i])
                    i += 1
                }
            }
            s = out
        }
        replace("ph", "f")
        replace("chr", "kr")
        replace("ck", "k")
        if s.starts(with: ["a", "i"]) { s.remove(at: 1) }
        for (i, c) in s.enumerated() where c == "c" {
            let next = i + 1 < s.count ? s[i + 1] : nil
            if next == "h" { continue }
            s[i] = next.map { "eiy".contains($0) } == true ? "s" : "k"
        }
        replace("ae", "e")
        for ending in ["ey", "ie", "ee"] where s.count > 3 && String(s.suffix(2)) == ending {
            s.removeLast(2)
            s.append("i")
        }
        s = s.map { $0 == "y" ? "i" : $0 }
        var kept: [Character] = []
        for (i, c) in s.enumerated() {
            if c == "h", i > 0 {
                let previous = s[i - 1]
                let next = i + 1 < s.count ? s[i + 1] : nil
                if isVowel(previous), next.map(isVowel) != true { continue }
                if "gkrdbjw".contains(previous) { continue }
            }
            if let last = kept.last, last == c { continue }
            kept.append(c)
        }
        return String(kept)
    }

    /// How a word sounds, roughly: Metaphone's consonants (Lawrence Philips, 1990) with
    /// voicing kept (d, g and v stay apart from t, k and f), "chr" read as "kr" and "y" read
    /// as a vowel after the first letter; plus the first vowel sound, whether the word ends
    /// on a vowel sound, and how many vowel groups it has.
    struct SoundKey: Equatable {
        let consonants: String
        let firstVowel: String
        let endsOnVowel: Bool
        let syllables: Int

        init(_ word: String) {
            let w = Array(SoundAlikeSpelling.letters(word))
            consonants = SoundAlikeSpelling.consonants(w)
            var start = 0
            while start < w.count, !(isVowel(w[start]) || (w[start] == "y" && start > 0)) { start += 1 }
            var end = start
            while end < w.count, isVowel(w[end]) || w[end] == "y" { end += 1 }
            firstVowel = Self.vowelSound(String(w[start..<end]))
            var tail = w
            if tail.count > 2, tail.last == "h", isVowel(tail[tail.count - 2]) { tail.removeLast() }
            if let last = tail.last {
                endsOnVowel = "aiouy".contains(last)
                    || (last == "e" && tail.count > 1 && isVowel(tail[tail.count - 2]))
            } else {
                endsOnVowel = false
            }
            var groups = 0
            var inGroup = false
            for (i, c) in tail.enumerated() {
                let vowel = isVowel(c) || (c == "y" && i > 0)
                if vowel, !inGroup { groups += 1 }
                inGroup = vowel
            }
            if tail.count > 2, tail.last == "e", !isVowel(tail[tail.count - 2]), groups > 1 { groups -= 1 }
            syllables = groups
        }

        private static let sounds: [String: String] = [
            "a": "a", "ai": "a", "ay": "a", "ae": "a", "aa": "a",
            "e": "e", "ee": "e", "ea": "e", "ei": "e", "ey": "e", "eo": "e",
            "i": "i", "y": "i", "ia": "i", "ya": "i", "ie": "i", "yi": "i", "io": "i", "yo": "i",
            "o": "o", "oa": "o", "oe": "o", "au": "o", "aw": "o",
            "oi": "oi", "oy": "oi", "ou": "ou", "ow": "ou",
            "u": "u", "ue": "u", "oo": "u", "ui": "u",
        ]

        private static func vowelSound(_ group: String) -> String {
            sounds[group] ?? sounds[String(group.prefix(2))] ?? sounds[String(group.prefix(1))] ?? ""
        }
    }

    private static func consonants(_ letters: [Character]) -> String {
        var w = letters
        for (prefix, replacement) in [("kn", "n"), ("gn", "n"), ("pn", "n"), ("ae", "e"), ("wr", "r")]
        where w.starts(with: Array(prefix)) {
            w = Array(replacement) + w.dropFirst(prefix.count)
            break
        }
        if w.first == "x" { w[0] = "s" }
        if w.starts(with: ["w", "h"]) { w.remove(at: 1) }
        var single: [Character] = []
        for c in w where !(single.last == c && c != "c") { single.append(c) }
        w = single

        func at(_ i: Int) -> Character? { i >= 0 && i < w.count ? w[i] : nil }
        func vowel(_ c: Character?) -> Bool { c.map { isVowel($0) || $0 == "y" } ?? false }
        func soft(_ c: Character?) -> Bool { c.map { "eiy".contains($0) } ?? false }
        var out: [Character] = []
        for (i, c) in w.enumerated() {
            let next = at(i + 1), previous = at(i - 1)
            if isVowel(c) || (c == "y" && i > 0) {
                if i == 0 { out.append(c) }
                continue
            }
            switch c {
            case "b":
                if !(previous == "m" && i == w.count - 1) { out.append("b") }
            case "c":
                if next == "h" {
                    out.append(previous == "s" || at(i + 2) == "r" ? "k" : "x")
                } else if next == "i", at(i + 2) == "a" {
                    out.append("x")
                } else if soft(next) {
                    if previous != "s" { out.append("s") }
                } else {
                    out.append("k")
                }
            case "d":
                out.append(next == "g" && soft(at(i + 2)) ? "j" : "d")
            case "g":
                if next == "h", !(i + 2 >= w.count || vowel(at(i + 2))) { continue }
                if next == "n", i + 2 == w.count { continue }
                if previous == "d", soft(next) { continue }
                out.append(soft(next) ? "j" : "g")
            case "h":
                if let previous, "cspgt".contains(previous) { continue }
                if i > 0, !vowel(next) { continue }
                out.append("h")
            case "k":
                if previous != "c" { out.append("k") }
            case "p":
                out.append(next == "h" ? "f" : "p")
            case "q":
                out.append("k")
            case "s":
                out.append(next == "h" || (next == "i" && (at(i + 2) == "o" || at(i + 2) == "a")) ? "x" : "s")
            case "t":
                if next == "i", at(i + 2) == "o" || at(i + 2) == "a" {
                    out.append("x")
                } else if next == "h" {
                    out.append("0")
                } else if !(next == "c" && at(i + 2) == "h") {
                    out.append("t")
                }
            case "w":
                if vowel(next) { out.append("w") }
            case "x":
                out += ["k", "s"]
            case "y":
                if vowel(next) { out.append("y") }
            case "z":
                out.append("s")
            default:
                out.append(c)
            }
        }
        var key: [Character] = []
        for c in out where key.last != c { key.append(c) }
        return String(key)
    }

    private static func isVowel(_ c: Character) -> Bool {
        "aeiou".contains(c)
    }

    /// Edits (insert, delete, substitute, swap two neighbours) from `a` to `b`, or nil past `limit`.
    private static func editDistance(_ a: String, _ b: String, limit: Int) -> Int? {
        let a = Array(a), b = Array(b)
        guard abs(a.count - b.count) <= limit else { return nil }
        var d = [[Int]](repeating: [Int](repeating: 0, count: b.count + 1), count: a.count + 1)
        for i in 0...a.count { d[i][0] = i }
        for j in 0...b.count { d[0][j] = j }
        for i in stride(from: 1, through: a.count, by: 1) {
            for j in stride(from: 1, through: b.count, by: 1) {
                d[i][j] = min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] == b[j - 1] ? 0 : 1))
                if i > 1, j > 1, a[i - 1] == b[j - 2], a[i - 2] == b[j - 1] {
                    d[i][j] = min(d[i][j], d[i - 2][j - 2] + 1)
                }
            }
        }
        return d[a.count][b.count] <= limit ? d[a.count][b.count] : nil
    }

    // MARK: - Words

    private static func words(in text: String) -> [Word] {
        var words: [Word] = []
        var start: String.Index?
        for index in text.indices {
            let c = text[index]
            if c.isLetter || c.isNumber || c == "'" || c == "’" {
                if start == nil { start = index }
            } else if let s = start {
                append(s..<index, of: text, to: &words)
                start = nil
            }
        }
        if let s = start { append(s..<text.endIndex, of: text, to: &words) }
        return words
    }

    private static func append(_ range: Range<String.Index>, of text: String, to words: inout [Word]) {
        var word = text[range]
        while let last = word.last, last == "'" || last == "’" { word = word.dropLast() }
        guard let first = word.first, first.isLetter else { return }
        var stem = String(word), possessive = ""
        for suffix in ["'s", "’s"] where stem.hasSuffix(suffix) {
            stem.removeLast(2)
            possessive = suffix
        }
        guard !stem.contains("'"), !stem.contains("’") else { return }
        let end = text.index(range.lowerBound, offsetBy: stem.count + possessive.count)
        words.append(Word(range: range.lowerBound..<end, stem: stem, possessive: possessive))
    }

    /// Whether only spaces, quotes or an opening bracket separate the word from the text's
    /// start or the end of a sentence, where every word has a capital.
    private static func startsSentence(_ index: String.Index, in text: String) -> Bool {
        var i = index
        while i > text.startIndex {
            i = text.index(before: i)
            let c = text[i]
            if c == " " || c == "\t" || "\"“”'‘(".contains(c) { continue }
            return ".!?…:\n".contains(c)
        }
        return true
    }
}
