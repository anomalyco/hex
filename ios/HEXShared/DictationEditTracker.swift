import Foundation

/// Follows each dictation Hex inserts for up to a minute and reports what became of it:
/// kept, edited (with word pairs), rewritten, undone, deleted, or dictated again. It
/// holds the dictation's text only in memory while it follows it.
@MainActor
final class DictationEditTracker {
    static let editWindow: TimeInterval = 60
    static let redictationWindow: TimeInterval = 20

    /// The text around the cursor, as the host shows it.
    struct Context: Equatable {
        var before: String?
        var after: String?
        var hasText: Bool
    }

    private struct Tracked {
        let insertion: DictationInsertion
        let heard: String?
        let insertedAt: TimeInterval
        var latest: DictationSpan
        /// True once the dictation was seen in place; nothing is reported before that.
        var located = false
        var emptiedAt: TimeInterval?
        var undone = false
    }

    private var tracked: Tracked?
    /// A dictation you removed, held for the redictation window in case you dictate again.
    private var removed: (tracked: Tracked, at: TimeInterval)?
    private let now: () -> TimeInterval
    var onResult: ((DictationEditResult) -> Void)?

    init(now: @escaping () -> TimeInterval = { Date().timeIntervalSince1970 }) {
        self.now = now
    }

    var isTracking: Bool { tracked != nil || removed != nil }

    /// Hex is about to insert `inserted` at the cursor; `context` is the text around it
    /// now. Settles the dictation before, pairing it with this one if you just removed it.
    /// `heard` is the raw transcript, or nil when its words can't be learned from.
    func willInsert(heard: String?, inserted: String, context: Context) {
        let time = now()
        if var previous = tracked {
            tracked = nil
            _ = update(&previous, with: context, fromKeys: false)
            settle(previous, next: inserted, at: time)
        } else if let pending = removed {
            removed = nil
            if time - pending.at <= Self.redictationWindow {
                emit(DictationEditClassifier.redictation(
                    previousHeard: pending.tracked.heard,
                    previousInserted: pending.tracked.insertion.inserted,
                    nextInserted: inserted
                ))
            } else {
                emitRemoval(pending.tracked)
            }
        }
        tracked = Tracked(
            insertion: DictationInsertion(inserted: inserted, before: context.before, after: context.after),
            heard: heard,
            insertedAt: time,
            latest: DictationSpan(text: inserted, startIsCertain: true, endIsCertain: true)
        )
    }

    /// The text changed: by Hex's own keys (`fromKeys`) or by the host (cursor moves,
    /// a send that clears the field, another field).
    func observe(_ context: Context, fromKeys: Bool) {
        guard var current = tracked else { return }
        if update(&current, with: context, fromKeys: fromKeys) {
            tracked = current
        } else {
            tracked = nil
            settle(current, next: nil, at: now())
        }
    }

    /// Undo dictation took the dictation back.
    func didUndo() {
        guard var current = tracked else { return }
        current.latest.text = ""
        current.located = true
        current.undone = true
        current.emptiedAt = now()
        tracked = current
    }

    /// The keyboard is going away, the field changed, or you sent: report everything now.
    func finish(_ context: Context?) {
        if var current = tracked {
            tracked = nil
            if let context {
                _ = update(&current, with: context, fromKeys: false)
            }
            settle(current, next: nil, at: now())
        }
        if let pending = removed {
            removed = nil
            emitRemoval(pending.tracked)
        }
    }

    /// Called on the keyboard's poll: ends the edit and redictation windows.
    func tick(_ context: () -> Context) {
        let time = now()
        if let pending = removed, time - pending.at > Self.redictationWindow {
            removed = nil
            emitRemoval(pending.tracked)
        }
        guard var current = tracked else { return }
        if let emptiedAt = current.emptiedAt {
            if time - emptiedAt > Self.redictationWindow {
                tracked = nil
                emitRemoval(current)
            }
        } else if time - current.insertedAt >= Self.editWindow {
            tracked = nil
            _ = update(&current, with: context(), fromKeys: false)
            settle(current, next: nil, at: time)
        }
    }

    /// Updates the dictation's place from the context; false when it can no longer be
    /// followed (the host cleared the field, or the anchors are gone), so it settles on the
    /// last place it was seen. Until it was first seen in place, the host may simply be
    /// behind, so it keeps waiting (the edit window or the keyboard going away ends that).
    private func update(_ current: inout Tracked, with context: Context, fromKeys: Bool) -> Bool {
        guard context.hasText else {
            guard current.located else { return true }
            // Hex's delete key emptied the field: a deletion. The host emptying it is a send.
            guard fromKeys || !current.latest.hasWords else { return false }
            current.latest.text = ""
            current.emptiedAt = current.emptiedAt ?? now()
            return true
        }
        guard let span = DictationSpanLocator.locate(current.insertion, before: context.before, after: context.after) else {
            return !current.located
        }
        if span.hasWords {
            current.latest = span
            current.located = true
            current.emptiedAt = nil
            current.undone = false
        } else if current.located {
            // Before it was ever seen in place an empty gap means the host is behind, not a deletion.
            current.latest = span
            current.emptiedAt = current.emptiedAt ?? now()
        }
        return true
    }

    private func settle(_ current: Tracked, next inserted: String?, at time: TimeInterval) {
        guard current.located else { return }
        guard !current.latest.hasWords else {
            emit(DictationEditClassifier.classify(
                heard: current.heard,
                inserted: current.insertion.inserted,
                final: current.latest
            ))
            return
        }
        let emptiedAt = current.emptiedAt ?? time
        if time - emptiedAt > Self.redictationWindow {
            emitRemoval(current)
        } else if let inserted {
            emit(DictationEditClassifier.redictation(
                previousHeard: current.heard,
                previousInserted: current.insertion.inserted,
                nextInserted: inserted
            ))
        } else {
            removed = (current, emptiedAt)
        }
    }

    private func emitRemoval(_ current: Tracked) {
        emit(DictationEditClassifier.classify(
            heard: current.heard,
            inserted: current.insertion.inserted,
            final: DictationSpan(text: "", startIsCertain: true, endIsCertain: true),
            undone: current.undone
        ))
    }

    private func emit(_ result: DictationEditResult?) {
        guard let result else { return }
        onResult?(result)
    }
}
