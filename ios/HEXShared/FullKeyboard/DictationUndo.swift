import Foundation

/// The dictation Undo dictation can take back, with the field's text either side of it when it
/// went in, so it can still be found after the cursor moves: within it, before it or after it.
struct DictationUndo: Equatable {
    let text: String
    /// As much of the field before and after the dictation as the host showed when it went in.
    let before: String
    let after: String

    /// How to take the dictation back from where the cursor is now.
    struct Plan: Equatable {
        /// UTF-16 units from the cursor to the dictation's end.
        var toEnd: Int
        /// Characters to delete from there.
        var delete: Int
        /// UTF-16 units from where the dictation was back to the cursor's place in the text.
        var back: Int
    }

    /// Finds the dictation in what the host shows around the cursor (`before` and `after`).
    /// Nil while text is selected, when the host doesn't show all of it (bar the space before
    /// it), when it shows it more than once, or when the text either side no longer matches:
    /// Undo must never delete anything else.
    func plan(before current: String, after following: String, selected: String?) -> Plan? {
        guard selected?.isEmpty ?? true, !text.isEmpty else { return nil }
        let window = Array(current) + Array(following)
        let cursor = current.count
        let dictation = Array(text)
        let n = dictation.count
        var start = 0
        var end = 0
        if let found = Self.only(dictation, in: window) {
            start = found
            end = found + n
        } else {
            // Hosts that show only the cursor's sentence leave out the space before it: the
            // sentence must start what they show.
            let core = Array(text.drop(while: \.isWhitespace))
            guard core.count < n, Self.only(core, in: window) == 0 else { return nil }
            end = core.count
        }
        let shownBefore = String(window[..<start])
        let shownAfter = String(window[end...])
        // Either side may be cut short by the host's window, now or when it went in.
        guard shownBefore.hasSuffix(before) || before.hasSuffix(shownBefore),
              shownAfter.hasPrefix(after) || after.hasPrefix(shownAfter)
        else { return nil }
        let toEnd = end >= cursor
            ? String(window[cursor..<end]).utf16.count
            : -String(window[end..<cursor]).utf16.count
        let back = if cursor < start {
            -String(window[cursor..<start]).utf16.count
        } else if cursor > end {
            String(window[end..<cursor]).utf16.count
        } else {
            0
        }
        return Plan(toEnd: toEnd, delete: n, back: back)
    }

    /// Where `part` starts in `window` when it's there exactly once.
    private static func only(_ part: [Character], in window: [Character]) -> Int? {
        guard !part.isEmpty, window.count >= part.count else { return nil }
        let starts = (0...(window.count - part.count)).filter { window[$0..<($0 + part.count)].elementsEqual(part) }
        return starts.count == 1 ? starts.first : nil
    }
}
