// The autocorrect oracle for the Rust port's parity fixture (`AutocorrectParityExport`):
// `+Vocabulary`, a copy of `AutocorrectEngine+Vocabulary.swift` as of dev commit d6efeddb, renamed (`ACOracle`,
// `TapOracle`, `ACO…`) so it runs on its own copies, independent of the app's. Changed from
// the copy: the dictionary is `ACOChecking` (no `SystemTextChecker`), learned words live in
// `ACOLearnedWords` (an in-memory copy of `LearnedWordsStore`), the bundled lists are read
// from the repository, and the two orders the owner fixed in the port go through
// `ACOracle.orderedValues` and `ACOOffensiveWords.bundled`. Delete with the exporter once iOS
// runs the Rust autocorrect.

import Foundation
@testable import HEX

/// Hex's vocabulary list (`CustomVocabularySettings.terms`) as the keys use it: programmer
/// slang, names and other words the dictionary doesn't know are kept as typed, spelled your
/// way ("github" -> "GitHub"), completed in the strip and read by Swipe typing.
@MainActor
extension ACOracle {
    /// The words your vocabulary adds to your own: each term, and each word of a phrase.
    static func vocabularyWords(_ terms: [String]) -> [String] {
        var words: [String] = []
        for term in terms {
            words.append(term)
            if term.contains(" ") { words += term.split(separator: " ").map(String.init) }
        }
        return words
    }

    /// The vocabulary word `token` spells, whatever its capitals; nil when it isn't one.
    func vocabularyTerm(for token: String) -> String? {
        guard !vocabulary.isEmpty else { return nil }
        return Self.vocabularyWords(vocabulary).first { $0.caseInsensitiveCompare(token) == .orderedSame }
    }

    /// Whether `word` is in your vocabulary spelled exactly so, capitals and all: it keeps
    /// them anywhere in a sentence, as you added it.
    func isVocabularySpelling(_ word: String) -> Bool {
        !vocabulary.isEmpty && Self.vocabularyWords(vocabulary).contains(word)
    }

    /// What space puts in for `token`, a vocabulary word: your spelling, with a capital you
    /// typed first ("github" -> "GitHub", "Kubectl" stays); nil when that is what you typed.
    static func vocabularySpelling(of token: String, as term: String) -> String? {
        let spelled = matchCase(of: token, onto: term)
        return spelled == token ? nil : spelled
    }

    /// Your vocabulary's first term that starts with `token` (two letters or more) and is
    /// longer, cased as `token` starts.
    func vocabularyCompletion(for token: String) -> String? {
        guard token.count >= 2, !vocabulary.isEmpty else { return nil }
        let lower = token.lowercased()
        guard let term = vocabulary.first(where: { $0.count > token.count && $0.lowercased().hasPrefix(lower) }) else { return nil }
        return Self.matchCase(of: token, onto: term)
    }
}
