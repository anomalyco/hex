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
    func setPreferredSampleRate(_ sampleRate: Double) throws
    func setPreferredInput(_ inPort: AVAudioSessionPortDescription?) throws
    func setActive(_ active: Bool, options: AVAudioSession.SetActiveOptions) throws
}

extension AVAudioSession: AudioSessionControlling {}
