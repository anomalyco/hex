import AVFAudio
import Foundation
import Observation
import SwiftUI
import UIKit

@MainActor
@Observable
final class DictationController {
    static let shared = DictationController(
        cleaner: AppleIntelligenceCleaner(),
        autoPrepareOnInit: ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] == nil
    )

    private static let minCaptureDuration: TimeInterval = 0.30
    private static let maxCaptureDuration: TimeInterval = 60.0
    private static let resultDeliveryTTL: TimeInterval = 30.0

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

    enum AutoReturnState: Equatable {
        case idle
        case attempting(HostApp)
        case succeeded
        case swipeFallback
    }

    private struct ActiveCaptureSession {
        let generation: UInt64
        let capture: any ActiveAudioCapture
        let startedAt: TimeInterval
        let keyboardJobID: String?
        let style: DictationStyle
        let copyToPasteboard: Bool
    }

    private struct PendingKeyboardHandoff {
        let jobID: String?
        let style: DictationStyle
        let requestedAt: TimeInterval
        let attemptAutoReturn: Bool
    }

    private var transcriber: any LocalTranscribing
    private let transcriberFactory: (() async -> any LocalTranscribing)?
    private let cleaner: (any TranscriptCleaning)?
    private let audio: any AudioManaging
    private let keyboardBridge: KeyboardBridge
    private let fileManager: FileManager
    private let now: () -> TimeInterval
    private let liveActivity = LiveActivityCoordinator()
    private var defaultURLOpener: @MainActor (URL) async -> Bool

    private var jobGeneration: UInt64 = 0
    private var modelPrepGeneration: UInt64 = 0
    private var activeCapture: ActiveCaptureSession?
    private var meterTask: Task<Void, Never>?
    private var transcriptionTask: Task<Void, Never>?
    private var keyboardSessionTask: Task<Void, Never>?
    private var autoReturnFallbackTask: Task<Void, Never>?

    private var lastKeyboardCommandID: String?
    private var lastHandledLaunchJobID: String?
    private var activeKeyboardJobID: String?
    private var keyboardResultID: String?
    private var keyboardResultExpiresAt: TimeInterval?
    private var keyboardResult: String?
    private var keyboardMessage: String?
    private var pendingHandoff: PendingKeyboardHandoff?
    private var keyboardSessionExpiresAt: TimeInterval = 0
    private var suspendedWarmSessionOnInterruption = false

    private(set) var phase = Phase.modelRequired
    private(set) var isModelReady = false
    private(set) var transcript = ""
    @ObservationIgnored private(set) var audioLevel: Float = 0
    @ObservationIgnored private(set) var loudnessDB: Float = -120
    @ObservationIgnored private var rawRecordingDuration: TimeInterval = 0
    private(set) var recordingWholeSeconds: Int = 0
    private(set) var recordingDuration: TimeInterval {
        get { rawRecordingDuration }
        set {
            rawRecordingDuration = newValue
            let whole = max(0, Int(newValue))
            if recordingWholeSeconds != whole {
                recordingWholeSeconds = whole
            }
        }
    }
    private(set) var copied = false
    private(set) var keyboardSessionEnabled = false
    @ObservationIgnored private var rawKeyboardSessionRemaining: TimeInterval = 0
    private(set) var keyboardSessionDisplayRemaining = "0:00"
    private(set) var keyboardSessionRemaining: TimeInterval {
        get { rawKeyboardSessionRemaining }
        set {
            rawKeyboardSessionRemaining = newValue
            let formatted = HexTheme.formatRemaining(newValue)
            if keyboardSessionDisplayRemaining != formatted {
                keyboardSessionDisplayRemaining = formatted
            }
        }
    }
    @ObservationIgnored private var lastPublishedSnapshotSignature: String?
    @ObservationIgnored private var lastPublishedSnapshotAt: TimeInterval = -100
    private(set) var warmDuration: WarmSessionDuration
    private(set) var style: DictationStyle
    private(set) var returnAppID: String?
    private(set) var allowBluetoothMic: Bool
    private(set) var speechModel: SpeechModel
    private(set) var speechModelError: String?
    /// Apple Speech exists on this device, so the speech model is a real choice.
    private(set) var canChooseSpeechModel: Bool
    private(set) var appleIntelligenceCleanup: Bool
    private(set) var cleanupAvailable: Bool
    private(set) var autoReturnState: AutoReturnState = .idle
    private(set) var handoffNotice: String?
    private(set) var engineName = "Local speech"
    private(set) var transcriptionLanguage = "English"
    private(set) var modelPreparation = ModelPreparationProgress(
        fractionCompleted: 0,
        detail: "Checking model files"
    )

    init(
        audio: (any AudioManaging)? = nil,
        keyboardBridge: KeyboardBridge? = nil,
        transcriber: (any LocalTranscribing)? = nil,
        transcriberFactory: (() async -> any LocalTranscribing)? = nil,
        cleaner: (any TranscriptCleaning)? = nil,
        fileManager: FileManager = .default,
        now: @escaping () -> TimeInterval = { Date().timeIntervalSince1970 },
        autoPrepareOnInit: Bool = true,
        urlOpener: (@MainActor (URL) async -> Bool)? = nil
    ) {
        let resolvedBridge = keyboardBridge ?? KeyboardBridge()
        let resolvedAudio = audio ?? SystemAudioManager()
        let initialTranscriber = transcriber ?? ParakeetTranscriber()

        self.audio = resolvedAudio
        self.keyboardBridge = resolvedBridge
        self.transcriber = initialTranscriber
        self.transcriberFactory = transcriberFactory
        self.cleaner = cleaner
        self.fileManager = fileManager
        self.now = now
        self.warmDuration = resolvedBridge.warmDuration
        self.style = resolvedBridge.style
        self.returnAppID = resolvedBridge.returnAppID
        self.allowBluetoothMic = resolvedBridge.allowBluetoothMic
        self.speechModel = resolvedBridge.speechModel
        self.canChooseSpeechModel = Self.appleSpeechSupported
        self.appleIntelligenceCleanup = resolvedBridge.appleIntelligenceCleanup
        self.cleanupAvailable = cleaner?.isAvailable ?? false
        self.defaultURLOpener = urlOpener ?? { url in
            await UIApplication.shared.open(url)
        }

        let hasCachedModel = ParakeetTranscriber.hasCachedModel
        resolvedBridge.publish(.offline)
        resolvedBridge.setModelInstalled(hasCachedModel)

        wireAudioCallbacks()
        wireSystemIntentBus()

        if Self.applyLaunchFixtureIfPresent(to: self) {
            return
        }

        if autoPrepareOnInit {
            if speechModel == .parakeet || !canChooseSpeechModel {
                engineName = "Parakeet V2"
                if hasCachedModel {
                    prepareModel()
                }
            } else if #available(iOS 26.0, *) {
                Task { [weak self] in
                    guard let self else { return }
                    if await AppleSpeechTranscriber.preferredLocale() != nil {
                        self.engineName = "Apple Speech"
                        self.prepareModel()
                    } else {
                        self.engineName = "Parakeet V2"
                        if hasCachedModel {
                            self.prepareModel()
                        }
                    }
                }
            }
        }
    }

    var isRecording: Bool {
        phase == .recording
    }

    var selectedReturnApp: HostApp? {
        HostAppCatalog.app(for: returnAppID)
    }

    var primaryTitle: String {
        switch phase {
        case .modelRequired, .failed:
            "Prepare \(engineName)"
        case .preparingModel:
            "Preparing \(engineName)"
        case .ready:
            "Start dictation"
        case .requestingPermission:
            "Checking microphone"
        case .recording:
            "Finish"
        case .transcribing:
            "Transcribing…"
        }
    }

    // MARK: - Settings

    func setWarmDuration(_ duration: WarmSessionDuration) {
        warmDuration = duration
        keyboardBridge.warmDuration = duration

        if keyboardSessionEnabled {
            if let seconds = duration.seconds {
                keyboardSessionExpiresAt = now() + seconds
                keyboardSessionRemaining = seconds
                publishKeyboardSnapshot()
            } else if phase == .ready {
                endWarmSession()
            }
        }
    }

    /// Used by the settings segmented control: picking a duration warms immediately; picking Off ends it.
    func selectWarmDuration(_ duration: WarmSessionDuration) {
        setWarmDuration(duration)
        if duration == .off {
            if keyboardSessionEnabled {
                endWarmSession()
            }
        } else if !keyboardSessionEnabled {
            requestWarmSession(attemptAutoReturn: false)
        }
    }

    func setStyle(_ newStyle: DictationStyle) {
        style = newStyle
        keyboardBridge.style = newStyle
    }

    func setReturnAppID(_ id: String?) {
        let normalized = (id?.isEmpty == true) ? nil : id
        returnAppID = normalized
        keyboardBridge.returnAppID = normalized
    }

    func setAllowBluetoothMic(_ allow: Bool) {
        guard allowBluetoothMic != allow else { return }
        allowBluetoothMic = allow
        keyboardBridge.allowBluetoothMic = allow
        if keyboardSessionEnabled, phase == .ready {
            try? audio.startKeepalive(allowBluetooth: allow)
        }
    }

    /// Switches the speech model. A model that is not on the phone yet downloads with
    /// progress; if that fails, the model that was ready stays in use.
    func selectSpeechModel(_ model: SpeechModel) {
        guard model != speechModel else { return }
        switch phase {
        case .recording, .transcribing, .requestingPermission:
            return
        default:
            break
        }
        let previous = speechModel
        speechModel = model
        keyboardBridge.speechModel = model
        speechModelError = nil
        prepareModel(fallback: isModelReady ? previous : nil)
    }

    func setAppleIntelligenceCleanup(_ enabled: Bool) {
        appleIntelligenceCleanup = enabled
        keyboardBridge.appleIntelligenceCleanup = enabled
    }

    private func cleansUp(_ style: DictationStyle) -> Bool {
        style == .polished && appleIntelligenceCleanup && cleaner != nil
    }

    // MARK: - Standalone & System Actions

    func performPrimaryAction() {
        switch phase {
        case .modelRequired, .failed:
            prepareModel()
        case .ready:
            startRecording(copyToPasteboard: false)
        case .recording:
            finishRecording()
        case .preparingModel, .requestingPermission, .transcribing:
            break
        }
    }

    func cancelCurrentJob() {
        jobGeneration &+= 1
        transcriptionTask?.cancel()
        transcriptionTask = nil
        liveActivity.endImmediately()

        if let session = activeCapture {
            activeCapture = nil
            stopMetering()
            session.capture.stop()
            try? fileManager.removeItem(at: session.capture.url)
        }

        if activeKeyboardJobID != nil {
            keyboardMessage = nil
        }
        activeKeyboardJobID = nil
        audioLevel = 0
        loudnessDB = -120
        recordingDuration = 0
        if phase == .recording || phase == .transcribing || phase == .requestingPermission {
            phase = .ready
        }
        restoreAudioAfterCapture()
        publishKeyboardSnapshot()
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

    func toggleWarmSession() {
        if keyboardSessionEnabled {
            endWarmSession()
        } else {
            if warmDuration == .off {
                setWarmDuration(.fifteenMinutes)
            }
            requestWarmSession(attemptAutoReturn: false)
        }
    }

    func endWarmSession(message: String? = nil) {
        keyboardSessionTask?.cancel()
        keyboardSessionTask = nil
        pendingHandoff = nil
        suspendedWarmSessionOnInterruption = false

        // Cancel an unaccepted in-progress recording, but preserve any already-accepted transcription!
        if let session = activeCapture {
            jobGeneration &+= 1
            activeCapture = nil
            stopMetering()
            session.capture.stop()
            try? fileManager.removeItem(at: session.capture.url)
            activeKeyboardJobID = nil
            audioLevel = 0
            loudnessDB = -120
            recordingDuration = 0
            liveActivity.endImmediately()
            if phase == .recording {
                phase = .ready
            }
        }

        keyboardSessionEnabled = false
        keyboardSessionRemaining = 0
        keyboardSessionExpiresAt = 0
        keyboardMessage = message

        if phase != .recording {
            audio.deactivateSession()
        }

        publishKeyboardSnapshot()
    }

    func noteSceneDidEnterBackground() {
        autoReturnFallbackTask?.cancel()
        autoReturnFallbackTask = nil
        if case .attempting = autoReturnState {
            autoReturnState = .succeeded
        }
    }

    // MARK: - Keyboard Launch & Auto-Return

    func handleKeyboardLaunch(
        _ request: KeyboardLaunchRequest,
        urlOpener: (@MainActor (URL) async -> Bool)? = nil
    ) {
        if case .setup = request {
            return
        }
        if let customOpener = urlOpener {
            defaultURLOpener = customOpener
        }
        autoReturnFallbackTask?.cancel()
        autoReturnState = .idle
        handoffNotice = nil

        if let appID = request.returnAppID {
            setReturnAppID(appID)
        }

        let currentTime = now()
        if request.isHandoffExpired(at: currentTime) {
            handoffNotice = "Timed out"
            pendingHandoff = PendingKeyboardHandoff(
                jobID: nil,
                style: style,
                requestedAt: currentTime,
                attemptAutoReturn: false
            )
            fulfillPendingHandoffOrPrepare()
            return
        }

        switch request {
        case .setup:
            break

        case .startSession:
            pendingHandoff = PendingKeyboardHandoff(
                jobID: nil,
                style: style,
                requestedAt: currentTime,
                attemptAutoReturn: true
            )
            fulfillPendingHandoffOrPrepare()

        case .startRecording(let jobID, _, let requestedStyle, _):
            guard !jobID.isEmpty, jobID != lastHandledLaunchJobID else { return }
            lastHandledLaunchJobID = jobID
            if let requestedStyle {
                setStyle(requestedStyle)
            }
            pendingHandoff = PendingKeyboardHandoff(
                jobID: jobID,
                style: requestedStyle ?? style,
                requestedAt: currentTime,
                attemptAutoReturn: true
            )
            fulfillPendingHandoffOrPrepare()
        }
    }

    func returnToHostApp(_ app: HostApp, remember: Bool = true) {
        if remember {
            setReturnAppID(app.id)
        }
        triggerAutoReturn(to: app)
    }

    private func fulfillPendingHandoffOrPrepare() {
        guard let handoff = pendingHandoff else { return }

        if now() - handoff.requestedAt > KeyboardLaunchRequest.handoffWindow {
            handoffNotice = "Timed out"
            pendingHandoff = PendingKeyboardHandoff(
                jobID: nil,
                style: handoff.style,
                requestedAt: now(),
                attemptAutoReturn: false
            )
        }

        switch phase {
        case .modelRequired, .failed:
            prepareModel()
        case .ready:
            activateWarmSessionAndRunHandoff()
        case .preparingModel, .requestingPermission, .recording, .transcribing:
            break
        }
    }

    private func requestWarmSession(attemptAutoReturn: Bool) {
        pendingHandoff = PendingKeyboardHandoff(
            jobID: nil,
            style: style,
            requestedAt: now(),
            attemptAutoReturn: attemptAutoReturn
        )
        fulfillPendingHandoffOrPrepare()
    }

    private func activateWarmSessionAndRunHandoff() {
        guard phase == .ready else { return }
        let handoff = pendingHandoff
        pendingHandoff = nil

        // If a one-shot recording was requested (`warmDuration == .off` and a jobID is present),
        // go straight into recording and release the mic as soon as capture finishes.
        if warmDuration == .off, let jobID = handoff?.jobID {
            let expired = handoff.map { now() - $0.requestedAt > KeyboardLaunchRequest.handoffWindow } ?? false
            if !expired {
                startRecording(
                    keyboardJobID: jobID,
                    styleOverride: handoff?.style,
                    copyToPasteboard: false,
                    attemptAutoReturnOnStart: handoff?.attemptAutoReturn ?? false
                )
            } else {
                handoffNotice = "Timed out"
                autoReturnState = .swipeFallback
            }
            return
        }

        if keyboardSessionEnabled, audio.isKeepaliveRunning {
            refreshWarmExpiry()
            if let jobID = handoff?.jobID {
                let expired = handoff.map { now() - $0.requestedAt > KeyboardLaunchRequest.handoffWindow } ?? false
                if !expired {
                    startRecording(
                        keyboardJobID: jobID,
                        styleOverride: handoff?.style,
                        copyToPasteboard: false,
                        attemptAutoReturnOnStart: handoff?.attemptAutoReturn ?? false
                    )
                    return
                } else {
                    handoffNotice = "Timed out"
                }
            }
            maybeAutoReturnAfterActivation(allowed: handoff?.attemptAutoReturn ?? false)
            return
        }

        jobGeneration &+= 1
        let requestGen = jobGeneration
        phase = .requestingPermission
        keyboardMessage = nil

        Task {
            let granted = await audio.requestPermission()
            guard self.jobGeneration == requestGen else { return }
            guard granted else {
                phase = .failed("Microphone access is off. Allow it in Settings to dictate.")
                publishKeyboardSnapshot()
                return
            }

            // If we also have a fresh recording job, begin capture directly so we don't bounce keepalive -> recorder!
            if let jobID = handoff?.jobID,
               let requestedAt = handoff?.requestedAt,
               self.now() - requestedAt <= KeyboardLaunchRequest.handoffWindow {
                self.enableWarmSessionBookkeeping()
                self.phase = .ready
                self.startKeyboardSessionLoop()
                self.startRecording(
                    keyboardJobID: jobID,
                    styleOverride: handoff?.style,
                    copyToPasteboard: false,
                    attemptAutoReturnOnStart: handoff?.attemptAutoReturn ?? false
                )
                return
            }

            if let jobID = handoff?.jobID, !jobID.isEmpty {
                self.handoffNotice = "Timed out"
            }

            do {
                try audio.startKeepalive(allowBluetooth: allowBluetoothMic)
                enableWarmSessionBookkeeping()
                phase = .ready
                startKeyboardSessionLoop()
                publishKeyboardSnapshot()
                maybeAutoReturnAfterActivation(allowed: handoff?.attemptAutoReturn ?? false)
            } catch {
                audio.deactivateSession()
                phase = .failed(error.localizedDescription)
                publishKeyboardSnapshot()
            }
        }
    }

    private func enableWarmSessionBookkeeping() {
        let seconds = warmDuration.seconds ?? (15 * 60)
        keyboardSessionEnabled = true
        keyboardSessionExpiresAt = now() + seconds
        keyboardSessionRemaining = seconds
    }

    private func refreshWarmExpiry() {
        guard keyboardSessionEnabled, let seconds = warmDuration.seconds else { return }
        keyboardSessionExpiresAt = now() + seconds
        keyboardSessionRemaining = seconds
    }

    private func maybeAutoReturnAfterActivation(allowed: Bool) {
        guard allowed else { return }
        if let app = selectedReturnApp {
            triggerAutoReturn(to: app)
        } else {
            autoReturnState = .swipeFallback
        }
    }

    private func triggerAutoReturn(to app: HostApp) {
        guard let url = app.url else {
            autoReturnState = .swipeFallback
            return
        }
        autoReturnFallbackTask?.cancel()
        autoReturnState = .attempting(app)
        let opener = defaultURLOpener

        Task {
            let opened = await opener(url)
            guard case .attempting = self.autoReturnState else { return }
            if !opened {
                self.autoReturnState = .swipeFallback
                return
            }
            self.autoReturnFallbackTask = Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(950))
                guard !Task.isCancelled, let self else { return }
                if case .attempting = self.autoReturnState {
                    self.autoReturnState = .swipeFallback
                }
            }
        }
    }

    // MARK: - Model Preparation

    func prepareModel() {
        prepareModel(fallback: nil)
    }

    private func prepareModel(fallback: SpeechModel?) {
        modelPrepGeneration &+= 1
        let prepGen = modelPrepGeneration
        phase = .preparingModel
        copied = false
        modelPreparation = ModelPreparationProgress(
            fractionCompleted: 0,
            detail: "Checking model files"
        )

        Task {
            do {
                let candidate = if let transcriberFactory {
                    await transcriberFactory()
                } else {
                    await Self.makeTranscriber(for: self.speechModel)
                }
                guard self.modelPrepGeneration == prepGen else { return }
                engineName = candidate.displayName
                try await candidate.prepare { [weak self] progress in
                    Task { @MainActor [weak self] in
                        guard let self, self.modelPrepGeneration == prepGen else { return }
                        self.modelPreparation = progress
                    }
                }
                guard self.modelPrepGeneration == prepGen else { return }
                self.transcriber = candidate
                self.transcriptionLanguage = await candidate.languageName
                self.keyboardBridge.setModelInstalled(true)
                self.isModelReady = true
                self.phase = .ready
                self.publishKeyboardSnapshot()
                if self.pendingHandoff != nil {
                    self.activateWarmSessionAndRunHandoff()
                }
            } catch {
                guard self.modelPrepGeneration == prepGen else { return }
                if let fallback {
                    self.speechModel = fallback
                    self.keyboardBridge.speechModel = fallback
                    self.speechModelError = error.localizedDescription
                    self.engineName = self.transcriber.displayName
                    self.phase = .ready
                    self.publishKeyboardSnapshot()
                    if self.pendingHandoff != nil {
                        self.activateWarmSessionAndRunHandoff()
                    }
                    return
                }
                self.keyboardBridge.setModelInstalled(ParakeetTranscriber.hasCachedModel)
                self.pendingHandoff = nil
                self.phase = .failed(error.localizedDescription)
                self.publishKeyboardSnapshot()
            }
        }
    }

    private static var appleSpeechSupported: Bool {
        if #available(iOS 26.0, *) {
            return AppleSpeechTranscriber.isAvailable
        }
        return false
    }

    /// Apple Speech when chosen and it supports the phone's language, else Parakeet.
    private static func makeTranscriber(for model: SpeechModel) async -> any LocalTranscribing {
        if model == .apple,
           #available(iOS 26.0, *),
           AppleSpeechTranscriber.isAvailable,
           let locale = await AppleSpeechTranscriber.preferredLocale() {
            return AppleSpeechTranscriber(locale: locale)
        }
        return ParakeetTranscriber()
    }

    // MARK: - Capture & Generation-Scoped Transcription

    func startRecording(
        keyboardJobID: String? = nil,
        styleOverride: DictationStyle? = nil,
        copyToPasteboard: Bool = false,
        attemptAutoReturnOnStart: Bool = false
    ) {
        guard activeCapture == nil else { return }
        if let keyboardJobID, keyboardJobID == activeKeyboardJobID {
            return
        }
        activeKeyboardJobID = keyboardJobID
        if keyboardJobID != nil {
            keyboardMessage = nil
        }

        // Bumping generation invalidates any stale winding-down transcription task so it can never
        // overwrite our new capture's phase, job state, or temporary WAV file.
        jobGeneration &+= 1
        let captureGeneration = jobGeneration
        transcriptionTask?.cancel()
        transcriptionTask = nil
        let effectiveStyle = styleOverride ?? style
        if cleansUp(effectiveStyle) {
            cleaner?.prewarm()
        }

        phase = .requestingPermission
        copied = false

        Task {
            let granted = await audio.requestPermission()
            guard self.jobGeneration == captureGeneration else { return }
            guard granted else {
                activeKeyboardJobID = nil
                let message = "Microphone access is off. Allow it in Settings."
                keyboardMessage = keyboardJobID == nil ? nil : message
                phase = .failed(message)
                publishKeyboardSnapshot()
                return
            }

            let captureURL = fileManager.temporaryDirectory
                .appendingPathComponent("hex-dictation-\(captureGeneration)-\(UUID().uuidString)")
                .appendingPathExtension("wav")

            do {
                let capture = try audio.beginCapture(
                    at: captureURL,
                    maxDuration: Self.maxCaptureDuration,
                    allowBluetooth: allowBluetoothMic
                )
                guard self.jobGeneration == captureGeneration else {
                    capture.stop()
                    try? fileManager.removeItem(at: captureURL)
                    return
                }

                let startedAt = now()
                activeCapture = ActiveCaptureSession(
                    generation: captureGeneration,
                    capture: capture,
                    startedAt: startedAt,
                    keyboardJobID: keyboardJobID,
                    style: effectiveStyle,
                    copyToPasteboard: copyToPasteboard
                )
                refreshWarmExpiry()
                audioLevel = 0
                loudnessDB = -120
                recordingDuration = 0
                phase = .recording
                liveActivity.startRecording(startedAt: Date(timeIntervalSince1970: startedAt))
                startMetering(for: captureGeneration)
                publishKeyboardSnapshot()

                if attemptAutoReturnOnStart {
                    maybeAutoReturnAfterActivation(allowed: true)
                }
            } catch {
                try? fileManager.removeItem(at: captureURL)
                activeKeyboardJobID = nil
                keyboardMessage = keyboardJobID == nil ? nil : error.localizedDescription
                phase = .failed(error.localizedDescription)
                restoreAudioAfterCapture()
                publishKeyboardSnapshot()
            }
        }
    }

    func finishRecording() {
        guard let session = activeCapture else { return }
        activeCapture = nil

        stopMetering()
        let finalDuration = max(recordingDuration, session.capture.currentTime)
        session.capture.stop()
        audioLevel = 0
        loudnessDB = -120

        let captureURL = session.capture.url
        let captureGeneration = session.generation
        let keyboardJobID = session.keyboardJobID

        guard finalDuration >= Self.minCaptureDuration else {
            try? fileManager.removeItem(at: captureURL)
            recordingDuration = 0
            activeKeyboardJobID = nil
            keyboardMessage = nil
            phase = .ready
            liveActivity.endImmediately()
            restoreAudioAfterCapture()
            publishKeyboardSnapshot()
            return
        }

        phase = .transcribing
        liveActivity.markTranscribing(startedAt: Date(timeIntervalSince1970: session.startedAt))
        restoreAudioAfterCapture()
        publishKeyboardSnapshot()

        let activeTranscriber = transcriber

        transcriptionTask?.cancel()
        transcriptionTask = Task { [weak self] in
            defer {
                try? FileManager.default.removeItem(at: captureURL)
            }

            do {
                let rawText = try await activeTranscriber.transcribe(captureURL)
                guard let self, self.jobGeneration == captureGeneration, !Task.isCancelled else {
                    return
                }
                var formatted = TranscriptFormatter.format(rawText, style: session.style)
                if self.cleansUp(session.style), let cleaner = self.cleaner {
                    if let cleaned = await cleaner.clean(formatted) {
                        formatted = TranscriptFormatter.finishPolished(cleaned)
                    }
                    guard self.jobGeneration == captureGeneration, !Task.isCancelled else {
                        return
                    }
                }
                self.recordingDuration = 0
                self.transcript = formatted
                if session.copyToPasteboard && !formatted.isEmpty {
                    UIPasteboard.general.string = formatted
                    self.copied = true
                    self.liveActivity.finishCopied()
                } else {
                    self.liveActivity.endImmediately()
                }

                if keyboardJobID != nil {
                    self.keyboardResultID = UUID().uuidString
                    self.keyboardResult = formatted
                    self.keyboardResultExpiresAt = self.now() + Self.resultDeliveryTTL
                    self.keyboardMessage = nil
                }
                self.activeKeyboardJobID = nil
                self.refreshWarmExpiry()
                self.phase = .ready
                self.publishKeyboardSnapshot()
            } catch {
                guard !Task.isCancelled, let self, self.jobGeneration == captureGeneration else {
                    return
                }
                self.recordingDuration = 0
                self.activeKeyboardJobID = nil
                self.liveActivity.endImmediately()
                self.keyboardMessage = keyboardJobID == nil ? nil : error.localizedDescription
                self.phase = .failed(error.localizedDescription)
                self.publishKeyboardSnapshot()
            }
        }
    }

    private func restoreAudioAfterCapture() {
        if keyboardSessionEnabled, let seconds = warmDuration.seconds, seconds > 0 {
            let remaining = keyboardSessionExpiresAt - now()
            if remaining <= 0 {
                keyboardSessionEnabled = false
                keyboardSessionRemaining = 0
                keyboardSessionExpiresAt = 0
                audio.deactivateSession()
                return
            }
            do {
                try audio.startKeepalive(allowBluetooth: allowBluetoothMic)
            } catch {
                keyboardSessionEnabled = false
                keyboardSessionRemaining = 0
                keyboardSessionExpiresAt = 0
                keyboardMessage = error.localizedDescription
                audio.deactivateSession()
            }
        } else {
            keyboardSessionEnabled = false
            keyboardSessionRemaining = 0
            keyboardSessionExpiresAt = 0
            audio.deactivateSession()
        }
    }

    private func startMetering(for generation: UInt64) {
        stopMetering()
        meterTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self,
                      let session = self.activeCapture,
                      session.generation == generation else {
                    break
                }

                // Recorder readiness is a sample, not the capture's lifetime. A transient
                // false must not strand the app meter and keyboard heartbeat until Stop.
                // Completion/interruption callbacks and explicit Stop own capture teardown.
                guard session.capture.isRecording else {
                    try? await Task.sleep(for: .milliseconds(33))
                    continue
                }

                let db = session.capture.averagePowerDB()
                self.loudnessDB = db
                self.audioLevel = max(0, min(1, (db + 48) / 48))
                self.recordingDuration = session.capture.currentTime
                self.publishKeyboardSnapshot(minInterval: 0.10)

                if self.recordingDuration >= Self.maxCaptureDuration - 0.25 {
                    self.finishRecording()
                    return
                }

                try? await Task.sleep(for: .milliseconds(33))
            }
        }
    }

    private func stopMetering() {
        meterTask?.cancel()
        meterTask = nil
    }

    // MARK: - Warm Session Loop & Snapshot Truth

    private func startKeyboardSessionLoop() {
        keyboardSessionTask?.cancel()
        keyboardSessionTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self, self.keyboardSessionEnabled else { return }

                // Never expire an in-flight recording or accepted transcription mid-work.
                if self.phase != .recording && self.phase != .transcribing {
                    let remaining = self.keyboardSessionExpiresAt - self.now()
                    if remaining <= 0 {
                        self.endWarmSession()
                        return
                    }
                    self.keyboardSessionRemaining = remaining

                    // Verify the keepalive engine is genuinely alive so we never publish a stuck "Ready".
                    if self.phase == .ready && !self.audio.isKeepaliveRunning && !self.suspendedWarmSessionOnInterruption {
                        do {
                            try self.audio.startKeepalive(allowBluetooth: self.allowBluetoothMic)
                        } catch {
                            self.endWarmSession()
                            return
                        }
                    }
                }

                self.consumeKeyboardCommand()
                if self.phase != .recording {
                    self.publishKeyboardSnapshot(minInterval: 0.60)
                }
                try? await Task.sleep(for: .milliseconds(100))
            }
        }
    }

    func processPendingKeyboardCommandForTesting() {
        consumeKeyboardCommand()
        publishKeyboardSnapshot()
    }

    func updateRecordingDurationForTesting(_ duration: TimeInterval) {
        recordingDuration = duration
    }

    private func consumeKeyboardCommand() {
        guard let command = keyboardBridge.latestCommand(),
              command.id != lastKeyboardCommandID else { return }
        lastKeyboardCommandID = command.id

        // Ignore stale commands left in UserDefaults from >10s ago.
        guard now() - command.sentAt <= KeyboardLaunchRequest.handoffWindow else { return }

        switch command.kind {
        case .start:
            guard phase == .ready, activeKeyboardJobID == nil else { return }
            setStyle(command.style)
            startRecording(
                keyboardJobID: command.jobID,
                styleOverride: command.style,
                copyToPasteboard: false
            )
        case .stop:
            guard phase == .recording, activeKeyboardJobID == command.jobID else { return }
            finishRecording()
        case .cancel:
            guard (phase == .recording || phase == .transcribing),
                  activeKeyboardJobID == command.jobID else { return }
            cancelCurrentJob()
        }
    }

    private func publishKeyboardSnapshot(minInterval: TimeInterval = 0) {
        let currentTime = now()

        if let expires = keyboardResultExpiresAt, expires <= currentTime {
            keyboardResultID = nil
            keyboardResult = nil
            keyboardResultExpiresAt = nil
        }

        let state: KeyboardDictationState
        switch phase {
        case .recording:
            // Hardware truth guard: only claim `.recording` while an active capture is genuinely recording.
            if let session = activeCapture, session.capture.isRecording {
                state = .recording
            } else {
                state = keyboardSessionEnabled ? .ready : .offline
            }
        case .transcribing:
            state = .transcribing
        case .failed:
            state = .failed
        case .modelRequired, .preparingModel, .requestingPermission:
            state = .offline
        case .ready:
            state = (keyboardSessionEnabled && audio.isKeepaliveRunning) ? .ready : .offline
        }

        let failureMessage: String?
        if case .failed(let message) = phase {
            failureMessage = message
        } else {
            failureMessage = keyboardMessage
        }

        let signature = "\(state.rawValue)|\(activeKeyboardJobID ?? "")|\(keyboardResultID ?? "")|\(failureMessage ?? "")"
        if minInterval > 0,
           signature == lastPublishedSnapshotSignature,
           currentTime - lastPublishedSnapshotAt < minInterval {
            return
        }
        lastPublishedSnapshotSignature = signature
        lastPublishedSnapshotAt = currentTime

        let effectiveHeartbeat: TimeInterval
        if state == .recording || state == .transcribing || (keyboardSessionEnabled && audio.isKeepaliveRunning) {
            effectiveHeartbeat = currentTime
        } else {
            effectiveHeartbeat = 0
        }

        keyboardBridge.publish(
            KeyboardSnapshot(
                state: state,
                heartbeat: effectiveHeartbeat,
                expiresAt: keyboardSessionEnabled ? keyboardSessionExpiresAt : 0,
                recordingStartedAt: activeCapture?.startedAt,
                audioLevel: state == .recording ? audioLevel : 0,
                loudnessDB: state == .recording ? loudnessDB : nil,
                jobID: activeKeyboardJobID,
                resultID: keyboardResultID,
                resultExpiresAt: keyboardResultExpiresAt,
                transcript: keyboardResult,
                message: failureMessage
            )
        )
    }

    // MARK: - Audio Interruptions, Route Changes, & System Intents

    private func wireAudioCallbacks() {
        audio.onUnexpectedCaptureEnd = { [weak self] finishedURL, succeeded in
            guard let self,
                  let session = self.activeCapture,
                  session.capture.url == finishedURL else {
                return
            }
            if succeeded {
                self.finishRecording()
            } else {
                self.cancelCurrentJob()
                self.phase = .failed("Recording stopped unexpectedly.")
                self.publishKeyboardSnapshot()
            }
        }

        audio.onSystemEvent = { [weak self] event in
            self?.handleAudioSystemEvent(event)
        }
    }

    func handleAudioSystemEvent(_ event: AudioSystemEvent) {
        switch event {
        case .interruptionBegan:
            suspendedWarmSessionOnInterruption = keyboardSessionEnabled
            if phase == .recording {
                // Save spoken audio if >= 300ms; otherwise cancel cleanly so "Listening" never sticks during a call.
                if let session = activeCapture, max(recordingDuration, session.capture.currentTime) >= Self.minCaptureDuration {
                    audio.stopKeepalive()
                    finishRecording()
                } else {
                    cancelCurrentJob()
                    keyboardMessage = "Interrupted"
                }
            } else {
                audio.stopKeepalive()
            }
            publishKeyboardSnapshot()

        case .interruptionEnded(let shouldResume):
            guard suspendedWarmSessionOnInterruption else { return }
            suspendedWarmSessionOnInterruption = false
            guard keyboardSessionEnabled, keyboardSessionExpiresAt > now() else {
                endWarmSession()
                return
            }
            if shouldResume || UIApplication.shared.applicationState == .active {
                do {
                    try audio.startKeepalive(allowBluetooth: allowBluetoothMic)
                    publishKeyboardSnapshot()
                } catch {
                    endWarmSession()
                }
            } else {
                endWarmSession()
            }

        case .routeOldDeviceUnavailable:
            // e.g. AirPods placed in case: finish active speech >= 300ms or rebind keepalive tap to built-in mic.
            if phase == .recording {
                if let session = activeCapture, max(recordingDuration, session.capture.currentTime) >= Self.minCaptureDuration {
                    finishRecording()
                } else {
                    cancelCurrentJob()
                }
            } else if keyboardSessionEnabled && phase == .ready {
                do {
                    try audio.startKeepalive(allowBluetooth: allowBluetoothMic)
                    publishKeyboardSnapshot()
                } catch {
                    endWarmSession()
                }
            }

        case .routeNewDeviceAvailable:
            if keyboardSessionEnabled && phase == .ready && allowBluetoothMic {
                try? audio.startKeepalive(allowBluetooth: allowBluetoothMic)
                publishKeyboardSnapshot()
            }

        case .mediaServicesReset:
            if phase == .recording {
                cancelCurrentJob()
            }
            if keyboardSessionEnabled {
                do {
                    try audio.startKeepalive(allowBluetooth: allowBluetoothMic)
                    publishKeyboardSnapshot()
                } catch {
                    endWarmSession()
                }
            }
        }
    }

    private func wireSystemIntentBus() {
        Task {
            await HexSystemCommandBus.shared.register { [weak self] command in
                guard let self else { return nil }
                return await self.performSystemCommand(command)
            }
        }
    }

    private func performSystemCommand(_ command: HexSystemCommand) async -> String? {
        switch command {
        case .toggleDictation:
            if phase == .recording {
                finishRecording()
                if let task = transcriptionTask {
                    await task.value
                }
                return transcript
            }
            if phase == .modelRequired || isFailedPhase {
                prepareModel()
                try? await waitUntilReady(timeoutSeconds: 10)
            }
            guard phase == .ready else { return nil }
            startRecording(copyToPasteboard: true)
            return ""

        case .stopDictation:
            guard phase == .recording else { return transcript }
            finishRecording()
            if let task = transcriptionTask {
                await task.value
            }
            return transcript

        case .cancelDictation:
            cancelCurrentJob()
            return nil

        case .warmSession:
            if !keyboardSessionEnabled {
                if warmDuration == .off {
                    setWarmDuration(.fifteenMinutes)
                }
                requestWarmSession(attemptAutoReturn: false)
            }
            return nil
        }
    }

    private var isFailedPhase: Bool {
        if case .failed = phase { return true }
        return false
    }

    private func waitUntilReady(timeoutSeconds: TimeInterval) async throws {
        let deadline = now() + timeoutSeconds
        while phase == .preparingModel && now() < deadline {
            try await Task.sleep(for: .milliseconds(100))
        }
    }

    // MARK: - Launch Fixtures for Simulator Verification (`--snap-state <name>`)

    private static func applyLaunchFixtureIfPresent(to controller: DictationController) -> Bool {
        let args = ProcessInfo.processInfo.arguments
        guard let flagIndex = args.firstIndex(of: "--snap-state"),
              flagIndex + 1 < args.count else {
            return false
        }
        let scene = args[flagIndex + 1]
        controller.engineName = "Apple Speech"
        controller.transcriptionLanguage = "English"
        controller.canChooseSpeechModel = true
        controller.cleanupAvailable = true
        controller.keyboardBridge.setModelInstalled(true)
        controller.isModelReady = true

        switch scene {
        case "setup":
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = false

        case "ready":
            controller.phase = .ready
            controller.warmDuration = .off
            controller.keyboardSessionEnabled = false
            controller.transcript = "Meet at ten on Thursday and bring the updated design notes."

        case "warm":
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 42
            controller.keyboardSessionExpiresAt = Date().timeIntervalSince1970 + 14 * 60 + 42
            controller.setReturnAppID("messages")
            controller.transcript = "Meet at ten on Thursday and bring the updated design notes."

        case "recording":
            controller.phase = .recording
            controller.warmDuration = .fifteenMinutes
            controller.audioLevel = 0.74
            controller.loudnessDB = -31.0
            controller.recordingDuration = 6.4
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 54

        case "transcribing":
            controller.phase = .transcribing
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 52

        case "handoff-listening", "handoff-no-home-bar":
            controller.phase = .recording
            controller.audioLevel = 0.68
            controller.loudnessDB = -32.5
            controller.recordingDuration = 3.2
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 57
            controller.autoReturnState = .swipeFallback
            Task { @MainActor in
                let start = Date().timeIntervalSince1970 - 3.0
                var simTime: Float = 0.0
                while !Task.isCancelled {
                    controller.loudnessDB = DotScopeSimulation.demoLoudness(at: simTime) + 4.5
                    controller.recordingDuration = Date().timeIntervalSince1970 - start
                    simTime += 0.033
                    try? await Task.sleep(for: .milliseconds(33))
                }
            }

        case "handoff-tour":
            controller.phase = .recording
            controller.audioLevel = 0.68
            controller.loudnessDB = -32.0
            controller.recordingDuration = 0.0
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 58
            controller.autoReturnState = .swipeFallback
            Task { @MainActor in
                let boot = Date().timeIntervalSince1970
                var simTime: Float = 0.0
                while !Task.isCancelled {
                    let elapsed = Date().timeIntervalSince1970 - boot
                    if elapsed < 3.4 {
                        if controller.phase != .recording {
                            withAnimation(HexMotion.stage) {
                                controller.phase = .recording
                            }
                        }
                        controller.loudnessDB = DotScopeSimulation.demoLoudness(at: simTime) + 4.5
                        controller.recordingDuration = elapsed
                        simTime += 0.033
                    } else if elapsed < 4.7 {
                        if controller.phase != .transcribing {
                            withAnimation(HexMotion.stage) {
                                controller.phase = .transcribing
                            }
                        }
                        controller.loudnessDB = -120.0
                    } else {
                        if controller.phase != .ready {
                            withAnimation(HexMotion.stage) {
                                controller.phase = .ready
                            }
                        }
                    }
                    try? await Task.sleep(for: .milliseconds(33))
                }
            }

        case "handoff-returning":
            controller.phase = .recording
            controller.audioLevel = 0.62
            controller.recordingDuration = 1.1
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 59
            controller.setReturnAppID("claude")
            controller.autoReturnState = .attempting(HostApp(id: "claude", name: "Claude", schemeURL: "claude://"))

        case "hosted-keyboard-warm":
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 42
            controller.keyboardSessionExpiresAt = Date().timeIntervalSince1970 + 14 * 60 + 42
            controller.setReturnAppID("messages")
            Task { @MainActor in
                while !Task.isCancelled {
                    let t = Date().timeIntervalSince1970
                    controller.keyboardBridge.publish(
                        KeyboardSnapshot(
                            state: .ready,
                            heartbeat: t,
                            expiresAt: t + 14 * 60 + 42
                        )
                    )
                    try? await Task.sleep(for: .milliseconds(250))
                }
            }

        case "hosted-keyboard-recording", "hosted-keyboard-light":
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 55
            let startWall = Date().timeIntervalSince1970 - 4.0
            Task { @MainActor in
                var simTime: Float = 0.4
                while !Task.isCancelled {
                    let t = Date().timeIntervalSince1970
                    let db = -28.5 + 5.2 * sin(simTime * 19.0) + 2.4 * cos(simTime * 7.3)
                    controller.keyboardBridge.publish(
                        KeyboardSnapshot(
                            state: .recording,
                            heartbeat: t,
                            expiresAt: t + 15 * 60,
                            recordingStartedAt: startWall,
                            audioLevel: 0.78,
                            loudnessDB: db,
                            jobID: "hosted-demo-job"
                        )
                    )
                    simTime += 0.033
                    try? await Task.sleep(for: .milliseconds(33))
                }
            }

        case "hosted-keyboard-tour":
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 58
            Task { @MainActor in
                let boot = Date().timeIntervalSince1970
                let freshResultID = "hosted-tour-\(UUID().uuidString)"
                var simTime: Float = 0.0
                while !Task.isCancelled {
                    let t = Date().timeIntervalSince1970
                    let elapsed = t - boot
                    if elapsed < 2.5 {
                        controller.keyboardBridge.publish(
                            KeyboardSnapshot(
                                state: .ready,
                                heartbeat: t,
                                expiresAt: t + 15 * 60
                            )
                        )
                    } else if elapsed < 5.2 {
                        let db = DotScopeSimulation.demoLoudness(at: simTime) + 4.5
                        controller.keyboardBridge.publish(
                            KeyboardSnapshot(
                                state: .recording,
                                heartbeat: t,
                                expiresAt: t + 15 * 60,
                                recordingStartedAt: boot + 2.5,
                                audioLevel: 0.78,
                                loudnessDB: db,
                                jobID: "hosted-tour-job"
                            )
                        )
                        simTime += 0.033
                    } else if elapsed < 6.4 {
                        controller.keyboardBridge.publish(
                            KeyboardSnapshot(
                                state: .transcribing,
                                heartbeat: t,
                                expiresAt: t + 15 * 60,
                                jobID: "hosted-tour-job"
                            )
                        )
                    } else {
                        controller.keyboardBridge.publish(
                            KeyboardSnapshot(
                                state: .ready,
                                heartbeat: t,
                                expiresAt: t + 14 * 60 + 54,
                                resultID: freshResultID,
                                resultExpiresAt: boot + 30,
                                transcript: "See you at ten."
                            )
                        )
                    }
                    try? await Task.sleep(for: .milliseconds(33))
                }
            }

        default:
            return false
        }
        return true
    }
}
