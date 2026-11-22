import UIKit

/// Smart quotes and dashes as Apple's keyboard types them (black box, iOS 27.2 simulator, in a
/// UITextView with each trait on and off, and in a text field):
/// - The ' and " keys type an opening quote at the start, after a space or a new line, after
///   ( [ { or after an opening quote (‘Hi’, (“ok”), ‘“b”’), else a closing one (it’s, 5’9”).
/// - The keys' language picks the marks (`SmartQuotes.forLanguage`), as each of Apple's
///   keyboards does: „ “ and ‚ ‘ in German (Swiss too, the apostrophe key typing ‘ even in a
///   word), « » with no-break spaces inside in French (France, Canada and Switzerland), „ ”
///   and ‚ ’ in Polish, « » without spaces in Norwegian, Russian and Ukrainian, and “ ” and
///   ‘ ’ in English, Spanish, Italian, Portuguese and Romanian. Serbian, which the simulator
///   has no keyboard for, gets those too.
/// - Autocorrect's and suggestions' apostrophes (don’t, I’d) are ’.
/// - A hyphen typed after a hyphen makes the two an em dash (a--b is a—b, a third stays a hyphen:
///   a—-b); delete then takes the dash whole.
/// A field's `smartQuotesType` and `smartDashesType` of `.no` turn each off, as there;
/// `.default` follows Smart Punctuation, which a keyboard can't read, so it counts as on, its
/// default. `smartInsertDeleteType` changed nothing Apple's keyboard types: a suggestion's
/// space went before , . ? ! ; : ) with it off too.
nonisolated struct SmartQuotes: Equatable, Sendable {
    let openDouble: String
    let closeDouble: String
    let openSingle: String
    let closeSingle: String

    static let english = SmartQuotes(openDouble: "\u{201C}", closeDouble: "\u{201D}", openSingle: "\u{2018}", closeSingle: "\u{2019}")
    static let german = SmartQuotes(openDouble: "\u{201E}", closeDouble: "\u{201C}", openSingle: "\u{201A}", closeSingle: "\u{2018}")
    static let french = SmartQuotes(openDouble: "\u{00AB}\u{00A0}", closeDouble: "\u{00A0}\u{00BB}", openSingle: "\u{2018}", closeSingle: "\u{2019}")
    static let polish = SmartQuotes(openDouble: "\u{201E}", closeDouble: "\u{201D}", openSingle: "\u{201A}", closeSingle: "\u{2019}")
    static let guillemets = SmartQuotes(openDouble: "\u{00AB}", closeDouble: "\u{00BB}", openSingle: "\u{2018}", closeSingle: "\u{2019}")

    static func forLanguage(_ language: KeyboardLanguage) -> SmartQuotes {
        switch language {
        case .german: .german
        case .french, .frenchCanadian, .frenchSwiss: .french
        case .polish: .polish
        case .norwegian, .russian, .ukrainian: .guillemets
        default: .english
        }
    }
}

nonisolated enum SmartPunctuation {
    static let apostrophe = "\u{2019}"
    static let emDash = "\u{2014}"

    /// `.default` (Smart Punctuation, on unless you turned it off) and `.yes` are on.
    static func isOn(_ type: UITextSmartQuotesType?) -> Bool {
        type == .default || type == .yes
    }

    static func isOn(_ type: UITextSmartDashesType?) -> Bool {
        type == .default || type == .yes
    }

    /// Whether a quote after `previous` opens: at the start, after a space or a new line, a
    /// bracket, or the language's own opening quotes (German's closing “ is English's opening).
    static func opens(after previous: Character?, quotes: SmartQuotes) -> Bool {
        guard let previous else { return true }
        return previous.isWhitespace || "([{".contains(previous)
            || previous == quotes.openDouble.first || previous == quotes.openSingle.first
    }

    /// `text` with its straight quotes curled, `previous` being the character before it: an
    /// apostrophe between letters is ’ (`apostrophe`), any other quote opens or closes by
    /// what comes before it.
    static func curled(_ text: String, after previous: Character?, quotes: SmartQuotes) -> String {
        guard text.contains(where: { $0 == "'" || $0 == "\"" }) else { return text }
        var out = ""
        var last = previous
        let characters = Array(text)
        for (index, character) in characters.enumerated() {
            let piece: String
            switch character {
            case "'":
                let next = index + 1 < characters.count ? characters[index + 1] : nil
                if characters.count > 1, last?.isLetter == true, next?.isLetter == true {
                    piece = apostrophe
                } else {
                    piece = opens(after: last, quotes: quotes) ? quotes.openSingle : quotes.closeSingle
                }
            case "\"":
                piece = opens(after: last, quotes: quotes) ? quotes.openDouble : quotes.closeDouble
            default:
                piece = String(character)
            }
            out += piece
            last = piece.last
        }
        return out
    }

    /// The text as the keys reason about it: curly quotes straight again, one character for
    /// one, so counts still match the field.
    static func plain(_ text: String) -> String {
        guard text.unicodeScalars.contains(where: { (0x2018...0x201E).contains($0.value) }) else { return text }
        return String(text.map { character -> Character in
            switch character {
            case "\u{2018}", "\u{2019}", "\u{201A}": "'"
            case "\u{201C}", "\u{201D}", "\u{201E}": "\""
            default: character
            }
        })
    }
}

extension KeyboardTextTarget {
    var smartQuotesType: UITextSmartQuotesType? { nil }
    var smartDashesType: UITextSmartDashesType? { nil }
}

extension DocumentProxyTextTarget {
    var smartQuotesType: UITextSmartQuotesType? { proxy()?.smartQuotesType }
    var smartDashesType: UITextSmartDashesType? { proxy()?.smartDashesType }
}

@MainActor
extension FullKeyboardEngine {
    /// The text before the cursor as the keys reason about it (`SmartPunctuation.plain`).
    var textBefore: String? {
        target.documentContextBeforeInput.map(SmartPunctuation.plain)
    }

    /// Smart quotes in this field: a text field that doesn't turn them off.
    var smartQuotes: Bool {
        field == .text && target.isSecureTextEntry != true && SmartPunctuation.isOn(target.smartQuotesType)
    }

    var smartDashes: Bool {
        field == .text && target.isSecureTextEntry != true && SmartPunctuation.isOn(target.smartDashesType)
    }

    /// `text` as it goes into the field, after `previous` (the field's character before the
    /// cursor unless given): its quotes curled while smart quotes are on.
    func smartened(_ text: String, after previous: Character?? = .none) -> String {
        guard smartQuotes else { return text }
        let before = previous ?? target.documentContextBeforeInput?.last
        return SmartPunctuation.curled(text, after: before, quotes: SmartQuotes.forLanguage(layoutLanguage ?? language))
    }

    /// The strip's words as they'd go in: with smart quotes, an apostrophe shows as ’ ("I’m"),
    /// as on Apple's strip.
    func displayed(_ suggestions: [KeyboardSuggestion]) -> [KeyboardSuggestion] {
        guard smartQuotes, suggestions.contains(where: { $0.displayText.contains("'") }) else { return suggestions }
        let quotes = SmartQuotes.forLanguage(layoutLanguage ?? language)
        return suggestions.map {
            guard $0.kind != .emoji, $0.displayText.contains("'") else { return $0 }
            return KeyboardSuggestion(displayText: SmartPunctuation.curled($0.displayText, after: nil, quotes: quotes), replacement: $0.replacement, kind: $0.kind)
        }
    }

    /// Marks that go against a suggestion's word, taking the space it typed, as on Apple's
    /// keyboard (black box, iOS 27.2: "Hello" picked, then "," types "Hello,"; a quote, a bracket
    /// or @ keeps the space), whatever the field's `smartInsertDeleteType`.
    static let suggestionJoiners: Set<Character> = [",", ".", "!", "?", ";", ":", ")"]

    /// Deletes a suggestion's space before `text`, one of `suggestionJoiners`, typed right
    /// after it. True when it did: the mark then goes in as typed.
    func dropsSuggestionSpace(before text: String) -> Bool {
        // A held space (`spacePending`) has nothing to delete, and the word is finished already.
        if spacePending, text.count == 1, let mark = text.first, Self.suggestionJoiners.contains(mark) { return true }
        guard text.count == 1, let mark = text.first, Self.suggestionJoiners.contains(mark),
              let spaced = suggestionSpaceBefore, spaced.last == " ", spaced == textBefore else { return false }
        typingLog?.edit("delete", [TypingLogField("deleted", .string(" ")), TypingLogField("src", .string("suggestion_space"))])
        forgetTypingContext()
        target.deleteBackward()
        return true
    }

    /// Types what `insert` was given: a typed or picked quote curled, and a hyphen typed
    /// after a hyphen making both an em dash. Accent picks and emoji go in as they are.
    func typeIntoField(_ text: String, source: String?) {
        guard !text.isEmpty else { return }
        guard source == "key" || source == "slide" || source == nil else { return target.insertText(text) }
        if text == "-", smartDashes, target.documentContextBeforeInput?.last == "-" {
            typingLog?.edit("smart_dash", [TypingLogField("deleted", .string("-")), TypingLogField("inserted", .string(SmartPunctuation.emDash))])
            rememberTypingContext(before: textBefore.map { String($0.dropLast(1)) }, inserted: SmartPunctuation.emDash)
            target.deleteBackward()
            return target.insertText(SmartPunctuation.emDash)
        }
        target.insertText(smartened(text))
    }
}
