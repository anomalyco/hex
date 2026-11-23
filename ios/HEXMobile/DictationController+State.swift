import Foundation

extension DictationController {
    /// A user-facing message paired with the wall-clock time it was recorded, so
    /// `KeyboardSnapshot.message` and `messageAt` can never drift apart.
    nonisolated struct TimedMessage: Equatable, Sendable {
        let text: String
        let at: TimeInterval
    }

    /// The speech model's preparation lifecycle, orthogonal to any capture in flight.
    nonisolated enum ModelState: Equatable, Sendable {
        case required
        case preparing
        case ready
        case failed(TimedMessage)

        var failureMessage: TimedMessage? {
            if case .failed(let message) = self { return message }
            return nil
        }
    }

    /// The warm keyboard session's state: either cold, or warm with an expiry and whether an
    /// audio interruption suspended its keepalive. Replaces the `keyboardSessionEnabled` /
    /// `keyboardSessionExpiresAt` / `suspendedWarmSessionOnInterruption` triple so teardown
    /// cannot drift across call sites.
    nonisolated enum Session: Equatable, Sendable {
        case cold
        case warm(expiresAt: TimeInterval, suspendedOnInterruption: Bool = false)

        var isWarm: Bool {
            if case .warm = self { return true }
            return false
        }

        var expiresAt: TimeInterval {
            if case .warm(let expiresAt, _) = self { return expiresAt }
            return 0
        }

        var isSuspendedOnInterruption: Bool {
            if case .warm(_, let suspended) = self { return suspended }
            return false
        }

        func remaining(at now: TimeInterval) -> TimeInterval {
            guard case .warm(let expiresAt, _) = self else { return 0 }
            return expiresAt - now
        }

        func suspendingForInterruption() -> Session {
            guard case .warm(let expiresAt, _) = self else { return .cold }
            return .warm(expiresAt: expiresAt, suspendedOnInterruption: true)
        }

        func clearingInterruption() -> Session {
            guard case .warm(let expiresAt, _) = self else { return .cold }
            return .warm(expiresAt: expiresAt, suspendedOnInterruption: false)
        }

        func refreshing(expiresAt: TimeInterval) -> Session {
            guard case .warm(_, let suspended) = self else { return .cold }
            return .warm(expiresAt: expiresAt, suspendedOnInterruption: suspended)
        }
    }

    /// Backoff schedule for restarting a warm keepalive engine that stops by itself. Each
    /// restart activates the session and reopens the microphone under every app recording at
    /// the time (a screen recording too), so one that keeps stopping soon after its restart
    /// waits twice as long each time (1 s, 2 s, … 30 s), not 100 ms; one that ran steadily
    /// restarts at once.
    nonisolated struct KeepaliveBackoff: Equatable, Sendable {
        static let steadyDuration: TimeInterval = 10
        static let maxDelay: TimeInterval = 30

        private(set) var lastRestartedAt: TimeInterval = -.infinity
        private(set) var delay: TimeInterval = 0
        private(set) var restartDueAt: TimeInterval?

        mutating func reset() {
            lastRestartedAt = -.infinity
            delay = 0
            restartDueAt = nil
        }

        mutating func noteRunning() {
            restartDueAt = nil
        }

        /// Returns `true` when a restart should be attempted at `now`, advancing the schedule.
        mutating func shouldRestart(at now: TimeInterval) -> Bool {
            if restartDueAt == nil {
                let stoppedSoon = now - lastRestartedAt < Self.steadyDuration
                delay = stoppedSoon
                    ? min(max(1, delay * 2), Self.maxDelay)
                    : 0
                restartDueAt = now + delay
            }
            guard let due = restartDueAt, now >= due else { return false }
            restartDueAt = nil
            lastRestartedAt = now
            return true
        }
    }

    /// Whether a capture or transcription belongs to a keyboard dictation job.
    nonisolated enum KeyboardJob: Equatable, Sendable {
        case none
        case active(id: String)

        init(id: String?) {
            if let id {
                self = .active(id: id)
            } else {
                self = .none
            }
        }

        var id: String? {
            if case .active(let id) = self { return id }
            return nil
        }

        var isActive: Bool {
            if case .active = self { return true }
            return false
        }
    }

    @MainActor
    struct ActiveCaptureSession {
        let generation: UInt64
        let capture: any ActiveAudioCapture
        let startedAt: TimeInterval
        let keyboardJob: KeyboardJob
        let preferences: DictationPipeline.Preferences
        let usesCleanup: Bool
        /// Whether the keyboard gets the raw transcript to learn from.
        let sharesHeard: Bool
        let copyToPasteboard: Bool
        /// The transcriber's language (BCP 47) when the capture started.
        let language: String

        var keyboardJobID: String? { keyboardJob.id }
    }

    /// The dictation capture lifecycle. Holding `ActiveCaptureSession` inside `.recording`
    /// makes `.recording` without a live capture unrepresentable, and holding `KeyboardJob`
    /// inside `.opening`/`.recording`/`.transcribing` ensures active keyboard job state clears
    /// automatically on every exit.
    @MainActor
    enum Capture {
        case idle
        case opening(generation: UInt64, job: KeyboardJob)
        case recording(ActiveCaptureSession)
        case transcribing(generation: UInt64, job: KeyboardJob)
        /// A capture, permission, keepalive, or transcription failure.
        /// `retryableWithModelReady` is true only for capture/transcription failures with a
        /// loaded model, letting the keyboard's next Hex key retry without reopening Hex.
        case failed(message: TimedMessage, retryableWithModelReady: Bool)

        var activeSession: ActiveCaptureSession? {
            if case .recording(let session) = self { return session }
            return nil
        }

        var keyboardJob: KeyboardJob {
            switch self {
            case .opening(_, let job), .transcribing(_, let job):
                job
            case .recording(let session):
                session.keyboardJob
            case .idle, .failed:
                .none
            }
        }

        var keyboardJobID: String? { keyboardJob.id }

        var isRecording: Bool {
            if case .recording = self { return true }
            return false
        }

        var isTranscribing: Bool {
            if case .transcribing = self { return true }
            return false
        }

        var isBusy: Bool {
            isRecording || isTranscribing
        }

        var allowsKeyboardRetry: Bool {
            if case .failed(_, let retryable) = self { return retryable }
            return false
        }

        var failureMessage: TimedMessage? {
            if case .failed(let message, _) = self { return message }
            return nil
        }
    }
}
