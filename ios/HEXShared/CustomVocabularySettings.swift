import Foundation

/// The custom vocabulary experiment's switch and terms in the shared App Group.
nonisolated struct CustomVocabularySettings {
    static let maxTermCount = 100
    static let maxTermLength = 64

    private enum Key {
        static let enabled = "experiments.customVocabulary"
        static let terms = "customVocabulary.terms"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Experiment: listed terms are spelled Hex's way after dictation. Off by default.
    var isEnabled: Bool {
        get { defaults?.bool(forKey: Key.enabled) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.enabled) }
    }

    /// The saved terms, always sanitized in the order they were added.
    var terms: [String] {
        get { Self.sanitize(defaults?.stringArray(forKey: Key.terms) ?? []) }
        nonmutating set { defaults?.set(Self.sanitize(newValue), forKey: Key.terms) }
    }

    /// Empty while the experiment is off, so dictation stays exactly as it is today.
    var activeTerms: [String] {
        isEnabled ? terms : []
    }

    /// Trims each term, drops blanks, terms whose letters and digits are shorter
    /// than two characters, duplicates of the same spelling key, terms longer than
    /// 64 characters, and anything past Apple's 100 contextual-string limit.
    static func sanitize(_ rawTerms: [String]) -> [String] {
        var seen = Set<String>()
        var result: [String] = []

        for rawTerm in rawTerms {
            let term = rawTerm.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !term.isEmpty, term.count <= maxTermLength else { continue }

            let key = VocabularySpelling.normalizedKey(term)
            guard key.count >= 2, seen.insert(key).inserted else { continue }

            result.append(term)
            if result.count == maxTermCount {
                break
            }
        }

        return result
    }
}
