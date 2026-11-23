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

    case idle
    /// The Hex key's touch-down, before the touch is a tap, a hold or a slide: capturing,
    /// unseen (no snapshot), until kept or dropped. `audioStartedAt` records when the first
    /// real audio buffer arrived while armed, if any.
    case armed(jobID: String, requestedAt: TimeInterval, audioStartedAt: TimeInterval?)
    /// Dictation started, waiting for the audio session and WebKit capture to deliver the first
    /// real PCM buffer.
    case starting(jobID: String, requestedAt: TimeInterval)
    /// At least one real PCM audio buffer has arrived: the microphone is live.
    case recording(jobID: String, startedAt: TimeInterval)
    case transcribing(jobID: String)
    case finished(
        jobID: String,
        resultID: String,
        transcript: String,
        heard: String?,
        stages: DictationStages?,
        translationFailure: TranslationFailure?
    )
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

    /// True while `jobID` owns capture: armed at touch-down, starting up, or actively recording.
    func isCapturing(_ jobID: String) -> Bool {
        switch self {
        case .armed(let current, _, _), .starting(let current, _), .recording(let current, _):
            current == jobID
        default:
            false
        }
    }

    /// Keeps a touch-down capture: goes straight to `recording` if audio already arrived while
    /// armed, otherwise enters `starting` until the first buffer arrives.
    @discardableResult
    mutating func keepArmed() -> Bool {
        guard case .armed(let jobID, let requestedAt, let audioStartedAt) = self else { return false }
        if let audioStartedAt {
            self = .recording(jobID: jobID, startedAt: audioStartedAt)
        } else {
            self = .starting(jobID: jobID, requestedAt: requestedAt)
        }
        return true
    }

    /// Records the arrival of a non-empty audio buffer for `jobID`.
    /// Returns `true` when this buffer promoted `starting` to `recording`.
    @discardableResult
    mutating func receiveAudio(jobID: String, at now: TimeInterval) -> Bool {
        switch self {
        case .armed(let current, let requestedAt, let audioStartedAt) where current == jobID:
            if audioStartedAt == nil {
                self = .armed(jobID: current, requestedAt: requestedAt, audioStartedAt: now)
            }
            return false
        case .starting(let current, _) where current == jobID:
            self = .recording(jobID: current, startedAt: now)
            return true
        default:
            return false
        }
    }

    func snapshot(now: TimeInterval, loudnessDB: Float) -> KeyboardSnapshot? {
        switch self {
        case .idle, .armed:
            return nil
        case .starting(let jobID, _):
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
        case .finished(let jobID, let resultID, let transcript, let heard, let stages, let translationFailure):
            return KeyboardSnapshot(
                state: .ready,
                heartbeat: now,
                expiresAt: now + 1,
                jobID: jobID,
                resultID: resultID,
                transcript: transcript,
                heard: heard,
                stages: stages,
                translationFailure: translationFailure
            )
        case .failed(let message, let until):
            guard until > now else { return nil }
            return KeyboardSnapshot(state: .failed, heartbeat: now, expiresAt: until, message: message, messageAt: now)
        }
    }
}
