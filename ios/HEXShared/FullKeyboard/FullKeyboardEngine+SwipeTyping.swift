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
        /// The text before and after the cursor right after it went in, as Hex expects the host
        /// to report them: an update that matches is the echo of Hex's own edit (`hostEchoed`).
        var before: String?
        var after: String?
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
        let before = textBefore ?? ""
        return AutocorrectEngine.previousWord(SwipeDecoder.needsSpace(before: before) ? before + " " : before, token: "")
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

    /// The state a swipe is typed in: the text before the cursor, shift, auto-capitalization,
    /// and whether the field takes text replacements.
    var swipeTypingState: SwipeDecoder.TypingState {
        SwipeDecoder.TypingState(
            before: typingContext, shift: shift.state, shiftHeld: shift.isHeld, autoCapitalization: autoCapitalizationMode,
            autoCapitalizes: autoCapitalizes, autocorrection: target.autocorrectionType != .no
        )
    }

    /// What a swipe whose readings are `readings` (best first, those that may be offered)
    /// types: a space when the text before needs one, the first cased as the sentence and your
    /// words need, and the strip's others. The keyboard core decides (`SwipeDecoder.typed`),
    /// asking the dictionary.
    func swipeTyped(_ readings: [String], in state: SwipeDecoder.TypingState? = nil) -> SwipeDecoder.Typed? {
        guard let words = autocorrect.swipeWords else { return nil }
        let checker = autocorrect.checker
        return SwipeDecoder.typed(readings, in: state ?? swipeTypingState, words: words) { checker.isWordValid($0) }
    }

    /// A reading as it's typed, with a capital first letter when `capital` (all capitals with
    /// caps lock): `swipeTyped` with no text before, so the only capital is shift's.
    func swipeCased(_ word: String, capital: Bool) -> String {
        var state = swipeTypingState
        state.before = nil
        state.shiftHeld = false
        state.shift = shift.state == .locked ? .locked : capital ? .on : .off
        return swipeTyped([word], in: state)?.word ?? word
    }

    /// Whether a swipe's reading, as the model spells it (lowercase), is a word the dictionary
    /// knows only with a capital: a name, day or place ("monday"), or an abbreviation.
    func swipeReadsAsName(_ word: String) -> Bool {
        guard let words = autocorrect.swipeWords else { return false }
        let checker = autocorrect.checker
        return SwipeDecoder.readsAsName(word, words: words) { checker.isWordValid($0) }
    }

    /// What goes against a swiped word, as on Apple's keyboard: closing punctuation and
    /// brackets, an apostrophe, a hyphen and a slash. Anything else tapped right after a swipe
    /// (a straight quote and opening brackets included) starts the next word.
    static let swipeJoiners: Set<Character> = [",", ".", "!", "?", ";", ":", ")", "]", "}", "'", "’", "-", "/"]
}

@MainActor
extension AutocorrectEngine {
    /// Your text replacements and words as the keyboard core types a swipe with them: made
    /// again whenever one of them differs from the last made (not only when the revision
    /// moves: `learnedWords` can change under it), as these were read live.
    var swipeWords: SwipeDecoder.Words? {
        let key = SwipeWordsKey(
            language: language.rawValue, hasDictionary: hasDictionary, replacements: lexiconShortcuts,
            own: lexiconTerms + Array(learning.typos.values) + learnedWords.words,
            vocabulary: vocabulary.isEmpty ? [] : Self.vocabularyWords(vocabulary)
        )
        if let made = Self.swipeWordsMade, made.key == key { return made.words }
        let words = SwipeDecoder.Words(
            language: key.language, hasDictionary: key.hasDictionary, replacements: key.replacements, own: key.own, vocabulary: key.vocabulary
        )
        Self.swipeWordsMade = (key, words)
        return words
    }

    private static var swipeWordsMade: (key: SwipeWordsKey, words: SwipeDecoder.Words?)?
}

private struct SwipeWordsKey: Equatable {
    var language: String
    var hasDictionary: Bool
    var replacements: [String: String]
    var own: [String]
    var vocabulary: [String]
}
