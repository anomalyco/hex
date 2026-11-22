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

    var hasPendingSelectionDictation: Bool { trackpadSelectionState.pendingDictation != nil }

    /// A second finger landed: both thumbs move one end, starting from the host cursor. The
    /// finger further left takes the start of the range, the other the end.
    func beginTwoThumbSelectionIfEnabled(with id: Int, at point: CGPoint) {
        guard startsTwoThumbSelection,
              let spaceID = touches.first(where: { $0.value.isTrackpad })?.key,
              var space = touches[spaceID],
              var base = space.trackpad else { return }
        let time = clock()
        // Settle the trackpad as its lift would: if that moves the cursor, the host follows
        // it, so the selection's anchor is where the host's cursor really is.
        let settleOffset = base.settle(at: time, lifting: true)
        if settleOffset != 0 {
            space.trackpad = base
            space.trackpadOffset += settleOffset
            trackpadUpdated(space, id: spaceID)
            typingLog?.edit("cursor", [TypingLogField("offset", .int(settleOffset))])
            trackpadMovedCursor(by: settleOffset)
        }
        let hostBefore = target.documentContextBeforeInput ?? ""
        let hostAfter = target.documentContextAfterInput ?? ""
        let cursor = min(max(0, base.cursor), base.layout.text.count)
        let knownBefore = String(base.layout.text[..<cursor])
        let knownAfter = String(base.layout.text[cursor...])
        // Clamp to what the host shares when it can be matched; a context that hasn't caught
        // up (a prefix, not a suffix) leaves the whole span, and the action checks again.
        let sharedBefore = matchingBeforeLength(known: knownBefore, host: hostBefore) ?? knownBefore.utf16.count
        let sharedAfter = matchingAfterLength(known: knownAfter, host: hostAfter) ?? knownAfter.utf16.count
        let session = TrackpadSelectionSession(state: base, sharedBefore: sharedBefore, sharedAfter: sharedAfter)
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
        // The space bar's own touch was the trackpad's: the selection ends its trackpad, so
        // settle it, say where it got to, and let go of its touch. The host cursor doesn't
        // move: the selection's anchor is where it stopped.
        if var trackpad = touches[id], trackpad.isTrackpad {
            liftTrackpad(&trackpad, movingCursor: false)
            typingLog?.ui("trackpad_end", [
                TypingLogField("id", .int(id)),
                TypingLogField("offset", .int(trackpad.trackpadOffset)),
            ])
            typingLog?.context("trackpad")
        }
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
        typingLog?.ui("selection_action", [
            TypingLogField("action", .string("dismiss")),
            TypingLogField("reason", .string("key")),
        ])
        onChange?()
    }

    /// Delete, Cut, Copy, Dictate or Cancel on the finished range. `copy` writes the general
    /// pasteboard (the view gives it only with Full Access); true asks the view to start a
    /// dictation, with the range left in place for its words to replace.
    @discardableResult
    func performTrackpadSelectionAction(
        _ action: TrackpadSelectionSession.Action,
        copy: (String) -> Void
    ) -> Bool {
        guard let session = trackpadSelectionState.session, session.phase == .actions else { return false }
        let verification = verifySelection(session)
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
            guard case .ok = verification else { return dismissSelection(reason: Self.staleReason(verification)) }
            typingLog?.ui("selection_action", fields)
            copy(selected)
            let delta = session.endFromAnchor
            trackpadSelectionState.session = nil
            if delta != 0 {
                typingLog?.edit("cursor", [
                    TypingLogField("offset", .int(delta)),
                    TypingLogField("src", .string("selection")),
                ])
                trackpadMovedCursor(by: delta)
            } else {
                refreshSuggestions()
            }
            typingLog?.context("selection")
            onChange?()
        case .cut:
            guard case .ok = verification else { return dismissSelection(reason: Self.staleReason(verification)) }
            typingLog?.ui("selection_action", fields)
            copy(selected)
            deleteSelectedRange(session, source: "selection_cut")
            trackpadSelectionState.session = nil
            onChange?()
        case .delete:
            guard case .ok = verification else { return dismissSelection(reason: Self.staleReason(verification)) }
            typingLog?.ui("selection_action", fields)
            deleteSelectedRange(session, source: "selection")
            trackpadSelectionState.session = nil
            onChange?()
        case .dictate:
            guard case .ok = verification else {
                return dismissSelection(reason: Self.staleReason(verification))
            }
            typingLog?.ui("selection_action", fields)
            // Nothing is taken out now: the words replace the range only when they arrive.
            trackpadSelectionState.pendingDictation = PendingSelectionDictation(
                session: session,
                document: target.documentIdentifier
            )
            trackpadSelectionState.session = nil
            onChange?()
            return true
        }
        return false
    }

    /// Forgets a Dictate that will never replace its range (cancelled, empty, timed out, a
    /// key or field change, or another dictation). Nothing was removed, so nothing changes.
    func clearPendingSelectionDictation() {
        trackpadSelectionState.pendingDictation = nil
    }

    /// A dictation's text is about to go in: replace the range Dictate picked if the field
    /// still shows it unchanged, otherwise leave the field and let the text insert at the
    /// cursor as any dictation would. True when the range was replaced.
    @discardableResult
    func applyPendingSelectionDictation() -> Bool {
        guard let pending = trackpadSelectionState.pendingDictation else { return false }
        trackpadSelectionState.pendingDictation = nil
        let reason: String?
        if pending.document != target.documentIdentifier {
            reason = "field"
        } else if case .stale(let stale) = verifySelection(pending.session) {
            reason = stale
        } else {
            reason = nil
        }
        if let reason {
            typingLog?.ui("selection_action", [
                TypingLogField("action", .string("dictate")),
                TypingLogField("result", .string("inserted")),
                TypingLogField("reason", .string(reason)),
            ])
            return false
        }
        typingLog?.ui("selection_action", [
            TypingLogField("action", .string("dictate")),
            TypingLogField("result", .string("replaced")),
        ])
        deleteSelectedRange(pending.session, source: "selection_dictate")
        return true
    }

    /// The host's text changed while a selection was made or its actions showed: if the text
    /// the range was picked from no longer holds, close the selection rather than act on
    /// stale offsets. A pending Dictate replacement can no longer be trusted either.
    func selectionTextChanged() {
        trackpadSelectionState.pendingDictation = nil
        guard let session = trackpadSelectionState.session else { return }
        guard selectionStaleReason(session) != nil else { return }
        trackpadSelectionState.session = nil
        typingLog?.ui("selection_action", [
            TypingLogField("action", .string("dismiss")),
            TypingLogField("reason", .string("host_edit")),
        ])
        onChange?()
    }

    private func dismissSelection(reason: String) -> Bool {
        trackpadSelectionState.session = nil
        typingLog?.ui("selection_action", [
            TypingLogField("action", .string("dismiss")),
            TypingLogField("reason", .string(reason)),
        ])
        onChange?()
        return false
    }

    private enum SelectionVerification {
        case ok(before: Int, after: Int)
        case stale(String)
    }

    /// Why the range can't act on the field now, or nil when it can. The host shares only so
    /// much around the cursor, and that may be less than the session knows (the trackpad
    /// learns more at its edges): the parts both agree on must cover the whole range, in the
    /// host's and the session's own units.
    private func verifySelection(_ session: TrackpadSelectionSession) -> SelectionVerification {
        let hostBefore = target.documentContextBeforeInput ?? ""
        let hostAfter = target.documentContextAfterInput ?? ""
        guard let before = matchingBeforeLength(known: session.before, host: hostBefore),
              let after = matchingAfterLength(known: session.after, host: hostAfter) else {
            return .stale("text")
        }
        let lower = session.anchor - before
        let upper = session.anchor + after
        guard session.range.lowerBound >= lower, session.range.upperBound <= upper else {
            return .stale("context")
        }
        return .ok(before: before, after: after)
    }

    /// Test hook: the stale reason, or nil when the range can act.
    func selectionStaleReason(_ session: TrackpadSelectionSession) -> String? {
        if case .stale(let reason) = verifySelection(session) { return reason }
        return nil
    }

    private static func staleReason(_ verification: SelectionVerification) -> String {
        if case .stale(let reason) = verification { return reason }
        return "text"
    }

    /// UTF-16 units of before-text both share up to the cursor, or nil when they don't meet
    /// there. The shorter one must end the longer one; which is longer doesn't matter.
    private func matchingBeforeLength(known: String, host: String) -> Int? {
        if host.utf16.count <= known.utf16.count {
            return known.hasSuffix(host) ? host.utf16.count : nil
        }
        return host.hasSuffix(known) ? known.utf16.count : nil
    }

    /// UTF-16 units of after-text both share from the cursor, or nil when they don't meet.
    private func matchingAfterLength(known: String, host: String) -> Int? {
        if host.utf16.count <= known.utf16.count {
            return known.hasPrefix(host) ? host.utf16.count : nil
        }
        return host.hasPrefix(known) ? known.utf16.count : nil
    }

    /// Move to the range's end and take its characters out one by one: a 👍🏽 is one.
    private func deleteSelectedRange(_ session: TrackpadSelectionSession, source: String) {
        let delta = session.endFromAnchor
        if delta != 0 {
            typingLog?.edit("cursor", [
                TypingLogField("offset", .int(delta)),
                TypingLogField("src", .string(source)),
            ])
            target.adjustTextPosition(byCharacterOffset: delta)
        }
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
        typingLog?.context("selection")
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
