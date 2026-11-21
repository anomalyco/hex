import QuartzCore
import UIKit

/// Settings › Keyboard › Swipe typing's read side: the word a swipe types and the readings the
/// strip offers for it, and how a reading is cased. The touch and decoding side is
/// `SwipeTyping`; the swipe state (`swipedWord`, `isSwiping`, `swipePreview`) and the methods
/// that change it stay in `FullKeyboardEngine`, since extensions hold no stored properties and
/// that state's setter is file-scoped.
@MainActor
extension FullKeyboardEngine {
    /// The word a swipe just typed, until anything else changes the text: delete takes it
    /// back whole, space never autocorrects it, and the strip offers the swipe's other
    /// readings, any of which takes its place.
    struct SwipedWord: Equatable {
        /// What the swipe put in: a space when one was needed, then the word.
        var inserted: String
        var word: String
        /// Its other readings, cased as the word is, best first.
        var alternatives: [String]
    }

    /// Whether a letter touch may become a swipe now: on the letters page of a text field
    /// (not an address or a URL), with no accent menu open.
    var allowsSwipes: Bool {
        layerState.layer == .letters && field == .text && activeAccentMenu == nil
    }

    /// The letter keys a swipe is read against, where they're drawn.
    var swipeKeys: TypingDecoder.Keys {
        TypingDecoder.Keys(layout: layout) ?? TypingDecoder.Keys(centres: [:], pitch: .zero, bottomRowY: 0)
    }

    /// The word before a swipe's, lowercased, when only spaces (or the one the swipe will
    /// add) come between them.
    var swipePreviousWord: String? {
        let before = target.documentContextBeforeInput ?? ""
        return AutocorrectEngine.previousWord(SwipeTyping.needsSpace(before: before) ? before + " " : before, token: "")
    }

    /// The strip while swiping (the word so far) and right after (the other readings, the
    /// likeliest in the centre slot); nil otherwise.
    var swipeSuggestions: [KeyboardSuggestion]? {
        if isSwiping {
            return swipePreview.map { [KeyboardSuggestion(displayText: $0, replacement: $0, kind: .primary(autoReplace: true))] } ?? []
        }
        guard let swipedWord else { return nil }
        let items = swipedWord.alternatives.prefix(3).map { KeyboardSuggestion(displayText: $0, replacement: $0, kind: .swiped) }
        return items.count >= 2 ? [items[1], items[0]] + items.dropFirst(2) : items
    }

    /// `words` that may be offered after the word before (`AutocorrectEngine.mayOffer`), once each.
    func swipeReadings(_ words: [String]) -> [String] {
        let previous = swipePreviousWord
        var seen = Set<String>()
        return words.filter { autocorrect.mayOffer($0, after: previous) && seen.insert($0.lowercased()).inserted }
    }

    /// Whether a swiped word starts with a capital: shift (one-shot, held, caps lock, or
    /// automatic), or the automatic capital the space the swipe adds brings ("Hi." then a
    /// swipe types " There").
    var swipeCapital: Bool {
        if shift.isUppercase { return true }
        guard shift.state == .off, !shift.isHeld else { return false }
        let before = target.documentContextBeforeInput
        guard SwipeTyping.needsSpace(before: before) else { return false }
        return AutoCapitalization.shouldCapitalize(
            mode: autoCapitalizationMode,
            before: (before ?? "") + " ",
            hasText: true
        )
    }

    /// A reading as it's typed: all capitals with caps lock, a capital first letter with
    /// `capital`, else a name the dictionary only knows capitalised ("monday" -> "Monday").
    /// One of your own words with a capital that is also a common word (a Contacts name such
    /// as "Bell" or "Will") is written as the sentence needs it, as Apple's keyboard swipes
    /// "the bell rang" beside a contact Kate Bell (`AutocorrectEngine.withoutStrayCapital`).
    /// With Auto-capitalization off, no capital you didn't ask for ("i'm", "monday"), but
    /// your own spellings keep theirs.
    func swipeCased(_ word: String, capital: Bool) -> String {
        if shift.state == .locked { return word.uppercased() }
        guard let first = word.first else { return word }
        if capital { return first.uppercased() + word.dropFirst() }
        if !autoCapitalizes {
            return autocorrect.isOwnSpelling(word) ? word : word.lowercased()
        }
        if swipeReadsAsName(word) { return first.uppercased() + word.dropFirst() }
        return autocorrect.withoutStrayCapital(word, typed: "")
    }

    /// The strip's other readings after a swipe, cased as its word is (`readings` best first,
    /// the first the one typed). A reading that is a text replacement's shortcut shows as its
    /// replacement ("home" as "at my house"), the swiped word's first, as Apple's keyboard
    /// offers one after a swipe (black box, iOS 27.2); space never puts it in.
    func swipeAlternatives(_ readings: [String], capital: Bool) -> [String] {
        func replacement(_ reading: String) -> String? {
            guard target.autocorrectionType != .no else { return nil }
            let typed = capital ? reading.prefix(1).uppercased() + reading.dropFirst() : reading
            guard let replaced = autocorrect.textReplacement(before: typed), replaced.typed.count == typed.count else { return nil }
            return replaced.replacement
        }
        var out: [String] = []
        if let first = readings.first, let phrase = replacement(first) { out.append(phrase) }
        for reading in readings.dropFirst() {
            let item = replacement(reading) ?? swipeCased(reading, capital: capital)
            if !out.contains(item) { out.append(item) }
        }
        return Array(out.prefix(3))
    }

    /// Whether a swipe's reading, as the model spells it (lowercase), is a word the dictionary
    /// knows only with a capital: a name, day or place ("monday"), or an abbreviation.
    func swipeReadsAsName(_ word: String) -> Bool {
        guard autocorrect.hasDictionary, word == word.lowercased(), word.count >= 2, let first = word.first else { return false }
        return autocorrect.checker.isWordValid(first.uppercased() + word.dropFirst()) && !autocorrect.checker.isWordValid(word)
    }

    /// What goes against a swiped word, as on Apple's keyboard: closing punctuation and
    /// brackets, an apostrophe, a hyphen and a slash. Anything else tapped right after a swipe
    /// (a straight quote and opening brackets included) starts the next word.
    static let swipeJoiners: Set<Character> = [",", ".", "!", "?", ";", ":", ")", "]", "}", "'", "’", "-", "/"]
}
