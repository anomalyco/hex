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
    func startKeepalive(allowBluetooth: Bool) throws
    func stopKeepalive()
    func deactivateSession()
    func beginCapture(at url: URL, maxDuration: TimeInterval, allowBluetooth: Bool) throws -> any ActiveAudioCapture
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
    private var engine = AVAudioEngine()
    private var tapInstalled = false
    nonisolated(unsafe) private var observers: [NSObjectProtocol] = []

    var onSystemEvent: ((AudioSystemEvent) -> Void)?
    var onUnexpectedCaptureEnd: ((URL, Bool) -> Void)?

    var isKeepaliveRunning: Bool {
        engine.isRunning
    }

    init() {
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

    func startKeepalive(allowBluetooth: Bool) throws {
        stopKeepalive()
        try configureAudioSession(allowBluetooth: allowBluetooth)

        // Changing the session (Always use iPhone microphone, or a route change) can move the
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
        try? AVAudioSession.sharedInstance().setActive(
            false,
            options: .notifyOthersOnDeactivation
        )
    }

    func beginCapture(
        at url: URL,
        maxDuration: TimeInterval,
        allowBluetooth: Bool
    ) throws -> any ActiveAudioCapture {
        stopKeepalive()
        try configureAudioSession(allowBluetooth: allowBluetooth)

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

    /// Bluetooth off (Always use iPhone microphone, the default): record from the iPhone's
    /// microphone and mix with other audio, so music or navigation keeps playing (over A2DP
    /// in a car or headphones) instead of pausing and dropping into call audio.
    private func configureAudioSession(allowBluetooth: Bool) throws {
        let session = AVAudioSession.sharedInstance()
        if allowBluetooth {
            try session.setCategory(.record, mode: .measurement, options: [.allowBluetoothHFP])
        } else {
            try session.setCategory(
                .playAndRecord,
                mode: .measurement,
                options: [.mixWithOthers, .allowBluetoothA2DP, .defaultToSpeaker]
            )
        }
        try session.setPreferredSampleRate(16_000)
        do {
            try session.setActive(true)
        } catch where UserFacingError.audioSessionCode(of: error) == .cannotInterruptOthers {
            // '!int': a non-mixable session can't go active from the background while another
            // app plays or records. Record alongside that audio instead, once.
            try session.setCategory(
                .playAndRecord,
                mode: .measurement,
                options: allowBluetooth
                    ? [.mixWithOthers, .allowBluetoothHFP, .allowBluetoothA2DP, .defaultToSpeaker]
                    : [.mixWithOthers, .allowBluetoothA2DP, .defaultToSpeaker]
            )
            try session.setActive(true)
        }
        if !allowBluetooth, let builtIn = session.availableInputs?.first(where: { $0.portType == .builtInMic }) {
            try? session.setPreferredInput(builtIn)
        }
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