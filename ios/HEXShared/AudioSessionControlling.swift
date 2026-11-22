import AVFAudio

/// The `AVAudioSession` calls Hex makes, so tests can check how it records and lets go.
protocol AudioSessionControlling: AnyObject {
    var isOtherAudioPlaying: Bool { get }
    var category: AVAudioSession.Category { get }
    var mode: AVAudioSession.Mode { get }
    var categoryOptions: AVAudioSession.CategoryOptions { get }
    /// The inputs the current category offers.
    var microphones: [Microphone] { get }
    func setCategory(
        _ category: AVAudioSession.Category,
        mode: AVAudioSession.Mode,
        options: AVAudioSession.CategoryOptions
    ) throws
    /// Records from `microphone`, or from iOS's choice for nil.
    func prefer(_ microphone: Microphone?) throws
    func setActive(_ active: Bool, options: AVAudioSession.SetActiveOptions) throws
}

extension AVAudioSession: AudioSessionControlling {
    var microphones: [Microphone] {
        (availableInputs ?? []).map(Microphone.init)
    }

    func prefer(_ microphone: Microphone?) throws {
        try setPreferredInput(microphone.flatMap { wanted in
            availableInputs?.first { $0.uid == wanted.uid }
        })
    }
}

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
