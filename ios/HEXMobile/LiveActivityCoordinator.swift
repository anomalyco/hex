import ActivityKit
import Foundation
import UIKit

/// The Dynamic Island and Lock Screen activity of a dictation started by the Action Button,
/// Control Center, Siri, or Shortcuts. Ending one also ends every other Hex activity, so none
/// outlives its dictation; each end runs under a background task, so a suspending app can't
/// leave one behind.
@MainActor
final class LiveActivityCoordinator {
    private var currentActivityID: String?
    /// Holds the app awake from transcribing until the activity ends: the microphone, which
    /// kept it running in the background, is already released.
    private var transcribingAssertion: BackgroundAssertion?

    func startRecording(startedAt: Date) {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return }
        let state = HexDictationActivityAttributes.ContentState(
            stage: .recording,
            startedAt: startedAt,
            subtitle: "Recording on this iPhone"
        )
        if let id = currentActivityID, Self.isActive(id) {
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
        if transcribingAssertion == nil {
            transcribingAssertion = BackgroundAssertion(name: "Hex transcription")
        }
        let state = HexDictationActivityAttributes.ContentState(
            stage: .transcribing,
            startedAt: startedAt,
            subtitle: "Transcribing locally…"
        )
        Task.detached {
            await Self.updateActivity(id: id, state: state, staleDate: Date().addingTimeInterval(30))
        }
    }

    /// Shows Copied for 2 s, then the activity goes; any other Hex activity ends now.
    func finishCopied() {
        guard let id = currentActivityID else {
            endImmediately()
            return
        }
        currentActivityID = nil
        let others = Self.activityIDs().filter { $0 != id }
        let state = HexDictationActivityAttributes.ContentState(
            stage: .copied,
            startedAt: Date(),
            subtitle: "Copied to clipboard"
        )
        runEnding {
            await Self.endActivities(ids: others)
            await Self.endActivity(
                id: id,
                state: state,
                staleDate: Date().addingTimeInterval(4),
                dismissAfterSeconds: 2
            )
        }
    }

    /// Ends every Hex activity there is now, the current one and any a suspended or
    /// killed process left behind. One a later dictation starts is not touched.
    func endImmediately() {
        currentActivityID = nil
        let ids = Self.activityIDs()
        guard !ids.isEmpty else {
            transcribingAssertion?.end()
            transcribingAssertion = nil
            return
        }
        runEnding {
            await Self.endActivities(ids: ids)
        }
    }

    private func runEnding(_ work: @escaping @Sendable () async -> Void) {
        let assertion = BackgroundAssertion(name: "Hex Live Activity end")
        let transcribing = transcribingAssertion
        transcribingAssertion = nil
        Task { @MainActor in
            await work()
            assertion.end()
            transcribing?.end()
        }
    }

    private static func activityIDs() -> [String] {
        Activity<HexDictationActivityAttributes>.activities.map(\.id)
    }

    private static func isActive(_ id: String) -> Bool {
        Activity<HexDictationActivityAttributes>.activities.contains { $0.id == id && $0.activityState == .active }
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

    nonisolated private static func endActivities(ids: [String]) async {
        let state = HexDictationActivityAttributes.ContentState(
            stage: .copied,
            startedAt: Date(),
            subtitle: "Done"
        )
        for activity in Activity<HexDictationActivityAttributes>.activities where ids.contains(activity.id) {
            await activity.end(
                ActivityContent(state: state, staleDate: nil),
                dismissalPolicy: .immediate
            )
        }
    }
}

/// A UIKit background task, ended once (or by the system's expiry).
@MainActor
private final class BackgroundAssertion {
    private var id = UIBackgroundTaskIdentifier.invalid

    init(name: String) {
        id = UIApplication.shared.beginBackgroundTask(withName: name) { [weak self] in
            MainActor.assumeIsolated { self?.end() }
        }
    }

    func end() {
        guard id != .invalid else { return }
        UIApplication.shared.endBackgroundTask(id)
        id = .invalid
    }
}
