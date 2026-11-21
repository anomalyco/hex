import AVFAudio

/// The `AVAudioSession` calls Hex makes, so tests can check how it records and lets go.
protocol AudioSessionControlling: AnyObject {
    var isOtherAudioPlaying: Bool { get }
    var availableInputs: [AVAudioSessionPortDescription]? { get }
    func setCategory(
        _ category: AVAudioSession.Category,
        mode: AVAudioSession.Mode,
        options: AVAudioSession.CategoryOptions
    ) throws
    func setPreferredInput(_ inPort: AVAudioSessionPortDescription?) throws
    func setActive(_ active: Bool, options: AVAudioSession.SetActiveOptions) throws
}

extension AVAudioSession: AudioSessionControlling {}

/// Lets other apps' audio that in-keyboard dictation interrupted play again.
///
/// The keyboard records through WebKit (`getUserMedia`), whose audio session lives in
/// WebKit's GPU process: WebKit sets `.playAndRecord` in `.videoChat` mode without
/// `.mixWithOthers`, and no page or app setting changes that. Going active, it interrupts
/// another app's video or music; ending, it deactivates without `.notifyOthersOnDeactivation`,
/// so the player is never told it may resume. When other audio played as the capture began
/// and nothing plays once it has ended, the keyboard activates its own session (nothing else
/// plays, so it interrupts nothing) and deactivates it with `.notifyOthersOnDeactivation`,
/// for players that honour `.shouldResume`. Whether iOS passes that on to a player WebKit
/// interrupted can only be seen on an iPhone: the simulator has no interruptions.
nonisolated enum InterruptedAudio {
    static func shouldAskToResume(playingAtStart: Bool, playingNow: Bool) -> Bool {
        playingAtStart && !playingNow
    }

    static func askToResume(_ session: AudioSessionControlling) throws {
        try session.setCategory(.playback, mode: .default, options: [])
        try session.setActive(true, options: [])
        try session.setActive(false, options: .notifyOthersOnDeactivation)
    }
}
