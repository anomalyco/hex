import UIKit

/// A space-bar trackpad's moves in two dimensions. Apple's moves the cursor up and down lines
/// as well as along them; a keyboard can only move it by characters
/// (`adjustTextPosition(byCharacterOffset:)`), so a line up or down is worked out from the
/// text around the cursor: the line before or after (split at new lines, and every
/// `lineLength` characters for a line the field wraps), at the same column. Shared by every
/// surface with a space-bar trackpad.
nonisolated enum TrackpadCursor {
    /// The text around the cursor as a trackpad gesture found it, where the gesture has moved
    /// the cursor since, and the column vertical moves keep. The field's own context lags the
    /// moves (the host reports the new cursor later: in the simulator a second line up read
    /// the text from before the first), so the gesture works from this copy, taking the field's
    /// once it has caught up (`catchUp`).
    struct State: Equatable {
        var text: [Character]
        var cursor: Int
        /// Kept across vertical moves (nil: the cursor's own), so moving across a short line and
        /// on comes back to it, as in a text view; a move along the line forgets it.
        var column: Int?

        init(before: String, after: String) {
            text = Array(before + after)
            cursor = before.count
        }

        /// The field's context once it has caught up with the moves (it reads as this copy does
        /// on both sides of the cursor): where it reaches further than this copy, it adds those
        /// lines. Hosts share only a few lines around the cursor (the simulator's UITextView about
        /// two above it, and only the cursor's own line once the cursor moved), so lines beyond
        /// what either showed are out of reach.
        mutating func catchUp(before: String, after: String) {
            let fresh = (before: Array(before), after: Array(after))
            let n = min(fresh.before.count, cursor, 48)
            let m = min(fresh.after.count, text.count - cursor, 48)
            guard n + m > 0, fresh.before.suffix(n).elementsEqual(text[(cursor - n)..<cursor]),
                  fresh.after.prefix(m).elementsEqual(text[cursor..<(cursor + m)]) else { return }
            let head = fresh.before.count > cursor ? fresh.before : Array(text[..<cursor])
            let tail = fresh.after.count > text.count - cursor ? fresh.after : Array(text[cursor...])
            text = head + tail
            cursor = head.count
        }

        /// The cursor moved `offset` characters along the text.
        mutating func moveAlong(_ offset: Int) {
            cursor = min(max(0, cursor + offset), text.count)
            column = nil
        }

        /// Moves the cursor `lines` lines down (up when negative), wrapping lines every
        /// `lineLength` characters: the character offset it moved, and the lines it moved (fewer
        /// past the first or last line, where it stays, as Apple's does).
        mutating func moveLines(_ lines: Int, lineLength: Int) -> (offset: Int, moved: Int) {
            let length = max(1, lineLength)
            let start = cursor
            let column = column ?? (cursor - TrackpadCursor.lineStart(text, cursor)) % length
            self.column = column
            var moved = 0
            for _ in 0..<abs(lines) {
                let next = lines < 0
                    ? TrackpadCursor.up(text, cursor, column, length)
                    : TrackpadCursor.down(text, cursor, column, length)
                if next == cursor { break }
                cursor = next
                moved += lines < 0 ? -1 : 1
            }
            return (cursor - start, moved)
        }
    }

    /// The characters a line of the field holds, estimated from the keyboard's width, as the
    /// field's own width and font can't be read: body text in a field about as wide as the
    /// screen, less its margins.
    static func lineLength(forWidth width: CGFloat) -> Int {
        max(12, Int(((width - lineMargins) / averageCharacterWidth).rounded()))
    }

    /// A UITextView's 17 pt body text, wrapped in a 360 pt line, held about 43 characters a
    /// line (iOS 27.2 simulator): about 8.2 pt a character.
    static let averageCharacterWidth: CGFloat = 8.2
    static let lineMargins: CGFloat = 40

    static func lineStart(_ text: [Character], _ position: Int) -> Int {
        var i = position
        while i > 0, !text[i - 1].isNewline { i -= 1 }
        return i
    }

    private static func lineEnd(_ text: [Character], _ position: Int) -> Int {
        var i = position
        while i < text.count, !text[i].isNewline { i += 1 }
        return i
    }

    /// The row of a line (its characters wrapped every `length`) a position in it is on.
    private static func row(_ offset: Int, lineLength: Int, _ length: Int) -> Int {
        let last = lineLength == 0 ? 0 : (lineLength - 1) / length
        return min(offset / length, last)
    }

    fileprivate static func up(_ text: [Character], _ position: Int, _ column: Int, _ length: Int) -> Int {
        let start = lineStart(text, position)
        let r = row(position - start, lineLength: lineEnd(text, position) - start, length)
        if r > 0 { return start + (r - 1) * length + column }
        // On the first line the keys can see, the cursor stays, as Apple's does on the first.
        guard start > 0 else { return position }
        let previous = lineStart(text, start - 1)
        let count = start - 1 - previous
        let last = count == 0 ? 0 : (count - 1) / length
        return previous + last * length + min(column, count - last * length)
    }

    fileprivate static func down(_ text: [Character], _ position: Int, _ column: Int, _ length: Int) -> Int {
        let start = lineStart(text, position)
        let end = lineEnd(text, position)
        let r = row(position - start, lineLength: end - start, length)
        let last = end == start ? 0 : (end - start - 1) / length
        if r < last { return min(end, start + (r + 1) * length + column) }
        // On the last line the keys can see, the cursor stays, as Apple's does on the last.
        guard end < text.count else { return position }
        let next = end + 1
        return next + min(column, lineEnd(text, next) - next)
    }
}

extension KeyboardTextTarget {
    var documentContextAfterInput: String? { nil }
}

extension DocumentProxyTextTarget {
    var documentContextAfterInput: String? { proxy()?.documentContextAfterInput }
}

@MainActor
extension FullKeyboardEngine {
    /// How far the finger moves the cursor a line on the trackpad. Apple's (black box, iOS 27.2,
    /// iPhone 17, a UITextView with 20.3 pt lines) moved a line every 15 to 16 pt up or down,
    /// the first after about 8 (from the middle of the cursor's line), and kept the cursor on
    /// the first or last line past it. Along a line it stopped at the line's ends, where Hex's
    /// moves on by characters as before.
    static let trackpadPointsPerLine: CGFloat = 15.5

    /// The trackpad finger moved to `point`: the characters to move the cursor by, along the
    /// line (each `trackpadPointsPerCharacter`) and up or down lines (each
    /// `trackpadPointsPerLine`, the first half as far), re-anchoring the touch for what it used.
    func trackpadSteps(_ touch: inout ActiveTouch, to point: CGPoint) -> Int {
        // The gesture's first move: the field's context is still where the cursor is.
        var state = touch.trackpad ?? TrackpadCursor.State(
            before: target.documentContextBeforeInput ?? "",
            after: target.documentContextAfterInput ?? ""
        )
        defer { touch.trackpad = state }
        let along = Int(((point.x - touch.trackpadAnchorX) / Self.trackpadPointsPerCharacter).rounded(.towardZero))
        if along != 0 {
            touch.trackpadAnchorX += CGFloat(along) * Self.trackpadPointsPerCharacter
            state.moveAlong(along)
        }
        let lines = Int(((point.y - touch.trackpadAnchorY) / Self.trackpadPointsPerLine).rounded())
        guard lines != 0 else { return along }
        if touch.trackpad != nil {
            state.catchUp(before: target.documentContextBeforeInput ?? "", after: target.documentContextAfterInput ?? "")
        }
        let move = state.moveLines(lines, lineLength: TrackpadCursor.lineLength(forWidth: width))
        // Past the first or last line the finger's further travel counts for nothing, so coming
        // back moves the cursor at once.
        touch.trackpadAnchorY = move.moved == lines ? touch.trackpadAnchorY + CGFloat(lines) * Self.trackpadPointsPerLine : point.y
        return along + move.offset
    }
}
