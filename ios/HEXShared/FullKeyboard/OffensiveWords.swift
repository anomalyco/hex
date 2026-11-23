import Foundation
import HexKeyboardCore

/// Words the keyboard never suggests or corrects to (`never-suggest.tsv`, beside this file:
/// LDNOOBW, curated by Hex, the same file as Hex for Android's): slurs always, swear words
/// unless Settings › Keyboard › Suggest swear words is on, as Apple's keyboard never offers
/// them. Checked when a suggestion is looked up, not removed from the word models, so
/// spatial autocorrect still knows they're words and a word typed in full stays as typed.
/// The keyboard core reads the list (`hk_never_suggest_*`); this is its Swift face.
nonisolated struct OffensiveWords: Sendable {
    enum Kind: Equatable, Sendable {
        /// Never offered, whatever the setting.
        case slur
        /// Offered only with Suggest swear words on.
        case swear
    }

    private let list: KeyboardCoreNeverSuggest?
    /// The list's language column ("en", "fr"; others read the shared `*` lines).
    let language: String

    /// The kind of a spelling or two-word expression, any case.
    func kind(of word: String) -> Kind? {
        guard let list else { return nil }
        return switch hk_never_suggest_kind(list.pointer, language, word) {
        case 1: .slur
        case 2: .swear
        default: nil
        }
    }

    /// Whether `candidate` (a word, or words split by spaces or hyphens) may not be offered:
    /// it, one of its words, or an expression it makes (with `previous`, the word before it),
    /// is a slur, or a swear word while `allowSwearing` is off. An expression's words alone
    /// are fine ("blow", "job").
    func blocks(_ candidate: String, allowSwearing: Bool, after previous: String? = nil) -> Bool {
        guard let list else { return false }
        return hk_never_suggest_blocks(list.pointer, language, candidate, allowSwearing ? 1 : 0, previous) == 1
    }

    /// The spellings and two-word expressions to block, as `blocks` would.
    func blocked(allowSwearing: Bool) -> [String] {
        guard let list else { return [] }
        var out = HkStringList()
        guard hk_never_suggest_blocked(list.pointer, language, allowSwearing ? 1 : 0, &out) == 0 else { return [] }
        return KeyboardCoreText.take(&out)
    }

    /// The list for a keyboard layout's language, by its base language ("fr" for French
    /// (Canada) too), from the bundle (the keyboard's, or the app's for its keyboard preview).
    @MainActor static func bundled(_ language: String) -> OffensiveWords {
        OffensiveWords(list: bundledList, language: KeyboardLanguage(rawValue: language)?.baseLanguage ?? language)
    }

    /// The bundled file as the core reads it, made once per process.
    @MainActor static let bundledList: KeyboardCoreNeverSuggest? = Bundle.main
        .url(forResource: "never-suggest", withExtension: "tsv")
        .flatMap { try? String(contentsOf: $0, encoding: .utf8) }
        .flatMap { KeyboardCoreNeverSuggest(tsv: $0) }
}
