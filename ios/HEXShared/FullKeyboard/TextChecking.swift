import UIKit

/// Narrow seam over `UITextChecker` so unit tests can run deterministically or against UIKit's dictionary.
@MainActor
protocol TextChecking: AnyObject {
    func isWordValid(_ word: String) -> Bool
    func guesses(for word: String) -> [String]
    func completions(for prefix: String) -> [String]
    /// Switches the dictionary (a `UITextChecker` language code such as `nb`).
    func useLanguage(_ code: String)
}

extension TextChecking {
    func useLanguage(_ code: String) {}
}

@MainActor
final class SystemTextChecker: TextChecking {
    private var checker: UITextChecker?
    private var language: String

    init(language: String = "en_US") {
        self.language = language
    }

    /// Hex's own words for the language Spatial autocorrect reads (`AutocorrectEngine.useOwnWords`):
    /// while set, they alone say what is a word, and `UITextChecker`'s guesses and completions
    /// count only while it is in that language (iOS may have no dictionary for it).
    var ownWords: HexOwnWords?

    func useLanguage(_ code: String) {
        language = code
    }

    /// Warms `UITextChecker` after the keyboard's first frame commits so cold open stays < 12 ms.
    func warmUp() {
        guard checker == nil else { return }
        let instance = UITextChecker()
        _ = instance.rangeOfMisspelledWord(
            in: "hello",
            range: NSRange(location: 0, length: 5),
            startingAt: 0,
            wrap: false,
            language: language
        )
        checker = instance
    }

    private var activeChecker: UITextChecker {
        if let checker { return checker }
        let instance = UITextChecker()
        checker = instance
        return instance
    }

    func isWordValid(_ word: String) -> Bool {
        if let ownWords { return ownWords.knows(word) }
        let ns = word as NSString
        let range = NSRange(location: 0, length: ns.length)
        guard range.length > 0 else { return true }
        let misspelled = activeChecker.rangeOfMisspelledWord(
            in: word,
            range: range,
            startingAt: 0,
            wrap: false,
            language: language
        )
        return misspelled.location == NSNotFound
    }

    private var speaksOwnLanguage: Bool {
        ownWords.map { language.hasPrefix($0.language) } ?? true
    }

    func guesses(for word: String) -> [String] {
        guard speaksOwnLanguage else { return [] }
        let ns = word as NSString
        let range = NSRange(location: 0, length: ns.length)
        guard range.length > 0 else { return [] }
        return activeChecker.guesses(forWordRange: range, in: word, language: language) ?? []
    }

    func completions(for prefix: String) -> [String] {
        guard speaksOwnLanguage else { return [] }
        let ns = prefix as NSString
        let range = NSRange(location: 0, length: ns.length)
        guard range.length > 0 else { return [] }
        return activeChecker.completions(forPartialWordRange: range, in: prefix, language: language) ?? []
    }
}
