import Foundation

/// Words the keyboard never suggests or corrects to (`never-suggest.tsv`, beside this file:
/// LDNOOBW, curated by Hex, the same list as Hex for Android's): slurs always, swear words
/// unless Settings › Keyboard › Suggest swear words is on, as Apple's keyboard never offers
/// them. Checked when a suggestion is looked up, not removed from the word models, so
/// spatial autocorrect still knows they're words and a word typed in full stays as typed.
nonisolated struct OffensiveWords: Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        /// Never offered, whatever the setting.
        case slur
        /// Offered only with Suggest swear words on.
        case swear
    }

    /// Lowercased spellings and two-word expressions (one space between), accents kept.
    private(set) var entries: [String: Kind] = [:]

    init(entries: [String: Kind] = [:]) {
        self.entries = entries
    }

    /// Reads `layout<TAB>slur|swear<TAB>spelling` lines for `language`, and the `*` lines
    /// every layout shares; `#` starts a comment.
    init(tsv: String, language: String) {
        var entries: [String: Kind] = [:]
        for line in tsv.split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
            let fields = line.split(separator: "\t", omittingEmptySubsequences: false)
            guard fields.count == 3, fields[0] == "*" || fields[0] == language[...] else { continue }
            let kind: Kind
            switch fields[1] {
            case "slur": kind = .slur
            case "swear": kind = .swear
            default: continue
            }
            let spelling = fields[2].lowercased().split(separator: " ").joined(separator: " ")
            guard !spelling.isEmpty else { continue }
            if entries[spelling] != .slur { entries[spelling] = kind }
        }
        self.init(entries: entries)
    }

    var isEmpty: Bool { entries.isEmpty }

    /// The kind of a spelling or two-word expression, by its lowercase.
    func kind(of word: String) -> Kind? {
        entries[word.lowercased()]
    }

    /// Whether `candidate` (a word, or words split by spaces or hyphens) may not be offered:
    /// it, one of its words, or an expression it makes (with `previous`, the word before it),
    /// is a slur, or a swear word while `allowSwearing` is off. An expression's words alone
    /// are fine ("blow", "job").
    func blocks(_ candidate: String, allowSwearing: Bool, after previous: String? = nil) -> Bool {
        func blocked(_ text: String) -> Bool {
            switch kind(of: text) {
            case .slur: true
            case .swear: !allowSwearing
            case nil: false
            }
        }
        let parts = candidate.split(whereSeparator: { $0 == " " || $0 == "-" }).map(String.init)
        guard let first = parts.first else { return false }
        if parts.contains(where: blocked) { return true }
        if let previous, blocked(previous + " " + first) { return true }
        return zip(parts, parts.dropFirst()).contains { blocked($0 + " " + $1) }
    }

    /// The spellings and two-word expressions to block, as `blocks` would.
    func blocked(allowSwearing: Bool) -> [String] {
        entries.compactMap { word, kind in kind == .slur || !allowSwearing ? word : nil }
    }

    /// The list for a keyboard layout's language ("en", "es", "de", "nb"; the shared `*` lines
    /// for the others), read once per process from the bundle (the keyboard's, or the app's
    /// for its keyboard preview).
    @MainActor static func bundled(_ language: String) -> OffensiveWords {
        if let cached = loaded[language] { return cached }
        let words = OffensiveWords(tsv: bundledTSV, language: language)
        loaded[language] = words
        return words
    }

    @MainActor private static var loaded: [String: OffensiveWords] = [:]

    @MainActor private static let bundledTSV: String = Bundle.main.url(forResource: "never-suggest", withExtension: "tsv")
        .flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
}
