import Foundation

/// What the keyboard's capture page last said of its audio context and microphone track
/// (`KeyboardLocalDictation`), with the page's own time.
nonisolated struct KeyboardCapturePageState: Equatable, Sendable {
    /// What made the page say it: a track's `mute`, `unmute` or `ended`, the context's
    /// `statechange`, a change seen on its 250 ms check, or opening the microphone again.
    var cause: String
    /// The `AudioContext`'s state: `running`, `suspended`, `interrupted` or `closed`.
    var context: String
    var muted: Bool
    var ended: Bool
    var at: TimeInterval

    /// No audio is coming from the microphone: iOS gave the audio session to another sound (a
    /// notification, an alarm, a call), and WebKit suspends or interrupts the context and mutes
    /// or ends the track.
    var isInterrupted: Bool { context != "running" || muted || ended }

    var summary: String {
        "context \(context), track \(ended ? "ended" : muted ? "muted" : "live")"
    }
}

/// One recording's watchdog clock (`KeyboardJobWatchdog`): when its audio last came, and since
/// when its page has said the capture is interrupted. The same dictation keeps it across an
/// interruption: its quiet is counted again only from when the page says it's back.
nonisolated struct KeyboardCaptureWatch: Equatable, Sendable {
    enum Change: Equatable, Sendable {
        case interrupted
        case resumed(after: TimeInterval)
    }

    private(set) var lastAudio: TimeInterval
    private(set) var interruptedSince: TimeInterval?

    init(startedAt: TimeInterval) {
        lastAudio = startedAt
    }

    mutating func heardAudio(at time: TimeInterval) {
        lastAudio = max(lastAudio, time)
    }

    mutating func page(_ state: KeyboardCapturePageState, at time: TimeInterval) -> Change? {
        if state.isInterrupted {
            guard interruptedSince == nil else { return nil }
            interruptedSince = time
            return .interrupted
        }
        guard let since = interruptedSince else { return nil }
        interruptedSince = nil
        lastAudio = max(lastAudio, time)
        return .resumed(after: time - since)
    }

    func isStalled(now: TimeInterval) -> Bool {
        KeyboardJobWatchdog.isStalled(.recording, lastProgress: lastAudio, interruptedSince: interruptedSince, now: now)
    }
}

/// The completed transcript payload of one keyboard dictation, shared by the containing app
/// (`DictationController`) and in-keyboard dictation (`KeyboardLocalDictation`) so all six
/// fields are created and cleared together.
nonisolated struct KeyboardResult: Equatable, Sendable {
    let id: String
    let transcript: String
    let heard: String?
    let stages: DictationStages?
    let translationFailure: TranslationFailure?
    let expiresAt: TimeInterval?

    init(
        id: String,
        transcript: String,
        heard: String? = nil,
        stages: DictationStages? = nil,
        translationFailure: TranslationFailure? = nil,
        expiresAt: TimeInterval? = nil
    ) {
        self.id = id
        self.transcript = transcript
        self.heard = heard
        self.stages = stages
        self.translationFailure = translationFailure
        self.expiresAt = expiresAt
    }

    func isOfferExpired(at now: TimeInterval) -> Bool {
        guard let expiresAt else { return false }
        return expiresAt + KeyboardSnapshot.resultOfferWindow <= now
    }
}

/// The keyboard's own dictation phase (`KeyboardLocalDictation`): separates microphone startup
/// (`starting`, before the first real PCM audio buffer has arrived) from live capture
/// (`recording`), so the UI never claims to be listening before audio flows.
nonisolated enum KeyboardLocalCapturePhase: Equatable, Sendable {
    enum VisibleStage: Equatable, Sendable {
        case idle
        case starting
        case recording
        case transcribing
        case finished
        case failed
    }

    /// Startup progression before the first real PCM audio buffer arrives.
    enum StartupStage: Equatable, Sendable {
        /// Waiting on the audio-session queue before telling the capture page to start.
        case awaitingAudioSession
        /// Capture page start has been sent; waiting for WebKit's `.started` event.
        case awaitingPage
        /// WebKit `.started` opened the audio file; waiting for the first non-empty PCM buffer.
        case awaitingFirstAudio
    }

    /// A failure that occurred while armed at touch-down, surfaced only if the touch is kept.
    struct ArmedFailure: Equatable, Sendable {
        let message: String
        let detail: String?

        init(message: String, detail: String? = nil) {
            self.message = message
            self.detail = detail
        }
    }

    /// Readiness of a touch-down capture before the touch becomes a tap, hold, or slide.
    enum ArmedReadiness: Equatable, Sendable {
        case starting(StartupStage)
        case receivingAudio(startedAt: TimeInterval)
        case failed(ArmedFailure)
    }

    /// Result of keeping an armed touch-down capture.
    enum KeepArmedOutcome: Equatable, Sendable {
        case notArmed
        case starting(StartupStage)
        case recording(startedAt: TimeInterval)
        case failed(ArmedFailure)
    }

    case idle
    /// The Hex key's touch-down, before the touch is a tap, a hold or a slide: capturing,
    /// unseen (no snapshot), until kept or dropped.
    case armed(jobID: String, requestedAt: TimeInterval, readiness: ArmedReadiness)
    /// Dictation started, waiting for the audio session and WebKit capture to deliver the first
    /// real PCM buffer.
    case starting(jobID: String, requestedAt: TimeInterval, stage: StartupStage = .awaitingFirstAudio)
    /// At least one real PCM audio buffer has arrived: the microphone is live.
    case recording(jobID: String, startedAt: TimeInterval)
    case transcribing(jobID: String)
    case finished(jobID: String, result: KeyboardResult)
    case failed(message: String, until: TimeInterval)

    var isActive: Bool {
        switch self {
        case .armed, .starting, .recording, .transcribing: true
        case .idle, .finished, .failed: false
        }
    }

    var visibleStage: VisibleStage {
        switch self {
        case .idle, .armed: .idle
        case .starting: .starting
        case .recording: .recording
        case .transcribing: .transcribing
        case .finished: .finished
        case .failed: .failed
        }
    }

    /// True while `jobID` is actively capturing: armed (and not failed), starting, or recording.
    func isCapturing(_ jobID: String) -> Bool {
        switch self {
        case .armed(let current, _, let readiness):
            if case .failed = readiness { return false }
            return current == jobID
        case .starting(let current, _, _), .recording(let current, _):
            return current == jobID
        default:
            return false
        }
    }

    /// True while `jobID` is still waiting on the audio-session queue before calling `capture.start`.
    func isAwaitingAudioSession(_ jobID: String) -> Bool {
        switch self {
        case .armed(let current, _, .starting(.awaitingAudioSession)),
             .starting(let current, _, .awaitingAudioSession):
            current == jobID
        default:
            false
        }
    }

    /// Advances `jobID` from `.awaitingAudioSession` to `.awaitingPage`.
    @discardableResult
    mutating func markAudioSessionReady(jobID: String) -> Bool {
        switch self {
        case .armed(let current, let requestedAt, .starting(.awaitingAudioSession)) where current == jobID:
            self = .armed(jobID: current, requestedAt: requestedAt, readiness: .starting(.awaitingPage))
            return true
        case .starting(let current, let requestedAt, .awaitingAudioSession) where current == jobID:
            self = .starting(jobID: current, requestedAt: requestedAt, stage: .awaitingPage)
            return true
        default:
            return false
        }
    }

    /// Advances `jobID` from `.awaitingPage` to `.awaitingFirstAudio` once WebKit opens the stream.
    @discardableResult
    mutating func markPageStarted(jobID: String) -> Bool {
        switch self {
        case .armed(let current, let requestedAt, .starting(.awaitingPage)) where current == jobID:
            self = .armed(jobID: current, requestedAt: requestedAt, readiness: .starting(.awaitingFirstAudio))
            return true
        case .starting(let current, let requestedAt, .awaitingPage) where current == jobID:
            self = .starting(jobID: current, requestedAt: requestedAt, stage: .awaitingFirstAudio)
            return true
        default:
            return false
        }
    }

    /// Keeps a touch-down capture: goes straight to `recording` if audio already arrived while
    /// armed, enters `starting` if still waiting on startup, or surfaces a deferred armed failure.
    @discardableResult
    mutating func keepArmed(now: TimeInterval = 0, failureDuration: TimeInterval = 6) -> KeepArmedOutcome {
        guard case .armed(let jobID, let requestedAt, let readiness) = self else { return .notArmed }
        switch readiness {
        case .starting(let stage):
            self = .starting(jobID: jobID, requestedAt: requestedAt, stage: stage)
            return .starting(stage)
        case .receivingAudio(let audioStartedAt):
            self = .recording(jobID: jobID, startedAt: audioStartedAt)
            return .recording(startedAt: audioStartedAt)
        case .failed(let failure):
            self = .failed(message: failure.message, until: now + failureDuration)
            return .failed(failure)
        }
    }

    /// Records the arrival of a non-empty audio buffer for `jobID`.
    /// Only promotes once startup has opened the audio file (`.awaitingFirstAudio`).
    /// Returns `true` when this buffer promoted `starting` to `recording`.
    @discardableResult
    mutating func receiveAudio(jobID: String, at now: TimeInterval) -> Bool {
        switch self {
        case .armed(let current, let requestedAt, .starting(.awaitingFirstAudio)) where current == jobID:
            self = .armed(jobID: current, requestedAt: requestedAt, readiness: .receivingAudio(startedAt: now))
            return false
        case .starting(let current, _, .awaitingFirstAudio) where current == jobID:
            self = .recording(jobID: current, startedAt: now)
            return true
        default:
            return false
        }
    }

    /// Records a failure: defers it inside `.armed` (shown only if kept), or transitions to `.failed`.
    /// Returns `true` when the failure was deferred while armed.
    @discardableResult
    mutating func recordFailure(
        message: String,
        detail: String? = nil,
        now: TimeInterval,
        duration: TimeInterval = 6
    ) -> Bool {
        if case .armed(let jobID, let requestedAt, _) = self {
            self = .armed(
                jobID: jobID,
                requestedAt: requestedAt,
                readiness: .failed(ArmedFailure(message: message, detail: detail))
            )
            return true
        }
        self = .failed(message: message, until: now + duration)
        return false
    }

    /// Clears a `.failed` phase whose display window has elapsed.
    @discardableResult
    mutating func expireFailureIfNeeded(at now: TimeInterval) -> Bool {
        guard case .failed(_, let until) = self, until <= now else { return false }
        self = .idle
        return true
    }

    func snapshot(now: TimeInterval, loudnessDB: Float) -> KeyboardSnapshot? {
        switch self {
        case .idle, .armed:
            return nil
        case .starting(let jobID, _, _):
            return KeyboardSnapshot(
                state: .starting,
                heartbeat: now,
                expiresAt: now + 60,
                jobID: jobID
            )
        case .recording(let jobID, let startedAt):
            return KeyboardSnapshot(
                state: .recording,
                heartbeat: now,
                expiresAt: now + 60,
                recordingStartedAt: startedAt,
                loudnessDB: loudnessDB,
                jobID: jobID
            )
        case .transcribing(let jobID):
            return KeyboardSnapshot(state: .transcribing, heartbeat: now, expiresAt: now + 60, jobID: jobID)
        case .finished(let jobID, let result):
            return KeyboardSnapshot(
                state: .ready,
                heartbeat: now,
                expiresAt: now + 1,
                jobID: jobID,
                resultID: result.id,
                transcript: result.transcript,
                heard: result.heard,
                stages: result.stages,
                translationFailure: result.translationFailure
            )
        case .failed(let message, let until):
            guard until > now else { return nil }
            // A local failure is not a warm Hex session: heartbeat/expiresAt stay 0 so
            // `isWarmAvailable` stays false while `statusSlot` still shows the message.
            return KeyboardSnapshot(state: .failed, heartbeat: 0, expiresAt: 0, message: message, messageAt: now)
        }
    }
}
