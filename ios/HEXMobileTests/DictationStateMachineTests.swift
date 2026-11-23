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
        XCTAssertEqual(controller.keyboardSessionExpiresAt, 0)
        XCTAssertEqual(controller.keyboardSessionRemaining, 0)
        XCTAssertEqual(controller.keyboardSessionDisplayRemaining, "0:00")

        // 2. Warm session activates with consistent expiry and display remaining.
        controller.selectWarmDuration(.fifteenMinutes)
        try await settle()
        XCTAssertEqual(controller.warmSession, .warm(expiresAt: now + 900, suspendedOnInterruption: false))
        XCTAssertTrue(controller.keyboardSessionEnabled)
        XCTAssertEqual(controller.keyboardSessionExpiresAt, now + 900)
        XCTAssertEqual(controller.keyboardSessionRemaining, 900)
        XCTAssertEqual(controller.keyboardSessionDisplayRemaining, "15m")

        // 3. Audio interruption suspends without dropping expiry; interruption end with expired session tears down cleanly.
        controller.handleAudioSystemEvent(.interruptionBegan)
        XCTAssertEqual(controller.warmSession, .warm(expiresAt: now + 900, suspendedOnInterruption: true))
        now += 905
        controller.handleAudioSystemEvent(.interruptionEnded(shouldResume: true))
        XCTAssertEqual(controller.warmSession, .cold)
        XCTAssertFalse(controller.keyboardSessionEnabled)
        XCTAssertEqual(controller.keyboardSessionExpiresAt, 0)
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
        XCTAssertEqual(controller.keyboardSessionExpiresAt, 0)
        XCTAssertEqual(controller.keyboardSessionRemaining, 0)
        XCTAssertEqual(controller.keyboardSessionDisplayRemaining, "0:00")
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertNil(controller.capture.activeSession)
        XCTAssertEqual(controller.capture.keyboardJob, .none)
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
        XCTAssertEqual(controller.capture.keyboardJob, .none)
        if case .failed = controller.phase {} else { XCTFail("Expected failed phase, got \(controller.phase)") }

        // Microphone failure with model ready IS keyboard-retryable and carries failure timestamp.
        audio.permissionGranted = true
        audio.captureShouldThrow = true
        now += 5
        controller.startRecording(keyboardJobID: "job-mic-fail")
        try await settle()
        XCTAssertTrue(controller.capture.allowsKeyboardRetry)
        XCTAssertEqual(controller.capture.failureMessage?.at, now)
        XCTAssertEqual(controller.capture.keyboardJob, .none)
        XCTAssertNil(controller.capture.activeSession)

        // Unexpected capture failure clears activeSession and marks capture retryable.
        audio.captureShouldThrow = false
        now += 5
        controller.startRecording(keyboardJobID: "job-unexpected")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        let activeURL = try XCTUnwrap(controller.capture.activeSession?.capture.url)
        XCTAssertEqual(controller.capture.keyboardJob, .active(id: "job-unexpected"))

        audio.onUnexpectedCaptureEnd?(activeURL, false)
        XCTAssertNil(controller.capture.activeSession)
        XCTAssertEqual(controller.capture.keyboardJob, .none)
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
        XCTAssertEqual(controller.capture.keyboardJob, .none)
        XCTAssertTrue(controller.capture.allowsKeyboardRetry)
        if case .failed = controller.phase {} else { XCTFail("Expected failed phase") }

        // Model re-preparation clears capture failure so phase reflects preparing -> ready.
        controller.prepareModel()
        XCTAssertEqual(controller.phase, .preparingModel)
        try await settle()
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertFalse(controller.capture.allowsKeyboardRetry)
    }

    // MARK: - KeyboardLocalCapturePhase State Machine

    func testLocalCaptureStartupProgressionRequiresAudioFileBeforeRecording() {
        let t0 = 40_000.0
        var phase: KeyboardLocalCapturePhase = .starting(
            jobID: "job-1",
            requestedAt: t0,
            stage: .awaitingAudioSession
        )

        XCTAssertTrue(phase.isActive)
        XCTAssertTrue(phase.isCapturing("job-1"))
        XCTAssertTrue(phase.isAwaitingAudioSession("job-1"))
        XCTAssertFalse(phase.isAwaitingAudioSession("other"))
        XCTAssertEqual(phase.visibleStage, .starting)

        // Audio arriving before audio session and page start never promotes to recording.
        XCTAssertFalse(phase.receiveAudio(jobID: "job-1", at: t0 + 0.05))
        XCTAssertEqual(phase.visibleStage, .starting)

        // Wrong job ID cannot advance audio session.
        XCTAssertFalse(phase.markAudioSessionReady(jobID: "other"))
        XCTAssertTrue(phase.markAudioSessionReady(jobID: "job-1"))
        XCTAssertFalse(phase.isAwaitingAudioSession("job-1"))
        XCTAssertEqual(phase, .starting(jobID: "job-1", requestedAt: t0, stage: .awaitingPage))

        // Audio arriving before WebKit .started still cannot promote to recording.
        XCTAssertFalse(phase.receiveAudio(jobID: "job-1", at: t0 + 0.10))
        XCTAssertEqual(phase.visibleStage, .starting)

        // Page .started advances to .awaitingFirstAudio; now first PCM buffer promotes to .recording.
        XCTAssertTrue(phase.markPageStarted(jobID: "job-1"))
        XCTAssertEqual(phase, .starting(jobID: "job-1", requestedAt: t0, stage: .awaitingFirstAudio))
        XCTAssertTrue(phase.receiveAudio(jobID: "job-1", at: t0 + 0.25))
        XCTAssertEqual(phase, .recording(jobID: "job-1", startedAt: t0 + 0.25))
        XCTAssertEqual(phase.visibleStage, .recording)
    }

    func testArmedCaptureDeferredFailureSurfacesOnlyWhenKeptAndDropsOnDiscard() {
        let t0 = 50_000.0
        var kept: KeyboardLocalCapturePhase = .armed(
            jobID: "armed-1",
            requestedAt: t0,
            readiness: .starting(.awaitingAudioSession)
        )
        XCTAssertTrue(kept.isCapturing("armed-1"))
        XCTAssertNil(kept.snapshot(now: t0 + 0.05, loudnessDB: -62))

        // Failure while armed stays hidden until the touch is kept.
        XCTAssertTrue(kept.recordFailure(message: "Mic busy", detail: "NotReadableError", now: t0 + 0.1))
        XCTAssertFalse(kept.isCapturing("armed-1"), "failed armed capture accepts no further audio")
        XCTAssertEqual(kept.visibleStage, .idle)
        XCTAssertNil(kept.snapshot(now: t0 + 0.15, loudnessDB: -62))

        // Keeping the armed touch surfaces the deferred failure.
        let outcome = kept.keepArmed(now: t0 + 0.2, failureDuration: 6)
        XCTAssertEqual(outcome, .failed(.init(message: "Mic busy", detail: "NotReadableError")))
        XCTAssertEqual(kept, .failed(message: "Mic busy", until: t0 + 6.2))
        XCTAssertEqual(kept.visibleStage, .failed)
        XCTAssertEqual(kept.snapshot(now: t0 + 0.3, loudnessDB: -62)?.state, .failed)

        // Failure expires cleanly back to idle.
        XCTAssertFalse(kept.expireFailureIfNeeded(at: t0 + 6.1))
        XCTAssertTrue(kept.expireFailureIfNeeded(at: t0 + 6.2))
        XCTAssertEqual(kept, .idle)
        XCTAssertNil(kept.snapshot(now: t0 + 6.2, loudnessDB: -62))
    }

    func testArmedCapturePreservesStartupStageAcrossKeepArmedAndAudioArrival() {
        let t0 = 60_000.0
        var phase: KeyboardLocalCapturePhase = .armed(
            jobID: "armed-2",
            requestedAt: t0,
            readiness: .starting(.awaitingAudioSession)
        )
        XCTAssertTrue(phase.markAudioSessionReady(jobID: "armed-2"))
        // Kept mid-startup (page still opening): preserves .awaitingPage in .starting.
        XCTAssertEqual(phase.keepArmed(now: t0 + 0.1), .starting(.awaitingPage))
        XCTAssertEqual(phase, .starting(jobID: "armed-2", requestedAt: t0, stage: .awaitingPage))
        XCTAssertTrue(phase.markPageStarted(jobID: "armed-2"))
        XCTAssertTrue(phase.receiveAudio(jobID: "armed-2", at: t0 + 0.3))
        XCTAssertEqual(phase, .recording(jobID: "armed-2", startedAt: t0 + 0.3))

        // Finished result carries one KeyboardResult payload and produces a deliverable snapshot.
        let result = KeyboardResult(
            id: "res-1",
            transcript: "Ship it.",
            heard: "ship it",
            stages: nil,
            translationFailure: nil
        )
        phase = .finished(jobID: "armed-2", result: result)
        let snap = phase.snapshot(now: t0 + 1.0, loudnessDB: -62)
        XCTAssertEqual(snap?.state, .ready)
        XCTAssertEqual(snap?.resultID, "res-1")
        XCTAssertEqual(snap?.transcript, "Ship it.")
        XCTAssertEqual(snap?.heard, "ship it")
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
