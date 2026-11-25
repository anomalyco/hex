import AVFAudio
import Foundation

nonisolated enum AudioSystemEvent: Equatable, Sendable {
    case interruptionBegan
    case interruptionEnded(shouldResume: Bool)
    case routeOldDeviceUnavailable
    case routeNewDeviceAvailable
    case mediaServicesReset
}

@MainActor
protocol ActiveAudioCapture: AnyObject {
    var url: URL { get }
    var isRecording: Bool { get }
    var currentTime: TimeInterval { get }
    func normalizedPowerLevel() -> Float
    func averagePowerDB() -> Float
    func stop()
}

extension ActiveAudioCapture {
    func averagePowerDB() -> Float {
        DotScopeSimulation.loudness(fromNormalized: normalizedPowerLevel())
    }
}

@MainActor
protocol AudioManaging: AnyObject {
    var isKeepaliveRunning: Bool { get }
    var onSystemEvent: ((AudioSystemEvent) -> Void)? { get set }
    var onUnexpectedCaptureEnd: ((URL, Bool) -> Void)? { get set }

    func requestPermission() async -> Bool
    /// Holds the iPhone's microphone between dictations (`MicrophoneRoute.keepalive`).
    func startKeepalive() throws
    func stopKeepalive()
    func deactivateSession()
    /// Records from `microphone`'s input for this route (`MicrophoneRoute.capture`).
    func beginCapture(at url: URL, maxDuration: TimeInterval, microphone: MicrophoneChoice) throws -> any ActiveAudioCapture
}

/// Keeps AVAudioRecorders that stopped or failed to start alive until their audio queue is
/// done calling back.
///
/// A recorder's AudioQueue input callback reads the recorder (a weak reference inside it) on
/// the queue's own thread. When the queue fails or stops from inside that callback (no
/// microphone, a lost route), it is disposed asynchronously and callbacks it already fetched
/// are still delivered. Freeing the recorder in that window crashed Hex in `objc_loadWeak`
/// (`AudioRecorderAQInputCallback`) when the microphone failed during the keyboard's switch to
/// Hex. Nothing reports when the last callback has run, so a retired recorder is held for
/// `holdTime`: the queue takes about a millisecond to wind down, and a stopped recorder costs
/// next to nothing to keep.
@MainActor
final class RetiredRecorders {
    static let shared = RetiredRecorders()
    static let holdTime: Duration = .seconds(5)

    private var held: [UInt64: AnyObject] = [:]
    private var nextToken: UInt64 = 0

    /// Recorders still held.
    var count: Int { held.count }

    func hold(_ recorder: AnyObject, for duration: Duration = holdTime) {
        nextToken &+= 1
        let token = nextToken
        held[token] = recorder
        Task { [weak self] in
            try? await Task.sleep(for: duration)
            self?.held[token] = nil
        }
    }
}

@MainActor
private final class RecorderCaptureAdapter: NSObject, ActiveAudioCapture, AVAudioRecorderDelegate {
    let url: URL
    private let recorder: AVAudioRecorder
    private let onFinish: @MainActor (URL, Bool) -> Void
    private var explicitlyStopped = false

    init(
        url: URL,
        recorder: AVAudioRecorder,
        onFinish: @escaping @MainActor (URL, Bool) -> Void
    ) {
        self.url = url
        self.recorder = recorder
        self.onFinish = onFinish
        super.init()
        recorder.delegate = self
    }

    var isRecording: Bool {
        recorder.isRecording
    }

    var currentTime: TimeInterval {
        recorder.currentTime
    }

    func averagePowerDB() -> Float {
        recorder.updateMeters()
        return recorder.averagePower(forChannel: 0)
    }

    func normalizedPowerLevel() -> Float {
        let decibels = averagePowerDB()
        return max(0, min(1, (decibels + 48) / 48))
    }

    func stop() {
        guard !explicitlyStopped else { return }
        explicitlyStopped = true
        recorder.delegate = nil
        recorder.stop()
        // The controller drops this capture next; its queue may still be calling back.
        RetiredRecorders.shared.hold(recorder)
    }

    nonisolated func audioRecorderDidFinishRecording(
        _ recorder: AVAudioRecorder,
        successfully flag: Bool
    ) {
        Task { @MainActor [weak self] in
            guard let self, !self.explicitlyStopped else { return }
            self.onFinish(self.url, flag)
        }
    }
}

@MainActor
final class SystemAudioManager: AudioManaging {
    private let session: any AudioSessionControlling
    private let voiceIsolation: VoiceIsolationSettings
    private var engine = AVAudioEngine()
    private var tapInstalled = false
    nonisolated(unsafe) private var observers: [NSObjectProtocol] = []

    var onSystemEvent: ((AudioSystemEvent) -> Void)?
    var onUnexpectedCaptureEnd: ((URL, Bool) -> Void)?

    var isKeepaliveRunning: Bool {
        engine.isRunning
    }

    init(
        session: any AudioSessionControlling = AVAudioSession.sharedInstance(),
        voiceIsolation: VoiceIsolationSettings = VoiceIsolationSettings()
    ) {
        self.session = session
        self.voiceIsolation = voiceIsolation
        installNotifications()
    }

    deinit {
        for token in observers {
            NotificationCenter.default.removeObserver(token)
        }
    }

    func requestPermission() async -> Bool {
        await withCheckedContinuation { continuation in
            AVAudioApplication.requestRecordPermission { granted in
                continuation.resume(returning: granted)
            }
        }
    }

    func startKeepalive() throws {
        stopKeepalive()
        try configureAudioSession(recordingFrom: nil)

        // Changing the session (a dictation's own input, or a route change) can move the
        // hardware to another sample rate. An engine built before that keeps the old input
        // format, and installTap raises an Objective-C exception Swift can't catch, so build a
        // fresh engine for the current route and tap with the format the hardware reports.
        engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.inputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0,
              input.outputFormat(forBus: 0).sampleRate == format.sampleRate
        else {
            throw CocoaError(.coderInvalidValue)
        }

        input.installTap(
            onBus: 0,
            bufferSize: 4_096,
            format: format,
            block: Self.discardSamples
        )
        tapInstalled = true
        engine.prepare()
        do {
            try engine.start()
        } catch {
            // Leave nothing half-installed: no tap on an engine that never ran.
            stopKeepalive()
            throw error
        }
    }

    func stopKeepalive() {
        if tapInstalled {
            engine.inputNode.removeTap(onBus: 0)
            tapInstalled = false
        }
        if engine.isRunning {
            engine.stop()
        }
        engine.reset()
    }

    func deactivateSession() {
        stopKeepalive()
        try? session.setActive(false, options: .notifyOthersOnDeactivation)
    }

    func beginCapture(
        at url: URL,
        maxDuration: TimeInterval,
        microphone: MicrophoneChoice
    ) throws -> any ActiveAudioCapture {
        stopKeepalive()
        try configureAudioSession(recordingFrom: microphone)

        let settings: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVSampleRateKey: 16_000,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsBigEndianKey: false,
            AVLinearPCMIsFloatKey: false,
        ]
        let recorder = try AVAudioRecorder(url: url, settings: settings)
        recorder.isMeteringEnabled = true
        recorder.prepareToRecord()
        guard recorder.record(forDuration: maxDuration) else {
            // The microphone didn't start (none in the simulator, or one another app holds).
            // The recorder's queue made it this far and is still winding down: freeing the
            // recorder here crashed Hex on the queue's thread.
            recorder.stop()
            RetiredRecorders.shared.hold(recorder)
            throw CocoaError(.fileWriteUnknown)
        }

        return RecorderCaptureAdapter(url: url, recorder: recorder) { [weak self] finishedURL, ok in
            self?.onUnexpectedCaptureEnd?(finishedURL, ok)
        }
    }

    /// Captures and the warm keepalive record alongside other audio: `.mixWithOthers` never
    /// interrupts another app, so its video or music keeps playing (over A2DP in a car or
    /// headphones) with nothing to resume, and no `.duckOthers`.
    ///
    /// A dictation records from Settings › Microphone's input for this route
    /// (`MicrophoneRoute.capture`): with Automatic (the default) a wired headset's
    /// microphone, the car's in CarPlay, else the iPhone's; Bluetooth hands-free microphones
    /// only with Bluetooth, the one choice that adds `.allowBluetoothHFP`. The keepalive
    /// (`choice` nil) stays on the iPhone's microphone without HFP, so a warm session never
    /// holds a car or headset in call audio between dictations. The input is always set,
    /// never left to an earlier preference or to iOS, which routes HFP and CarPlay first.
    ///
    /// No preferred sample rate: `.playAndRecord` runs the microphone and the output at one
    /// hardware rate, so asking for 16 kHz moved everything the iPhone played and recorded (a
    /// screen recording's audio too) to 16 kHz for as long as Hex held the session, and back
    /// at its end. The hardware keeps its own rate: the recorder writes 16 kHz itself, and the
    /// keepalive taps whatever format the hardware reports.
    ///
    /// Labs › Voice isolation is the one exception to `.measurement`: with it on the
    /// session records in `.voiceChat` mode, whose voice processing lets Control Center's Mic
    /// Mode offer Voice Isolation while Hex records. Off, the mode and every option are exactly
    /// as before, so Hex records as it always has.
    ///
    /// The category is set only when it changes: on iOS 27 setting it again, even unchanged,
    /// while Hex is in the background asks iOS to interrupt others, which it refuses there
    /// ("can't stop others because it is in the background"), so a warm session's dictation
    /// failed with "Another app is using audio" (cannotInterruptOthers, TestFlight build 129).
    func configureAudioSession(recordingFrom choice: MicrophoneChoice?) throws {
        let options = MicrophoneRoute.categoryOptions(allowingBluetooth: choice?.allowsBluetooth ?? false)
        let mode: AVAudioSession.Mode = voiceIsolation.isEnabled ? .voiceChat : .measurement
        // `.voiceChat` adds `.allowBluetoothHFP` itself, so comparing the raw
        // options would see a change on every recording and set the category
        // again — the TestFlight 129 background cannotInterruptOthers failure.
        let implicit: AVAudioSession.CategoryOptions = mode == .voiceChat ? [.allowBluetoothHFP] : []
        if session.category != .playAndRecord || session.mode != mode
            || session.categoryOptions.subtracting(implicit) != options.subtracting(implicit) {
            try session.setCategory(.playAndRecord, mode: mode, options: options)
        }
        try session.setActive(true, options: [])
        let inputs = session.microphones
        let input = choice.map { MicrophoneRoute.capture($0, from: inputs) } ?? MicrophoneRoute.keepalive(from: inputs)
        try? session.prefer(input)
    }

    nonisolated private static func discardSamples(
        _ buffer: AVAudioPCMBuffer,
        at time: AVAudioTime
    ) {}

    private func installNotifications() {
        let center = NotificationCenter.default

        observers.append(
            center.addObserver(
                forName: AVAudioSession.interruptionNotification,
                object: AVAudioSession.sharedInstance(),
                queue: .main
            ) { [weak self] notification in
                let typeRaw = notification.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt
                let optionsRaw = notification.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt
                MainActor.assumeIsolated {
                    guard let self,
                          let typeRaw,
                          let type = AVAudioSession.InterruptionType(rawValue: typeRaw) else {
                        return
                    }
                    switch type {
                    case .began:
                        self.onSystemEvent?(.interruptionBegan)
                    case .ended:
                        let options = AVAudioSession.InterruptionOptions(rawValue: optionsRaw ?? 0)
                        self.onSystemEvent?(.interruptionEnded(shouldResume: options.contains(.shouldResume)))
                    @unknown default:
                        break
                    }
                }
            }
        )

        observers.append(
            center.addObserver(
                forName: AVAudioSession.routeChangeNotification,
                object: AVAudioSession.sharedInstance(),
                queue: .main
            ) { [weak self] notification in
                let reasonRaw = notification.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt
                MainActor.assumeIsolated {
                    guard let self,
                          let reasonRaw,
                          let reason = AVAudioSession.RouteChangeReason(rawValue: reasonRaw) else {
                        return
                    }
                    switch reason {
                    case .oldDeviceUnavailable:
                        self.onSystemEvent?(.routeOldDeviceUnavailable)
                    case .newDeviceAvailable:
                        self.onSystemEvent?(.routeNewDeviceAvailable)
                    default:
                        break
                    }
                }
            }
        )

        observers.append(
            center.addObserver(
                forName: AVAudioSession.mediaServicesWereResetNotification,
                object: AVAudioSession.sharedInstance(),
                queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.tapInstalled = false
                    self.engine = AVAudioEngine()
                    self.onSystemEvent?(.mediaServicesReset)
                }
            }
        )
    }
}