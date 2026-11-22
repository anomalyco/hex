import Foundation

/// Settings › Experiments › Emoji shortcodes: the token right before the cursor and the emoji
/// it names. A token is a colon and a name (":pizza") after whitespace or at the start of the
/// field, so a colon inside a word ("abc:pizza") is not one. Matching reads the emoji index's
/// own words (`EmojiLibrary`, built by scripts/build-emoji-data.py from Unicode's names and
/// Hex's chat words): an exact name or alias comes first, then the names and keywords the
/// token starts, with your use breaking exact ties and moving prefix matches up.
nonisolated enum EmojiShortcode {
    /// The shortest name a token holds: two letters or digits, so ":pi" suggests and ":p" is
    /// just a colon.
    static let minimumNameLength = 2

    /// A ":name" right before the cursor, as typed.
    struct Token: Equatable, Sendable {
        /// The name after the colon, as typed.
        let name: String
        /// The whole token, ":name", which a pick takes back.
        let text: String

        /// The name in the emoji index's words: lowercased, with "_", "+" and "-" as spaces,
        /// since the index splits chat words that way ("sweat_smile" is "sweat smile" there).
        var lookupName: String {
            name.lowercased().split { $0 == "_" || $0 == "+" || $0 == "-" }.joined(separator: " ")
        }
    }

    /// The emoji an exact name names, and the token it replaces.
    struct Completion: Equatable, Sendable {
        let token: Token
        let entry: Int
    }

    /// What a closing ":" or a strip tap puts in a token's place.
    struct Replacement: Equatable, Sendable {
        /// The ":name" as typed.
        let token: String
        let emoji: String
    }

    // MARK: - The token

    /// The token right before the cursor, or nil when the text doesn't end in one: a colon and
    /// a name after whitespace or at the start of the field. The name starts with a letter or
    /// digit and holds at least two of them, then only letters, digits, "_", "+" and "-"
    /// (case-insensitive), so the shapes of chat emoticons (":-D", ":+1") stay text.
    static func token(before text: String?) -> Token? {
        guard let text else { return nil }
        var index = text.endIndex
        var typed: [Character] = []
        while index > text.startIndex {
            let previous = text.index(before: index)
            let character = text[previous]
            guard isNameCharacter(character) else { break }
            typed.append(character)
            index = previous
        }
        guard index > text.startIndex else { return nil }
        let colonIndex = text.index(before: index)
        guard text[colonIndex] == ":" else { return nil }
        if colonIndex > text.startIndex, !text[text.index(before: colonIndex)].isWhitespace { return nil }
        let name = String(typed.reversed())
        guard let first = name.first, isNameStart(first),
              name.filter({ $0.isLetter || $0.isNumber }).count >= minimumNameLength else { return nil }
        return Token(name: name, text: ":" + name)
    }

    /// A name's first character: a letter or digit, never "_", "+" or "-".
    private static func isNameStart(_ character: Character) -> Bool {
        character.isASCII && (character.isLetter || character.isNumber)
    }

    /// A name's other characters: ASCII letters, digits, "_", "+" and "-".
    private static func isNameCharacter(_ character: Character) -> Bool {
        guard character.isASCII else { return false }
        return character.isLetter || character.isNumber || character == "_" || character == "+" || character == "-"
    }

    // MARK: - Matching

    /// The strip's emoji for the token before the cursor, best first: nothing while the
    /// experiment is off, when there is no token, or when no emoji matches. One emoji per
    /// family, so a running man and a running woman don't take two slots.
    static func matches(
        before text: String?,
        enabled: Bool,
        library: EmojiLibrary,
        usage: [Int: Float] = [:],
        limit: Int = 3
    ) -> [Int] {
        guard enabled, let token = token(before: text) else { return [] }
        return matches(token: token, library: library, usage: usage, limit: limit)
    }

    static func matches(
        token: Token,
        library: EmojiLibrary,
        usage: [Int: Float] = [:],
        limit: Int = 3
    ) -> [Int] {
        var picked: [Int] = []
        var seen: Set<Int> = []
        var families: Set<Int32> = []
        func take(_ entry: Int) {
            guard picked.count < limit, seen.insert(entry).inserted else { return }
            let family = library.family(of: entry)
            guard family < 0 || families.insert(family).inserted else { return }
            picked.append(entry)
        }
        for hit in exactHits(for: token, library: library, usage: usage) {
            take(Int(hit.entry))
        }
        guard picked.count < limit else { return picked }
        // The names and keywords the token starts, the index's own order and your use deciding.
        for entry in library.search(token.lookupName, limit: limit + 8, boost: { usage[$0] ?? 0 }) {
            take(entry)
        }
        return picked
    }

    /// The exact emoji a complete name before the cursor names, when its closing ":" arrives:
    /// only a name or alias, never a prefix.
    static func completion(
        before text: String?,
        library: EmojiLibrary,
        usage: [Int: Float] = [:]
    ) -> Completion? {
        guard let token = token(before: text),
              let hit = exactHits(for: token, library: library, usage: usage).first else { return nil }
        return Completion(token: token, entry: Int(hit.entry))
    }

    /// The emoji whose name or alias is exactly the token's, strongest first: a one-word name
    /// scores 1, an alias 0.9 to 1.2, and keywords (`strongWeight` and under) are not names.
    private static func exactHits(
        for token: Token,
        library: EmojiLibrary,
        usage: [Int: Float]
    ) -> [EmojiLibrary.Hit] {
        let name = token.lookupName
        guard !name.isEmpty else { return [] }
        let hits = name.contains(" ") ? library.hits(forPhrase: name) : library.hits(forWord: name)
        return (hits ?? [])
            .filter { $0.weight >= EmojiSuggester.strongWeight }
            .sorted { lhs, rhs in
                if lhs.weight != rhs.weight { return lhs.weight > rhs.weight }
                let left = usage[Int(lhs.entry)] ?? 0
                let right = usage[Int(rhs.entry)] ?? 0
                return left != right ? left > right : lhs.entry < rhs.entry
            }
    }
}
