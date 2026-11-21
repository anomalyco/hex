import UIKit

/// The engine's two-thumb selection state: the switch, the range under the thumbs, and what
/// Dictate took out (`TrackpadSelectionSession`).
@MainActor
struct TrackpadSelectionState {
    var enabled = false
    var session: TrackpadSelectionSession?
    var pendingRemoval: TrackpadSelectionSession.Removal?
}

/// Settings › Experiments › Two-thumb selection: while the space bar is the trackpad, a second
/// finger starts a selection instead of being ignored. A keyboard can't select in the host, so
/// the range lives here, shown in the strip, and the actions change the text through the same
/// `KeyboardTextTarget` moves the keys use. Each thumb moves one end with its own
/// `TrackpadCursor.State` (the trackpad's own gain, acceleration and rows), and the range is
/// always normalized.
@MainActor
final class TrackpadSelectionSession {
    enum Phase: Equatable {
        /// Both thumbs are down, one end each.
        case selecting
        /// Both lifted with a range: the strip offers Delete · Cut · Copy · Dictate · Cancel.
        case actions
    }

    /// Which end a finger moves: the one further left when the selection starts is `left`.
    enum Role: Hashable {
        case left
        case right
    }

    enum Action: Equatable {
        case delete
        case cut
        case copy
        case dictate
        case cancel
    }

    /// What Dictate took out, and the text around its place, so a cancelled or empty dictation
    /// can put it back.
    struct Removal: Equatable {
        var text: String
        var before: String
        var after: String
    }

    /// One thumb's end: its `TrackpadCursor.State` and the finger moves that set its speed.
    private struct End {
        var state: TrackpadCursor.State
        var path: [TrackpadCursor.Sample]

        init(state: TrackpadCursor.State, point: CGPoint, time: TimeInterval) {
            self.state = state
            self.path = [TrackpadCursor.Sample(time: time, point: point)]
        }
    }

    /// What the strip draws: the text around the range with `selected` highlighted, or the
    /// actions once both thumbs lift.
    struct Presentation: Equatable {
        var phase: Phase
        var before: String
        var selected: String
        var after: String

        var hasSelection: Bool { !selected.isEmpty }
    }

    /// The host's text around the cursor when the selection started, its UTF-16 boundaries,
    /// and where the host's cursor was in it.
    let before: String
    let after: String
    let text: [Character]
    let utf16: [Int]
    let anchor: Int

    private(set) var phase: Phase = .selecting
    private(set) var touchRoles: [Int: Role] = [:]
    private(set) var lifted: Set<Int> = []
    private var left: End
    private var right: End

    init(before: String, after: String, wrapWidth: CGFloat, lineHeight: CGFloat, width: (Character) -> CGFloat) {
        self.before = before
        self.after = after
        let head = Array(before)
        let clipboard = TrackpadCursor.State(
            before: before,
            after: after,
            wrapWidth: wrapWidth,
            lineHeight: lineHeight,
            width: width
        )
        left = End(state: clipboard, point: .zero, time: 0)
        right = End(state: clipboard, point: .zero, time: 0)
        let boundaries = clipboard.layout.utf16
        text = clipboard.layout.text
        utf16 = boundaries
        anchor = boundaries[min(max(0, head.count), boundaries.count - 1)]
    }

    /// The fingers that control the ends: the one further left the start, the other the end.
    func add(touch id: Int, role: Role, point: CGPoint, time: TimeInterval) {
        guard touchRoles[id] == nil else { return }
        touchRoles[id] = role
        let end = End(state: role == .left ? left.state : right.state, point: point, time: time)
        if role == .left { left = end } else { right = end }
    }

    /// The finger moved: its end follows with the trackpad's own gain, and stays within the
    /// text the host shared (there's no host to show more while a selection is being made).
    func move(touch id: Int, to point: CGPoint, time: TimeInterval) {
        guard phase == .selecting, !lifted.contains(id), let role = touchRoles[id] else { return }
        var end = role == .left ? left : right
        let last = end.path.last ?? TrackpadCursor.Sample(time: time, point: point)
        let sample = TrackpadCursor.Sample(time: time, point: point)
        end.path = time > last.time ? end.path.suffix(7) + [sample] : [sample]
        let gain = TrackpadCursor.gain(speed: TrackpadCursor.speed(along: end.path))
        let vertical = end.path.count > 1 ? TrackpadCursor.isVertical(end.path) : abs(point.y - last.point.y) > abs(point.x - last.point.x)
        var state = end.state
        _ = state.move(
            by: CGVector(dx: (point.x - last.point.x) * gain, dy: (point.y - last.point.y) * gain),
            at: time,
            vertical: vertical
        )
        // Nothing lays out more text for a selection, so an edge means the end, not a wait.
        _ = state.settle(at: time, lifting: true)
        end.state = state
        if role == .left { left = end } else { right = end }
    }

    enum Lift {
        /// Still one thumb down.
        case selecting
        /// Both lifted with no range: a normal trackpad lift.
        case ended
        /// Both lifted with a range: the actions show.
        case actions
    }

    func lift(touch id: Int) -> Lift {
        guard touchRoles[id] != nil, !lifted.contains(id) else { return .selecting }
        lifted.insert(id)
        guard lifted.count == touchRoles.count else { return .selecting }
        guard !range.isEmpty else {
            phase = .selecting
            return .ended
        }
        phase = .actions
        return .actions
    }

    /// The selection as UTF-16 offsets into `text`, from the internal ends' cursors.
    var range: Range<Int> {
        let a = offset(left)
        let b = offset(right)
        return min(a, b)..<max(a, b)
    }

    private func offset(_ end: End) -> Int {
        utf16[min(max(0, end.state.cursor), text.count)]
    }

    /// The character index at a UTF-16 offset this session's ends produced.
    private func character(at offset: Int) -> Int {
        var low = 0, high = utf16.count - 1
        while low < high {
            let mid = (low + high) / 2
            if utf16[mid] < offset { low = mid + 1 } else { high = mid }
        }
        return low
    }

    var selectedCharacters: Range<Int> {
        let range = range
        return character(at: range.lowerBound)..<character(at: range.upperBound)
    }

    var selectedText: String {
        String(text[selectedCharacters])
    }

    /// The offset from the host's cursor to the range's end, in UTF-16 units.
    var endFromAnchor: Int { range.upperBound - anchor }

    /// What taking the range out would leave, for putting it back if a dictation is cancelled.
    var removal: Removal {
        let characters = selectedCharacters
        return Removal(
            text: String(text[characters]),
            before: String(text[..<characters.lowerBound]),
            after: String(text[characters.upperBound...])
        )
    }

    /// What the strip shows: the text around the range, or its actions.
    var presentation: Presentation? {
        let range = range
        let characters = selectedCharacters
        let start = max(0, characters.lowerBound - Self.contextCharacters)
        let end = min(text.count, characters.upperBound + Self.contextCharacters)
        var before = String(text[start..<characters.lowerBound])
        if start > 0 { before = "…" + before }
        var selected = String(text[characters])
        if selected.count > Self.maximumSelectedCharacters { selected = String(selected.prefix(Self.maximumSelectedCharacters)) + "…" }
        var after = String(text[characters.upperBound..<end])
        if end < text.count { after += "…" }
        return Presentation(phase: phase, before: before, selected: selected, after: after)
    }

    /// How much of the text either side of the range the strip shows.
    static let contextCharacters = 20
    /// A long range is shown from its start, with the highlight running off the end of the line.
    static let maximumSelectedCharacters = 200
}
