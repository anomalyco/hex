import ActivityKit
import Foundation

@MainActor
final class LiveActivityCoordinator {
    private var currentActivityID: String?

    func startRecording(startedAt: Date) {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return }
        let state = HexDictationActivityAttributes.ContentState(
            stage: .recording,
            startedAt: startedAt,
            subtitle: "Recording on this iPhone"
        )
        if let id = currentActivityID {
            Task.detached {
                await Self.updateActivity(id: id, state: state, staleDate: startedAt.addingTimeInterval(90))
            }
            return
        }

        let attributes = HexDictationActivityAttributes(sessionID: UUID().uuidString)
        if let created = try? Activity.request(
            attributes: attributes,
            content: ActivityContent(state: state, staleDate: startedAt.addingTimeInterval(90)),
            pushType: nil
        ) {
            currentActivityID = created.id
        }
    }

    func markTranscribing(startedAt: Date) {
        guard let id = currentActivityID else { return }
        let state = HexDictationActivityAttributes.ContentState(
            stage: .transcribing,
            startedAt: startedAt,
            subtitle: "Transcribing locally…"
        )
        Task.detached {
            await Self.updateActivity(id: id, state: state, staleDate: Date().addingTimeInterval(30))
        }
    }

    func finishCopied() {
        guard let id = currentActivityID else { return }
        currentActivityID = nil
        let state = HexDictationActivityAttributes.ContentState(
            stage: .copied,
            startedAt: Date(),
            subtitle: "Copied to clipboard"
        )
        Task.detached {
            await Self.endActivity(
                id: id,
                state: state,
                staleDate: Date().addingTimeInterval(4),
                dismissAfterSeconds: 2
            )
        }
    }

    func endImmediately() {
        let id = currentActivityID
        currentActivityID = nil
        Task.detached {
            await Self.endAllOrMatching(id: id)
        }
    }

    nonisolated private static func updateActivity(
        id: String,
        state: HexDictationActivityAttributes.ContentState,
        staleDate: Date?
    ) async {
        guard let activity = Activity<HexDictationActivityAttributes>.activities.first(where: { $0.id == id }) else {
            return
        }
        await activity.update(ActivityContent(state: state, staleDate: staleDate))
    }

    nonisolated private static func endActivity(
        id: String,
        state: HexDictationActivityAttributes.ContentState,
        staleDate: Date?,
        dismissAfterSeconds: TimeInterval
    ) async {
        guard let activity = Activity<HexDictationActivityAttributes>.activities.first(where: { $0.id == id }) else {
            return
        }
        await activity.end(
            ActivityContent(state: state, staleDate: staleDate),
            dismissalPolicy: .after(Date().addingTimeInterval(dismissAfterSeconds))
        )
    }

    nonisolated private static func endAllOrMatching(id: String?) async {
        let state = HexDictationActivityAttributes.ContentState(
            stage: .copied,
            startedAt: Date(),
            subtitle: "Done"
        )
        for activity in Activity<HexDictationActivityAttributes>.activities where id == nil || activity.id == id {
            await activity.end(
                ActivityContent(state: state, staleDate: nil),
                dismissalPolicy: .immediate
            )
        }
    }
}
