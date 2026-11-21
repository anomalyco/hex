import UIKit

/// Settings › Experiments › Two-thumb selection: while the space bar is the trackpad, a second
/// finger picks a range instead of being ignored. The host's selection can't be set from a
/// keyboard, so the range is shown in the strip and the actions (Delete, Cut, Copy, Dictate,
/// Cancel) change the text through the proxy. See `TrackpadSelectionSession`.
@MainActor
extension FullKeyboardEngine {
    /// A second finger on the trackpad starts a selection, rather than typing the blank key
    /// under it: Settings › Experiments › Two-thumb selection, off by default.
    var startsTwoThumbSelection: Bool {
        trackpadSelectionState.enabled && trackpadSelectionState.session == nil && isTrackpadActive
    }

    /// A selection is being made or its actions show.
    var isSelectionActive: Bool { trackpadSelectionState.session != nil }

    /// Both thumbs lifted with a range: the strip shows the actions.
    var selectionActionsShown: Bool { trackpadSelectionState.session?.phase == .actions }

    /// What the strip draws, or nil when no selection is on.
    var selectionPresentation: TrackpadSelectionSession.Presentation? {
        trackpadSelectionState.session?.presentation
    }

    var hasPendingSelectionDictation: Bool { trackpadSelectionState.pendingRemoval != nil }

    /// A second finger landed: both thumbs move one end, starting from the host cursor. The
    /// finger further left takes the start of the range, the other the end.
    func beginTwoThumbSelectionIfEnabled(with id: Int, at point: CGPoint) {
        guard startsTwoThumbSelection,
              let spaceID = touches.first(where: { $0.value.isTrackpad })?.key,
              let space = touches[spaceID] else { return }
        let before = target.documentContextBeforeInput ?? ""
        let after = target.documentContextAfterInput ?? ""
        let session = TrackpadSelectionSession(
            before: before,
            after: after,
            wrapWidth: TrackpadCursor.wrapWidth(forWidth: width),
            lineHeight: Self.trackpadFont().lineHeight,
            width: Self.trackpadWidth
        )
        let time = clock()
        let second: TrackpadSelectionSession.Role = point.x < space.point.x ? .left : .right
        session.add(touch: id, role: second, point: point, time: time)
        session.add(touch: spaceID, role: second == .left ? .right : .left, point: space.point, time: time)
        trackpadSelectionState.session = session
        typingLog?.ui("selection_start", [
            TypingLogField("id", .int(id)),
            TypingLogField("thumb", .string("second")),
            TypingLogField("anchor", .int(session.anchor)),
        ])
        onChange?()
    }

    /// A selection finger moved: it moves its own end, not the host's cursor.
    func moveTwoThumbSelection(id: Int, to point: CGPoint, time: TimeInterval?) -> Bool {
        guard let session = trackpadSelectionState.session, session.touchRoles[id] != nil else { return false }
        session.move(touch: id, to: point, time: time ?? clock())
        onChange?()
        return true
    }

    /// A selection finger lifted: with both up and a range, the strip offers its actions; an
    /// empty range ends like a normal trackpad lift.
    func endTwoThumbSelection(id: Int, at point: CGPoint) -> Bool {
        guard let session = trackpadSelectionState.session, session.touchRoles[id] != nil else { return false }
        switch session.lift(touch: id) {
        case .selecting:
            break
        case .ended:
            typingLog?.ui("selection_end", [
                TypingLogField("empty", .bool(true)),
                TypingLogField("id", .int(id)),
            ])
            typingLog?.ui("trackpad_end", [
                TypingLogField("id", .int(id)),
                TypingLogField("offset", .int(0)),
            ])
            typingLog?.context("trackpad")
            trackpadSelectionState.session = nil
        case .actions:
            let range = session.range
            typingLog?.ui("selection_end", [
                TypingLogField("empty", .bool(false)),
                TypingLogField("id", .int(id)),
                TypingLogField("start", .int(range.lowerBound - session.anchor)),
                TypingLogField("end", .int(range.upperBound - session.anchor)),
            ])
        }
        // The space bar's own touch was the trackpad's: it is over with the selection, whatever
        // end it moved, so the glyphs come back and no trackpad lift runs later.
        removeSelectionTouch(id)
        onChange?()
        return true
    }

    /// The system took a selection finger away: nothing changes in the host.
    func cancelTwoThumbSelection(id: Int) -> Bool {
        guard let session = trackpadSelectionState.session, session.touchRoles[id] != nil else { return false }
        for touch in session.touchRoles.keys {
            removeSelectionTouch(touch)
        }
        trackpadSelectionState.session = nil
        typingLog?.ui("selection_cancel", [TypingLogField("id", .int(id))])
        onChange?()
        return true
    }

    /// Any other key going down while the actions show dismisses them without acting.
    func dismissTrackpadSelectionIfNeeded() {
        guard let session = trackpadSelectionState.session, session.phase == .actions else { return }
        trackpadSelectionState.session = nil
        typingLog?.ui("selection_action", [TypingLogField("action", .string("dismiss"))])
        onChange?()
    }

    /// Delete, Cut, Copy, Dictate or Cancel on the finished range. `copy` writes the general
    /// pasteboard (the view gives it only with Full Access); true asks the view to start a
    /// dictation, after the range is already out.
    @discardableResult
    func performTrackpadSelectionAction(
        _ action: TrackpadSelectionSession.Action,
        copy: (String) -> Void
    ) -> Bool {
        guard let session = trackpadSelectionState.session, session.phase == .actions else { return false }
        let range = session.range
        let selected = session.selectedText
        let fields = [
            TypingLogField("action", .string(Self.selectionActionName(action))),
            TypingLogField("start", .int(range.lowerBound - session.anchor)),
            TypingLogField("end", .int(range.upperBound - session.anchor)),
        ]
        switch action {
        case .cancel:
            typingLog?.ui("selection_action", fields)
            trackpadSelectionState.session = nil
            onChange?()
        case .copy:
            guard selectionContextMatches(session) else { return dismissSelection() }
            typingLog?.ui("selection_action", fields)
            copy(selected)
            let delta = session.endFromAnchor
            trackpadSelectionState.session = nil
            if delta != 0 { trackpadMovedCursor(by: delta) } else { refreshSuggestions() }
            onChange?()
        case .cut:
            guard selectionContextMatches(session) else { return dismissSelection() }
            typingLog?.ui("selection_action", fields)
            copy(selected)
            deleteSelectedRange(session, source: "selection_cut")
            trackpadSelectionState.session = nil
            onChange?()
        case .delete:
            guard selectionContextMatches(session) else { return dismissSelection() }
            typingLog?.ui("selection_action", fields)
            deleteSelectedRange(session, source: "selection")
            trackpadSelectionState.session = nil
            onChange?()
        case .dictate:
            guard selectionContextMatches(session) else { return dismissSelection() }
            typingLog?.ui("selection_action", fields)
            let removal = session.removal
            deleteSelectedRange(session, source: "selection_dictate")
            trackpadSelectionState.pendingRemoval = removal
            trackpadSelectionState.session = nil
            onChange?()
            return true
        }
        return false
    }

    /// Puts back what an action took out if the field still shows the text it left: a dictation
    /// that was cancelled or heard nothing. The keys' own insert calls `finishSelectionDictation`.
    func restoreSelectionDictation() {
        guard let removal = trackpadSelectionState.pendingRemoval else { return }
        trackpadSelectionState.pendingRemoval = nil
        let before = target.documentContextBeforeInput ?? ""
        let after = target.documentContextAfterInput ?? ""
        guard before.hasSuffix(String(removal.before.suffix(40))),
              after.hasPrefix(String(removal.after.prefix(40))) else { return }
        typingLog?.ui("selection_action", [
            TypingLogField("action", .string("restore")),
            TypingLogField("length", .int(removal.text.count)),
        ])
        typingLog?.edit("insert", [
            removal.text.count <= 80
                ? TypingLogField("inserted", .string(removal.text))
                : TypingLogField("insertedCount", .int(removal.text.count)),
            TypingLogField("src", .string("selection_restore")),
        ])
        target.insertText(removal.text)
        didEdit()
    }

    /// A dictation put its own text in, so there is nothing to put back.
    func finishSelectionDictation() {
        trackpadSelectionState.pendingRemoval = nil
    }

    private func dismissSelection() -> Bool {
        trackpadSelectionState.session = nil
        onChange?()
        return false
    }

    /// The host still shows the text the range was picked from (an edit or a moved cursor
    /// elsewhere means the offsets no longer hold).
    private func selectionContextMatches(_ session: TrackpadSelectionSession) -> Bool {
        let before = target.documentContextBeforeInput ?? ""
        let after = target.documentContextAfterInput ?? ""
        return before.hasSuffix(String(session.before.suffix(40)))
            && after.hasPrefix(String(session.after.prefix(40)))
    }

    /// Move to the range's end and take its characters out one by one: a 👍🏽 is one.
    private func deleteSelectedRange(_ session: TrackpadSelectionSession, source: String) {
        let delta = session.endFromAnchor
        if delta != 0 { target.adjustTextPosition(byCharacterOffset: delta) }
        let characters = session.selectedCharacters.count
        let selected = session.selectedText
        typingLog?.edit("delete", [
            selected.count <= 80
                ? TypingLogField("deleted", .string(selected))
                : TypingLogField("deletedCount", .int(characters)),
            TypingLogField("src", .string(source)),
        ])
        for _ in 0..<characters { target.deleteBackward() }
        didEdit()
    }

    private static func selectionActionName(_ action: TrackpadSelectionSession.Action) -> String {
        switch action {
        case .delete: "delete"
        case .cut: "cut"
        case .copy: "copy"
        case .dictate: "dictate"
        case .cancel: "cancel"
        }
    }
}
