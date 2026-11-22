import Foundation

/// The space after a word mid-sentence, where the text after the cursor already starts with a
/// space or a closing mark (TestFlight: "Tapping on a word to select it, and then quickly
/// typing in a correction doesn't account for the trailing space and you end up with 2 spaces
/// inserted"). A picked suggestion, or a space after a typed or swiped word (corrected or
/// not), types no second space there: the space is held, and goes in only if the next key
/// starts another word, as a swiped word's does (`spaceAfterSwipedWord`). "brown" in "brown
/// fox" selected, "black" and space typed, reads "black fox"; "cat" next reads "black cat
/// fox". A mark goes against the word ("black, fox"), delete takes the held space back (so an
/// autocorrection's revert follows as after a typed space, `offeringRevert`), and moving the
/// cursor drops it.
///
/// Apple's keyboard (black box, iOS 27.2, a UITextField) types the second space after a typed
/// word, corrected or not ("black  fox"); a picked suggestion steps over the space after the
/// cursor, and a word typed next then joins the one after it ("black catfox"). Hex holds the
/// space instead, which reads right both ways.
@MainActor
extension FullKeyboardEngine {
    /// Marks a space never goes before: closing punctuation, quotes and apostrophes.
    static let closingMarks: Set<Character> = [".", ",", "!", "?", ";", ":", ")", "]", "}", "\"", "'", "»", "…"]
    /// French puts a space before these (`FrenchSpacing`), so there they take it as usual.
    static let frenchSpacedMarks: Set<Character> = ["?", "!", ";", ":", "»"]

    /// Whether the text after the cursor starts with what a space after the word before it
    /// would double: a space (not a line break) or a closing mark.
    var textAfterSeparates: Bool {
        guard let first = target.documentContextAfterInput.map(SmartPunctuation.plain)?.first else { return false }
        if first.isWhitespace { return !first.isNewline }
        guard Self.closingMarks.contains(first) else { return false }
        let french = [KeyboardLanguage.french, .frenchCanadian, .frenchSwiss].contains(layout.language)
        return !(french && Self.frenchSpacedMarks.contains(first))
    }

    /// Whether the space bar holds its space rather than typing it: right after a word or a
    /// mark, before a space, or right after a word, before a closing mark.
    var holdsTypedSpace: Bool {
        guard let last = textBefore?.last, !last.isWhitespace, textAfterSeparates else { return false }
        return target.documentContextAfterInput?.first?.isWhitespace == true || last.isLetter || last.isNumber
    }

    /// Whether a held space waits for the next key: nothing has changed since it was held.
    var spacePending: Bool {
        guard let held = suggestionSpaceBefore, held.last?.isWhitespace == false else { return false }
        return held == typingContext && textAfterSeparates
    }

    /// The text before the cursor as the strip reads it: with a held space, as after a typed
    /// one, so the strip offers the next word rather than the word just finished.
    var stripBefore: String? {
        spacePending ? textBefore.map { $0 + " " } : textBefore
    }

    /// Holds the space after the word just finished, until the next key. The space bar's
    /// counts as a space for the keys' page too: 123 goes back to ABC.
    func holdSpaceAfterWord(spaceBar: Bool = false) {
        suggestionSpaceBefore = typingContext
        if spaceBar {
            layerState.didInsert(" ")
            rebuildLayoutIfNeeded()
        }
        refreshSuggestions()
    }

    /// Types the held space before a pick from the strip, which then offers the next word:
    /// the pick goes in after it, as after a typed space, never in the finished word's place.
    func typeHeldSpace(before suggestion: KeyboardSuggestion) {
        guard spacePending, suggestion.kind != .revert, suggestions.contains(suggestion) else { return }
        suggestionSpaceBefore = nil
        typingLog?.edit("insert", [
            TypingLogField("inserted", .string(" ")),
            TypingLogField("src", .string("held_space")),
        ])
        rememberTypingContext(before: typingContext, inserted: " ")
        target.insertText(" ")
    }

    /// Types a picked suggestion's word and the space after it, held where the text after the
    /// cursor separates it already (`textAfterSeparates`).
    func insertPicked(_ word: String) {
        let holds = textAfterSeparates
        insert(holds ? word : word + " ", source: nil)
        if holds {
            holdSpaceAfterWord()
        } else {
            suggestionSpaceBefore = typingContext
        }
        typingLog?.context("suggestion")
    }

    /// Types the held space before `text`, the next key, when it starts another word (anything
    /// but a mark in `swipeJoiners`); else a swiped word's (`spaceAfterSwipedWord`).
    func spaceBeforeNextWord(_ text: String) {
        guard spacePending, let first = text.first, !Self.swipeJoiners.contains(first) else {
            return spaceAfterSwipedWord(before: text)
        }
        suggestionSpaceBefore = nil
        typingLog?.edit("insert", [
            TypingLogField("inserted", .string(" ")),
            TypingLogField("src", .string("held_space")),
        ])
        rememberTypingContext(before: typingContext, inserted: " ")
        target.insertText(" ")
        if shift.state == .off { refreshAutoCapitalization() }
    }

    /// Delete right after a held space takes it back and deletes nothing. True when it did.
    func dropsHeldSpace() -> Bool {
        guard spacePending else { return false }
        suggestionSpaceBefore = nil
        typingLog?.edit("delete", [
            TypingLogField("deletedCount", .int(0)),
            TypingLogField("src", .string("held_space")),
        ])
        refreshSuggestions()
        onChange?()
        return true
    }
}
