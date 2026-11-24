import Foundation
import XCTest
@testable import HEX

@MainActor
private final class StateTestAudioCapture: ActiveAudioCapture {
    let url: URL
    var isRecording = true
    var currentTime: TimeInterval
    var power: Float = 0.5

    init(url: URL, initialDuration: TimeInterval = 1.2) {
        self.url = url
        self.currentTime = initialDuration
        FileManager.default.createFile(atPath: url.path, contents: Data("wav".utf8))
    }

    func normalizedPowerLevel() -> Float { power }
    func stop() { isRecording = false }
}

@MainActor
private final class StateTestAudioManager: AudioManaging {
    var isKeepaliveRunning = false
    var permissionGranted = true
    var keepaliveShouldThrow = false
    var captureShouldThrow = false
    var nextCaptureDuration: TimeInterval = 1.2
    var deactivateCount = 0
    var createdCaptures: [StateTestAudioCapture] = []

    var onSystemEvent: ((AudioSystemEvent) -> Void)?
    var onUnexpectedCaptureEnd: ((URL, Bool) -> Void)?

    func requestPermission() async -> Bool { permissionGranted }

    func startKeepalive() throws {
        if keepaliveShouldThrow {
            isKeepaliveRunning = false
            throw CocoaError(.coderInvalidValue)
        }
        isKeepaliveRunning = true
    }

    func stopKeepalive() {
        isKeepaliveRunning = false
    }

    func deactivateSession() {
        isKeepaliveRunning = false
        deactivateCount += 1
    }

    func beginCapture(
        at url: URL,
        maxDuration: TimeInterval,
        microphone: MicrophoneChoice
    ) throws -> any ActiveAudioCapture {
        isKeepaliveRunning = false
        if captureShouldThrow {
            throw CocoaError(.fileWriteUnknown)
        }
        let capture = StateTestAudioCapture(url: url, initialDuration: nextCaptureDuration)
        createdCaptures.append(capture)
        return capture
    }
}

private actor StateTestTranscriber: LocalTranscribing {
    nonisolated let engine: DictationEngine = .appleSpeech
    let languageName = "English"
    var nextError: Error?

    func setNextError(_ error: Error?) { nextError = error }

    func prepare(progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void) async throws {
        progressHandler(ModelPreparationProgress(fractionCompleted: 1, detail: "Ready"))
    }

    func transcribe(_ audioURL: URL) async throws -> String {
        if let error = nextError {
            nextError = nil
            throw error
        }
        return "verified"
    }
}

@MainActor
final class DictationStateMachineTests: XCTestCase {
    private var suiteName = ""
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "hex.ism.tests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    // MARK: - DictationController.KeepaliveBackoff

    func testKeepaliveBackoffDoublesUpToCapAndResetsAfterSteadyUptime() {
        var backoff = DictationController.KeepaliveBackoff()
        let t0 = 10_000.0

        // First failure restarts immediately.
        XCTAssertTrue(backoff.shouldRestart(at: t0))
        XCTAssertEqual(backoff.delay, 0)

        // Rapid subsequent stops back off exponentially: 1, 2, 4, 8, 16, 30, 30.
        var cursor = t0
        for expectedDelay in [1.0, 2.0, 4.0, 8.0, 16.0, 30.0, 30.0] {
            XCTAssertFalse(backoff.shouldRestart(at: cursor + 0.1), "waits for \(expectedDelay)s")
            XCTAssertEqual(backoff.delay, expectedDelay)
            XCTAssertFalse(backoff.shouldRestart(at: cursor + expectedDelay - 0.05))
            cursor += expectedDelay + 0.1
            XCTAssertTrue(backoff.shouldRestart(at: cursor), "fires once \(expectedDelay)s elapsed")
        }

        // Running steadily for >= steadyDuration resets the delay to 0.
        backoff.noteRunning()
        XCTAssertNil(backoff.restartDueAt)
        cursor += DictationController.KeepaliveBackoff.steadyDuration + 1
        XCTAssertTrue(backoff.shouldRestart(at: cursor))
        XCTAssertEqual(backoff.delay, 0)

        backoff.reset()
        XCTAssertEqual(backoff.delay, 0)
        XCTAssertNil(backoff.restartDueAt)
        XCTAssertEqual(backoff.lastRestartedAt, -.infinity)
    }

    // MARK: - DictationController.Session & Warm Teardown Invariants

    func testWarmSessionStateTransitionsKeepTripleInSyncAcrossAllTeardownPaths() async throws {
        var now = 20_000.0
        let audio = StateTestAudioManager()
        let transcriber = StateTestTranscriber()
        let bridge = KeyboardBridge(defaults: defaults)
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false
        )
        controller.prepareModel()
        try await settle()

        // 1. Cold at start.
        XCTAssertEqual(controller.warmSession, .cold)
        XCTAssertFalse(controller.keyboardSessionEnabled)
        XCTAssertEqual(controller.warmSession.expiresAt, 0)
        XCTAssertEqual(controller.keyboardSessionRemaining, 0)
        XCTAssertEqual(controller.keyboardSessionDisplayRemaining, "0:00")

        // 2. Warm session activates with consistent expiry and display remaining.
        controller.selectWarmDuration(.fifteenMinutes)
        try await settle()
        XCTAssertEqual(controller.warmSession, .warm(expiresAt: now + 900, suspendedOnInterruption: false))
        XCTAssertTrue(controller.keyboardSessionEnabled)
        XCTAssertEqual(controller.warmSession.expiresAt, now + 900)
        XCTAssertEqual(controller.keyboardSessionRemaining, 900)
        XCTAssertEqual(controller.keyboardSessionDisplayRemaining, "15m")

        // 3. Audio interruption suspends without dropping expiry; interruption end with expired session tears down cleanly.
        controller.handleAudioSystemEvent(.interruptionBegan)
        XCTAssertEqual(controller.warmSession, .warm(expiresAt: now + 900, suspendedOnInterruption: true))
        now += 905
        controller.handleAudioSystemEvent(.interruptionEnded(shouldResume: true))
        XCTAssertEqual(controller.warmSession, .cold)
        XCTAssertFalse(controller.keyboardSessionEnabled)
        XCTAssertEqual(controller.warmSession.expiresAt, 0)
        XCTAssertEqual(controller.keyboardSessionRemaining, 0)
        XCTAssertEqual(controller.keyboardSessionDisplayRemaining, "0:00")

        // 4. Keepalive failure during restoreAudioAfterCapture tears down the entire warm triple atomically.
        controller.selectWarmDuration(.fifteenMinutes)
        try await settle()
        XCTAssertTrue(controller.warmSession.isWarm)
        controller.startRecording(keyboardJobID: "job-restore-fail")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        XCTAssertNotNil(controller.capture.activeSession)

        audio.keepaliveShouldThrow = true
        controller.cancelCurrentJob()
        XCTAssertEqual(controller.warmSession, .cold)
        XCTAssertFalse(controller.keyboardSessionEnabled)
        XCTAssertEqual(controller.warmSession.expiresAt, 0)
        XCTAssertEqual(controller.keyboardSessionRemaining, 0)
        XCTAssertEqual(controller.keyboardSessionDisplayRemaining, "0:00")
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertNil(controller.capture.activeSession)
        XCTAssertNil(controller.capture.keyboardJobID)
    }

    // MARK: - DictationController.Capture & Derived Phase Invariants

    func testCaptureEnumDerivesPhaseAndEnforcesRetryAndFailureInvariants() async throws {
        var now = 30_000.0
        let audio = StateTestAudioManager()
        let transcriber = StateTestTranscriber()
        let bridge = KeyboardBridge(defaults: defaults)
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false
        )
        XCTAssertEqual(controller.modelState, .required)
        XCTAssertEqual(controller.phase, .modelRequired)

        controller.prepareModel()
        try await settle()
        XCTAssertEqual(controller.modelState, .ready)
        XCTAssertEqual(controller.phase, .ready)

        // Permission denial is NOT keyboard-retryable even when model is ready.
        audio.permissionGranted = false
        controller.startRecording(keyboardJobID: "job-denied")
        try await settle()
        XCTAssertFalse(controller.capture.allowsKeyboardRetry)
        XCTAssertNil(controller.capture.keyboardJobID)
        if case .failed = controller.phase {} else { XCTFail("Expected failed phase, got \(controller.phase)") }

        // Microphone failure with model ready IS keyboard-retryable and carries failure timestamp.
        audio.permissionGranted = true
        audio.captureShouldThrow = true
        now += 5
        controller.startRecording(keyboardJobID: "job-mic-fail")
        try await settle()
        XCTAssertTrue(controller.capture.allowsKeyboardRetry)
        XCTAssertEqual(controller.capture.failureMessage?.at, now)
        XCTAssertNil(controller.capture.keyboardJobID)
        XCTAssertNil(controller.capture.activeSession)

        // Unexpected capture failure clears activeSession and marks capture retryable.
        audio.captureShouldThrow = false
        now += 5
        controller.startRecording(keyboardJobID: "job-unexpected")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        let activeURL = try XCTUnwrap(controller.capture.activeSession?.capture.url)
        XCTAssertEqual(controller.capture.keyboardJobID, "job-unexpected")

        // Transitioning modelState to .ready during an active capture preserves the recording.
        controller.transition(model: .ready)
        XCTAssertEqual(controller.phase, .recording)
        XCTAssertEqual(controller.capture.keyboardJobID, "job-unexpected")

        audio.onUnexpectedCaptureEnd?(activeURL, false)
        XCTAssertNil(controller.capture.activeSession)
        XCTAssertNil(controller.capture.keyboardJobID)
        XCTAssertTrue(controller.capture.allowsKeyboardRetry)
        XCTAssertEqual(controller.phase, .failed(String(localized: "Recording stopped.")))

        // Transcription error with model ready is also retryable and clears active job.
        await transcriber.setNextError(CocoaError(.fileReadCorruptFile))
        now += 5
        controller.startRecording(keyboardJobID: "job-tx-fail")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        controller.finishRecording()
        try await settle()
        XCTAssertNil(controller.capture.activeSession)
        XCTAssertNil(controller.capture.keyboardJobID)
        XCTAssertTrue(controller.capture.allowsKeyboardRetry)
        if case .failed = controller.phase {} else { XCTFail("Expected failed phase") }

        // Model re-preparation clears capture failure so phase reflects preparing -> ready.
        controller.prepareModel()
        XCTAssertEqual(controller.phase, .preparingModel)
        try await settle()
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertFalse(controller.capture.allowsKeyboardRetry)
    }

    // MARK: - KeyboardLocalCapturePhase Snapshot Projection

    func testLocalCapturePhaseSnapshotTransitionsDriveViewModel() throws {
        let t0 = 40_000.0
        let model = KeyboardViewModel()
        func apply(_ phase: KeyboardLocalCapturePhase, at now: TimeInterval, loudnessDB: Float = -62) {
            model.applyBridgeState(
                hasFullAccess: true,
                snapshot: phase.snapshot(now: now, loudnessDB: loudnessDB) ?? .offline,
                isModelInstalled: true,
                style: .polished,
                now: now,
                animated: false
            )
        }

        // Armed touch-down emits no snapshot so the keyboard stays idle until kept.
        let armed = KeyboardLocalCapturePhase.armed(jobID: "job-1")
        XCTAssertTrue(armed.isActive)
        XCTAssertTrue(armed.isCapturing("job-1"))
        XCTAssertNil(armed.snapshot(now: t0, loudnessDB: -62))
        apply(armed, at: t0)
        XCTAssertEqual(model.stagePhase, .idle)

        // Kept before audio flows: .starting keeps UI in .starting with no running clock.
        let starting = KeyboardLocalCapturePhase.starting(jobID: "job-1")
        XCTAssertEqual(starting.visibleStage, .starting)
        apply(starting, at: t0 + 0.1)
        XCTAssertEqual(model.stagePhase, .starting)
        XCTAssertEqual(model.clock.recordingText, "0:00")

        // First audio arrives: .recording advances clock from startedAt and passes loudness.
        let recording = KeyboardLocalCapturePhase.recording(jobID: "job-1", startedAt: t0 + 0.25)
        apply(recording, at: t0 + 2.25, loudnessDB: -24)
        XCTAssertEqual(model.stagePhase, .recording)
        XCTAssertEqual(model.liveLoudnessDB, -24)
        XCTAssertEqual(model.clock.recordingText, "0:02")

        // Transcribing stops capturing while keeping the job active.
        let transcribing = KeyboardLocalCapturePhase.transcribing(jobID: "job-1")
        XCTAssertTrue(transcribing.isActive)
        XCTAssertFalse(transcribing.isCapturing("job-1"))
        apply(transcribing, at: t0 + 2.5)
        XCTAssertEqual(model.stagePhase, .transcribing)

        // Finished carries one KeyboardResult payload and produces a deliverable snapshot.
        let result = KeyboardResult(
            id: "res-1",
            transcript: "Ship it.",
            heard: "ship it",
            stages: nil,
            translationFailure: nil
        )
        let finished = KeyboardLocalCapturePhase.finished(jobID: "job-1", result: result)
        let finSnap = try XCTUnwrap(finished.snapshot(now: t0 + 3.0, loudnessDB: -62))
        XCTAssertEqual(finSnap.state, .ready)
        XCTAssertEqual(finSnap.resultID, "res-1")
        XCTAssertEqual(finSnap.transcript, "Ship it.")
        XCTAssertEqual(finSnap.heard, "ship it")

        // Failure surfaces with heartbeat/expiresAt 0 (not warm) and clears once expired.
        let failed = KeyboardLocalCapturePhase.failed(message: "Mic busy", until: t0 + 10.0)
        apply(failed, at: t0 + 4.0)
        XCTAssertEqual(model.stagePhase, .failed)
        XCTAssertFalse(model.isWarmAvailable)
        XCTAssertEqual(model.statusSlot, .message("Mic busy", isFailure: true))
        XCTAssertNil(failed.snapshot(now: t0 + 10.0, loudnessDB: -62))
        apply(failed, at: t0 + 10.0)
        XCTAssertEqual(model.stagePhase, .idle)
    }

    func testKeyboardCaptureWatchDetectsStartupAndInterruptionStalls() {
        let t0 = 70_000.0
        var watch = KeyboardCaptureWatch(startedAt: t0)

        // Before first audio: stalls after the recording quiet watchdog window.
        XCTAssertFalse(watch.isStalled(now: t0 + KeyboardJobWatchdog.captureSilence))
        XCTAssertTrue(watch.isStalled(now: t0 + KeyboardJobWatchdog.captureSilence + 0.1))

        // Audio refreshes the clock.
        watch.heardAudio(at: t0 + 3.0)
        XCTAssertFalse(watch.isStalled(now: t0 + 3.0 + KeyboardJobWatchdog.captureSilence))

        // Interruption extends the stall window to captureInterruption, and resuming resets lastAudio.
        let interrupted = KeyboardCapturePageState(
            cause: "mute",
            context: "interrupted",
            muted: true,
            ended: false,
            at: t0 + 4.0
        )
        XCTAssertEqual(watch.page(interrupted, at: t0 + 4.0), .interrupted)
        XCTAssertFalse(watch.isStalled(now: t0 + 4.0 + KeyboardJobWatchdog.captureSilence + 1))
        XCTAssertTrue(watch.isStalled(now: t0 + 4.0 + KeyboardJobWatchdog.captureInterruption + 0.1))

        let resumed = KeyboardCapturePageState(
            cause: "unmute",
            context: "running",
            muted: false,
            ended: false,
            at: t0 + 9.0
        )
        XCTAssertEqual(watch.page(resumed, at: t0 + 9.0), .resumed(after: 5.0))
        XCTAssertNil(watch.interruptedSince)
        XCTAssertFalse(watch.isStalled(now: t0 + 9.0 + KeyboardJobWatchdog.captureSilence))
    }

    private func settle() async throws {
        try await Task.sleep(for: .milliseconds(25))
    }
}
