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

    /// Words taken back right after a swipe typed them, at one place in the text: the next
    /// swipe there types its next reading instead, and the strip may still offer them (Record
    /// typing logs: "two" swiped three times running for "yep", with "yep" in the strip). Taken
    /// back: delete took the whole word back, or deletes took out all its letters (Settings ›
    /// Experiments › Delete swiped words letter by letter), with nothing else typed since the
    /// swipe. One place: the text before it is the same, trailing spaces aside, in the same
    /// field. Every word taken back there in a row counts. Any other edit (a letter, a space,
    /// punctuation, a pick from the strip), a swipe somewhere else or another field forgets
    /// them. Only which reading goes in changes; the decoder's readings for a path don't.
    @MainActor
    final class SwipeRetries {
        private var place: String?
        /// Lowercased, the first taken back first.
        private var words: [String] = []
        /// A swiped word deletes are taking out one character at a time: its place, the word,
        /// the characters left (the word and the space the swipe added) and that space's length.
        private var deleting: (place: String?, word: String, left: Int, space: Int)?
        /// The next edit is this record's own (a swipe here, or a delete taking a swiped word
        /// back), not one that forgets it.
        private var ownEdit = false

        /// `readings` with the words taken back where `before` ends moved after the others.
        func reordered(_ readings: [String], before: String?) -> [String] {
            guard !words.isEmpty, Self.place(of: before) == place else { return readings }
            return readings.filter { !words.contains($0.lowercased()) } + readings.filter { words.contains($0.lowercased()) }
        }

        /// `reordered`, for a swipe going in now; one somewhere else forgets the words.
        func typing(_ readings: [String], before: String?) -> [String] {
            guard !readings.isEmpty else { return readings }
            deleting = nil
            if Self.place(of: before) == place { ownEdit = true } else { forget() }
            return reordered(readings, before: before)
        }

        /// Delete took back the whole word `swiped` typed.
        func tookBack(_ swiped: SwipedWord) {
            took(swiped.word, at: Self.place(of: swiped.before.map { String($0.dropLast(swiped.inserted.count)) }))
        }

        /// A delete is about to go; `swiped` is the word a swipe just typed when this delete
        /// starts taking it out one character at a time.
        func willDelete(letterOf swiped: SwipedWord?) {
            if let swiped {
                let place = Self.place(of: swiped.before.map { String($0.dropLast(swiped.inserted.count)) })
                deleting = (place, swiped.word, swiped.inserted.count, swiped.inserted.count - swiped.word.count)
            }
            guard var pending = deleting else { return }
            pending.left -= 1
            ownEdit = true
            if pending.left == pending.space { took(pending.word, at: pending.place) }
            deleting = pending.left > 0 ? pending : nil
        }

        /// The keys edited the text (`FullKeyboardEngine.didEdit`).
        func edited() {
            if ownEdit { ownEdit = false } else { forget() }
        }

        func forget() {
            place = nil
            words = []
            deleting = nil
            ownEdit = false
        }

        private func took(_ word: String, at place: String?) {
            if place != self.place { words = [] }
            self.place = place
            words.append(word.lowercased())
            ownEdit = true
        }

        /// A real iPhone reports nothing before the cursor in an empty field (nil, where a
        /// swipe's record keeps ""): the same place.
        private static func place(of before: String?) -> String {
            var text = before ?? ""
            while text.last == " " { text.removeLast() }
            return text
        }
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
        let before = typingContext
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
