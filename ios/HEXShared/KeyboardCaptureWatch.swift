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
