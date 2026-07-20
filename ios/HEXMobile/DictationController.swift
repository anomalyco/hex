import AVFAudio
import Foundation
import Observation
import UIKit

@MainActor
@Observable
final class DictationController: NSObject, AVAudioRecorderDelegate {
    private static let keyboardSessionDuration: TimeInterval = 15 * 60

    enum Phase: Equatable {
        case modelRequired
        case preparingModel
        case ready
        case requestingPermission
        case recording
        case transcribing
        case failed(String)

        var isBusy: Bool {
            switch self {
            case .preparingModel, .requestingPermission, .transcribing:
                true
            default:
                false
            }
        }
    }

    private var transcriber: any LocalTranscribing = ParakeetTranscriber()
    private let keyboardBridge = KeyboardBridge()
    private let sessionEngine = AVAudioEngine()
    private var recorder: AVAudioRecorder?
    private var meterTask: Task<Void, Never>?
    private var keyboardSessionTask: Task<Void, Never>?
    private var recordingURL: URL?
    private var sessionTapInstalled = false
    private var lastKeyboardCommandID: String?
    private var lastKeyboardRecordingRequestJobID: String?
    private var activeKeyboardJobID: String?
    private var keyboardResultID: String?
    private var keyboardResult: String?
    private var keyboardMessage: String?
    private var keyboardSessionRequested = false
    private var keyboardRecordingRequestedJobID: String?
    private var keyboardSessionExpiresAt: TimeInterval = 0

    private(set) var phase = Phase.modelRequired
    private(set) var transcript = ""
    private(set) var audioLevel: Float = 0
    private(set) var recordingDuration: TimeInterval = 0
    private(set) var copied = false
    private(set) var keyboardSessionEnabled = false
    private(set) var keyboardSessionRemaining: TimeInterval = 0
    private(set) var engineName = "Local speech"
    private(set) var transcriptionLanguage = "Preferred language"
    private(set) var modelPreparation = ModelPreparationProgress(
        fractionCompleted: 0,
        detail: "Checking model files"
    )

    override init() {
        let hasCachedModel = ParakeetTranscriber.hasCachedModel
        super.init()
        keyboardBridge.publish(.offline)
        keyboardBridge.setModelInstalled(hasCachedModel)
        if #available(iOS 26.0, *) {
            engineName = "Apple Speech"
            prepareModel()
        } else if hasCachedModel {
            engineName = "Parakeet V2"
            prepareModel()
        }
    }

    var isRecording: Bool {
        phase == .recording
    }

    var primaryTitle: String {
        switch phase {
        case .modelRequired, .failed:
            "Prepare \(engineName)"
        case .preparingModel:
            "Preparing \(engineName)"
        case .ready:
            "Start recording"
        case .requestingPermission:
            "Checking microphone"
        case .recording:
            "Finish recording"
        case .transcribing:
            "Transcribing locally"
        }
    }

    func performPrimaryAction() {
        switch phase {
        case .modelRequired, .failed:
            prepareModel()
        case .ready:
            startRecording()
        case .recording:
            finishRecording()
        case .preparingModel, .requestingPermission, .transcribing:
            break
        }
    }

    func clearTranscript() {
        transcript = ""
        copied = false
    }

    func copyTranscript() {
        guard !transcript.isEmpty else { return }
        UIPasteboard.general.string = transcript
        copied = true
    }

    func startKeyboardSessionFromKeyboard() {
        if keyboardSessionEnabled {
            startRequestedKeyboardRecording()
            return
        }
        keyboardSessionRequested = true

        switch phase {
        case .modelRequired, .failed:
            prepareModel()
        case .ready:
            startKeyboardSession()
        case .preparingModel, .requestingPermission, .recording, .transcribing:
            break
        }
    }

    func startKeyboardRecordingFromKeyboard(jobID: String) {
        guard !jobID.isEmpty,
              jobID != lastKeyboardRecordingRequestJobID,
              activeKeyboardJobID == nil,
              keyboardRecordingRequestedJobID == nil else { return }
        lastKeyboardRecordingRequestJobID = jobID
        keyboardRecordingRequestedJobID = jobID
        startKeyboardSessionFromKeyboard()
    }

    private func prepareModel() {
        phase = .preparingModel
        copied = false
        modelPreparation = ModelPreparationProgress(
            fractionCompleted: 0,
            detail: "Checking model files"
        )

        Task {
            do {
                let transcriber = await preferredTranscriber()
                engineName = transcriber.displayName
                try await transcriber.prepare { [weak self] progress in
                    Task { @MainActor [weak self] in
                        self?.modelPreparation = progress
                    }
                }
                self.transcriber = transcriber
                transcriptionLanguage = await transcriber.languageName
                keyboardBridge.setModelInstalled(true)
                phase = .ready
                if keyboardSessionRequested {
                    startKeyboardSession()
                }
            } catch {
                keyboardBridge.setModelInstalled(ParakeetTranscriber.hasCachedModel)
                keyboardSessionRequested = false
                phase = .failed(error.localizedDescription)
            }
        }
    }

    private func preferredTranscriber() async -> any LocalTranscribing {
        if #available(iOS 26.0, *),
           AppleSpeechTranscriber.isAvailable,
           let locale = await AppleSpeechTranscriber.preferredLocale() {
            return AppleSpeechTranscriber(locale: locale)
        }
        return ParakeetTranscriber()
    }

    private func startRecording(keyboardJobID: String? = nil) {
        if let keyboardJobID {
            guard activeKeyboardJobID == nil else { return }
            activeKeyboardJobID = keyboardJobID
            keyboardResultID = nil
            keyboardResult = nil
            keyboardMessage = nil
        }
        phase = .requestingPermission
        copied = false

        Task {
            let granted = await requestMicrophonePermission()
            guard granted else {
                activeKeyboardJobID = nil
                phase = .failed("Microphone access is required to record dictation. Enable it in Settings and try again.")
                return
            }

            do {
                stopSessionAudio()
                try beginCapture()
                phase = .recording
                startMetering()
            } catch {
                discardCapture()
                keyboardMessage = keyboardJobID == nil ? nil : error.localizedDescription
                phase = .failed(error.localizedDescription)
            }
        }
    }

    private func startKeyboardSession() {
        guard phase == .ready else { return }
        keyboardSessionRequested = false
        phase = .requestingPermission
        keyboardMessage = nil

        Task {
            let granted = await requestMicrophonePermission()
            guard granted else {
                keyboardSessionRequested = false
                phase = .failed("Microphone access is required for the HEX keyboard session.")
                return
            }

            do {
                try startSessionAudio()
                keyboardSessionEnabled = true
                keyboardSessionExpiresAt = Date().timeIntervalSince1970 + Self.keyboardSessionDuration
                keyboardSessionRemaining = Self.keyboardSessionDuration
                phase = .ready
                startKeyboardSessionLoop()
                startRequestedKeyboardRecording()
            } catch {
                stopSessionAudio()
                phase = .failed(error.localizedDescription)
            }
        }
    }

    private func stopKeyboardSession() {
        keyboardSessionTask?.cancel()
        keyboardSessionTask = nil

        if activeKeyboardJobID != nil {
            discardCapture()
            activeKeyboardJobID = nil
            phase = .ready
        }

        keyboardSessionEnabled = false
        keyboardSessionRemaining = 0
        keyboardSessionExpiresAt = 0
        keyboardResultID = nil
        keyboardResult = nil
        keyboardMessage = nil
        keyboardRecordingRequestedJobID = nil
        stopSessionAudio()
        try? AVAudioSession.sharedInstance().setActive(
            false,
            options: .notifyOthersOnDeactivation
        )
        keyboardBridge.publish(.offline)
    }

    private func startKeyboardSessionLoop() {
        keyboardSessionTask?.cancel()
        keyboardSessionTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self, self.keyboardSessionEnabled else { return }

                let remaining = self.keyboardSessionExpiresAt - Date().timeIntervalSince1970
                if remaining <= 0 {
                    self.stopKeyboardSession()
                    return
                }

                self.keyboardSessionRemaining = remaining
                self.consumeKeyboardCommand()
                self.publishKeyboardSnapshot()
                try? await Task.sleep(for: .milliseconds(100))
            }
        }
    }

    private func consumeKeyboardCommand() {
        guard let command = keyboardBridge.latestCommand(),
              command.id != lastKeyboardCommandID else { return }
        lastKeyboardCommandID = command.id

        switch command.kind {
        case .start:
            guard phase == .ready, activeKeyboardJobID == nil else { return }
            startRecording(keyboardJobID: command.jobID)
        case .stop:
            guard phase == .recording, activeKeyboardJobID == command.jobID else { return }
            finishRecording()
        case .cancel:
            guard phase == .recording, activeKeyboardJobID == command.jobID else { return }
            discardCapture()
            activeKeyboardJobID = nil
            keyboardMessage = "Dictation cancelled"
            phase = .ready
        }
    }

    private func startRequestedKeyboardRecording() {
        guard phase == .ready, let jobID = keyboardRecordingRequestedJobID else { return }
        keyboardRecordingRequestedJobID = nil
        startRecording(keyboardJobID: jobID)
    }

    private func publishKeyboardSnapshot() {
        let state: KeyboardDictationState
        switch phase {
        case .recording:
            state = .recording
        case .transcribing:
            state = .transcribing
        case .failed:
            state = .failed
        case .modelRequired, .preparingModel, .requestingPermission:
            state = .offline
        case .ready:
            state = .ready
        }

        let failureMessage: String?
        if case .failed(let message) = phase {
            failureMessage = message
        } else {
            failureMessage = keyboardMessage
        }

        keyboardBridge.publish(
            KeyboardSnapshot(
                state: state,
                heartbeat: Date().timeIntervalSince1970,
                expiresAt: keyboardSessionExpiresAt,
                jobID: activeKeyboardJobID,
                resultID: keyboardResultID,
                transcript: keyboardResult,
                message: failureMessage
            )
        )
    }

    private func startSessionAudio() throws {
        try startSessionAudioEngine()
    }

    private func startSessionAudioEngine() throws {
        guard !sessionEngine.isRunning else { return }

        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.record, mode: .measurement)
        try session.setPreferredSampleRate(16_000)
        try session.setActive(true)

        let input = sessionEngine.inputNode
        let format = input.outputFormat(forBus: 0)
        input.installTap(
            onBus: 0,
            bufferSize: 4_096,
            format: format,
            block: Self.discardSessionAudio
        )
        sessionTapInstalled = true
        sessionEngine.prepare()
        try sessionEngine.start()
    }

    nonisolated private static func discardSessionAudio(
        _ buffer: AVAudioPCMBuffer,
        at time: AVAudioTime
    ) {}

    private func stopSessionAudio() {
        if sessionTapInstalled {
            sessionEngine.inputNode.removeTap(onBus: 0)
            sessionTapInstalled = false
        }
        sessionEngine.stop()
        sessionEngine.reset()
    }

    private func requestMicrophonePermission() async -> Bool {
        await withCheckedContinuation { continuation in
            AVAudioApplication.requestRecordPermission { granted in
                continuation.resume(returning: granted)
            }
        }
    }

    private func beginCapture() throws {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.record, mode: .measurement)
        try session.setPreferredSampleRate(16_000)
        try session.setActive(true)

        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("hex-dictation-\(UUID().uuidString)")
            .appendingPathExtension("wav")
        let settings: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVSampleRateKey: 16_000,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsBigEndianKey: false,
            AVLinearPCMIsFloatKey: false,
        ]
        let recorder = try AVAudioRecorder(url: url, settings: settings)
        recorder.delegate = self
        recorder.isMeteringEnabled = true
        recorder.prepareToRecord()
        guard recorder.record(forDuration: 60) else {
            throw CocoaError(.fileWriteUnknown)
        }

        recordingURL = url
        self.recorder = recorder
        audioLevel = 0
        recordingDuration = 0
    }

    private func startMetering() {
        meterTask?.cancel()
        meterTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self, let recorder = self.recorder, recorder.isRecording else {
                    break
                }

                recorder.updateMeters()
                let decibels = recorder.averagePower(forChannel: 0)
                self.audioLevel = max(0, min(1, (decibels + 48) / 48))
                self.recordingDuration = recorder.currentTime

                try? await Task.sleep(for: .milliseconds(50))
            }

            if let self, self.phase == .recording, self.recordingDuration >= 59.5 {
                self.finishRecording()
            }
        }
    }

    private func finishRecording() {
        guard let recorder, let recordingURL else { return }
        let keyboardJobID = activeKeyboardJobID

        meterTask?.cancel()
        meterTask = nil
        recorder.stop()
        self.recorder = nil
        audioLevel = 0

        guard recordingDuration >= 0.3 else {
            try? FileManager.default.removeItem(at: recordingURL)
            self.recordingURL = nil
            recordingDuration = 0
            activeKeyboardJobID = nil
            keyboardMessage = keyboardJobID == nil ? nil : "Recording was too short"
            phase = .ready
            restoreAudioAfterCapture()
            return
        }

        phase = .transcribing
        restoreAudioAfterCapture()
        Task {
            defer {
                try? FileManager.default.removeItem(at: recordingURL)
                self.recordingURL = nil
                self.recordingDuration = 0
            }

            do {
                transcript = try await transcriber.transcribe(recordingURL)
                if keyboardJobID != nil {
                    keyboardResultID = UUID().uuidString
                    keyboardResult = transcript
                    keyboardMessage = nil
                }
                activeKeyboardJobID = nil
                phase = .ready
            } catch {
                activeKeyboardJobID = nil
                keyboardMessage = keyboardJobID == nil ? nil : error.localizedDescription
                phase = .failed(error.localizedDescription)
            }
        }
    }

    private func restoreAudioAfterCapture() {
        if keyboardSessionEnabled {
            do {
                try startSessionAudio()
            } catch {
                keyboardMessage = error.localizedDescription
                phase = .failed(error.localizedDescription)
            }
        } else {
            try? AVAudioSession.sharedInstance().setActive(
                false,
                options: .notifyOthersOnDeactivation
            )
        }
    }

    private func discardCapture() {
        meterTask?.cancel()
        meterTask = nil
        recorder?.stop()
        recorder = nil
        if let recordingURL {
            try? FileManager.default.removeItem(at: recordingURL)
        }
        recordingURL = nil
        activeKeyboardJobID = nil
        audioLevel = 0
        recordingDuration = 0
        restoreAudioAfterCapture()
    }

    nonisolated func audioRecorderDidFinishRecording(
        _ recorder: AVAudioRecorder,
        successfully flag: Bool
    ) {
        Task { @MainActor [weak self] in
            guard let self, self.phase == .recording else { return }
            if flag {
                self.finishRecording()
            } else {
                self.discardCapture()
                self.phase = .failed("The recording ended unexpectedly.")
            }
        }
    }
}
