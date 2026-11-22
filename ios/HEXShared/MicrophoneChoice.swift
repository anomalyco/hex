import AVFAudio

/// Settings › Dictation › Microphone: what dictation records from while a car, headset or
/// headphones are connected. The app's recorder and the keyboard's capture page both follow it.
nonisolated enum MicrophoneChoice: String, CaseIterable, Sendable {
    /// A wired or USB headset's microphone, the car's in CarPlay, otherwise the iPhone's:
    /// Bluetooth headphones keep playing at full quality, and nothing waits for call audio.
    case automatic
    /// Always the iPhone's own microphone.
    case iPhone
    /// Bluetooth hands-free microphones too: AirPods, headsets and Bluetooth car kits, which
    /// switch to call audio (8–16 kHz, other audio in call quality) while dictating.
    case bluetooth

    var label: String {
        switch self {
        case .automatic: String(localized: "Automatic")
        case .iPhone: String(localized: "iPhone")
        case .bluetooth: String(localized: "Bluetooth")
        }
    }

    var detail: String {
        switch self {
        case .automatic: String(localized: "Car mic in CarPlay, otherwise iPhone")
        case .iPhone: String(localized: "Keeps music playing and starts faster")
        case .bluetooth: String(localized: "AirPods and car mics, in call quality")
        }
    }

    /// Whether a capture may record from a Bluetooth hands-free microphone, which needs
    /// `.allowBluetoothHFP` in the session's category.
    var allowsBluetooth: Bool { self == .bluetooth }
}

/// An input iOS offers (`AVAudioSessionPortDescription`), as a value tests can make.
nonisolated struct Microphone: Equatable, Sendable {
    let uid: String
    let name: String
    let port: AVAudioSession.Port

    init(uid: String, name: String, port: AVAudioSession.Port) {
        self.uid = uid
        self.name = name
        self.port = port
    }

    init(_ description: AVAudioSessionPortDescription) {
        self.init(uid: description.uid, name: description.portName, port: description.portType)
    }
}

/// Which of the inputs iOS offers a dictation and the warm session record from.
nonisolated enum MicrophoneRoute {
    /// The input a dictation records from; nil when none of the choice's kinds is offered.
    ///
    /// Automatic takes a wired or USB headset's microphone (close to the mouth, no call audio)
    /// and, in CarPlay, the car's: the one the car has for its driver, which Siri and calls
    /// use, where the iPhone's would hear the car's speakers and the road. A Bluetooth car kit
    /// can't be told from headphones, so it takes `bluetooth`.
    static func capture(_ choice: MicrophoneChoice, from inputs: [Microphone]) -> Microphone? {
        let order: [AVAudioSession.Port] = switch choice {
        case .iPhone: [.builtInMic]
        case .automatic: [.headsetMic, .usbAudio, .carAudio, .builtInMic]
        case .bluetooth: [.headsetMic, .usbAudio, .carAudio, .bluetoothHFP, .bluetoothLE, .builtInMic]
        }
        for port in order {
            if let input = inputs.first(where: { $0.port == port }) { return input }
        }
        return nil
    }

    /// The warm session's keepalive, which holds the microphone between dictations: always the
    /// iPhone's, so a warm session never keeps a car or headset in call audio for its minutes.
    /// Each dictation moves to its own input as it starts.
    static func keepalive(from inputs: [Microphone]) -> Microphone? {
        inputs.first { $0.port == .builtInMic }
    }

    /// The category options for recording: `.mixWithOthers` keeps other apps' video and music
    /// playing, and A2DP keeps headphones in stereo while another microphone records.
    /// Bluetooth hands-free inputs are offered only when allowed.
    static func categoryOptions(allowingBluetooth: Bool) -> AVAudioSession.CategoryOptions {
        allowingBluetooth
            ? [.mixWithOthers, .allowBluetoothHFP, .allowBluetoothA2DP, .defaultToSpeaker]
            : [.mixWithOthers, .allowBluetoothA2DP, .defaultToSpeaker]
    }
}
