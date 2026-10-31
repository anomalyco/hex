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
        explicitlyStopped = true
        recorder.delegate = nil
        recorder.stop()
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

        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
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
        try engine.start()
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
            throw CocoaError(.fileWriteUnknown)
        }

        return RecorderCaptureAdapter(url: url, recorder: recorder) { [weak self] finishedURL, ok in
            self?.onUnexpectedCaptureEnd?(finishedURL, ok)
        }
    }

    private func configureAudioSession(allowBluetooth: Bool) throws {
        let session = AVAudioSession.sharedInstance()
        let options: AVAudioSession.CategoryOptions = allowBluetooth ? [.allowBluetoothHFP] : []
        try session.setCategory(.record, mode: .measurement, options: options)
        try session.setPreferredSampleRate(16_000)
        try session.setActive(true)
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
