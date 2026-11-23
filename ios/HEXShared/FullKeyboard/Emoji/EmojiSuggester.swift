import Foundation

/// The words before the cursor, newest last, as the emoji suggester reads them.
nonisolated struct EmojiContext: Equatable, Sendable {
    struct Word: Equatable, Sendable {
        let text: String
        /// The newest word counts most: 1, then 0.85, 0.72, ...
        let weight: Float
    }

    /// Words that aren't stopwords, newest last.
    let words: [Word]
    /// Every folded word of the tail, stopwords included, for phrases ("good luck").
    let tokens: [String]
    /// The word still being typed (no space after it yet), for prefix matches.
    let partial: String?

    static let maxWords = 8

    init(words: [Word], tokens: [String], partial: String?) {
        self.words = words
        self.tokens = tokens
        self.partial = partial
    }

    /// Reads the end of `before` (hosts show a few hundred characters at most).
    init(before: String?, stopwords: Set<String>) {
        let tail = String((before ?? "").suffix(240))
        let tokens = Array(EmojiText.words(in: tail).suffix(24))
        let endsInWord = tail.last.map { $0.isLetter || $0.isNumber } ?? false
        var content: [Word] = []
        var rank = 0
        for token in tokens.reversed() where !stopwords.contains(token) && token.count >= 2 {
            content.append(Word(text: token, weight: pow(0.85, Float(rank))))
            rank += 1
            if rank == Self.maxWords { break }
        }
        self.init(words: content.reversed(), tokens: tokens, partial: endsInWord ? tokens.last : nil)
    }

    var isEmpty: Bool { words.isEmpty && partial == nil }
}

/// Ranks emoji for the text before the cursor, on the iPhone: the words' own emoji
/// (CLDR names and keywords, Hex's chat words), then, when a semantic index is given,
/// emoji for words near them in meaning, then what you use most.
nonisolated enum EmojiSuggester {
    /// Finds vocabulary words near a word in meaning, with their similarity (0...1).
    typealias Neighbors = (String) -> [(word: String, similarity: Float)]

    static let minimumScore: Float = 0.12
    /// A word near in meaning counts this much of a word that matches.
    static let semanticScale: Float = 0.6

    /// Scores every emoji the context points at. Each word adds its best match per emoji;
    /// an emoji several words point at gains a third of the rest.
    static func contextScores(
        _ context: EmojiContext,
        library: EmojiLibrary,
        neighbors: Neighbors? = nil
    ) -> [Int: Float] {
        var perWord: [[Int: Float]] = []

        for word in context.words {
            var best: [Int: Float] = [:]
            for (form, factor) in forms(of: word.text, language: library.language) {
                guard let hits = library.hits(forWord: form) else { continue }
                for hit in hits {
                    let value = hit.weight * factor * word.weight
                    if value > best[Int(hit.entry), default: 0] { best[Int(hit.entry)] = value }
                }
                // The word itself, or its first form that has emoji, is enough.
                if !best.isEmpty { break }
            }
            // Words near in meaning only stand in for a word with no emoji of its own,
            // so they never push out what the words themselves say.
            if let neighbors, best.isEmpty {
                for (near, similarity) in neighbors(word.text) where near != word.text {
                    guard let hits = library.hits(forWord: near) else { continue }
                    for hit in hits.prefix(12) {
                        let value = hit.weight * similarity * semanticScale * word.weight
                        if value > best[Int(hit.entry), default: 0] { best[Int(hit.entry)] = value }
                    }
                }
            }
            perWord.append(best)
        }

        // Phrases of two to four words ending anywhere in the last eight.
        let tokens = context.tokens
        if tokens.count >= 2 {
            var best: [Int: Float] = [:]
            for end in stride(from: tokens.count, to: max(1, tokens.count - 8), by: -1) {
                let recency = pow(0.85, Float(tokens.count - end))
                for length in 2...max(2, min(library.longestPhrase, 4)) where end - length >= 0 {
                    let phrase = tokens[(end - length)..<end].joined(separator: " ")
                    guard let hits = library.hits(forPhrase: phrase) else { continue }
                    for hit in hits {
                        let value = hit.weight * 1.2 * recency
                        if value > best[Int(hit.entry), default: 0] { best[Int(hit.entry)] = value }
                    }
                }
            }
            perWord.append(best)
        }

        // The word being typed: names and keywords it starts.
        if let partial = context.partial, partial.count >= 3, library.hits(forWord: partial) == nil {
            var best: [Int: Float] = [:]
            for (rank, index) in library.search(partial, limit: 12).enumerated() {
                best[index] = 0.8 - 0.03 * Float(rank)
            }
            perWord.append(best)
        }

        var total: [Int: Float] = [:]
        var strongest: [Int: Float] = [:]
        for scores in perWord {
            for (entry, value) in scores {
                total[entry, default: 0] += value
                strongest[entry] = max(strongest[entry, default: 0], value)
            }
        }
        var combined: [Int: Float] = [:]
        for (entry, sum) in total {
            let top = strongest[entry] ?? 0
            combined[entry] = top + (sum - top) / 3
        }
        return combined
    }

    /// The suggestion row: context matches (your favourites first among equals), then the
    /// emoji you use most, then everyday defaults. One emoji per family ("person running",
    /// "woman running"); the panel applies your skin tones.
    static func suggestions(
        context: EmojiContext,
        library: EmojiLibrary,
        neighbors: Neighbors? = nil,
        usage: [Int: Float] = [:],
        limit: Int = 8,
        fill: Bool = true
    ) -> [Int] {
        let scores = contextScores(context, library: library, neighbors: neighbors)
        var ranked = scores
            .filter { $0.value >= minimumScore }
            .map { (entry: $0.key, score: $0.value + 0.1 * min(1, usage[$0.key] ?? 0)) }
        ranked.sort { $0.score != $1.score ? $0.score > $1.score : $0.entry < $1.entry }

        var picked: [Int] = []
        var families: Set<Int32> = []
        func take(_ entry: Int) {
            guard picked.count < limit, !picked.contains(entry) else { return }
            let family = library.family(of: entry)
            guard family < 0 || families.insert(family).inserted else { return }
            picked.append(entry)
        }
        for item in ranked { take(item.entry) }
        guard fill else { return picked }
        for (entry, _) in usage.sorted(by: { $0.value != $1.value ? $0.value > $1.value : $0.key < $1.key }) {
            take(entry)
        }
        for emoji in defaults {
            if let entry = library.entryByEmoji[emoji] { take(entry) }
        }
        return picked
    }

    /// A finished word's own emoji for the suggestion strip ("pizza " → 🍕): only strong
    /// matches (an emoji's whole name in the keyboard's language or English, or one of Hex's
    /// chat words), at most `limit`, one per family. Nothing after a stopword, a number or
    /// punctuation.
    static func forFinishedWord(_ before: String?, library: EmojiLibrary, limit: Int = 3) -> [Int] {
        guard let before, before.hasSuffix(" ") else { return [] }
        let trimmed = before.dropLast()
        guard trimmed.last?.isLetter == true,
              let word = EmojiText.words(in: String(trimmed.suffix(40))).last else { return [] }
        return forWord(word, library: library, limit: limit)
    }

    /// A word's own emoji, as `forFinishedWord` finds them, for the word still being typed too
    /// ("smile" → 😊): `word` folded as `EmojiText.words` folds it; none for a stopword.
    static func forWord(_ word: String, library: EmojiLibrary, limit: Int = 3) -> [Int] {
        guard !library.stopwords.contains(word) else { return [] }
        for (form, factor) in forms(of: word, language: library.language) {
            guard let hits = library.hits(forWord: form) else { continue }
            // The index counts a word several emoji share for less (`build-emoji-data.py`'s
            // spread); an emoji whose whole name it is, in the keyboard's language or English,
            // still counts in full ("fuego" names 🔥 and is a keyword of ❤️‍🔥 and 🐦‍🔥).
            let spread = 1 + 0.5 * log2(max(1, Float(hits.count) / 2))
            var picked: [Int] = []
            var families: Set<Int32> = []
            for hit in hits where hit.weight * spread * factor >= strongWeight - 0.005 && picked.count < limit {
                let entry = Int(hit.entry)
                let family = library.family(of: entry)
                guard family < 0 || families.insert(family).inserted else { continue }
                picked.append(entry)
            }
            return picked
        }
        return []
    }

    /// An emoji's one-word name scores 1, a chat word's emoji 0.9 to 1.2.
    static let strongWeight: Float = 0.85

    /// The search's results: name matches first, your emoji and the everyday ones first among
    /// equals.
    static func search(_ query: String, library: EmojiLibrary, usage: [Int: Float] = [:], limit: Int = 60) -> [Int] {
        var everyday: [Int: Float] = [:]
        for emoji in defaults {
            if let entry = library.entryByEmoji[emoji] { everyday[entry] = 0.5 }
        }
        return library.search(query, limit: limit) { max(usage[$0] ?? 0, everyday[$0] ?? 0) }
    }

    /// Before you've used any: the emoji people send most, roughly.
    static let defaults = ["😂", "❤️", "👍", "😊", "🙏", "😍", "🎉", "🔥", "😭", "🥰", "😘", "😅"]

    /// A word and the forms it may be listed under, with how much each counts: itself,
    /// without a held-down letter ("soooo"), then English plurals and endings, or the
    /// common endings of the other layouts' languages.
    static func forms(of word: String, language: String) -> [(String, Float)] {
        var out: [(String, Float)] = [(word, 1)]
        func add(_ form: String, _ factor: Float = 0.85) {
            if form.count >= 2, !out.contains(where: { $0.0 == form }) { out.append((form, factor)) }
        }
        for collapsed in EmojiText.collapsingRepeats(word) { add(collapsed, 0.95) }
        guard word.count >= 4 else { return out }
        switch language {
        case "en":
            if word.hasSuffix("ies") { add(String(word.dropLast(3)) + "y") }
            if word.hasSuffix("es") { add(String(word.dropLast(2))) }
            if word.hasSuffix("s"), !word.hasSuffix("ss") { add(String(word.dropLast())) }
            for suffix in ["ing", "ed"] where word.hasSuffix(suffix) {
                let stem = String(word.dropLast(suffix.count))
                add(stem)
                add(stem + "e")
                if let last = stem.last, stem.dropLast().last == last { add(String(stem.dropLast())) }
            }
            if word.hasSuffix("y") { add(String(word.dropLast())) }
        default:
            for suffix in ["en", "er", "es", "e", "s", "n"] where word.hasSuffix(suffix) {
                add(String(word.dropLast(suffix.count)))
            }
        }
        return out
    }
}
