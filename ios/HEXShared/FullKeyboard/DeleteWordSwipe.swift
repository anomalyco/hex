import UIKit

/// Press and hold delete, then slide left: each key width the finger travels left of where it
/// went down deletes one more word, sliding back right types those words back, and lifting the
/// finger keeps what is deleted. Words are what Option-Delete removes on a Mac: the spaces and
/// punctuation before the cursor go with the word in front of them. Only the text proxy is
/// used: each word deleted is remembered so it can be typed back.
@MainActor
final class DeleteWordSwipe {
    let touchID: Int
    private let engine: FullKeyboardEngine
    private let startX: CGFloat
    private let step: CGFloat
    /// What each step deleted, oldest first; a slide back types the last one back.
    private var deleted: [String] = []
    /// Once the finger is a key width left: hold-to-repeat stops and the slide owns delete.
    private(set) var isActive = false

    init(engine: FullKeyboardEngine, touchID: Int, startX: CGFloat) {
        self.engine = engine
        self.touchID = touchID
        self.startX = startX
        step = max(1, engine.layout.letterWidth)
    }

    var deletedWordCount: Int { deleted.count }

    /// The finger moved: delete or restore words until their count matches how far left it
    /// is. True once the slide is active.
    @discardableResult
    func move(to x: CGFloat) -> Bool {
        let wanted = max(0, Int(((startX - x) / step).rounded(.down)))
        if !isActive {
            guard wanted > 0 else { return false }
            isActive = true
            engine.typingLog?.ui("delete_swipe_start", [TypingLogField("id", .int(touchID))])
        }
        var changed = false
        while deleted.count < wanted, deleteWord() {
            changed = true
        }
        while deleted.count > wanted, let text = deleted.popLast() {
            engine.typingLog?.edit("insert", [
                TypingLogField("inserted", .string(text)),
                TypingLogField("src", .string("delete_swipe_restore")),
            ])
            engine.forgetTextAroundKeys()
            engine.target.insertText(text)
            engine.onSelectionStep?()
            changed = true
        }
        if changed {
            engine.textDidChange()
            engine.onEdit?()
        }
        return true
    }

    /// The finger lifted (or the touch was cancelled): what is deleted stays deleted.
    func finish() {
        guard isActive else { return }
        engine.typingLog?.ui("delete_swipe_end", [
            TypingLogField("id", .int(touchID)),
            TypingLogField("words", .int(deleted.count)),
        ])
        engine.typingLog?.context("delete_swipe")
        deleted.removeAll()
    }

    private func deleteWord() -> Bool {
        guard let before = engine.target.documentContextBeforeInput,
              let count = Self.wordLength(before: before) else { return false }
        let text = String(before.suffix(count))
        engine.typingLog?.edit("delete_word", [
            TypingLogField("deleted", .string(text)),
            TypingLogField("src", .string("delete_swipe")),
        ])
        engine.forgetTextAroundKeys()
        for _ in 0..<count {
            engine.target.deleteBackward()
        }
        deleted.append(text)
        engine.onSelectionStep?()
        return true
    }

    /// How many characters at the end of `before` make up its last word with the spaces and
    /// punctuation after it, as Option-Delete counts them ("Hello, world. " ends in
    /// "world. "); nil when nothing is before the cursor. Only spaces and punctuation go at once.
    static func wordLength(before: String) -> Int? {
        guard !before.isEmpty else { return nil }
        var count = 0
        var index = before.endIndex
        while index > before.startIndex, !isWordCharacter(before[before.index(before: index)]) {
            index = before.index(before: index)
            count += 1
        }
        while index > before.startIndex, isWordCharacter(before[before.index(before: index)]) {
            index = before.index(before: index)
            count += 1
        }
        return count
    }

    /// Letters, digits, emoji and the apostrophe inside "don't" belong to a word.
    private static func isWordCharacter(_ character: Character) -> Bool {
        if character.isLetter || character.isNumber || character == "_" { return true }
        if character == "'" || character == "’" { return true }
        return character.unicodeScalars.first?.properties.isEmojiPresentation == true
    }
}

@MainActor
extension FullKeyboardEngine {
    /// The delete key held down, per `repeatDelay`: one character a tick, two once it
    /// accelerates, whole words after `wordDeleteAfter`.
    func repeatDelete(heldFor: TimeInterval) {
        if heldFor >= Self.wordDeleteAfter {
            deleteWordBackward()
        } else if heldFor >= Self.fastRepeatAfter {
            deleteBackward(takesSwipeBack: false, source: "repeat_fast")
            deleteBackward(takesSwipeBack: false, source: "repeat_fast")
        } else {
            deleteBackward(takesSwipeBack: false, source: "repeat")
        }
    }
}
