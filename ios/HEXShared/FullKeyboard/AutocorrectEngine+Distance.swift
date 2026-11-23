import Foundation
import HexKeyboardCore

/// What autocorrect answers from text and the keys alone, without an engine: the keyboard
/// core's (`hk_text*`, `hk_edit_cost`, `hk_adjacent_keys`, `hk_language_words`).
@MainActor
extension AutocorrectEngine {
    /// The keyboard cost of a slip from `source` to `target` within one Damerau-Levenshtein
    /// edit on `language`'s keys in `letters`' arrangement; nil when they're further apart.
    static func singleEditKeyboardCost(
        from source: String,
        to target: String,
        language: KeyboardLanguage = .english,
        letters: KeyboardLetterLayout? = nil
    ) -> Double? {
        var cost = Double.nan
        let status = grid(language, letters).withUnsafeBufferPointer { cells in
            hk_edit_cost(source, target, language.rawValue, cells.baseAddress, cells.count, &cost)
        }
        return status == 0 && !cost.isNaN ? cost : nil
    }

    /// Whether the keys of `a` and `b` neighbour on `language`'s keys in `letters`' arrangement.
    static func areAdjacentKeys(
        _ a: Character,
        _ b: Character,
        language: KeyboardLanguage = .english,
        letters: KeyboardLetterLayout? = nil
    ) -> Bool {
        grid(language, letters).withUnsafeBufferPointer { cells in
            hk_adjacent_keys(String(a), String(b), cells.baseAddress, cells.count) == 1
        }
    }

    /// The letter grid a layout shows, made once per layout.
    private static func grid(_ language: KeyboardLanguage, _ letters: KeyboardLetterLayout?) -> [HkKeyCentre] {
        let key = "\(language.rawValue)/\(language.offered(letters))"
        if let cells = grids[key] { return cells }
        let cells = KeyboardCoreAutocorrect.cells(language, letters)
        grids[key] = cells
        return cells
    }

    private static var grids: [String: [HkKeyCentre]] = [:]

    /// The word before `token` at the end of `before`, lowercased, when only spaces separate
    /// them; nil at the start or after punctuation.
    static func previousWord(_ before: String, token: String) -> String? {
        KeyboardCoreText.text(1, before, token)
    }

    /// Whether `token`, before a period, is an abbreviation the period doesn't correct ("etc").
    static func isAbbreviation(_ token: String) -> Bool {
        hk_text_is(3, token, nil, nil) == 1
    }

    /// The English contractions Hex corrects to or offers, for Swipe typing's extra words.
    static let englishContractions: [String] = KeyboardCoreText.languageWords(KeyboardLanguage.english.rawValue, 3)
}
