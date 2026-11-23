import Foundation
import HexKeyboardCore

/// French typography's spaces before `? ! ; :` and inside `« »`, the dictation pipeline's last
/// step (`DictationPipeline.format`). Apple Speech doesn't write them (black box, macOS 27.2.1:
/// fr-FR and fr-CH write a plain space before "?", fr-CA's DictationTranscriber none) and the
/// styles take out every space before a mark, so French came out as "ça va?". Apple's French
/// keyboards don't add one either (iOS 27.2 simulator), so the full keyboard types marks as you
/// tap them.
///
/// - France (and every French but Canada's and Switzerland's): a narrow no-break space
///   (U+202F) before `? ! ;`, a no-break space (U+00A0) before `:` and inside `« »`.
/// - Canada (OQLF): no space before `? ! ;`, a no-break space before `:` and inside `« »`.
/// - Switzerland (Guide du typographe): narrow no-break spaces before `? ! ;` and inside
///   `« »`, a no-break space before `:`.
///
/// Whatever space was there (none, one, several, or another kind) becomes exactly that one. A
/// mark is spaced only where it ends a word: followed by a space, a line break, the end,
/// another mark or a closing quote or bracket. So times (10:30), ratios, URLs ("https://",
/// "?q=1"), a URL or address the mark is typed against, emoticons (":)", ";-)", ":D"), code in
/// backticks, a mark at a line's start, and the second mark of "?!" stay as they are.
///
/// The keyboard core's (`hk_dictation_french_spacing`, crates/keyboard/core/src/dictation/
/// french.rs).
nonisolated enum FrenchSpacing {
    enum Convention: Equatable {
        case france
        case canada
        case switzerland
    }

    static let narrowNoBreakSpace: Character = "\u{202F}"
    static let noBreakSpace: Character = "\u{00A0}"

    /// The convention for `language` (BCP 47), or nil when it isn't French.
    static func convention(for language: String?) -> Convention? {
        guard let language else { return nil }
        let convention = DictationCore.withLanguage(language) { hk_dictation_french_convention($0) }
        switch convention {
        case 0: return .france
        case 1: return .canada
        case 2: return .switzerland
        default: return nil
        }
    }

    /// `text` with French spacing when `language` is French; otherwise unchanged.
    static func apply(_ text: String, language: String?) -> String {
        DictationCore.stage(text, language: language, hk_dictation_french_spacing)
    }

    static func apply(_ text: String, convention: Convention) -> String {
        let core: UInt32 = switch convention {
        case .france: 0
        case .canada: 1
        case .switzerland: 2
        }
        return DictationCore.withText(text) { string in
            DictationCore.text(text) { hk_dictation_french_spacing_with(string, core, $0) }
        }
    }
}
