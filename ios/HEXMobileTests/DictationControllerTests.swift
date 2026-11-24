import AVFAudio
import Speech
import UIKit
import XCTest
@testable import HEX

@MainActor
private final class FakeAudioCapture: ActiveAudioCapture {
    let url: URL
    var isRecording = true
    var currentTime: TimeInterval
    var power: Float = 0.6

    init(url: URL, initialDuration: TimeInterval = 1.2) {
        self.url = url
        self.currentTime = initialDuration
        FileManager.default.createFile(atPath: url.path, contents: Data("wav".utf8))
    }

    func normalizedPowerLevel() -> Float {
        power
    }

    func stop() {
        isRecording = false
    }
}

@MainActor
private final class FakeAudioManager: AudioManaging {
    var isKeepaliveRunning = false
    var permissionGranted = true
    var keepaliveShouldThrow = false
    var keepaliveError: Error = CocoaError(.coderInvalidValue)
    var nextCaptureDuration: TimeInterval = 1.4
    var nextCaptureIsRecording = true
    /// A microphone that doesn't start, as in the simulator: the recorder fails in `record`.
    var captureShouldThrow = false
    var deactivateCount = 0
    var createdCaptures: [FakeAudioCapture] = []
    var keepaliveStarts = 0
    /// The Settings › Microphone choice each capture was asked to record from.
    var captureMicrophones: [MicrophoneChoice] = []
    /// An engine that stops again by itself as soon as it starts.
    var keepaliveStopsRightAway = false

    var onSystemEvent: ((AudioSystemEvent) -> Void)?
    var onUnexpectedCaptureEnd: ((URL, Bool) -> Void)?

    func requestPermission() async -> Bool {
        permissionGranted
    }

    func startKeepalive() throws {
        keepaliveStarts += 1
        if keepaliveShouldThrow {
            isKeepaliveRunning = false
            throw keepaliveError
        }
        isKeepaliveRunning = !keepaliveStopsRightAway
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
        captureMicrophones.append(microphone)
        isKeepaliveRunning = false
        if captureShouldThrow {
            throw CocoaError(.fileWriteUnknown)
        }
        let capture = FakeAudioCapture(url: url, initialDuration: nextCaptureDuration)
        capture.isRecording = nextCaptureIsRecording
        createdCaptures.append(capture)
        return capture
    }
}

/// A preparation that never answers, as Speech or its asset store not answering.
private actor SilentTranscriber: LocalTranscribing {
    nonisolated let engine: DictationEngine = .appleSpeech
    let languageName = "English"
    private(set) var preparationCancelled = false

    func prepare(progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void) async throws {
        do {
            try await Task.sleep(for: .seconds(3_600))
        } catch {
            preparationCancelled = true
            throw error
        }
    }

    func transcribe(_ audioURL: URL) async throws -> String { "" }
}

private actor ControlledTranscriber: LocalTranscribing {
    nonisolated let engine: DictationEngine = .appleSpeech
    nonisolated let displayName = "Controlled Speech"
    let languageName = "English"

    private var gate: CheckedContinuation<String, Error>?
    private var pendingResume: String?
    private var cannedOutput = "ship the build"
    private(set) var transcribedURLs: [URL] = []
    private(set) var vocabularyArguments: [[String]] = []
    private var suspendNext = false
    private var nextError: Error?

    func failNext(with error: Error) {
        nextError = error
    }

    func armSuspension() {
        suspendNext = true
        pendingResume = nil
    }

    func setCannedOutput(_ text: String) {
        cannedOutput = text
    }

    func resumeSuspended(with text: String) {
        if let continuation = gate {
            gate = nil
            continuation.resume(returning: text)
        } else {
            pendingResume = text
        }
    }

    func prepare(progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void) async throws {
        progressHandler(ModelPreparationProgress(fractionCompleted: 1, detail: "Ready"))
    }

    func transcribe(_ audioURL: URL) async throws -> String {
        transcribedURLs.append(audioURL)
        if let error = nextError {
            nextError = nil
            throw error
        }
        if suspendNext {
            suspendNext = false
            if let early = pendingResume {
                pendingResume = nil
                return early
            }
            return try await withCheckedThrowingContinuation { continuation in
                gate = continuation
            }
        }
        return cannedOutput
    }

    func transcribe(_ audioURL: URL, vocabulary: [String]) async throws -> String {
        vocabularyArguments.append(vocabulary)
        return try await transcribe(audioURL)
    }
}

/// A transcriber whose model can't be got ready, such as a language Apple Speech can't install.
private actor UnpreparableTranscriber: LocalTranscribing {
    nonisolated let engine: DictationEngine = .appleSpeech
    let languageName = "Russian"
    let error: Error

    init(error: Error) {
        self.error = error
    }

    func prepare(progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void) async throws {
        throw error
    }

    func transcribe(_ audioURL: URL) async throws -> String {
        throw error
    }
}

@MainActor
private final class RecordingCleaner: TranscriptCleaning {
    var isAvailable = true
    var output: String?
    private(set) var cleanedInputs: [String] = []

    func clean(_ text: String) async -> String? {
        cleanedInputs.append(text)
        return output
    }
}

@MainActor
final class DictationControllerTests: XCTestCase {
    private var suiteName = ""
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "hex.tests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    func testStaleTranscriptionAfterSessionEndCannotClobberNewerCaptureOrDeleteItsFile() async throws {
        let now = 1_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
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
        XCTAssertEqual(controller.phase, .ready)

        // Start Job A and suspend its transcription in-flight.
        await transcriber.armSuspension()
        controller.toggleWarmSession()
        try await settle()
        XCTAssertTrue(controller.keyboardSessionEnabled)

        bridge.send(.start, jobID: "job-a", style: .polished, sentAt: now)
        controller.processPendingKeyboardCommandForTesting()
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        let firstCaptureURL = try XCTUnwrap(audio.createdCaptures.first?.url)
        XCTAssertTrue(FileManager.default.fileExists(atPath: firstCaptureURL.path))

        controller.finishRecording()
        XCTAssertEqual(controller.phase, .transcribing)
        try await settle()

        // Expire / end the warm session while Job A is still transcribing, then start a new capture Job B!
        controller.endWarmSession()
        XCTAssertFalse(controller.keyboardSessionEnabled)

        controller.startRecording(keyboardJobID: "job-b")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        let secondCaptureURL = try XCTUnwrap(audio.createdCaptures.last?.url)
        XCTAssertNotEqual(firstCaptureURL, secondCaptureURL)
        XCTAssertTrue(FileManager.default.fileExists(atPath: secondCaptureURL.path))

        // Complete stale Job A: it must clean up ONLY firstCaptureURL and leave Job B recording untouched!
        await transcriber.resumeSuspended(with: "stale transcript")
        try await settle()

        XCTAssertFalse(FileManager.default.fileExists(atPath: firstCaptureURL.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: secondCaptureURL.path))
        XCTAssertEqual(controller.phase, .recording)
        XCTAssertEqual(controller.transcript, "")

        controller.cancelCurrentJob()
        XCTAssertFalse(FileManager.default.fileExists(atPath: secondCaptureURL.path))
    }

    func testDictationRecordsFromTheMicrophoneChosenInSettings() async throws {
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        let bridge = KeyboardBridge(defaults: defaults)
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { 1_500 },
            autoPrepareOnInit: false
        )
        controller.prepareModel()
        try await settle()
        XCTAssertEqual(controller.microphone, .automatic)

        controller.startRecording(keyboardJobID: "job-a")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        controller.cancelCurrentJob()

        controller.setMicrophone(.bluetooth)
        XCTAssertEqual(bridge.microphone, .bluetooth, "the keyboard's capture page follows it too")
        controller.startRecording(keyboardJobID: "job-b")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        controller.cancelCurrentJob()

        XCTAssertEqual(audio.captureMicrophones, [.automatic, .bluetooth])
    }

    func testAcceptedJobDeliversTranscriptEvenWhenWarmSessionCloses() async throws {
        let now = 2_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.armSuspension()
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

        controller.setWarmDuration(.off)
        controller.handleKeyboardLaunch(
            .startRecording(jobID: "one-shot-job", requestedAt: now, style: .casual)
        )
        try await settle()
        XCTAssertEqual(controller.phase, .recording)

        // Finishing capture in one-shot mode immediately releases the mic while transcription completes.
        controller.finishRecording()
        XCTAssertEqual(controller.phase, .transcribing)
        XCTAssertFalse(controller.keyboardSessionEnabled)
        XCTAssertGreaterThan(audio.deactivateCount, 0)
        try await settle()

        await transcriber.resumeSuspended(with: "Um, ship it tonight.")
        try await settle()

        XCTAssertEqual(controller.phase, .ready)
        XCTAssertEqual(controller.transcript, "ship it tonight")
        let snap = bridge.snapshot()
        XCTAssertTrue(snap.hasDeliverableResult(at: now + 1))
        XCTAssertEqual(snap.transcript, "ship it tonight")
    }

    /// TestFlight build 56's sequence from Hex's side: the keyboard couldn't dictate by itself
    /// and handed the dictation to Hex, you swiped back and tapped Done in the keyboard. Done
    /// reaches Hex in One shot as in a warm session (One shot never read it, so the recording
    /// ran to its limit), Hex heartbeats the job until its text is in, so the keyboard never
    /// takes it for over, and the text stays on offer after its time to go in by itself.
    func testAKeyboardHandoffFinishedFromTheKeyboardDeliversItsTextInEitherMode() async throws {
        for duration in [WarmSessionDuration.off, .fifteenMinutes] {
            var now = 7_000.0
            let audio = FakeAudioManager()
            let transcriber = ControlledTranscriber()
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
            controller.setWarmDuration(duration)
            await transcriber.armSuspension()

            let job = "handoff-\(duration.rawValue)"
            controller.handleKeyboardLaunch(.startRecording(jobID: job, requestedAt: now, style: .polished))
            try await settle()
            XCTAssertEqual(controller.phase, .recording, "\(duration)")
            XCTAssertEqual(bridge.snapshot().jobID, job)

            // Back in the app: only the keyboard's command tells Hex to stop.
            now += 2
            bridge.send(.stop, jobID: job, sentAt: now)
            try await settle()
            XCTAssertEqual(controller.phase, .transcribing, "\(duration): Done reached Hex")

            now += 4
            try await settle()
            let waiting = bridge.snapshot()
            XCTAssertEqual(waiting.state, .transcribing)
            XCTAssertFalse(waiting.isOrphanedJob(at: now), "\(duration): the transcription heartbeats")

            await transcriber.resumeSuspended(with: "ship it tonight")
            try await settle()
            XCTAssertEqual(controller.phase, .ready)
            let done = bridge.snapshot()
            XCTAssertTrue(done.hasDeliverableResult(at: now), "\(duration)")
            XCTAssertEqual(done.transcript, controller.transcript)

            now += 40
            controller.processPendingKeyboardCommandForTesting()
            XCTAssertFalse(bridge.snapshot().hasDeliverableResult(at: now))
            XCTAssertTrue(bridge.snapshot().hasOfferableResult(at: now), "\(duration): offered as Paste dictation")
            now += KeyboardSnapshot.resultOfferWindow
            controller.processPendingKeyboardCommandForTesting()
            XCTAssertNil(bridge.snapshot().transcript, "\(duration): not kept past the offer")
            controller.endWarmSession()
        }
    }

    /// A keyboard's Cancel in One shot ends the handed-off recording, as in a warm session.
    func testAOneShotHandoffCancelledFromTheKeyboardStops() async throws {
        let now = 7_500.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
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
        controller.setWarmDuration(.off)
        controller.handleKeyboardLaunch(.startRecording(jobID: "one-shot", requestedAt: now, style: .polished))
        try await settle()
        XCTAssertEqual(controller.phase, .recording)

        bridge.send(.cancel, jobID: "one-shot", sentAt: now)
        try await settle()
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertNil(bridge.snapshot().jobID)
        XCTAssertNotEqual(bridge.snapshot().state, .recording)
        let transcribed = await transcriber.transcribedURLs
        XCTAssertTrue(transcribed.isEmpty, "nothing transcribed")
    }

    func testTenSecondHandoffWindow() async throws {
        var now = 5_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
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

        // 1. Expired (>10s) handoff warms session without surprise-recording.
        controller.handleKeyboardLaunch(
            .startRecording(jobID: "expired-job", requestedAt: now - 11.5, style: .polished)
        )
        try await settle()
        XCTAssertTrue(controller.keyboardSessionEnabled)
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertNotNil(controller.handoffNotice)

        // 2. A fresh handoff starts recording; the user swipes back to their app themselves.
        now += 1
        controller.handleKeyboardLaunch(
            .startRecording(jobID: "fresh-job", requestedAt: now - 0.4, style: .polished)
        )
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        controller.cancelCurrentJob()
    }

    /// A tester's iPhone locked mid-dictation: recording and transcribing keep the screen on,
    /// and every way out of them hands it back to Auto-Lock.
    func testRecordingAndTranscribingKeepTheScreenOn() async throws {
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        let controller = DictationController(
            audio: audio,
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { 6_000 },
            autoPrepareOnInit: false
        )
        controller.prepareModel()
        try await settle()
        UIApplication.shared.isIdleTimerDisabled = false

        await transcriber.armSuspension()
        controller.startRecording(keyboardJobID: "awake")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        XCTAssertTrue(UIApplication.shared.isIdleTimerDisabled, "recording")
        controller.finishRecording()
        XCTAssertEqual(controller.phase, .transcribing)
        XCTAssertTrue(UIApplication.shared.isIdleTimerDisabled, "transcribing")
        await transcriber.resumeSuspended(with: "done")
        try await settle()
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertFalse(UIApplication.shared.isIdleTimerDisabled, "done")

        controller.startRecording(keyboardJobID: "cancelled")
        try await settle()
        XCTAssertTrue(UIApplication.shared.isIdleTimerDisabled)
        controller.cancelCurrentJob()
        XCTAssertFalse(UIApplication.shared.isIdleTimerDisabled, "cancelled")

        await transcriber.failNext(with: CocoaError(.fileReadCorruptFile))
        controller.startRecording(keyboardJobID: "failed")
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertFalse(UIApplication.shared.isIdleTimerDisabled, "failed")
    }

    func testAudioInterruptionsAndRouteChangesNeverLeaveStuckListeningState() async throws {
        let now = 8_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("saved before call")
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
        controller.toggleWarmSession()
        try await settle()

        // Long capture (>= 0.3s) interrupted by phone call saves and transcribes spoken words!
        audio.nextCaptureDuration = 2.1
        controller.startRecording(keyboardJobID: "call-job")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)

        controller.handleAudioSystemEvent(.interruptionBegan)
        try await settle()
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertEqual(controller.transcript, "Saved before call.")
        XCTAssertNotEqual(bridge.snapshot().state, .recording)

        // Short capture (< 0.3s) interrupted cancels cleanly out of recording.
        audio.nextCaptureDuration = 0.1
        controller.startRecording(keyboardJobID: "short-call-job")
        try await settle()
        controller.handleAudioSystemEvent(.interruptionBegan)
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertNotEqual(bridge.snapshot().state, .recording)
    }

    func testRecordedWAVFixtureIsValidSilentTestInput() throws {
        let bundle = Bundle(for: Self.self)
        let wavURL = try XCTUnwrap(
            bundle.url(forResource: "term-hex-1", withExtension: "wav"),
            "Expected recorded WAV fixture in test bundle"
        )
        let audioFile = try AVAudioFile(forReading: wavURL)
        XCTAssertEqual(audioFile.fileFormat.sampleRate, 16_000, accuracy: 1)
        XCTAssertGreaterThan(audioFile.length, 4_000)
    }

    func testSelectWarmDurationWarmsAndEndsWithoutSeparateButton() async throws {
        let now = 12_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
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

        controller.setWarmDuration(.off)
        XCTAssertFalse(controller.keyboardSessionEnabled)

        controller.selectWarmDuration(.fifteenMinutes)
        try await settle()
        XCTAssertTrue(controller.keyboardSessionEnabled)
        XCTAssertTrue(audio.isKeepaliveRunning)
        XCTAssertEqual(controller.warmDuration, .fifteenMinutes)

        controller.selectWarmDuration(.off)
        XCTAssertFalse(controller.keyboardSessionEnabled)
        XCTAssertFalse(audio.isKeepaliveRunning)
    }

    /// Where the keyboard dictates by itself Keep mic warm reads Off and hides. TestFlight build
    /// 129: a keyboard that opened Hex anyway, for a reason it left no record of, got a one
    /// shot every time and no warm session, so every tap opened Hex. Any dictation the keyboard
    /// hands to Hex now brings back the 15-minute default and its row from that moment (a late
    /// one warms as before 129), and a dictation the keyboard finishes by itself takes them
    /// away. (iOS 18 to 25: `KeyboardBridgeTests`.)
    func testKeepMicWarmFollowsWhetherTheKeyboardDictatesItself() async throws {
        try XCTSkipUnless(ProcessInfo.processInfo.operatingSystemVersion.majorVersion >= 26)
        let now = 9_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        let controller = DictationController(
            audio: audio,
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false
        )
        controller.prepareModel()
        try await settle()
        XCTAssertEqual(controller.warmDuration, .off)
        XCTAssertFalse(controller.showsKeepMicWarm)

        let record = KeyboardSelfDictation(defaults: defaults)
        XCTAssertNil(record.handedToHex)
        controller.handleKeyboardLaunch(.startRecording(jobID: "late", requestedAt: now - 30, style: .polished))
        try await settle()
        XCTAssertEqual(controller.handoffNotice, String(localized: "Timed out"))
        XCTAssertEqual(record.handedToHex?.reason, .openedHex, "no record from the keyboard: Hex writes one")
        XCTAssertEqual(controller.warmDuration, .fifteenMinutes)
        XCTAssertTrue(controller.showsKeepMicWarm)
        XCTAssertTrue(controller.keyboardSessionEnabled, "a late handoff warms, as before 129")
        XCTAssertTrue(audio.isKeepaliveRunning)

        record.recordHandedToHex(.memory)
        controller.handleKeyboardLaunch(.startSession(requestedAt: now))
        try await settle()
        XCTAssertEqual(record.handedToHex?.reason, .memory, "the keyboard's reason stays")

        KeyboardSelfDictation(defaults: defaults).clearHandedToHex()
        controller.refreshWarmDuration()
        XCTAssertEqual(controller.warmDuration, .off)
        XCTAssertFalse(controller.keyboardSessionEnabled, "an idle warm session ends")
        XCTAssertFalse(controller.showsKeepMicWarm)
    }

    func testMeterRecoversAfterCaptureStartsTemporarilyNotRecording() async throws {
        try await checkMeterRecovery(initiallyRecording: false)
    }

    func testMeterRecoversAfterRecordingTemporarilyStops() async throws {
        try await checkMeterRecovery(initiallyRecording: true)
    }

    private func checkMeterRecovery(initiallyRecording: Bool) async throws {
        var now = 20_000.0
        let audio = FakeAudioManager()
        audio.nextCaptureIsRecording = initiallyRecording
        let transcriber = ControlledTranscriber()
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
        controller.startRecording(keyboardJobID: "meter-recovery")
        try await settle()
        let capture = try XCTUnwrap(audio.createdCaptures.last)
        defer { controller.cancelCurrentJob() }

        // No completion/interruption callback: the same capture becomes available again.
        capture.isRecording = false
        try await settle()
        capture.power = 0.9
        capture.currentTime = 2.4
        capture.isRecording = true
        now += 1
        try await settle()

        let db = capture.averagePowerDB()
        XCTAssertEqual(controller.phase, .recording)
        XCTAssertEqual(controller.loudnessDB, db, accuracy: 0.001)
        XCTAssertEqual(controller.audioLevel, (db + 48) / 48, accuracy: 0.001)
        XCTAssertEqual(controller.recordingDuration, 2.4)
        XCTAssertEqual(bridge.snapshot().state, .recording)
        XCTAssertEqual(bridge.snapshot().effectiveLoudnessDB, db, accuracy: 0.001)
        XCTAssertEqual(bridge.snapshot().heartbeat, now)

        // A level-only change must still get through the keyboard publication throttle.
        capture.power = 0.2
        now += 1
        try await settle()
        XCTAssertEqual(controller.loudnessDB, capture.averagePowerDB(), accuracy: 0.001)
        XCTAssertEqual(bridge.snapshot().effectiveLoudnessDB, capture.averagePowerDB(), accuracy: 0.001)

        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.transcript, "Ship the build.")
        XCTAssertEqual(controller.audioLevel, 0)
        XCTAssertEqual(bridge.snapshot().audioLevel, 0)
    }

    func testCleanupRunsBeforeFormattingOnlyWhenCapturedAndSkipsVerbatim() async throws {
        let now = 40_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("we should use cloud code for this")
        let bridge = KeyboardBridge(defaults: defaults)
        let settings = AppleIntelligenceCleanupSettings(defaults: defaults)
        let cleaner = RecordingCleaner()
        cleaner.output = "We should use Claude Code for this."
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false,
            cleaner: cleaner,
            cleanupSettings: settings
        )
        controller.prepareModel()
        try await settle()

        // Off: the transcript is formatted exactly as before.
        settings.isEnabled = false
        controller.startRecording(keyboardJobID: "cleanup-off")
        try await settle()
        settings.isEnabled = true // A mid-capture toggle must not affect this session.
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(cleaner.cleanedInputs, [])
        XCTAssertEqual(controller.transcript, "We should use cloud code for this.")

        // On: the setting is captured at start, and cleanup runs before formatting.
        controller.clearTranscript()
        settings.isEnabled = true
        controller.startRecording(keyboardJobID: "cleanup-on")
        try await settle()
        settings.isEnabled = false
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(cleaner.cleanedInputs, ["we should use cloud code for this"])
        XCTAssertEqual(controller.transcript, "We should use Claude Code for this.")

        // Verbatim skips cleanup even with the experiment on.
        controller.clearTranscript()
        settings.isEnabled = true
        controller.startRecording(keyboardJobID: "cleanup-verbatim", styleOverride: .verbatim)
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(cleaner.cleanedInputs.count, 1)
        XCTAssertEqual(controller.transcript, "we should use cloud code for this")

        // A cleaner that declines leaves the raw transcript for the formatter.
        controller.clearTranscript()
        cleaner.output = nil
        settings.isEnabled = true
        controller.startRecording(keyboardJobID: "cleanup-fallback")
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(cleaner.cleanedInputs.count, 2)
        XCTAssertEqual(controller.transcript, "We should use cloud code for this.")

        // Empty transcripts skip cleanup too.
        controller.clearTranscript()
        await transcriber.setCannedOutput("   ")
        settings.isEnabled = true
        controller.startRecording(keyboardJobID: "cleanup-empty")
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(cleaner.cleanedInputs.count, 2)
        XCTAssertEqual(controller.transcript, "")
    }

    func testCustomVocabularyAppliesOnlyWhenCaptured() async throws {
        let now = 50_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("i use open code")
        let bridge = KeyboardBridge(defaults: defaults)
        let settings = CustomVocabularySettings(defaults: defaults)
        settings.terms = ["OpenCode"]
        let cleanupSettings = AppleIntelligenceCleanupSettings(defaults: defaults)
        let cleaner = RecordingCleaner()
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false,
            cleaner: cleaner,
            cleanupSettings: cleanupSettings,
            customVocabularySettings: settings
        )
        controller.prepareModel()
        try await settle()

        // Off: the transcript is formatted exactly as before.
        settings.isEnabled = false
        controller.startRecording(keyboardJobID: "vocabulary-off", styleOverride: .verbatim)
        try await settle()
        settings.isEnabled = true // A mid-capture toggle must not affect this session.
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.transcript, "i use open code")

        // On: the setting is captured at start, before formatting.
        controller.clearTranscript()
        settings.isEnabled = true
        controller.startRecording(keyboardJobID: "vocabulary-on", styleOverride: .verbatim)
        try await settle()
        settings.isEnabled = false
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.transcript, "i use OpenCode")

        // Cleanup receives the spelled transcript, then formatting runs.
        controller.clearTranscript()
        settings.isEnabled = true
        cleanupSettings.isEnabled = true
        controller.startRecording(keyboardJobID: "vocabulary-cleanup", styleOverride: .polished)
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(cleaner.cleanedInputs, ["i use OpenCode"])
        XCTAssertEqual(controller.transcript, "I use OpenCode.")
    }

    /// In-keyboard dictation runs `DictationPipeline` as the app does (but Apple
    /// Intelligence's cleanup), so every style and Lowercase come out the same on both paths,
    /// with Clean up what I say off and on.
    func testInKeyboardDictationWritesEachStyleAsTheAppDoes() async throws {
        let tagger = CleanupTaggerSettings()
        let wasEnabled = tagger.isEnabled
        defer { tagger.isEnabled = wasEnabled }
        let heard = "Um, buy milk. Call the dentist about the iphone, no wait, the ipad."
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput(heard)
        let vocabulary = CustomVocabularySettings(defaults: defaults)
        vocabulary.terms = ["iPhone", "iPad"]
        vocabulary.isEnabled = true
        let lowercase = DictationLowercaseSettings(defaults: defaults)
        let controller = DictationController(
            audio: audio,
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { 97_000 },
            autoPrepareOnInit: false,
            cleanupSettings: AppleIntelligenceCleanupSettings(defaults: defaults),
            customVocabularySettings: vocabulary,
            lowercaseSettings: lowercase,
            typingJournalSettings: TypingJournalSettings(defaults: defaults)
        )
        controller.prepareModel()
        try await settle()

        var outputs: [String] = []
        for (cleanup, lower) in [(false, false), (true, false), (false, true)] {
            tagger.isEnabled = cleanup
            lowercase.isEnabled = lower
            for style in DictationStyle.allCases {
                controller.clearTranscript()
                controller.startRecording(keyboardJobID: "parity-\(style)-\(cleanup)-\(lower)", styleOverride: style)
                try await settle()
                controller.finishRecording()
                try await settle()
                let keyboard = await DictationPipeline(
                    style: style,
                    language: "en-US",
                    vocabulary: vocabulary.activeTerms,
                    lowercase: lower
                ).run(heard).formatted
                XCTAssertEqual(keyboard, controller.transcript, "\(style), cleanup \(cleanup), lowercase \(lower)")
                outputs.append(keyboard)
            }
        }
        XCTAssertEqual(outputs.prefix(4), [
            "Buy milk. Call the dentist about the iPhone, no wait, the iPad.",
            "buy milk. call the dentist about the iPhone, no wait, the iPad",
            "- Buy milk\n- Call the dentist about the iPhone, no wait, the iPad",
            "Um, buy milk. Call the dentist about the iPhone, no wait, the iPad.",
        ])
        XCTAssertEqual(outputs.suffix(4), [
            "buy milk. call the dentist about the iphone, no wait, the ipad.",
            "buy milk. call the dentist about the iphone, no wait, the ipad",
            "- buy milk\n- call the dentist about the iphone, no wait, the ipad",
            "um, buy milk. call the dentist about the iphone, no wait, the ipad.",
        ])
        for start in stride(from: 0, to: outputs.count, by: 4) {
            XCTAssertEqual(Set(outputs[start..<start + 4]).count, 4, "every style differs")
        }
    }

    /// A microphone test reads the same from the app, where Parakeet writes "Test 1, 2, 3", and
    /// from in-keyboard dictation, where Apple's SpeechTranscriber writes "Test one, 2, 3." and
    /// DictationTranscriber "Test 123", in every style and with Lowercase, with Clean up what I
    /// say and Format lists on (TestFlight, Nov 20: "Test three." in the keyboard, "test 123"
    /// in the app).
    func testACountReadsTheSameFromTheAppAndTheKeyboard() async throws {
        let tagger = CleanupTaggerSettings()
        let wasEnabled = tagger.isEnabled
        defer { tagger.isEnabled = wasEnabled }
        tagger.isEnabled = true
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("Test 1, 2, 3")
        let lowercase = DictationLowercaseSettings(defaults: defaults)
        let controller = DictationController(
            audio: audio,
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { 97_500 },
            autoPrepareOnInit: false,
            cleanupSettings: AppleIntelligenceCleanupSettings(defaults: defaults),
            customVocabularySettings: CustomVocabularySettings(defaults: defaults),
            lowercaseSettings: lowercase,
            listSettings: DictationListSettings(defaults: defaults),
            periodSettings: DictationPeriodSettings(defaults: defaults),
            typingJournalSettings: TypingJournalSettings(defaults: defaults)
        )
        controller.prepareModel()
        try await settle()

        var outputs: [String] = []
        for lower in [false, true] {
            lowercase.isEnabled = lower
            for style in [DictationStyle.polished, .casual, .notes] {
                controller.clearTranscript()
                controller.startRecording(keyboardJobID: "count-\(style)-\(lower)", styleOverride: style)
                try await settle()
                controller.finishRecording()
                try await settle()
                for heard in ["Test one, 2, 3.", "Test 123"] {
                    let keyboard = await DictationPipeline(
                        style: style, language: "en-US", lowercase: lower, formatLists: true
                    ).run(heard).formatted
                    XCTAssertEqual(keyboard, controller.transcript, "\(heard), \(style), lowercase \(lower)")
                }
                outputs.append(controller.transcript)
            }
        }
        XCTAssertEqual(outputs, ["Test 1, 2, 3.", "test 1, 2, 3", "Test 1, 2, 3", "test 1, 2, 3.", "test 1, 2, 3", "test 1, 2, 3"])
    }

    func testLowercaseAppliesOnlyWhenCaptured() async throws {
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("Ship the iPhone build to Ravi.")
        let lowercase = DictationLowercaseSettings(defaults: defaults)
        let controller = DictationController(
            audio: audio,
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { 98_000 },
            autoPrepareOnInit: false,
            cleanupSettings: AppleIntelligenceCleanupSettings(defaults: defaults),
            customVocabularySettings: CustomVocabularySettings(defaults: defaults),
            lowercaseSettings: lowercase
        )
        controller.prepareModel()
        try await settle()

        // The Action Button's and Control Center's dictation (it copies) too.
        lowercase.isEnabled = true
        controller.startRecording(styleOverride: .polished, copyToPasteboard: true)
        try await settle()
        lowercase.isEnabled = false // A mid-capture toggle must not affect this session.
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.transcript, "ship the iphone build to ravi.")

        controller.clearTranscript()
        controller.startRecording(keyboardJobID: "lowercase-off", styleOverride: .polished)
        try await settle()
        lowercase.isEnabled = true
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.transcript, "Ship the iPhone build to Ravi.")
    }

    func testCustomVocabularyReachesTheTranscriberOnlyWhileEnabled() async throws {
        let now = 60_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("i use open code")
        let bridge = KeyboardBridge(defaults: defaults)
        let settings = CustomVocabularySettings(defaults: defaults)
        settings.terms = ["OpenCode"]
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false,
            customVocabularySettings: settings
        )
        controller.prepareModel()
        try await settle()

        settings.isEnabled = false
        controller.startRecording(keyboardJobID: "vocabulary-hint-off")
        try await settle()
        controller.finishRecording()
        try await settle()
        var arguments = await transcriber.vocabularyArguments
        XCTAssertEqual(arguments, [[]])

        settings.isEnabled = true
        controller.startRecording(keyboardJobID: "vocabulary-hint-on")
        try await settle()
        controller.finishRecording()
        try await settle()
        arguments = await transcriber.vocabularyArguments
        XCTAssertEqual(arguments, [[], ["OpenCode"]])
    }

    func testCustomVocabularySurvivesCasualFormatting() async throws {
        let now = 70_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("iphone 17 broke")
        let bridge = KeyboardBridge(defaults: defaults)
        let settings = CustomVocabularySettings(defaults: defaults)
        settings.terms = ["iPhone 17"]
        settings.isEnabled = true
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false,
            customVocabularySettings: settings
        )
        controller.prepareModel()
        try await settle()

        // Casual lowercases the whole sentence, so the second spelling pass has to
        // restore the listed term.
        controller.startRecording(keyboardJobID: "vocabulary-casual", styleOverride: .casual)
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.transcript, "iPhone 17 broke")
    }

    func testCustomVocabularySurvivesPolishedFormatting() async throws {
        let now = 80_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("iphone 17 broke")
        let bridge = KeyboardBridge(defaults: defaults)
        let settings = CustomVocabularySettings(defaults: defaults)
        settings.terms = ["iPhone 17"]
        settings.isEnabled = true
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false,
            customVocabularySettings: settings
        )
        controller.prepareModel()
        try await settle()

        // Polished capitalizes the sentence start ("IPhone"), so the second pass
        // has to restore the listed term.
        controller.startRecording(keyboardJobID: "vocabulary-polished", styleOverride: .polished)
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.transcript, "iPhone 17 broke.")
    }

    func testLearnedDictationFixesApplyHintAndReachTheKeyboard() async throws {
        let now = 90_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("ask cloud about open code")
        let bridge = KeyboardBridge(defaults: defaults)
        let journalSettings = TypingJournalSettings(defaults: defaults)
        journalSettings.isEnabled = true
        let learned = [
            DictationCorrection(heard: "cloud", meant: "Claude"),
            DictationCorrection(heard: "open code", meant: "OpenCode"),
        ]
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false,
            customVocabularySettings: CustomVocabularySettings(defaults: defaults),
            typingJournalSettings: journalSettings,
            learnedDictationCorrections: { _ in learned }
        )
        controller.prepareModel()
        try await settle()

        controller.startRecording(keyboardJobID: "learned-on", styleOverride: .polished)
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.transcript, "Ask Claude about OpenCode.")
        let arguments = await transcriber.vocabularyArguments
        XCTAssertEqual(arguments, [["Claude", "OpenCode"]], "meant words go to Apple Speech as hints")
        XCTAssertEqual(bridge.snapshot().transcript, "Ask Claude about OpenCode.")
        XCTAssertEqual(bridge.snapshot().heard, "ask cloud about open code", "the keyboard learns from the raw words")

        // With Learn how I type off, the keyboard gets no raw transcript.
        journalSettings.isEnabled = false
        controller.clearTranscript()
        controller.startRecording(keyboardJobID: "learned-off", styleOverride: .polished)
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertNil(bridge.snapshot().heard)
    }

    /// Clean up what I say reads its switch from the App Group at the call site, so this
    /// sets the real switch and puts it back.
    func testCleanupTaggerRunsAfterLearnedFixesAndSkipsVerbatim() async throws {
        let tagger = CleanupTaggerSettings()
        let wasEnabled = tagger.isEnabled
        defer { tagger.isEnabled = wasEnabled }
        CleanupTagger.shared.unload()
        XCTAssertFalse(CleanupTagger.shared.isLoaded)

        let now = 95_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        await transcriber.setCannedOutput("Let's ship it Tuesday, no wait, Thursday.")
        let controller = DictationController(
            audio: audio,
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false,
            cleanupSettings: AppleIntelligenceCleanupSettings(defaults: defaults),
            customVocabularySettings: CustomVocabularySettings(defaults: defaults),
            typingJournalSettings: TypingJournalSettings(defaults: defaults),
            learnedDictationCorrections: { _ in [DictationCorrection(heard: "ship", meant: "send")] }
        )
        controller.prepareModel()
        try await settle()

        // Off first: recording start must not load the model.
        for (style, enabled, spoken) in [
            (DictationStyle.polished, false, "Let's send it Tuesday, no wait, Thursday."),
            (.polished, true, "Let's send it Thursday."),
            (.verbatim, true, "Let's send it Tuesday, no wait, Thursday."),
        ] {
            tagger.isEnabled = enabled
            controller.clearTranscript()
            controller.startRecording(keyboardJobID: "tagger-\(style)-\(enabled)", styleOverride: style)
            if enabled, style == .polished {
                XCTAssertTrue(CleanupTagger.shared.isLoaded, "recording start loads the model")
            }
            try await settle()
            controller.finishRecording()
            try await settle()
            XCTAssertEqual(
                controller.transcript,
                TranscriptFormatter.format(spoken, style: style),
                "\(style), experiment \(enabled ? "on" : "off")"
            )
            if !enabled {
                XCTAssertFalse(CleanupTagger.shared.isLoaded, "nothing loads while the experiment is off")
            }
        }
    }

    func testDictationLanguageChangeWaitsForTheDictationInFlight() async throws {
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        let languages = LanguageSettings(defaults: defaults)
        var preparations = 0
        let controller = DictationController(
            audio: audio,
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: {
                preparations += 1
                return transcriber
            },
            autoPrepareOnInit: false,
            languageSettings: languages
        )
        controller.prepareModel()
        try await settle()
        XCTAssertEqual(preparations, 1)
        XCTAssertNil(controller.dictationLanguage, "Automatic by default")

        controller.setDictationLanguage("nb-NO")
        try await settle()
        XCTAssertEqual(languages.dictationLanguage, "nb-NO", "saved in the App Group for the keyboard")
        XCTAssertEqual(preparations, 2, "an idle change prepares the new language at once")
        XCTAssertEqual(controller.phase, .ready)

        controller.startRecording(keyboardJobID: "language-job")
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        controller.setDictationLanguage("de-DE")
        try await settle()
        XCTAssertEqual(preparations, 2, "never while recording")
        XCTAssertEqual(controller.phase, .recording)

        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.transcript, "Ship the build.")
        XCTAssertEqual(preparations, 3, "prepared once the dictation is done")
        XCTAssertEqual(controller.phase, .ready)

        controller.setDictationLanguage(nil)
        try await settle()
        XCTAssertNil(languages.dictationLanguage)
        XCTAssertEqual(preparations, 4)

        // The keyboard's switch writes the App Group; Hex follows without writing it back.
        languages.dictationLanguage = "he-IL"
        controller.followDictationLanguageSetting()
        try await settle()
        XCTAssertEqual(controller.dictationLanguage, "he-IL")
        XCTAssertEqual(preparations, 5)
        controller.followDictationLanguageSetting()
        try await settle()
        XCTAssertEqual(preparations, 5, "no change, no preparation")
        XCTAssertEqual(languages.recentDictationLanguages, ["de-DE", "nb-NO"], "picks in Hex are recents")
    }

    /// With English and Spanish on the keyboard and dictation on Automatic, a switch on the
    /// space bar prepares the other language before the next dictation (TestFlight build 56:
    /// Spanish speech transcribed as English was "incomprehensible"); a language picked by
    /// hand stays put.
    func testAutomaticDictationFollowsTheKeyboardsLanguage() async throws {
        let transcriber = ControlledTranscriber()
        let languages = LanguageSettings(defaults: defaults)
        languages.supportedDictationLanguages = ["en-US", "es-ES"]
        languages.automaticDictationLanguage = "en-US"
        var preparations = 0
        let controller = DictationController(
            audio: FakeAudioManager(),
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: {
                preparations += 1
                return transcriber
            },
            autoPrepareOnInit: false,
            languageSettings: languages
        )
        controller.prepareModel()
        try await settle()
        XCTAssertEqual(preparations, 1)

        languages.keyboardLanguages = [.english, .spanish]
        controller.followDictationLanguageSetting()
        try await settle()
        XCTAssertEqual(preparations, 2, "now following the keyboard's English")
        languages.currentKeyboardLanguage = .spanish
        controller.followDictationLanguageSetting()
        try await settle()
        XCTAssertEqual(preparations, 3, "switched to Español on the space bar")
        XCTAssertEqual(languages.effectiveDictationLanguage, "es-ES")
        XCTAssertNil(controller.dictationLanguage, "still Automatic")
        controller.followDictationLanguageSetting()
        try await settle()
        XCTAssertEqual(preparations, 3, "no change, no preparation")
        languages.currentKeyboardLanguage = .english
        controller.followDictationLanguageSetting()
        try await settle()
        XCTAssertEqual(preparations, 4, "and back to English")

        controller.setDictationLanguage("es-MX")
        try await settle()
        XCTAssertEqual(preparations, 5)
        languages.currentKeyboardLanguage = .spanish
        controller.followDictationLanguageSetting()
        try await settle()
        XCTAssertEqual(preparations, 5, "a language picked by hand doesn't follow")
        XCTAssertEqual(languages.effectiveDictationLanguage, "es-MX")
    }

    /// Parakeet V2 knows only English: Spanish goes to Apple Speech whatever happens, even when
    /// Apple Speech doesn't answer, rather than Parakeet hearing it as English words.
    func testSpanishNeverGoesToEnglishOnlyParakeet() throws {
        guard #available(iOS 26.0, *) else { throw XCTSkip("Apple Speech arrived in iOS 26") }
        func language(_ identifier: String, _ module: SpeechModuleKind) -> SpeechLanguage {
            SpeechLanguage(locale: Locale(identifier: identifier), module: module)
        }
        typealias Controller = DictationController
        XCTAssertEqual(Controller.appleSpeechLanguage(picked: language("es-ES", .speech), wanted: "es-ES", parakeetFits: true)?.identifier, "es-ES")
        XCTAssertEqual(Controller.appleSpeechLanguage(picked: language("es-MX", .dictation), wanted: "es-MX", parakeetFits: true)?.identifier, "es-MX")
        XCTAssertEqual(Controller.appleSpeechLanguage(picked: language("en-US", .speech), wanted: nil, parakeetFits: true)?.identifier, "en-US")
        XCTAssertNil(Controller.appleSpeechLanguage(picked: language("en-US", .dictation), wanted: nil, parakeetFits: true), "English on DictationTranscriber: Parakeet, as before")
        // Speech not answering: English may go to Parakeet, Spanish never.
        XCTAssertNil(Controller.appleSpeechLanguage(picked: nil, wanted: "en-US", parakeetFits: true))
        XCTAssertNil(Controller.appleSpeechLanguage(picked: nil, wanted: nil, parakeetFits: true))
        XCTAssertEqual(Controller.appleSpeechLanguage(picked: nil, wanted: "es-ES", parakeetFits: true)?.identifier, "es-ES")
        XCTAssertEqual(Controller.appleSpeechLanguage(picked: nil, wanted: "es-US", parakeetFits: true)?.identifier, "es-US")
    }

    func testErrorsShowShortMessagesNeverCodesOrTypeNames() {
        // 560557684 is '!int', AVAudioSession's cannotInterruptOthers.
        let cannotInterrupt = NSError(domain: NSOSStatusErrorDomain, code: 560_557_684)
        XCTAssertEqual(
            UserFacingError.message(for: cannotInterrupt, context: .microphone),
            "Another app is using audio. Try again."
        )
        XCTAssertEqual(UserFacingError.audioSessionCode(of: cannotInterrupt), .cannotInterruptOthers)
        XCTAssertEqual(
            UserFacingError.message(for: NSError(domain: NSOSStatusErrorDomain, code: 561_017_449), context: .microphone),
            "Another app is using the microphone. Try again."
        )

        // Unknown codes and non-audio errors fall back to the context's sentence.
        XCTAssertEqual(
            UserFacingError.message(for: NSError(domain: NSOSStatusErrorDomain, code: -10_875), context: .microphone),
            UserFacingError.Context.microphone.fallback
        )
        XCTAssertEqual(
            UserFacingError.message(for: CocoaError(.coderInvalidValue), context: .microphone),
            UserFacingError.Context.microphone.fallback
        )
        enum PlainFailure: Error { case broke }
        let plain = UserFacingError.message(for: PlainFailure.broke, context: .transcription)
        XCTAssertEqual(plain, UserFacingError.Context.transcription.fallback)

        // A described error keeps its sentence.
        XCTAssertEqual(
            UserFacingError.message(for: ParakeetTranscriber.TranscriberError.notPrepared, context: .transcription),
            "The speech model isn't ready."
        )
        for message in [plain, UserFacingError.message(for: cannotInterrupt, context: .transcription)] {
            XCTAssertFalse(message.contains("OSStatus"))
            XCTAssertFalse(message.contains("PlainFailure"))
        }

        // Apple's own sentences are for logs: a TestFlight keyboard showed this one (iOS 26.2).
        let asset = NSError(
            domain: "SFSpeechErrorDomain",
            code: 1,
            userInfo: [NSLocalizedDescriptionKey: "transcription.ru asset not found after attempted download."]
        )
        XCTAssertEqual(UserFacingError.message(for: asset, context: .transcription), UserFacingError.Context.transcription.fallback)
        XCTAssertEqual(UserFacingError.message(for: asset, context: .model), UserFacingError.Context.model.fallback)
        let offline = NSError(
            domain: "SFSpeechErrorDomain",
            code: 1,
            userInfo: [NSUnderlyingErrorKey: URLError(.notConnectedToInternet) as NSError]
        )
        XCTAssertEqual(UserFacingError.message(for: offline, context: .model), UserFacingError.offline)
        XCTAssertEqual(
            UserFacingError.message(for: ParakeetTranscriber.TranscriberError.downloadFailed, context: .model),
            "The speech model didn't download. Check your connection and try again."
        )

        // Hex's own language messages, short enough for the keyboard's strip.
        let messages = [
            SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US"),
            SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US", automatic: false),
            SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone),
            SpeechModelUnavailable(language: "ru-RU", reason: .downloadFailed),
        ].map { UserFacingError.message(for: $0, context: .transcription) }
        XCTAssertEqual(messages, [
            "Russian isn't available on this iPhone yet. Using English.",
            "Russian isn't available on this iPhone yet. Try English.",
            "Russian isn't available for dictation on this iPhone yet.",
            "Russian speech didn't download. Check your connection.",
        ])
        XCTAssertTrue(messages.allSatisfy { $0.count <= 60 }, "fits the full keyboard's strip")
    }

    func testAutomaticMovesOnFromALanguageThisIPhoneCantDictate() async throws {
        let languages = LanguageSettings(defaults: defaults)
        let english = ControlledTranscriber()
        let russian = UnpreparableTranscriber(
            error: SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US")
        )
        var made = 0
        let controller = DictationController(
            audio: FakeAudioManager(),
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: english,
            transcriberFactory: {
                made += 1
                return made == 1 ? russian : english
            },
            autoPrepareOnInit: false,
            languageSettings: languages
        )
        controller.prepareModel()
        try await settle()
        XCTAssertEqual(made, 2, "prepared again in the language Automatic picks now")
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertEqual(controller.languageNotice, "Russian isn't available on this iPhone yet. Using English.")
        XCTAssertNil(controller.languageOffer)
    }

    func testAnExplicitLanguageThisIPhoneCantDictateOffersEnglish() async throws {
        let languages = LanguageSettings(defaults: defaults)
        let english = ControlledTranscriber()
        let russian = UnpreparableTranscriber(
            error: SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US", automatic: false)
        )
        var made = 0
        let bridge = KeyboardBridge(defaults: defaults)
        let controller = DictationController(
            audio: FakeAudioManager(),
            keyboardBridge: bridge,
            transcriber: english,
            transcriberFactory: {
                made += 1
                return made == 1 ? russian : english
            },
            autoPrepareOnInit: false,
            languageSettings: languages
        )
        controller.prepareModel()
        try await settle()
        XCTAssertEqual(made, 1, "never switched behind a choice made by hand")
        XCTAssertEqual(controller.phase, .failed("Russian isn't available on this iPhone yet. Try English."))
        XCTAssertEqual(bridge.snapshot().message, "Russian isn't available on this iPhone yet. Try English.")
        XCTAssertEqual(controller.languageOffer, "en-US")

        controller.useOfferedLanguage()
        try await settle()
        XCTAssertEqual(languages.dictationLanguage, "en-US")
        XCTAssertEqual(made, 2)
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertNil(controller.languageOffer)
        XCTAssertNil(controller.languageNotice)
    }

    func testIPhonesWithoutRoomForParakeetDictateEnglishWithApple() throws {
        guard #available(iOS 26.0, *) else { throw XCTSkip("Apple Speech needs iOS 26") }
        let dictation = SpeechLanguage(locale: Locale(identifier: "en-US"), module: .dictation)
        XCTAssertFalse(
            AppleSpeechTranscriber.shouldUse(dictation, parakeetFits: true, hasSpeechTranscriber: false, appleSetAside: false),
            "Parakeet where it fits on an iPhone without SpeechTranscriber, as before"
        )
        XCTAssertTrue(AppleSpeechTranscriber.shouldUse(dictation, parakeetFits: false, hasSpeechTranscriber: false), "an iPhone 11 has 4 GB")
        let speech = SpeechLanguage(locale: Locale(identifier: "en-US"), module: .speech)
        XCTAssertTrue(AppleSpeechTranscriber.shouldUse(speech, parakeetFits: true, hasSpeechTranscriber: true))
    }

    /// TestFlight build 126 (iPhone 15 Pro Max, 8 GB, iOS 26.6): one failed install of
    /// SpeechTranscriber's English set it aside, English went to DictationTranscriber, and on a
    /// 6 GB+ iPhone that lost to Parakeet V2, which needs a download: "Set up", then a download
    /// that stalled, until a reinstall cleared the mark. English stays on Apple Speech now.
    func testASetAsideSpeechTranscriberNeverSendsEnglishToADownload() throws {
        guard #available(iOS 26.0, *) else { throw XCTSkip("Apple Speech needs iOS 26") }
        let dictation = SpeechLanguage(locale: Locale(identifier: "en-US"), module: .dictation)
        XCTAssertTrue(AppleSpeechTranscriber.shouldUse(dictation, parakeetFits: true, hasSpeechTranscriber: true, appleSetAside: false))
        XCTAssertEqual(
            DictationController.appleSpeechLanguage(
                picked: dictation, wanted: "en-US", parakeetFits: true, hasSpeechTranscriber: true, appleSetAside: false
            ),
            dictation
        )
        XCTAssertFalse(
            AppleSpeechTranscriber.shouldUse(dictation, parakeetFits: true, hasSpeechTranscriber: true, appleSetAside: true),
            "neither Apple module's English installed, twice in a row: Parakeet is the way left"
        )
        // Speech not answering at launch: English stays on Apple Speech where it has SpeechTranscriber.
        XCTAssertEqual(
            DictationController.appleSpeechLanguage(
                picked: nil, wanted: "en-US", parakeetFits: true, hasSpeechTranscriber: true, appleSetAside: false
            ),
            SpeechLanguage(locale: Locale(identifier: "en-US"), module: .speech)
        )
        XCTAssertNil(
            DictationController.appleSpeechLanguage(picked: nil, wanted: "en-US", parakeetFits: true, hasSpeechTranscriber: false),
            "an iPhone without SpeechTranscriber keeps Parakeet for English"
        )
        XCTAssertEqual(
            DictationController.appleSpeechLanguage(picked: nil, wanted: "de-DE", parakeetFits: true, hasSpeechTranscriber: true)?.module,
            .dictation
        )
    }

    func testAPreparationThatSaysNothingEndsInPlainWordsWithTryAgain() async throws {
        let stuck = SilentTranscriber()
        let working = ControlledTranscriber()
        var factory: [any LocalTranscribing] = [stuck, working]
        let controller = DictationController(
            audio: FakeAudioManager(),
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: working,
            transcriberFactory: { factory.removeFirst() },
            autoPrepareOnInit: false
        )
        controller.preparationLimits = (.milliseconds(600), .milliseconds(600))
        controller.prepareModel()
        try await settle()
        XCTAssertEqual(controller.phase, .preparingModel)
        XCTAssertEqual(controller.modelPreparation.detail, "Checking the speech model")
        for _ in 0..<40 where controller.phase == .preparingModel {
            try await Task.sleep(for: .milliseconds(50))
        }
        XCTAssertEqual(controller.phase, .failed("The speech model isn't ready. Try again."), "never a spinner for good")

        // The preparation Hex gave up on is cancelled, so it can't publish after the failure.
        var cancelled = false
        for _ in 0..<100 where !cancelled {
            cancelled = await stuck.preparationCancelled
            if !cancelled { try await Task.sleep(for: .milliseconds(10)) }
        }
        XCTAssertTrue(cancelled)

        // Try again (Set up) starts a new preparation, which gets ready.
        controller.performPrimaryAction()
        try await settle()
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertTrue(factory.isEmpty)
    }

    func testParakeetsFirstCompileKeepsTheBarMovingAndSaysWhy() {
        var progress = ParakeetPreparationProgress(downloads: true, compiledBefore: false)
        XCTAssertEqual(progress.snapshot(at: 0).detail, "Checking the speech model")
        progress.download(0.5)
        let halfDownloaded = progress.snapshot(at: 1)
        XCTAssertEqual(halfDownloaded.fractionCompleted, 0.35, accuracy: 0.001)
        XCTAssertEqual(halfDownloaded.detail, "Downloading the speech model")
        progress.download(0.2)
        XCTAssertEqual(progress.snapshot(at: 1).fractionCompleted, 0.35, accuracy: 0.001, "never back")

        // Core ML reports only each model's start and end; the encoder's compile alone is
        // a minute or more on an iPhone 14, where the bar used to sit at 50%.
        progress.startLoading(at: 100)
        progress.finishModel(at: 103)
        var last = progress.snapshot(at: 103).fractionCompleted
        XCTAssertGreaterThanOrEqual(last, 0.7)
        // However long this iPhone's compile takes, the bar keeps moving.
        for second in stride(from: 108.0, through: 900, by: 5) {
            let snapshot = progress.snapshot(at: second)
            XCTAssertGreaterThan(snapshot.fractionCompleted, last, "moves at \(second)")
            XCTAssertLessThan(snapshot.fractionCompleted, 0.99)
            last = snapshot.fractionCompleted
        }
        XCTAssertEqual(progress.snapshot(at: 120).detail, "Optimizing for this iPhone, about a minute")
        XCTAssertEqual(
            progress.snapshot(at: 100 + progress.expectedLoadSeconds * 2 + 1).detail,
            "Still optimizing. The first time can take a few minutes."
        )

        progress.finishModel(at: 901)
        XCTAssertGreaterThanOrEqual(progress.snapshot(at: 901).fractionCompleted, last, "a model's end never moves it back")
        progress.finish()
        XCTAssertEqual(progress.snapshot(at: 902).fractionCompleted, 1)
    }

    func testAParakeetLoadAfterTheFirstSaysLoadingAndUsesTheWholeBar() {
        var progress = ParakeetPreparationProgress(downloads: false, compiledBefore: true)
        progress.startLoading(at: 0)
        XCTAssertEqual(progress.snapshot(at: 0).fractionCompleted, 0, accuracy: 0.001)
        XCTAssertEqual(progress.snapshot(at: 0.5).detail, "Loading the speech model")
        XCTAssertLessThan(progress.expectedLoadSeconds, 10)
    }

    func testParakeetsCompileRecordFollowsTheSystem() {
        let record = ParakeetCompileRecord(defaults: defaults, system: "Version 18.6 (Build 22G86)")
        XCTAssertFalse(record.isCurrent)
        record.markCurrent()
        XCTAssertTrue(record.isCurrent)
        XCTAssertFalse(
            ParakeetCompileRecord(defaults: defaults, system: "Version 18.7 (Build 22H20)").isCurrent,
            "Core ML compiles again after an iOS update"
        )
    }

    func testADownloadCutOffInTheBackgroundStartsAgainOnReturn() async throws {
        let failing = UnpreparableTranscriber(error: ParakeetTranscriber.TranscriberError.downloadFailed)
        let working = ControlledTranscriber()
        var made = 0
        let controller = DictationController(
            audio: FakeAudioManager(),
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: working,
            transcriberFactory: {
                made += 1
                return made == 1 ? failing : working
            },
            autoPrepareOnInit: false
        )
        controller.appMovedToBackground()
        controller.prepareModel()
        try await settle()
        guard case .failed = controller.phase else { return XCTFail("\(controller.phase)") }

        controller.appBecameActive()
        try await settle()
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertEqual(made, 2)

        controller.appBecameActive()
        try await settle()
        XCTAssertEqual(made, 2, "once")
    }

    func testAFailureWhileHexIsOpenWaitsForTryAgain() async throws {
        var made = 0
        let controller = DictationController(
            audio: FakeAudioManager(),
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: ControlledTranscriber(),
            transcriberFactory: {
                made += 1
                return UnpreparableTranscriber(error: ParakeetTranscriber.TranscriberError.downloadFailed)
            },
            autoPrepareOnInit: false
        )
        controller.prepareModel()
        try await settle()
        controller.appMovedToBackground()
        controller.appBecameActive()
        try await settle()
        XCTAssertEqual(made, 1)
        XCTAssertFalse(DictationController.isDownloadInterruption(CancellationError()))
        XCTAssertTrue(DictationController.isDownloadInterruption(StallWatch.Stalled()))
    }

    func testAParakeetLoadThatEndedHexIsNotRepeatedAtLaunch() {
        let loads = ParakeetLoadGuard(defaults: defaults)
        loads.noteLaunch()
        XCTAssertFalse(loads.lastLoadWasInterrupted)

        // iOS ends Hex while Core ML compiles (or ends it in the background, or it is swiped
        // away): one ended load is no evidence, and the next launch loads it again.
        loads.begin()
        let nextLaunch = ParakeetLoadGuard(defaults: defaults)
        nextLaunch.noteLaunch()
        XCTAssertFalse(nextLaunch.lastLoadWasInterrupted, "Hex ended once mid-load: tried again")

        // The same again: two in a row set it aside.
        nextLaunch.begin()
        nextLaunch.noteLaunch()
        XCTAssertTrue(nextLaunch.lastLoadWasInterrupted)

        nextLaunch.begin()
        nextLaunch.end(loaded: false)
        nextLaunch.noteLaunch()
        XCTAssertTrue(nextLaunch.lastLoadWasInterrupted, "a failed load keeps it")
        nextLaunch.begin()
        nextLaunch.end(loaded: true)
        XCTAssertFalse(nextLaunch.lastLoadWasInterrupted, "a load that worked clears it")
    }

    func testASetAsideParakeetLoadTriesAgainNextDayOrNextBuild() {
        let start = Date(timeIntervalSince1970: 1_000_000)
        var now = start
        func guardAt(build: String = "126") -> ParakeetLoadGuard {
            ParakeetLoadGuard(defaults: defaults, build: build, now: { now })
        }
        guardAt().setAside()
        XCTAssertTrue(guardAt().lastLoadWasInterrupted)
        XCTAssertFalse(guardAt(build: "127").lastLoadWasInterrupted, "a new build tries again")
        now = start.addingTimeInterval(ParakeetLoadGuard.setAsideDuration + 1)
        XCTAssertFalse(guardAt().lastLoadWasInterrupted, "a day later it tries again")

        // An older build's lasting flag gives the load another try.
        defaults.set(true, forKey: "parakeet.loadInterrupted")
        let upgraded = guardAt()
        upgraded.noteLaunch()
        XCTAssertFalse(upgraded.lastLoadWasInterrupted)
        XCTAssertNil(defaults.object(forKey: "parakeet.loadInterrupted"))
    }

    // MARK: - Experiments › Multilingual model (Parakeet)

    /// 6 GB and 4 GB iPhones report about this much.
    private static let sixGB: UInt64 = 5_900_000_000
    private static let fourGB: UInt64 = 3_700_000_000

    func testMultilingualParakeetIsOffByDefaultAndPicksNothing() {
        let settings = MultilingualParakeetSettings(defaults: defaults)
        XCTAssertFalse(settings.isEnabled)
        XCTAssertNil(settings.language(set: "nl-NL", keyboard: nil, preferred: ["nl-NL"], physicalMemory: Self.sixGB))
        XCTAssertNil(
            ParakeetUltraTranscriber.chosenLanguage(language: settings.language(), availableMemory: 0, setAside: false),
            "off, Hex keeps the engine it uses without it"
        )
    }

    func testMultilingualParakeetTakesItsLanguagesOnIPhonesWithRoom() {
        let settings = MultilingualParakeetSettings(defaults: defaults)
        settings.isEnabled = true
        func language(set: String? = nil, keyboard: String? = nil, preferred: [String] = ["en-US"], memory: UInt64 = Self.sixGB) -> String? {
            settings.language(set: set, keyboard: keyboard, preferred: preferred, physicalMemory: memory)
        }
        XCTAssertEqual(language(set: "nl-NL"), "nl-NL")
        XCTAssertEqual(language(keyboard: "de-DE"), "de-DE", "Automatic follows the keyboard's language")
        XCTAssertEqual(language(preferred: ["en-NL", "nl-NL"]), "en-NL", "else the iPhone's first language")
        XCTAssertNil(language(set: "ja-JP"), "a language it doesn't know stays with Apple Speech")
        XCTAssertNil(language(set: "hi-IN"))
        XCTAssertNil(language(preferred: ["ja-JP", "en-US"]), "Automatic doesn't skip the iPhone's first language")
        XCTAssertNil(language(set: "nl-NL", memory: Self.fourGB), "4 GB iPhones can't load it")

        XCTAssertEqual(ParakeetUltraTranscriber.chosenLanguage(language: "nl-NL", availableMemory: 2_500 << 20, setAside: false), "nl-NL")
        XCTAssertEqual(ParakeetUltraTranscriber.chosenLanguage(language: "nl-NL", availableMemory: 0, setAside: false), "nl-NL", "no limit (simulator)")
        XCTAssertNil(ParakeetUltraTranscriber.chosenLanguage(language: "nl-NL", availableMemory: 900 << 20, setAside: false), "too little left")
        XCTAssertNil(ParakeetUltraTranscriber.chosenLanguage(language: "nl-NL", availableMemory: 0, setAside: true), "a load ended Hex")
    }

    func testAMultilingualParakeetLoadThatEndedHexIsKeptApartFromV2s() {
        let ultra = ParakeetLoadGuard(model: ParakeetUltraTranscriber.loadGuardModel, defaults: defaults)
        for _ in 0..<2 {
            ultra.begin()
            ultra.noteLaunch()
        }
        let v2 = ParakeetLoadGuard(defaults: defaults)
        v2.noteLaunch()
        XCTAssertFalse(v2.lastLoadWasInterrupted)
        XCTAssertTrue(ultra.lastLoadWasInterrupted)
        ultra.forget()
        XCTAssertFalse(ultra.lastLoadWasInterrupted, "turning the experiment on again tries again")
        XCTAssertFalse(ParakeetCompileRecord(key: ParakeetUltraTranscriber.compileRecordKey, defaults: defaults).isCurrent)
    }

    func testMultilingualParakeetNeverTakesDictationUntilItIsReady() {
        let step = DictationController.multilingualStep
        XCTAssertEqual(step(nil, true, true), .none, "off, another language, or set aside")
        XCTAssertEqual(step("nl-NL", false, false), .download, "the usual engine dictates while it downloads")
        XCTAssertEqual(step("nl-NL", false, true), .download)
        XCTAssertEqual(step("nl-NL", true, false), .loadBeside, "a first load compiles beside the usual engine")
        XCTAssertEqual(step("nl-NL", true, true), .dictate)
        XCTAssertEqual(
            ParakeetPreparationProgress(downloads: true, compiledBefore: false).downloadDetail,
            "Downloading the speech model",
            "V2's words are unchanged"
        )
    }

    func testTheKeyboardOpensHexForMultilingualParakeetOnlyOnceItIsReady() {
        let settings = MultilingualParakeetSettings(defaults: defaults)
        XCTAssertFalse(settings.keyboardOpensHex(language: "nl-NL"), "off")
        settings.isEnabled = true
        XCTAssertFalse(settings.keyboardOpensHex(language: "nl-NL"), "on, still downloading: the keyboard dictates itself")
        settings.isModelReady = true
        XCTAssertTrue(settings.keyboardOpensHex(language: "nl-NL"))
        XCTAssertFalse(settings.keyboardOpensHex(language: nil), "a language it doesn't take")
        settings.isEnabled = false
        XCTAssertFalse(settings.keyboardOpensHex(language: "nl-NL"))
    }

    func testOnlyASwitchTurnedOffDeletesTheMultilingualModelAtLaunch() {
        let settings = MultilingualParakeetSettings(defaults: defaults)
        XCTAssertFalse(settings.isTurnedOff, "never set: nothing to delete")
        settings.isEnabled = true
        XCTAssertFalse(settings.isTurnedOff)
        settings.isEnabled = false
        XCTAssertTrue(settings.isTurnedOff)
        XCTAssertFalse(
            MultilingualParakeetSettings(defaults: nil).isTurnedOff,
            "an App Group Hex can't read says nothing about the switch"
        )
    }

    func testSpeechModelInKeyboardIsOnUnlessTurnedOffAndNeedsTheMemory() {
        let settings = KeyboardParakeetSettings(defaults: defaults)
        XCTAssertTrue(settings.isEnabled, "on by default")
        XCTAssertFalse(settings.isTurnedOff, "never set: nothing to delete")
        XCTAssertFalse(settings.isTurnedOnByHand, "on by default waits for Wi-Fi and the keyboard's measure")
        settings.isEnabled = false
        XCTAssertFalse(settings.isEnabled)
        XCTAssertTrue(settings.isTurnedOff)
        settings.isEnabled = true
        XCTAssertTrue(settings.isTurnedOnByHand, "turned back on: downloads at once")
        XCTAssertNil(settings.keyboardLimit, "until the keyboard first opens")
        settings.recordKeyboardLimit(177 * KeyboardParakeetBudget.megabyte)
        XCTAssertEqual(settings.keyboardLimit, 177 * KeyboardParakeetBudget.megabyte)
        XCTAssertTrue(KeyboardParakeetSettings.fits(physicalMemory: Self.sixGB))
        XCTAssertFalse(KeyboardParakeetSettings.fits(physicalMemory: Self.fourGB))
        settings.setKeyboardState(.preparing, now: Date(timeIntervalSince1970: 100))
        XCTAssertEqual(settings.keyboardState?.state, .preparing)
        XCTAssertEqual(settings.keyboardState?.at, Date(timeIntervalSince1970: 100))
        settings.setKeyboardState(nil)
        XCTAssertNil(settings.keyboardState)
    }

    func testTheKeyboardLoadsParakeetOnlyWithTheRoomMeasuredOnAnIPhone() {
        let mb = KeyboardParakeetBudget.megabyte
        XCTAssertEqual(KeyboardParakeetBudget.requiredLimit, 131 * mb, "V2 and Ultra alike")
        // An iPhone 17 Pro's keyboard: a 177 MB limit, 33 MB used before Hex's keyboard records.
        XCTAssertTrue(KeyboardParakeetBudget.allowsLoad(footprint: 33 * mb, available: 144 * mb))
        XCTAssertFalse(
            KeyboardParakeetBudget.allowsLoad(footprint: 20 * mb, available: 80 * mb),
            "a 100 MB limit: with the model loaded the keyboard records at up to 91 MB"
        )
        XCTAssertFalse(
            KeyboardParakeetBudget.allowsLoad(footprint: 33 * mb, available: 44 * mb),
            "a 77 MB limit keeps Apple's speech recognition"
        )
        XCTAssertFalse(
            KeyboardParakeetBudget.allowsLoad(footprint: 120 * mb, available: 50 * mb),
            "too little left, whatever the limit"
        )
        XCTAssertTrue(KeyboardParakeetBudget.allowsTranscription(footprint: 60 * mb, available: 117 * mb, audioSeconds: 10))
        XCTAssertTrue(
            KeyboardParakeetBudget.allowsTranscription(footprint: 56 * mb, available: 121 * mb, audioSeconds: 9.4),
            "Hex's keyboard recording on the iPhone 17 Pro"
        )
        XCTAssertFalse(KeyboardParakeetBudget.allowsTranscription(footprint: 160 * mb, available: 17 * mb, audioSeconds: 5))
        XCTAssertTrue(
            KeyboardParakeetBudget.allowsLoad(footprint: 20 * mb, available: 0),
            "no limit where iOS sets none (the simulator)"
        )
    }

    func testALongDictationGoesToApplesEngineWhichStreamsIt() {
        let mb = KeyboardParakeetBudget.megabyte
        XCTAssertEqual(KeyboardParakeetBudget.transcribeHeadroom(audioSeconds: 60), 54 * mb, "24 MB and 0.5 MB a second")
        // The iPhone 17 Pro's keyboard after many dictations: 91 MB of its 177 MB.
        XCTAssertTrue(KeyboardParakeetBudget.allowsTranscription(footprint: 91 * mb, available: 86 * mb, audioSeconds: 60))
        XCTAssertTrue(KeyboardParakeetBudget.allowsTranscription(footprint: 91 * mb, available: 86 * mb, audioSeconds: 90))
        XCTAssertFalse(
            KeyboardParakeetBudget.allowsTranscription(footprint: 91 * mb, available: 86 * mb, audioSeconds: 91),
            "longer than 90 s goes to Apple's engine, whatever the room"
        )
        XCTAssertFalse(
            KeyboardParakeetBudget.allowsTranscription(footprint: 91 * mb, available: 50 * mb, audioSeconds: 60),
            "a keyboard with less room: 60 s needs 54 MB"
        )
        XCTAssertTrue(KeyboardParakeetBudget.allowsTranscription(footprint: 91 * mb, available: 50 * mb, audioSeconds: 40))
        XCTAssertFalse(
            KeyboardParakeetBudget.allowsTranscription(footprint: 20 * mb, available: 0, audioSeconds: 120),
            "the cap holds where iOS sets no limit (the simulator)"
        )
    }

    func testTheKeyboardsSpeechModelDownloadsByItselfOnlyWhereTheKeyboardWouldUseIt() {
        let mb = KeyboardParakeetBudget.megabyte
        func hold(
            otherEngine: Bool = false,
            english: Bool = true,
            limit: Int? = 177 * mb,
            unmetered: Bool? = true,
            free: Int64? = 8_000_000_000
        ) -> KeyboardParakeetDownload.Hold? {
            let plan = KeyboardParakeetPlan.resolve(
                isEnabled: true,
                fitsDevice: true,
                twoLanguageEnabled: otherEngine,
                multilingualLanguage: nil,
                wantedLanguage: english ? "en-US" : "fr-FR",
                keyboardLimit: limit,
                v2Downloaded: false,
                ultraDownloaded: false
            )
            return KeyboardParakeetDownload.hold(plan: plan, unmetered: unmetered, freeBytes: free)
        }
        XCTAssertNil(hold(), "an iPhone 17 Pro on Wi-Fi with room")
        XCTAssertEqual(hold(english: false), .notEnglish)
        XCTAssertEqual(hold(otherEngine: true), .otherEngine)
        XCTAssertEqual(hold(limit: nil), .keyboardNotMeasured, "nothing before the keyboard has opened")
        XCTAssertEqual(hold(limit: 80 * mb), .keyboardTooSmall)
        XCTAssertEqual(hold(limit: 100 * mb), .keyboardTooSmall, "barely above what the keyboard records at")
        XCTAssertEqual(hold(unmetered: false), .notOnWiFi, "cellular, a hotspot or Low Data Mode")
        XCTAssertEqual(hold(unmetered: nil), .notOnWiFi, "before the connection is known")
        XCTAssertEqual(hold(free: 1_500_000_000), .needsSpace)
        XCTAssertNil(hold(free: nil), "space iOS doesn't report doesn't hold it")
    }

    func testHomesSpeechModelCardShowsOnlyWhereTheKeyboardWouldUseItAndStaysDismissed() {
        func shows(enabled: Bool = true, fits: Bool = true, hold: KeyboardParakeetDownload.Hold? = nil, dismissed: Bool = false) -> Bool {
            SpeechModelHomeCard.shows(enabled: enabled, fits: fits, hold: hold, dismissed: dismissed)
        }
        XCTAssertTrue(shows(), "downloading, downloaded or ready")
        XCTAssertTrue(shows(hold: .notOnWiFi), "waiting says why")
        XCTAssertTrue(shows(hold: .keyboardNotMeasured))
        XCTAssertTrue(shows(hold: .needsSpace))
        XCTAssertFalse(shows(enabled: false), "the switch turned off")
        XCTAssertFalse(shows(fits: false), "under 6 GB")
        XCTAssertFalse(shows(hold: .notEnglish))
        XCTAssertFalse(shows(hold: .otherEngine))
        XCTAssertFalse(shows(hold: .keyboardTooSmall))
        XCTAssertFalse(shows(dismissed: true))
        let suite = "speech-model-card-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        XCTAssertFalse(SpeechModelHomeCard.isDismissed(defaults))
        SpeechModelHomeCard.dismiss(defaults)
        XCTAssertTrue(SpeechModelHomeCard.isDismissed(UserDefaults(suiteName: suite)!), "dismissed stays dismissed")
    }

    func testTheKeyboardsSpeechModelNeverDownloadsByItselfOverCellular() {
        func unmetered(satisfied: Bool = true, expensive: Bool = false, constrained: Bool = false, cellular: Bool = false) -> Bool {
            KeyboardParakeetDownload.isUnmetered(satisfied: satisfied, expensive: expensive, constrained: constrained, cellular: cellular)
        }
        XCTAssertTrue(unmetered(), "Wi-Fi, or a VPN over Wi-Fi")
        XCTAssertFalse(unmetered(satisfied: false), "no connection")
        XCTAssertFalse(unmetered(expensive: true), "a personal hotspot, or cellular as iOS usually prices it")
        XCTAssertFalse(unmetered(constrained: true), "Low Data Mode")
        XCTAssertFalse(unmetered(cellular: true), "5G that iOS calls not expensive under Allow More Data on 5G")
        XCTAssertFalse(unmetered(expensive: true, cellular: true), "ordinary cellular")
    }

    func testParakeetsFilesMoveIntoTheAppGroupRatherThanDownloadingAgain() throws {
        XCTAssertEqual(ParakeetTranscriber.cacheDirectory.lastPathComponent, SpeechModelStore.parakeetV2Folder)
        XCTAssertEqual(ParakeetTranscriber.cacheDirectory, SpeechModelStore.parakeetV2)
        XCTAssertEqual(ParakeetUltraModel.directory, SpeechModelStore.parakeetUltra)
        XCTAssertEqual(
            SpeechModelStore.parakeetUltraFolder,
            "parakeet-ultra-coreml-\(ParakeetUltraModel.revision.prefix(7))",
            "the keyboard looks for Ultra under its pinned revision"
        )
        let files = FileManager.default
        let root = files.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let old = root.appendingPathComponent("old")
        let new = root.appendingPathComponent("group/new")
        func partial(in folder: URL) throws {
            try files.createDirectory(at: folder, withIntermediateDirectories: true)
            try Data("w".utf8).write(to: folder.appendingPathComponent("weight.bin.partial"))
        }
        let compiled = ParakeetCompileRecord(key: "test.compiledForSystem", defaults: defaults, system: "27.2")
        compiled.markCurrent()
        try partial(in: old)
        SpeechModelStore.move(from: old, to: new, isComplete: { _ in false }, forgetting: compiled, fileManager: files)
        XCTAssertTrue(files.fileExists(atPath: new.appendingPathComponent("weight.bin.partial").path), "a download under way moves, partial file and all")
        XCTAssertFalse(files.fileExists(atPath: old.path))
        XCTAssertFalse(SpeechModelStore.isComplete(new))
        XCTAssertFalse(compiled.isCurrent, "Core ML compiles moved files again, and the next load says so")

        compiled.markCurrent()
        SpeechModelStore.markComplete(new)
        try partial(in: old)
        SpeechModelStore.move(from: old, to: new, isComplete: { SpeechModelStore.isComplete($0) }, forgetting: compiled, fileManager: files)
        XCTAssertTrue(SpeechModelStore.isComplete(new), "a complete copy in the App Group stays")
        XCTAssertFalse(files.fileExists(atPath: old.path), "and the old, partial one goes")
        XCTAssertTrue(compiled.isCurrent, "nothing moved: what Core ML compiled still holds")
    }

    func testTheMultilingualDownloadResumesFromItsPartialFileAndSaysItsBytes() async throws {
        let model = try UltraStubModel()
        defer { model.remove() }
        let first = model.files[0]
        let partial = model.folder.appendingPathComponent(first.path).appendingPathExtension("partial")
        try FileManager.default.createDirectory(at: partial.deletingLastPathComponent(), withIntermediateDirectories: true)
        try model.bodies[first.path]!.prefix(1000).write(to: partial)
        XCTAssertEqual(ParakeetUltraModel.bytesOnDisk(in: model.folder, files: model.files), 1000)

        let download = model.download()
        download.start()
        if case .downloading(let bytes) = download.state {
            XCTAssertEqual(bytes, 1000, "picks up from what's on disk")
        } else {
            XCTFail("\(download.state)")
        }
        XCTAssertEqual(download.status, "Downloading the multilingual model: 1\u{00A0}KB of 5\u{00A0}KB")
        try await waitUntil { download.state == .loading }

        XCTAssertTrue(ParakeetUltraModel.isDownloaded(in: model.folder, files: model.files))
        XCTAssertEqual(UltraStubProtocol.requested.first ?? nil, "bytes=1000-", "resumed with a range")
    }

    func testAMultilingualDownloadThatDropsOrStallsTriesAgainFromWhereItWas() async throws {
        let model = try UltraStubModel()
        defer { model.remove() }
        UltraStubProtocol.script([.cut(1200), .hang(800), .serve])
        let download = model.download()
        var finished = 0
        download.onDownloaded = { finished += 1 }
        download.start()
        try await waitUntil(seconds: 10) { download.state == .loading }

        XCTAssertEqual(finished, 1)
        XCTAssertTrue(ParakeetUltraModel.isDownloaded(in: model.folder, files: model.files))
        XCTAssertEqual(
            Array(UltraStubProtocol.requested.prefix(3)),
            [nil, "bytes=1200-", "bytes=2000-"],
            "a dropped connection and a stalled one each pick up from the partial file"
        )
    }

    func testAMultilingualDownloadThatKeepsMovingNeverRunsOutOfTries() async throws {
        let model = try UltraStubModel()
        defer { model.remove() }
        UltraStubProtocol.script(Array(repeating: .cut(400), count: ParakeetUltraDownload.automaticTries + 1))
        let download = model.download()
        download.start()
        try await waitUntil(seconds: 10) { download.state == .loading || download.isFailed }
        XCTAssertEqual(download.state, .loading, "each drop came after more bytes, so it kept going")
        XCTAssertTrue(ParakeetUltraModel.isDownloaded(in: model.folder, files: model.files))
    }

    func testTheMultilingualDownloadRestartsOnlyAfterALongBackgroundTrip() async throws {
        let model = try UltraStubModel()
        defer { model.remove() }
        UltraStubProtocol.script([.hang(1200), .serve])
        var now = Date(timeIntervalSinceReferenceDate: 0)
        let download = model.download(stallLimit: .seconds(30), now: { now })
        download.start()
        try await waitUntil(seconds: 5) { download.state == .downloading(1200) }

        // Inactive to active (Control Center, Face ID, an alert) suspends nothing.
        download.appBecameActive()
        try await Task.sleep(for: .milliseconds(150))
        XCTAssertEqual(UltraStubProtocol.requested.count, 1, "nothing was suspended, so nothing restarts")

        // A trip too short for iOS to have dropped the connection is left alone too.
        download.appMovedToBackground()
        now = now.addingTimeInterval(10)
        download.appBecameActive()
        try await Task.sleep(for: .milliseconds(150))
        XCTAssertEqual(UltraStubProtocol.requested.count, 1, "a short trip suspends nothing long enough")

        // One long enough for iOS to cut it starts over from the bytes on disk.
        download.appMovedToBackground()
        now = now.addingTimeInterval(30)
        download.appBecameActive()
        try await waitUntil(seconds: 5) { download.state == .loading }
        XCTAssertEqual(
            Array(UltraStubProtocol.requested.prefix(2)),
            [nil, "bytes=1200-"],
            "the suspended transfer resumed from its partial file"
        )
        XCTAssertTrue(ParakeetUltraModel.isDownloaded(in: model.folder, files: model.files))
    }

    func testAMultilingualDownloadThatKeepsFailingSaysWhatHappened() async throws {
        let model = try UltraStubModel()
        defer { model.remove() }
        UltraStubProtocol.script(Array(repeating: .status(503), count: 10))
        let download = model.download()
        download.start()
        try await waitUntil { download.isFailed }
        XCTAssertEqual(UltraStubProtocol.requested.count, ParakeetUltraDownload.automaticTries)
        XCTAssertEqual(download.status, "The multilingual model didn't download (error 503). Try again later.")

        UltraStubProtocol.script([.status(404)])
        download.restart()
        try await waitUntil { download.isFailed }
        XCTAssertEqual(UltraStubProtocol.requested.count, 1, "a refused request isn't tried again by itself")

        XCTAssertEqual(
            ParakeetUltraDownload.message(for: URLError(.notConnectedToInternet)),
            "The multilingual model's download stopped. Check your connection and try again."
        )
        XCTAssertTrue(ParakeetUltraDownload.message(for: CocoaError(.fileWriteOutOfSpace)).contains("free space"))
        XCTAssertEqual(ParakeetUltraDownload.backoff(1), 2)
        XCTAssertEqual(ParakeetUltraDownload.backoff(4), 16)
        XCTAssertEqual(ParakeetUltraDownload.backoff(9), 30)
    }

    func testTurningTheMultilingualModelOffStopsItsDownloadBeforeItIsDeleted() async throws {
        let model = try UltraStubModel()
        defer { model.remove() }
        UltraStubProtocol.script([.hang(800)])
        let download = model.download(stallLimit: .seconds(30))
        download.start()
        try await waitUntil { ParakeetUltraModel.bytesOnDisk(in: model.folder, files: model.files) == 800 }

        await download.stop()
        XCTAssertEqual(download.state, .idle)
        ParakeetUltraModel.remove(from: model.folder)
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertFalse(FileManager.default.fileExists(atPath: model.folder.path), "nothing wrote after stop")
    }

    func testParakeetUltrasFilesArePinnedAndComplete() throws {
        let files = ParakeetUltraModel.files
        XCTAssertEqual(Set(files.map(\.path)).count, files.count)
        for model in ["Preprocessor", "Encoder", "Decoder", "JointDecisionv3"] {
            XCTAssertTrue(files.contains { $0.path == "\(model).mlmodelc/weights/weight.bin" }, model)
            XCTAssertTrue(files.contains { $0.path == "\(model).mlmodelc/model.mil" }, model)
        }
        XCTAssertTrue(files.allSatisfy { $0.sha256.count == 64 })
        XCTAssertEqual(ParakeetUltraModel.totalBytes, 632_162_964)
        for file in files {
            XCTAssertTrue(ParakeetUltraModel.url(for: file).absoluteString.contains("/resolve/\(ParakeetUltraModel.revision)/"))
        }

        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        XCTAssertFalse(ParakeetUltraModel.isDownloaded(in: folder))
        let vocabulary = folder.appendingPathComponent("vocab.json")
        try Data(#"{"0": "<unk>", "8191": "▁ja"}"#.utf8).write(to: vocabulary)
        XCTAssertEqual(try ParakeetUltraModel.vocabulary(at: vocabulary), [0: "<unk>", 8191: "▁ja"])
        XCTAssertEqual(
            try ParakeetUltraModel.sha256(of: vocabulary),
            "ec2185c71d79a203174eba2b46e5d20431bdc1d0f275735d1f22f284d1759b24"
        )
    }

    func testTurningMultilingualParakeetOnOrOffPreparesAgainOnceIdle() async throws {
        let transcriber = ControlledTranscriber()
        var made = 0
        let controller = DictationController(
            audio: FakeAudioManager(),
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: {
                made += 1
                return transcriber
            },
            autoPrepareOnInit: false
        )
        controller.prepareModel()
        try await settle()
        XCTAssertEqual(made, 1)

        // Turned off (the test host's App Group has it off) while the usual engine dictates:
        // nothing to prepare again.
        controller.multilingualParakeetChanged()
        try await settle()
        XCTAssertEqual(made, 1)
        XCTAssertEqual(controller.phase, .ready)

        // Turned off while Parakeet Ultra dictated: Hex prepares the engine it picks without it.
        controller.engine = .parakeetUltra
        controller.multilingualParakeetChanged()
        try await settle()
        XCTAssertEqual(made, 2)
        XCTAssertEqual(controller.phase, .ready)
    }

    func testWarmSessionAudioErrorReachesAppAndKeyboardAsASentence() async throws {
        let audio = FakeAudioManager()
        audio.keepaliveShouldThrow = true
        audio.keepaliveError = NSError(domain: NSOSStatusErrorDomain, code: 560_557_684)
        let transcriber = ControlledTranscriber()
        let bridge = KeyboardBridge(defaults: defaults)
        let controller = DictationController(
            audio: audio,
            keyboardBridge: bridge,
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { 30_000 },
            autoPrepareOnInit: false
        )
        controller.prepareModel()
        try await settle()

        controller.selectWarmDuration(.fifteenMinutes)
        try await settle()

        XCTAssertEqual(controller.phase, .failed("Another app is using audio. Try again."))
        XCTAssertEqual(bridge.snapshot().message, "Another app is using audio. Try again.")
    }

    func testEmptyCaptureRule() {
        XCTAssertTrue(EmptyCapture.isTooShort(frames: 0, sampleRate: 48_000), "Done before the microphone came up")
        XCTAssertTrue(EmptyCapture.isTooShort(frames: 4_800, sampleRate: 48_000), "0.1 s is a tap")
        XCTAssertTrue(EmptyCapture.isTooShort(frames: 4_800, sampleRate: 0))
        XCTAssertFalse(EmptyCapture.isTooShort(frames: 14_400, sampleRate: 48_000), "0.3 s can hold a word")

        XCTAssertTrue(EmptyCapture.endsQuietly(CancellationError()))
        XCTAssertTrue(EmptyCapture.endsQuietly(ParakeetTranscriber.TranscriberError.emptyTranscript))
        XCTAssertFalse(EmptyCapture.endsQuietly(ParakeetTranscriber.TranscriberError.notPrepared))
        XCTAssertFalse(EmptyCapture.endsQuietly(NSError(domain: NSOSStatusErrorDomain, code: 560_557_684)))
    }

    func testNoSpeechEndsTheKeyboardDictationQuietly() async throws {
        let now = 40_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
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
        controller.selectWarmDuration(.fifteenMinutes)
        try await settle()

        await transcriber.failNext(with: ParakeetTranscriber.TranscriberError.emptyTranscript)
        controller.startRecording(keyboardJobID: "silent-job")
        try await settle()
        controller.finishRecording()
        try await settle()

        XCTAssertEqual(controller.phase, .ready, "no speech is not a failure")
        let snap = bridge.snapshot()
        XCTAssertEqual(snap.state, .ready)
        XCTAssertNil(snap.message)
        XCTAssertNil(snap.transcript)

        // Anything else still fails, as a sentence.
        await transcriber.failNext(with: ParakeetTranscriber.TranscriberError.notPrepared)
        controller.startRecording(keyboardJobID: "broken-job")
        try await settle()
        controller.finishRecording()
        try await settle()
        XCTAssertEqual(controller.phase, .failed("The speech model isn't ready."))
        XCTAssertEqual(bridge.snapshot().message, "The speech model isn't ready.")
    }

    /// The keyboard opened Hex to dictate (its speech model was missing) and the microphone
    /// failed: Hex says so in the app and the keyboard, keeps no capture or keepalive half
    /// started, and the next Hex key dictates once the microphone works.
    func testAMicrophoneThatFailsWhenTheKeyboardOpensHexSaysSoAndTheNextTryRecords() async throws {
        let now = 50_000.0
        let audio = FakeAudioManager()
        audio.captureShouldThrow = true
        audio.keepaliveShouldThrow = true
        let transcriber = ControlledTranscriber()
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

        controller.handleKeyboardLaunch(
            .startRecording(jobID: "no-mic-job", requestedAt: now - 0.5, style: nil)
        )
        try await settle()

        let message = "Hex couldn't start the microphone. Try again."
        XCTAssertEqual(controller.phase, .failed(message))
        let failed = bridge.snapshot()
        XCTAssertEqual(failed.state, .failed)
        XCTAssertEqual(failed.message, message)
        XCTAssertNil(failed.jobID, "no job left recording")
        XCTAssertNil(failed.recordingStartedAt)
        XCTAssertTrue(audio.createdCaptures.isEmpty)
        XCTAssertFalse(audio.isKeepaliveRunning)
        XCTAssertFalse(controller.keyboardSessionEnabled, "no warm session claimed without a microphone")
        XCTAssertGreaterThan(audio.deactivateCount, 0, "the audio session is given back")

        // The microphone works again: the next Hex key records.
        audio.captureShouldThrow = false
        audio.keepaliveShouldThrow = false
        controller.handleKeyboardLaunch(
            .startRecording(jobID: "mic-back-job", requestedAt: now - 0.3, style: nil)
        )
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        XCTAssertEqual(bridge.snapshot().state, .recording)
        XCTAssertEqual(bridge.snapshot().jobID, "mic-back-job")
        controller.cancelCurrentJob()
    }

    /// Home after a microphone that didn't start, with the model still ready: the button is
    /// Dictate (as home's Ready says), not Set up, and a tap records instead of loading the
    /// model again.
    func testHomeDictatesAgainAfterAMicrophoneFailureWithTheModelReady() async throws {
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        let controller = DictationController(
            audio: audio,
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { 53_000 },
            autoPrepareOnInit: false
        )
        XCTAssertTrue(controller.primaryActionPreparesModel, "no model yet: Set up")
        controller.prepareModel()
        try await settle()
        XCTAssertFalse(controller.primaryActionPreparesModel)

        audio.captureShouldThrow = true
        controller.performPrimaryAction()
        try await settle()
        XCTAssertEqual(controller.phase, .failed("Hex couldn't start the microphone. Try again."))
        XCTAssertTrue(controller.isModelReady)
        XCTAssertFalse(controller.primaryActionPreparesModel, "Dictate, not Set up")

        audio.captureShouldThrow = false
        controller.performPrimaryAction()
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        controller.cancelCurrentJob()

        // A failed model with none ready still sets up.
        controller.isModelReady = false
        controller.failModel("The speech model isn't ready. Try again.")
        XCTAssertTrue(controller.primaryActionPreparesModel)
    }

    /// A warm session's microphone failed for a keyboard's dictation (another app held it):
    /// Hex stays marked failed, and ignored every start the keyboard sent until it was opened,
    /// while its snapshot kept saying so to every keyboard. The failure carries when it
    /// happened, so the keyboard stops showing it, and the next Hex key is Hex's Try again.
    func testAWarmSessionsMicrophoneFailureIsRetriedFromTheKeyboard() async throws {
        var now = 52_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
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
        controller.selectWarmDuration(.fifteenMinutes)
        try await settle()
        XCTAssertTrue(audio.isKeepaliveRunning)

        audio.captureShouldThrow = true
        bridge.send(.start, jobID: "busy-mic", style: .polished, sentAt: now)
        try await settle()
        let message = "Hex couldn't start the microphone. Try again."
        XCTAssertEqual(controller.phase, .failed(message))
        let failed = bridge.snapshot()
        XCTAssertEqual(failed.state, .failed)
        XCTAssertEqual(failed.message, message)
        XCTAssertEqual(failed.messageAt, now)
        XCTAssertFalse(failed.hasStaleMessage(at: now + 2))
        XCTAssertTrue(failed.hasStaleMessage(at: now + KeyboardSnapshot.messageShownFor + 1))
        XCTAssertTrue(audio.isKeepaliveRunning, "the warm session goes on")

        // The keyboard's next Hex key, with the microphone free again: Hex records.
        audio.captureShouldThrow = false
        now += 12
        bridge.send(.start, jobID: "try-again", style: .polished, sentAt: now)
        try await settle()
        XCTAssertEqual(controller.phase, .recording)
        XCTAssertEqual(bridge.snapshot().jobID, "try-again")
        XCTAssertNil(bridge.snapshot().message)
        XCTAssertNil(bridge.snapshot().messageAt)
        controller.endWarmSession()
    }

    /// A recorder's end can arrive after Hex already tore its capture down (Cancel, or the
    /// next dictation started): it changes nothing, and the dictation running now goes on.
    func testAnEndFromATornDownCaptureChangesNothing() async throws {
        let now = 51_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
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
        controller.selectWarmDuration(.fifteenMinutes)
        try await settle()

        controller.startRecording(keyboardJobID: "torn-down")
        try await settle()
        let tornDown = try XCTUnwrap(audio.createdCaptures.last)
        controller.cancelCurrentJob()
        audio.onUnexpectedCaptureEnd?(tornDown.url, false)
        try await settle()
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertNil(bridge.snapshot().message)

        controller.startRecording(keyboardJobID: "current")
        try await settle()
        audio.onUnexpectedCaptureEnd?(tornDown.url, false)
        audio.onUnexpectedCaptureEnd?(tornDown.url, true)
        try await settle()
        XCTAssertEqual(controller.phase, .recording, "the current dictation keeps recording")
        XCTAssertEqual(bridge.snapshot().jobID, "current")
        controller.cancelCurrentJob()
    }

    /// A keepalive that keeps stopping by itself (another app changing the hardware's format,
    /// say) is restarted at once, then less and less often instead of every 100 ms: each
    /// restart activates the session and reopens the microphone under a screen recording.
    func testAKeepaliveThatKeepsStoppingIsRestartedLessAndLessOften() async throws {
        var now = 52_000.0
        let audio = FakeAudioManager()
        let transcriber = ControlledTranscriber()
        let controller = DictationController(
            audio: audio,
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: transcriber,
            transcriberFactory: { transcriber },
            now: { now },
            autoPrepareOnInit: false
        )
        controller.prepareModel()
        try await settle()
        controller.selectWarmDuration(.fifteenMinutes)
        try await settle()
        XCTAssertTrue(audio.isKeepaliveRunning)
        let started = audio.keepaliveStarts

        audio.keepaliveStopsRightAway = true
        audio.isKeepaliveRunning = false
        try await settle(twice: true)
        XCTAssertEqual(audio.keepaliveStarts, started + 1, "restarted at once")
        now += 0.5
        try await settle(twice: true)
        XCTAssertEqual(audio.keepaliveStarts, started + 1, "stopped again: the next waits 1 s")
        now += 0.7
        try await settle(twice: true)
        XCTAssertEqual(audio.keepaliveStarts, started + 2)
        now += 1.8
        try await settle(twice: true)
        XCTAssertEqual(audio.keepaliveStarts, started + 2, "then 2 s")
        now += 0.4
        try await settle(twice: true)
        XCTAssertEqual(audio.keepaliveStarts, started + 3)
        XCTAssertTrue(controller.keyboardSessionEnabled, "the warm session goes on")

        audio.keepaliveStopsRightAway = false
        now += 4.1
        try await settle(twice: true)
        XCTAssertEqual(audio.keepaliveStarts, started + 4, "then 4 s, and now it runs")
        XCTAssertTrue(audio.isKeepaliveRunning)

        now += 60
        try await settle()
        audio.isKeepaliveRunning = false
        try await settle()
        XCTAssertEqual(audio.keepaliveStarts, started + 5, "one that ran a while restarts at once")
        XCTAssertTrue(audio.isKeepaliveRunning)
        controller.endWarmSession()
    }

    /// A recorder that stopped, or failed to start, outlives the capture that dropped it,
    /// so callbacks its audio queue already fetched still find it; then it goes.
    func testARetiredRecorderOutlivesItsTeardownWhileItsQueueCanCallBack() async throws {
        let retired = RetiredRecorders()
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("retired-\(UUID().uuidString)")
            .appendingPathExtension("wav")
        weak var released: AVAudioRecorder?
        try autoreleasepool {
            let recorder = try AVAudioRecorder(
                url: url,
                settings: [
                    AVFormatIDKey: kAudioFormatLinearPCM,
                    AVSampleRateKey: 16_000,
                    AVNumberOfChannelsKey: 1,
                ]
            )
            released = recorder
            recorder.stop()
            retired.hold(recorder, for: .milliseconds(200))
        }
        XCTAssertNotNil(released, "held past its owner")
        XCTAssertEqual(retired.count, 1)

        try await Task.sleep(for: .milliseconds(700))
        XCTAssertNil(released, "and let go after the hold")
        XCTAssertEqual(retired.count, 0)
        try? FileManager.default.removeItem(at: url)
    }

    // MARK: - Which engine dictates (SpeechEngineChoice)

    private static let gigabyte: UInt64 = 1 << 30

    /// An iPhone XR reports 2.8 GB; a 6 GB iPhone about 5.5 GB.
    private func device(
        system: Int = 18,
        memory: UInt64 = 5_500 << 20,
        available: UInt64 = 0,
        setAside: Bool = false
    ) -> SpeechEngineChoice {
        SpeechEngineChoice(system: system, physicalMemory: memory, availableMemory: available, parakeetSetAside: setAside)
    }

    func testBeforeIOS26EnglishStaysOnParakeetOnlyWhereItHasRoom() {
        XCTAssertEqual(device().engine(for: "en-US", appleSpeechPicks: false), .parakeet, "a 6 GB iPhone on iOS 18, as before")
        XCTAssertEqual(device().engine(for: nil, appleSpeechPicks: false), .parakeet, "no language reads as English")
        XCTAssertEqual(device(available: 3 * Self.gigabyte).engine(for: "en-GB", appleSpeechPicks: false), .parakeet)
        XCTAssertEqual(
            device(memory: 2_813 << 20).engine(for: "en-US", appleSpeechPicks: false),
            .onDeviceRecognizer,
            "iPhone XR, 3 GB: iOS ended Hex while Parakeet loaded (build 56)"
        )
        XCTAssertEqual(device(memory: 3 * Self.gigabyte).engine(for: "en-US", appleSpeechPicks: false), .onDeviceRecognizer)
        XCTAssertEqual(
            device(memory: 3_700 << 20, available: 1_100 << 20).engine(for: "en-US", appleSpeechPicks: false),
            .onDeviceRecognizer,
            "less left than loading Parakeet takes"
        )
        XCTAssertEqual(
            device(memory: 3_700 << 20, available: 1_900 << 20).engine(for: "en-US", appleSpeechPicks: false),
            .parakeet
        )
    }

    func testBeforeIOS26EveryOtherLanguageGoesToTheOnDeviceRecognizer() {
        for language in ["zh-CN", "zh-Hans-CN", "es-ES", "fr-FR", "ja-JP"] {
            XCTAssertEqual(device(memory: 8 * Self.gigabyte).engine(for: language, appleSpeechPicks: false), .onDeviceRecognizer, language)
        }
        XCTAssertNil(device().notice(for: "zh-CN"), "nothing to say: Parakeet never had Chinese")
        XCTAssertNil(device(memory: 2_813 << 20).notice(for: "en-US"), "nor on an iPhone it never fit")
    }

    func testFromIOS26TheRecognizerIsNeverPickedSoSpeechRecognitionIsNeverAsked() async {
        for system in [26, 27] {
            for memory in [2_813 << 20, 3_700 << 20, 5_500 << 20] as [UInt64] {
                for setAside in [false, true] {
                    for language in ["en-US", "zh-CN", nil] as [String?] {
                        let choice = device(system: system, memory: memory, available: 1 << 20, setAside: setAside)
                        XCTAssertEqual(choice.engine(for: language, appleSpeechPicks: true), .appleSpeech)
                        XCTAssertEqual(choice.engine(for: language, appleSpeechPicks: false), .parakeet)
                        XCTAssertNil(choice.notice(for: language))
                    }
                }
            }
        }
        XCTAssertTrue(device(system: 26, memory: 5_500 << 20).parakeetFitsBesideAppleSpeech)
        XCTAssertFalse(device(system: 26, memory: 3_700 << 20).parakeetFitsBesideAppleSpeech, "iPhone 11, as before")
        XCTAssertFalse(device(system: 26, setAside: true).parakeetFitsBesideAppleSpeech, "Apple Speech after a load that ended Hex")
        XCTAssertFalse(device(system: 26, available: 1 << 30).parakeetFitsBesideAppleSpeech)
        if #available(iOS 26.0, *) {
            // Only `OnDeviceSpeechTranscriber.prepare` asks for Speech Recognition; iOS 26's
            // default engine is never it, whatever this iPhone's languages.
            let transcriber = await DictationController.defaultTranscriber()
            XCTAssertFalse(transcriber is OnDeviceSpeechTranscriber, "\(type(of: transcriber))")
        }
    }

    func testAParakeetLoadThatEndedHexMovesEnglishToTheRecognizerBeforeIOS26() {
        let loads = ParakeetLoadGuard(defaults: defaults)
        let nextLaunch = ParakeetLoadGuard(defaults: defaults)
        for _ in 0..<2 {
            loads.begin()
            nextLaunch.noteLaunch()
        }
        let choice = device(setAside: nextLaunch.lastLoadWasInterrupted)
        XCTAssertEqual(choice.engine(for: "en-US", appleSpeechPicks: false), .onDeviceRecognizer)
        XCTAssertEqual(
            choice.notice(for: "en-US"),
            "Not enough memory for the speech model. Hex is using Apple's on-device speech recognition."
        )
        let transcriber = DictationController.transcriberBeforeAppleSpeech(choice: choice, suite: suiteName, resolve: { _ in "en-US" })
        XCTAssertTrue(transcriber is OnDeviceSpeechTranscriber)
        XCTAssertTrue(DictationController.transcriberBeforeAppleSpeech(choice: device(), suite: suiteName) is ParakeetTranscriber)

        // Too little memory at the load counts the same way, before anything loads.
        let fresh = ParakeetLoadGuard(defaults: UserDefaults(suiteName: "\(suiteName).fresh")!)
        fresh.setAside()
        XCTAssertTrue(fresh.lastLoadWasInterrupted)
        UserDefaults(suiteName: "\(suiteName).fresh")!.removePersistentDomain(forName: "\(suiteName).fresh")
        XCTAssertTrue(ParakeetTranscriber.hasRoomToLoad(available: 0), "the simulator says nothing")
        XCTAssertFalse(ParakeetTranscriber.hasRoomToLoad(available: 1_200 << 20))
        XCTAssertTrue(ParakeetTranscriber.hasRoomToLoad(available: 2 << 30))
    }

    func testParakeetWithoutRoomPreparesApplesEngineInsteadOfLoading() async throws {
        let apple = ControlledTranscriber()
        var made = 0
        let controller = DictationController(
            audio: FakeAudioManager(),
            keyboardBridge: KeyboardBridge(defaults: defaults),
            transcriber: apple,
            transcriberFactory: {
                made += 1
                return made == 1 ? UnpreparableTranscriber(error: ParakeetTranscriber.TranscriberError.notEnoughMemory) : apple
            },
            autoPrepareOnInit: false
        )
        controller.prepareModel()
        try await settle()
        XCTAssertEqual(made, 2, "prepared again with what Hex picks now")
        XCTAssertEqual(controller.phase, .ready)
        XCTAssertEqual(
            controller.languageNotice,
            "Not enough memory for the speech model. Hex is using Apple's on-device speech recognition."
        )
    }

    func testTheRecognizerAsksForSpeechRecognitionOnlyAsItGetsReady() async throws {
        let access = CountingSpeechAccess(answer: .denied)
        let xr = device(memory: 2_813 << 20)
        let transcriber = DictationController.transcriberBeforeAppleSpeech(
            choice: xr,
            suite: suiteName,
            access: access.access,
            resolve: { _ in "zh-CN" }
        )
        XCTAssertEqual(access.requests, 0, "picking the engine asks nothing")
        let details = DetailLog()
        do {
            try await transcriber.prepare { details.append($0.detail) }
            XCTFail("prepared without Speech Recognition")
        } catch {
            XCTAssertEqual(error as? OnDeviceSpeechTranscriber.TranscriberError, .notAllowed)
            XCTAssertEqual(
                UserFacingError.message(for: error, context: .model),
                "Allow Speech Recognition for Hex in Settings. It runs on this iPhone."
            )
        }
        XCTAssertEqual(access.requests, 1)
        XCTAssertEqual(
            details.values,
            ["Checking on-device speech recognition", "Allow Speech Recognition. It stays on this iPhone."],
            "says it stays on the iPhone before iOS asks"
        )
        let languageID = await transcriber.languageIdentifier
        XCTAssertEqual(languageID, "zh-CN")

        // English on an iPhone Parakeet fits never asks at all.
        let parakeet = DictationController.transcriberBeforeAppleSpeech(choice: device(), suite: suiteName, access: access.access)
        XCTAssertTrue(parakeet is ParakeetTranscriber)
        XCTAssertEqual(access.requests, 1)
        XCTAssertTrue(OnDeviceSpeechTranscriber.isNoSpeech(NSError(domain: "kAFAssistantErrorDomain", code: 1110)))
    }

    func testTheRecognizerFindsChineseAndEnglishAsAppleListsThem() {
        let supported = ["en-AU", "en-GB", "en-IN", "en-US", "fr-CA", "fr-FR", "yue-CN", "zh-CN", "zh-HK", "zh-TW"]
        XCTAssertEqual(OnDeviceSpeechLanguages.match("zh-Hans-CN", in: supported), "zh-CN", "a Simplified Chinese iPhone")
        XCTAssertEqual(OnDeviceSpeechLanguages.match("zh-Hans", in: supported), "zh-CN")
        XCTAssertEqual(OnDeviceSpeechLanguages.match("zh-Hant-TW", in: supported), "zh-TW")
        XCTAssertEqual(OnDeviceSpeechLanguages.match("zh-Hant", in: supported), "zh-TW")
        XCTAssertEqual(OnDeviceSpeechLanguages.match("zh-Hant-HK", in: supported), "zh-HK")
        XCTAssertEqual(OnDeviceSpeechLanguages.match("en-GB", in: supported), "en-GB")
        XCTAssertEqual(OnDeviceSpeechLanguages.match("en", in: supported), "en-US")
        XCTAssertEqual(OnDeviceSpeechLanguages.match("en-NZ", in: supported), "en-US")
        XCTAssertEqual(OnDeviceSpeechLanguages.match("fr-BE", in: supported), "fr-FR")
        XCTAssertNil(OnDeviceSpeechLanguages.match("he-IL", in: supported))

        let languages = LanguageSettings(defaults: defaults)
        let onDevice: (String) -> Bool = { $0 != "fr-CA" }
        XCTAssertEqual(
            OnDeviceSpeechLanguages.resolve(settings: languages, preferred: ["zh-Hans-CN", "en-CN"], supported: supported, onDevice: onDevice),
            "zh-CN"
        )
        XCTAssertEqual(
            OnDeviceSpeechLanguages.resolve(settings: languages, preferred: ["he-IL", "fr-CA"], supported: supported, onDevice: onDevice),
            "en-US",
            "neither Hebrew nor Canadian French on device: English, which every iPhone on iOS 18 has"
        )
        languages.markUnavailable("zh-CN")
        XCTAssertEqual(
            OnDeviceSpeechLanguages.resolve(settings: languages, preferred: ["zh-Hans-CN", "en-CN"], supported: supported, onDevice: onDevice),
            "en-US",
            "Automatic passes over a language set aside"
        )
        languages.chooseDictationLanguage("zh-Hans-CN")
        XCTAssertEqual(
            OnDeviceSpeechLanguages.resolve(settings: languages, preferred: ["en-US"], supported: supported, onDevice: onDevice),
            "zh-CN",
            "a choice made by hand is tried as it is"
        )
    }

    func testBeforeIOS26ATranslationWithoutHexsWindowSaysSo() async {
        XCTAssertEqual(DictationTranslator.reason(for: TranslationEngine.NoSession()), .needsHexOpen)
        let failure = TranslationFailure(.needsHexOpen, source: Locale.Language(identifier: "zh"), target: Locale.Language(identifier: "en"))
        XCTAssertEqual(failure.fix, "iOS 18 translates only with Hex open")
        XCTAssertLessThanOrEqual(failure.fix.count, 40, "fits the keyboard's second line")

        let settings = DictationTranslationSettings(defaults: defaults)
        settings.isEnabled = true
        settings.targetLanguage = "en"
        let stub = StubTranslator()
        stub.replies = [.failure(TranslationEngine.NoSession()), .failure(TranslationEngine.NoSession())]
        let outcome = await DictationTranslator.translate(
            "我们明天见。",
            source: Locale.Language(identifier: "zh-CN"),
            settings: settings,
            engine: stub.engine
        )
        XCTAssertEqual(outcome.failure?.reason, .needsHexOpen)
        XCTAssertEqual(stub.inputs.count, 1, "not tried again: another wait wouldn't bring a session")
        XCTAssertEqual(settings.lastFailure?.reason, .needsHexOpen, "Settings says why")
    }

    /// `twice` lets the warm session loop (every 100 ms) look at least twice.
    private func settle(twice: Bool = false) async throws {
        for _ in 0..<(twice ? 16 : 8) {
            try await Task.sleep(for: .milliseconds(25))
        }
    }

    private func waitUntil(seconds: Double = 5, _ condition: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition() {
            guard Date() < deadline else {
                XCTFail("timed out")
                return
            }
            try await Task.sleep(for: .milliseconds(20))
        }
    }
}

/// Two small files standing in for Parakeet Ultra's, served by `UltraStubProtocol`.
@MainActor
private struct UltraStubModel {
    let folder = FileManager.default.temporaryDirectory.appendingPathComponent("ultra-\(UUID().uuidString)")
    let bodies: [String: Data]
    let files: [ParakeetUltraModel.File]

    init() throws {
        var bodies: [String: Data] = [:]
        var files: [ParakeetUltraModel.File] = []
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: scratch) }
        for (path, size) in [("Encoder.mlmodelc/weights/weight.bin", 3000), ("parakeet_vocab.json", 2000)] {
            let body = Data((0..<size).map { UInt8(truncatingIfNeeded: $0 &* 31 &+ path.count) })
            try body.write(to: scratch)
            bodies[path] = body
            files.append(.init(path: path, size: Int64(size), sha256: try ParakeetUltraModel.sha256(of: scratch)))
        }
        self.bodies = bodies
        self.files = files
        UltraStubProtocol.reset(bodies: bodies)
    }

    func download(
        stallLimit: Duration = .seconds(1),
        now: @escaping () -> Date = { Date() }
    ) -> ParakeetUltraDownload {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [UltraStubProtocol.self]
        return ParakeetUltraDownload(
            directory: folder,
            files: files,
            base: "https://stub.invalid/model",
            session: URLSession(configuration: configuration),
            stallLimit: stallLimit,
            backoff: { _ in 0 },
            now: now
        )
    }

    func remove() {
        try? FileManager.default.removeItem(at: folder)
    }
}

/// Serves `UltraStubModel`'s files with ranges, following a script for its next requests:
/// cut the connection after some bytes, send some and then nothing, or answer a status.
private final class UltraStubProtocol: URLProtocol, @unchecked Sendable {
    enum Step: Sendable {
        case serve
        case cut(Int)
        case hang(Int)
        case status(Int)
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var bodies: [String: Data] = [:]
    nonisolated(unsafe) private static var steps: [Step] = []
    nonisolated(unsafe) private static var ranges: [String?] = []

    static func reset(bodies: [String: Data]) {
        lock.withLock {
            self.bodies = bodies
            steps = []
            ranges = []
        }
    }

    static func script(_ steps: [Step]) {
        lock.withLock {
            self.steps = steps
            ranges = []
        }
    }

    /// Each request's Range header, in order.
    static var requested: [String?] {
        lock.withLock { ranges }
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let url = request.url!
        let range = request.value(forHTTPHeaderField: "Range")
        let path = url.path.replacingOccurrences(of: "/model/", with: "")
        let (step, body): (Step, Data?) = Self.lock.withLock {
            Self.ranges.append(range)
            let step = Self.steps.isEmpty ? .serve : Self.steps.removeFirst()
            return (step, Self.bodies[path])
        }
        guard let body else {
            respond(404, url: url)
            client?.urlProtocolDidFinishLoading(self)
            return
        }
        if case .status(let status) = step {
            respond(status, url: url)
            client?.urlProtocolDidFinishLoading(self)
            return
        }
        let offset = range.flatMap { Int($0.dropFirst("bytes=".count).dropLast()) } ?? 0
        respond(offset > 0 ? 206 : 200, url: url)
        let rest = body.dropFirst(offset)
        switch step {
        case .cut(let count):
            client?.urlProtocol(self, didLoad: Data(rest.prefix(count)))
            // The connection drops a moment after the bytes arrive, as on a network.
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.2) {
                self.client?.urlProtocol(self, didFailWithError: URLError(.networkConnectionLost))
            }
        case .hang(let count):
            client?.urlProtocol(self, didLoad: Data(rest.prefix(count)))
        case .serve, .status:
            client?.urlProtocol(self, didLoad: Data(rest))
            client?.urlProtocolDidFinishLoading(self)
        }
    }

    override func stopLoading() {}

    private func respond(_ status: Int, url: URL) {
        let response = HTTPURLResponse(
            url: url,
            statusCode: status,
            httpVersion: "HTTP/1.1",
            headerFields: ["Content-Type": "application/octet-stream"]
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
    }
}

/// The dictation language rule: an explicit choice, else the first preferred language either
/// Apple Speech module has, `SpeechTranscriber` first.
final class SpeechLanguagePickerTests: XCTestCase {
    /// SpeechTranscriber's and DictationTranscriber's locales as of iOS 26 (abridged).
    private let speechLocales = ["en-US", "en-GB", "de-DE", "es-ES", "es-MX", "fr-FR", "hi-IN"]
    private let dictationLocales = ["en-US", "en-GB", "de-DE", "es-ES", "es-MX", "fr-FR", "nb-NO", "da-DK", "sv-SE", "he-IL"]

    private func lookup(_ list: [String]) -> (Locale) async -> Locale? {
        { wanted in
            let code = wanted.language.languageCode?.identifier
            let region = wanted.region?.identifier
            let matches = list.map { Locale(identifier: $0) }.filter { $0.language.languageCode?.identifier == code }
            return matches.first { $0.region?.identifier == region } ?? matches.first
        }
    }

    /// `speechTranscriber` false: `SpeechTranscriber.isAvailable` is false (iPhone 11, SE 2nd
    /// generation), so only `DictationTranscriber` has languages.
    private func pick(
        explicit: String? = nil,
        preferred: [String],
        current: String = "en_US",
        setAside: Set<String> = [],
        speechTranscriber: Bool = true,
        region: String = "US"
    ) async -> SpeechLanguage? {
        await SpeechLanguagePicker.pick(
            explicit: explicit,
            candidates: SpeechLanguagePicker.automaticCandidates(preferred: preferred, current: current, region: region),
            region: region,
            speech: speechTranscriber ? lookup(speechLocales) : { _ in nil },
            dictation: lookup(dictationLocales),
            skip: { setAside.contains($0.identifier(.bcp47)) }
        )
    }

    func testANorwegianIPhoneGetsNorwegianFromDictationTranscriber() async {
        let picked = await pick(preferred: ["nb-NO", "en-US"], current: "nb_NO")
        XCTAssertEqual(picked?.identifier, "nb-NO")
        XCTAssertEqual(picked?.module, .dictation)
    }

    func testSpeechTranscriberStaysWhereItAlreadyWorks() async {
        let english = await pick(preferred: ["en-US"])
        XCTAssertEqual(english?.identifier, "en-US")
        XCTAssertEqual(english?.module, .speech)
        let spanish = await pick(preferred: ["es-MX", "en-US"])
        XCTAssertEqual(spanish?.identifier, "es-MX")
        XCTAssertEqual(spanish?.module, .speech)
    }

    /// Ukrainian is `DictationTranscriber`'s only (macOS 27.2 lists uk-UA there and not in
    /// `SpeechTranscriber`): picked by hand or by a Ukrainian iPhone, it dictates in Ukrainian.
    func testUkrainianComesFromDictationTranscriber() async {
        let speech = lookup(speechLocales)
        let dictation = lookup(dictationLocales + ["uk-UA", "ru-RU"])
        let chosen = await SpeechLanguagePicker.pick(
            explicit: "uk-UA", candidates: ["en-US"], speech: speech, dictation: dictation
        )
        XCTAssertEqual(chosen?.identifier, "uk-UA")
        XCTAssertEqual(chosen?.module, .dictation)
        let automatic = await SpeechLanguagePicker.pick(
            explicit: nil,
            candidates: SpeechLanguagePicker.automaticCandidates(preferred: ["uk-UA", "en-US"], current: "uk_UA"),
            speech: speech,
            dictation: dictation
        )
        XCTAssertEqual(automatic?.identifier, "uk-UA")
        XCTAssertEqual(automatic?.module, .dictation)
        XCTAssertFalse(SpeechLanguagePicker.isEnglish("uk-UA"), "Apple Speech, never English-only Parakeet")
        XCTAssertEqual(SpeechLanguagePicker.shortNames(for: ["uk-UA"], in: Locale(identifier: "en-US"))["uk-UA"], "Ukrainian")
    }

    /// Polish is `DictationTranscriber`'s only (macOS 27.2 lists pl-PL there and not in
    /// `SpeechTranscriber`): picked by hand or by a Polish iPhone, it dictates in Polish.
    func testPolishComesFromDictationTranscriber() async {
        let speech = lookup(speechLocales)
        let dictation = lookup(dictationLocales + ["pl-PL"])
        let chosen = await SpeechLanguagePicker.pick(
            explicit: "pl-PL", candidates: ["en-US"], speech: speech, dictation: dictation
        )
        XCTAssertEqual(chosen?.identifier, "pl-PL")
        XCTAssertEqual(chosen?.module, .dictation)
        let automatic = await SpeechLanguagePicker.pick(
            explicit: nil,
            candidates: SpeechLanguagePicker.automaticCandidates(preferred: ["pl-PL", "en-US"], current: "pl_PL"),
            speech: speech,
            dictation: dictation
        )
        XCTAssertEqual(automatic?.identifier, "pl-PL")
        XCTAssertEqual(automatic?.module, .dictation)
        XCTAssertFalse(SpeechLanguagePicker.isEnglish("pl-PL"), "Apple Speech, never English-only Parakeet")
        XCTAssertEqual(SpeechLanguagePicker.shortNames(for: ["pl-PL"], in: Locale(identifier: "en-US"))["pl-PL"], "Polish")
    }

    /// Greek is `DictationTranscriber`'s only (macOS 27.2 lists el-GR there and not in
    /// `SpeechTranscriber`): picked by hand or by a Greek iPhone, it dictates in Greek.
    func testGreekComesFromDictationTranscriber() async {
        let speech = lookup(speechLocales)
        let dictation = lookup(dictationLocales + ["el-GR"])
        let chosen = await SpeechLanguagePicker.pick(
            explicit: "el-GR", candidates: ["en-US"], speech: speech, dictation: dictation
        )
        XCTAssertEqual(chosen?.identifier, "el-GR")
        XCTAssertEqual(chosen?.module, .dictation)
        let automatic = await SpeechLanguagePicker.pick(
            explicit: nil,
            candidates: SpeechLanguagePicker.automaticCandidates(preferred: ["el-GR", "en-US"], current: "el_GR"),
            speech: speech,
            dictation: dictation
        )
        XCTAssertEqual(automatic?.identifier, "el-GR")
        XCTAssertEqual(automatic?.module, .dictation)
        XCTAssertFalse(SpeechLanguagePicker.isEnglish("el-GR"), "Apple Speech, never English-only Parakeet")
        XCTAssertEqual(SpeechLanguagePicker.shortNames(for: ["el-GR"], in: Locale(identifier: "en-US"))["el-GR"], "Greek")
    }

    /// Czech is `DictationTranscriber`'s only (macOS 27.2.1 lists cs-CZ there and not in
    /// `SpeechTranscriber`): picked by hand, by a Czech iPhone, or by the keyboard showing
    /// Čeština, it dictates in Czech through Apple Speech, never English-only Parakeet.
    func testCzechComesFromDictationTranscriber() async {
        let speech = lookup(speechLocales)
        let dictation = lookup(dictationLocales + ["cs-CZ"])
        let chosen = await SpeechLanguagePicker.pick(
            explicit: "cs-CZ", candidates: ["en-US"], speech: speech, dictation: dictation
        )
        XCTAssertEqual(chosen?.identifier, "cs-CZ")
        XCTAssertEqual(chosen?.module, .dictation)
        let automatic = await SpeechLanguagePicker.pick(
            explicit: nil,
            candidates: SpeechLanguagePicker.automaticCandidates(preferred: ["cs-CZ", "en-US"], current: "cs_CZ"),
            speech: speech,
            dictation: dictation
        )
        XCTAssertEqual(automatic?.identifier, "cs-CZ")
        XCTAssertEqual(automatic?.module, .dictation)
        XCTAssertFalse(SpeechLanguagePicker.isEnglish("cs-CZ"), "Apple Speech, never English-only Parakeet")
        XCTAssertEqual(SpeechLanguagePicker.shortNames(for: ["cs-CZ"], in: Locale(identifier: "en-US"))["cs-CZ"], "Czech")
        XCTAssertEqual(KeyboardLanguage.czech.dictationLanguage(supported: ["en-US", "cs-CZ"], preferred: ["en-US"]), "cs-CZ")
    }

    func testThePreferredOrderWinsOverTheModule() async {
        // Hebrew first: DictationTranscriber's Hebrew beats SpeechTranscriber's English.
        let picked = await pick(preferred: ["he-IL", "en-US"])
        XCTAssertEqual(picked?.identifier, "he-IL")
        XCTAssertEqual(picked?.module, .dictation)
    }

    func testNynorskFallsBackToBokmal() async {
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["nn-NO", "en-US"], current: "nn_NO"),
            ["nn-NO", "nb-NO", "en-US"]
        )
        let picked = await pick(preferred: ["nn-NO", "en-US"], current: "nn_NO")
        XCTAssertEqual(picked?.identifier, "nb-NO")
    }

    /// Apple Speech's Spanish is Spain's es-ES and the Americas' es-US and es-MX: es-419 and
    /// its Latin American regions are the Americas' (es-US, then es-MX), never Spain's.
    func testLatinAmericanSpanishPicksTheAmericasVariant() async {
        let speech = lookup(speechLocales + ["es-US"])
        let dictation = lookup(dictationLocales + ["es-US"])
        let latin = await SpeechLanguagePicker.pick(
            explicit: nil,
            candidates: SpeechLanguagePicker.automaticCandidates(
                preferred: ["es-419", "en-US"],
                current: "es_419",
                region: "US"
            ),
            region: "US",
            speech: speech,
            dictation: dictation
        )
        XCTAssertEqual(latin?.identifier, "es-US")
        XCTAssertEqual(latin?.module, .speech)

        let withoutUS = await pick(preferred: ["es-419", "en-US"], current: "es_419", region: "ES")
        XCTAssertEqual(withoutUS?.identifier, "es-MX", "es-US missing")

        let colombian = await pick(preferred: ["es-CO", "en-US"], current: "es_CO", region: "US")
        XCTAssertEqual(colombian?.identifier, "es-MX", "es-US missing for a Colombian iPhone too")

        let explicit = await SpeechLanguagePicker.pick(
            explicit: "es-419",
            candidates: ["en-US"],
            region: "US",
            speech: speech,
            dictation: dictation
        )
        XCTAssertEqual(explicit?.identifier, "es-US", "a pick from before the variants")
    }

    /// Plain "es" names no region: the iPhone's Region decides between Mexico's es-MX, the
    /// United States' and Latin America's es-US, and Spain's es-ES.
    func testPlainSpanishFollowsTheIPhonesRegion() async {
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["es", "en-US"], current: "en_US", region: "MX").first,
            "es-MX"
        )
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["es", "en-US"], current: "en_US", region: "US").first,
            "es-US"
        )
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["es", "en-US"], current: "en_US", region: "ES").first,
            "es-ES"
        )
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["es", "en-US"], current: "en_US", region: "FR").first,
            "es-ES"
        )
        let mexican = await pick(preferred: ["es", "en-US"], region: "MX")
        XCTAssertEqual(mexican?.identifier, "es-MX")
        XCTAssertEqual(mexican?.module, .speech)
        let spain = await pick(preferred: ["es", "en-US"], region: "ES")
        XCTAssertEqual(spain?.identifier, "es-ES")
        let american = await pick(preferred: ["es", "en-US"], region: "US")
        XCTAssertEqual(american?.identifier, "es-MX", "es-US missing here, es-MX next")
    }

    func testAnExplicitChoiceWinsAndFallsBackToAutomaticWhenUnsupported() async {
        let danish = await pick(explicit: "da-DK", preferred: ["en-US"])
        XCTAssertEqual(danish?.identifier, "da-DK")
        XCTAssertEqual(danish?.module, .dictation)
        let german = await pick(explicit: "de-DE", preferred: ["nb-NO"])
        XCTAssertEqual(german?.identifier, "de-DE")
        XCTAssertEqual(german?.module, .speech)
        let klingon = await pick(explicit: "tlh", preferred: ["nb-NO", "en-US"])
        XCTAssertEqual(klingon?.identifier, "nb-NO", "an unsupported choice falls back to Automatic")
    }

    func testAutomaticPassesOverALanguageSetAsideButAChoiceStillTriesIt() async {
        // A Norwegian iPhone whose Norwegian model wouldn't install: English next.
        let automatic = await pick(preferred: ["nb-NO", "en-US"], current: "nb_NO", setAside: ["nb-NO"])
        XCTAssertEqual(automatic?.identifier, "en-US")
        let chosen = await pick(explicit: "nb-NO", preferred: ["nb-NO", "en-US"], setAside: ["nb-NO"])
        XCTAssertEqual(chosen?.identifier, "nb-NO", "an explicit choice is tried and says why it fails")
    }

    func testAnEnglishOnlyIPhoneStillDictatesEnglishAfterItsModelFailedOnce() async {
        // TestFlight build 31, iPhone SE (3rd generation), iOS 26.6.1: English's model didn't
        // install once, so it was set aside, and with no other language to move to Automatic
        // picked nothing: "Apple's speech recognition can't dictate this language on this iPhone." It tries
        // English again instead.
        let english = await pick(preferred: ["en-US"], setAside: ["en-US"])
        XCTAssertEqual(english?.identifier, "en-US")
        XCTAssertEqual(english?.module, .speech)
        let british = await pick(preferred: ["en-GB"], current: "en_GB", setAside: ["en-GB", "en-US"])
        XCTAssertEqual(british?.identifier, "en-GB", "the iPhone's own English, not the last resort")
        let norwegian = await pick(preferred: ["nb-NO", "en-US"], current: "nb_NO", setAside: ["nb-NO", "en-US"])
        XCTAssertEqual(norwegian?.identifier, "nb-NO", "everything set aside: the first preferred again")
    }

    func testAnIPhoneWithoutSpeechTranscriberDictatesEnglishWithDictationTranscriber() async {
        // 4 GB iPhones without SpeechTranscriber, in the app and in the keyboard alike, whatever
        // English the iPhone names, a keyboard's region-only locale, or English set aside.
        let cases: [(preferred: [String], current: String, setAside: Set<String>, expected: String)] = [
            (["en-US"], "en_US", [], "en-US"),
            (["en-GB"], "en_GB", [], "en-GB"),
            (["en-IN"], "_IN", [], "en-US"),
            ([], "_US", [], "en-US"),
            (["en-US"], "en_US", ["en-US"], "en-US"),
        ]
        for item in cases {
            let picked = await pick(
                preferred: item.preferred,
                current: item.current,
                setAside: item.setAside,
                speechTranscriber: false
            )
            XCTAssertEqual(picked?.identifier, item.expected, "\(item.preferred) \(item.current)")
            XCTAssertEqual(picked?.module, .dictation)
        }
    }

    func testSetAsideLanguagesComeBackAfterThreeDays() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = LanguageSettings(defaults: defaults)
        let marked = Date(timeIntervalSince1970: 1_000_000)
        settings.markUnavailable("ru-RU", at: marked)
        settings.markSpeechModuleFailed("ru_RU", at: marked)
        XCTAssertTrue(settings.isUnavailable("ru-RU", now: marked.addingTimeInterval(3_600)))
        XCTAssertTrue(settings.speechModuleFailed("ru-RU", now: marked.addingTimeInterval(3_600)), "BCP 47 either way")
        XCTAssertFalse(settings.isUnavailable("uk-UA", now: marked))
        XCTAssertFalse(settings.isUnavailable("ru-RU", now: marked.addingTimeInterval(LanguageSettings.setAsideDuration + 1)))
        settings.clearUnavailable("ru-RU")
        XCTAssertFalse(settings.isUnavailable("ru-RU", now: marked))
    }

    func testUSEnglishIsTheLastResort() async {
        let picked = await pick(preferred: ["xx-XX"], current: "xx_XX")
        XCTAssertEqual(picked?.identifier, "en-US")
    }

    func testShortNamesDropTheRegionOnlyWhenItIsTheOnlyOne() {
        let english = Locale(identifier: "en-US")
        let names = SpeechLanguagePicker.shortNames(for: ["nb-NO", "en-US", "en-GB", "he-IL"], in: english)
        XCTAssertEqual(names["nb-NO"], "Norwegian Bokmål")
        XCTAssertEqual(names["he-IL"], "Hebrew")
        XCTAssertEqual(names["en-US"], "English (United States)")
        XCTAssertEqual(SpeechLanguagePicker.shortName(for: "en-US", in: english), "English (US)")
        XCTAssertEqual(SpeechLanguagePicker.shortName(for: "es-US", in: english), "Spanish (US)")
    }

    func testOnlyEnglishCountsAsEnglish() {
        XCTAssertTrue(SpeechLanguagePicker.isEnglish(nil))
        XCTAssertTrue(SpeechLanguagePicker.isEnglish("en-GB"))
        XCTAssertFalse(SpeechLanguagePicker.isEnglish("nb-NO"))
        XCTAssertFalse(SpeechLanguagePicker.isEnglish("he-IL"))
    }
}

/// Getting a language's speech model: `SpeechTranscriber`'s, then `DictationTranscriber`'s, then
/// a plain message. The Apple Speech calls are fakes; the simulator has no speech models.
final class SpeechModelInstallTests: XCTestCase {
    private var suite = ""
    private var settings = LanguageSettings(defaults: nil)

    override func setUp() {
        suite = "hex.tests.\(UUID().uuidString)"
        settings = LanguageSettings(defaults: UserDefaults(suiteName: suite))
    }

    override func tearDown() {
        UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
    }

    private let russian = SpeechLanguage(locale: Locale(identifier: "ru-RU"), module: .speech)

    /// Each module's status before and after its download; a download either throws, hangs
    /// without progress, or installs.
    private final class FakeAssets: @unchecked Sendable {
        enum Download { case installs, fails, hangs }
        let lock = NSLock()
        var status: [SpeechModuleKind: SpeechModelInstall.Availability] = [.speech: .downloadable, .dictation: .downloadable]
        var downloads: [SpeechModuleKind: Download] = [:]
        var dictationHas = true
        var online = true
        /// A status call that never answers, as after the speech daemon stops responding.
        var hangsStatus = false
        var downloaded: [SpeechModuleKind] = []

        var steps: SpeechModelInstall.Steps {
            SpeechModelInstall.Steps(
                dictationEquivalent: { [self] locale in dictationHas ? locale : nil },
                status: { [self] language in
                    if lock.withLock({ hangsStatus }) {
                        try? await Task.sleep(for: .seconds(60))
                    }
                    return lock.withLock { status[language.module] ?? .unsupported }
                },
                download: { [self] language, progress in
                    let outcome = lock.withLock {
                        downloaded.append(language.module)
                        return downloads[language.module] ?? .installs
                    }
                    switch outcome {
                    case .installs:
                        progress(0.5)
                        lock.withLock { status[language.module] = .installed }
                    case .fails:
                        throw NSError(
                            domain: "SFSpeechErrorDomain",
                            code: 1,
                            userInfo: [NSLocalizedDescriptionKey: "transcription.ru asset not found after attempted download."]
                        )
                    case .hangs:
                        progress(0.5)
                        try await Task.sleep(for: .seconds(60))
                    }
                },
                isOnline: { [self] in online }
            )
        }
    }

    private func run(_ assets: FakeAssets) async throws -> SpeechLanguage {
        try await SpeechModelInstall.run(russian, steps: assets.steps, settings: settings, stallLimit: .milliseconds(300))
    }

    func testAnInstalledModelIsUsedAsItIs() async throws {
        let assets = FakeAssets()
        assets.status[.speech] = .installed
        let installed = try await run(assets)
        XCTAssertEqual(installed, russian)
        XCTAssertTrue(assets.downloaded.isEmpty)
    }

    func testAStatusThatNeverAnswersFailsInsteadOfInstalling() async throws {
        let assets = FakeAssets()
        assets.hangsStatus = true
        do {
            _ = try await run(assets)
            XCTFail("a status that never answers must not install")
        } catch {
            XCTAssertTrue(error is SpeechModelUnresponsive, "\(error)")
        }
        XCTAssertTrue(assets.downloaded.isEmpty, "nothing is downloaded behind a hung status call")
    }

    func testASpeechTranscriberAssetThatNeverArrivesFallsBackToDictationTranscriber() async throws {
        let assets = FakeAssets()
        assets.downloads[.speech] = .fails
        let installed = try await run(assets)
        XCTAssertEqual(installed.module, .dictation)
        XCTAssertEqual(installed.identifier, "ru-RU")
        XCTAssertEqual(assets.downloaded, [.speech, .dictation])
        XCTAssertFalse(settings.speechModuleFailed("ru-RU"), "one failure sets nothing aside")
        _ = try await run(assets)
        XCTAssertTrue(settings.speechModuleFailed("ru-RU"), "twice in a row: the next dictation goes straight to DictationTranscriber")
        XCTAssertFalse(settings.isUnavailable("ru-RU"))
    }

    func testASpeechTranscriberModelThatInstallsBetweenTwoFailuresIsNotSetAside() async throws {
        let failing = FakeAssets()
        failing.downloads[.speech] = .fails
        _ = try await run(failing)
        _ = try await run(FakeAssets())
        _ = try await run(failing)
        XCTAssertFalse(settings.speechModuleFailed("ru-RU"), "not in a row")
    }

    func testTheKeyboardsInstallFailuresSetNothingAsideForHex() async {
        // The keyboard's own reservation released or over Apple's limit ("Too many asset
        // locales allocated") is no evidence about the language: Hex keeps SpeechTranscriber.
        let assets = FakeAssets()
        assets.downloads = [.speech: .fails, .dictation: .fails]
        for _ in 0..<3 {
            _ = try? await SpeechModelInstall.run(
                russian, steps: assets.steps, settings: settings, setsAside: false, stallLimit: .milliseconds(300)
            )
        }
        XCTAssertFalse(settings.speechModuleFailed("ru-RU"))
        XCTAssertFalse(settings.isUnavailable("ru-RU"))
    }

    func testNeitherModelOnlineSetsTheLanguageAside() async {
        let assets = FakeAssets()
        assets.downloads = [.speech: .fails, .dictation: .fails]
        do {
            _ = try await run(assets)
            XCTFail("nothing installed")
        } catch {
            XCTAssertEqual(error as? SpeechModelUnavailable, SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone))
            XCTAssertEqual(UserFacingError.message(for: error, context: .transcription), "Russian isn't available for dictation on this iPhone yet.")
        }
        XCTAssertFalse(settings.isUnavailable("ru-RU"), "one failure sets nothing aside")
        _ = try? await run(assets)
        XCTAssertTrue(settings.isUnavailable("ru-RU"))
    }

    func testUnsupportedEverywhereIsNotAvailableWithoutDownloading() async {
        let assets = FakeAssets()
        assets.status = [.speech: .unsupported, .dictation: .unsupported]
        do {
            _ = try await run(assets)
            XCTFail("nothing installed")
        } catch {
            XCTAssertEqual((error as? SpeechModelUnavailable)?.reason, .notOnThisIPhone)
        }
        XCTAssertTrue(assets.downloaded.isEmpty)
    }

    func testOfflineIsADownloadFailureNotAMissingLanguage() async {
        let assets = FakeAssets()
        assets.downloads = [.speech: .fails, .dictation: .fails]
        assets.online = false
        do {
            _ = try await run(assets)
            XCTFail("nothing installed")
        } catch {
            XCTAssertEqual((error as? SpeechModelUnavailable)?.reason, .downloadFailed)
            XCTAssertEqual(
                UserFacingError.message(for: error, context: .transcription),
                "Russian speech didn't download. Check your connection."
            )
        }
        XCTAssertFalse(settings.isUnavailable("ru-RU"), "tried again next time")
    }

    func testAStalledDownloadEndsInsteadOfHanging() async {
        let assets = FakeAssets()
        assets.downloads = [.speech: .hangs, .dictation: .hangs]
        let started = Date()
        do {
            _ = try await run(assets)
            XCTFail("nothing installed")
        } catch {
            XCTAssertEqual((error as? SpeechModelUnavailable)?.reason, .downloadFailed)
        }
        XCTAssertLessThan(Date().timeIntervalSince(started), 5, "two 0.3 s stalls, not two 60 s downloads")
        XCTAssertEqual(assets.downloaded, [.speech, .dictation])
    }

    func testAStalledSpeechTranscriberDownloadStillTriesDictationTranscriber() async throws {
        let assets = FakeAssets()
        assets.downloads = [.speech: .hangs]
        let installed = try await run(assets)
        XCTAssertEqual(installed.module, .dictation)
    }

    func testTheKeyboardHandsAModelItCantGetToHexInsteadOfSayingTheLanguageIsMissing() {
        // English released or removed since Hex downloaded it, and the keyboard's own install
        // failing: Apple has English, so the keyboard sends people to Hex to download it.
        let cases: [Error] = [
            SpeechModelUnavailable(language: "en-US", reason: .notOnThisIPhone),
            SpeechModelUnavailable(language: "en-US", reason: .downloadFailed),
            StallWatch.Stalled(),
            NSError(domain: "SFSpeechErrorDomain", code: 13, userInfo: [NSLocalizedDescriptionKey: "Too many asset locales allocated"]),
        ]
        for error in cases {
            settings.clearKeyboardNeedsHex()
            // One failure is no evidence: the keyboard keeps dictating itself.
            let first = SpeechModelNeedsHex.keyboardError(for: error, language: "en-US", settings: settings)
            XCTAssertEqual(UserFacingError.message(for: first, context: .transcription), "Hex couldn't transcribe that. Try again.")
            XCTAssertFalse(settings.keyboardNeedsHex(), "\(error)")
            // The second in a row hands its dictations to Hex.
            let shown = SpeechModelNeedsHex.keyboardError(for: error, language: "en-US", settings: settings)
            XCTAssertEqual(shown as? SpeechModelNeedsHex, SpeechModelNeedsHex(language: "en-US"), "\(error)")
            XCTAssertEqual(UserFacingError.message(for: shown, context: .transcription), "Open Hex to download English.")
            XCTAssertTrue(settings.keyboardNeedsHex(), "the Hex key opens Hex, which downloads it")
        }
        // Hebrew picked by hand on an English iPhone: the keyboard's install failing names
        // English to try, but Hex set nothing aside, so Hex downloads Hebrew too.
        let hebrew = SpeechModelUnavailable(language: "he-IL", reason: .notOnThisIPhone, fallback: "en-US", automatic: false)
        settings.clearKeyboardNeedsHex()
        let first = SpeechModelNeedsHex.keyboardError(for: hebrew, language: "he-IL", settings: settings)
        XCTAssertEqual(UserFacingError.message(for: first, context: .transcription), "Hex couldn't transcribe that. Try again.")
        let shown = SpeechModelNeedsHex.keyboardError(for: hebrew, language: "he-IL", settings: settings)
        XCTAssertEqual(UserFacingError.message(for: shown, context: .transcription), "Open Hex to download Hebrew.")
        XCTAssertTrue(settings.keyboardNeedsHex())
    }

    func testAKeyboardDictationThatWorksBetweenTwoFailuresKeepsTheKeyboardDictating() async throws {
        let failure = StallWatch.Stalled()
        _ = SpeechModelNeedsHex.keyboardError(for: failure, language: "en-US", settings: settings)
        let installed = FakeAssets()
        installed.status[.speech] = .installed
        _ = try await run(installed)
        _ = SpeechModelNeedsHex.keyboardError(for: failure, language: "en-US", settings: settings)
        XCTAssertFalse(settings.keyboardNeedsHex(), "not in a row")

        // A first failure hours ago no longer counts.
        let marked = Date(timeIntervalSince1970: 1_000_000)
        settings.clearKeyboardNeedsHex()
        XCTAssertFalse(settings.noteKeyboardModelFailure(at: marked))
        XCTAssertFalse(settings.noteKeyboardModelFailure(at: marked.addingTimeInterval(LanguageSettings.keyboardStrikeWindow + 1)))
        XCTAssertTrue(settings.noteKeyboardModelFailure(at: marked.addingTimeInterval(LanguageSettings.keyboardStrikeWindow + 60)))
    }

    /// Only a language Hex itself set aside keeps the sentence naming another one to try.
    func testTheKeyboardKeepsAMoveToAnotherLanguageAndACancel() {
        let switched = SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US")
        let offer = SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US", automatic: false)
        // Hex's own installs of Russian failed twice in a row.
        settings.noteUnavailable("ru-RU")
        settings.noteUnavailable("ru-RU")
        XCTAssertTrue(settings.isUnavailable("ru-RU"))
        for error in [switched, offer] {
            let shown = SpeechModelNeedsHex.keyboardError(for: error, language: "ru-RU", settings: settings)
            XCTAssertEqual(shown as? SpeechModelUnavailable, error, "the next dictation works without Hex")
        }
        XCTAssertTrue(SpeechModelNeedsHex.keyboardError(for: CancellationError(), language: "en-US", settings: settings) is CancellationError)
        XCTAssertFalse(settings.keyboardNeedsHex())
    }

    func testAModelInPlaceEndsTheKeyboardsHandoffAndAFailureKeepsIt() async throws {
        settings.markKeyboardNeedsHex()
        let failing = FakeAssets()
        failing.downloads = [.speech: .fails, .dictation: .fails]
        _ = try? await run(failing)
        XCTAssertTrue(settings.keyboardNeedsHex())

        _ = try await run(FakeAssets())
        XCTAssertFalse(settings.keyboardNeedsHex(), "downloaded, by Hex or the keyboard")

        settings.markKeyboardNeedsHex()
        let installed = FakeAssets()
        installed.status[.speech] = .installed
        _ = try await run(installed)
        XCTAssertFalse(settings.keyboardNeedsHex(), "already installed")
    }

    func testTheKeyboardsHandoffEndsAfterADayOrWhenALanguageIsChosen() {
        let marked = Date(timeIntervalSince1970: 1_000_000)
        settings.markKeyboardNeedsHex(at: marked)
        XCTAssertTrue(settings.keyboardNeedsHex(now: marked.addingTimeInterval(3_600)))
        XCTAssertFalse(settings.keyboardNeedsHex(now: marked.addingTimeInterval(LanguageSettings.keyboardNeedsHexDuration + 1)))
        settings.markKeyboardNeedsHex()
        settings.chooseDictationLanguage("es-ES")
        XCTAssertFalse(settings.keyboardNeedsHex(), "another language is the keyboard's to try")
    }
}

final class StallWatchTests: XCTestCase {
    func testAMovingOperationFinishes() async throws {
        let fraction = ProgressReading()
        let value = try await StallWatch.run(limit: .milliseconds(200), poll: .milliseconds(20), progress: { fraction.value }) {
            for step in 1...10 {
                try await Task.sleep(for: .milliseconds(50))
                fraction.value = Double(step) / 10
            }
            return "done"
        }
        XCTAssertEqual(value, "done")
    }

    func testAnOperationThatStopsMovingIsCancelled() async {
        let cancelled = ProgressReading()
        let started = Date()
        do {
            _ = try await StallWatch.run(limit: .milliseconds(200), poll: .milliseconds(20), progress: { 0.5 }) {
                do {
                    try await Task.sleep(for: .seconds(30))
                } catch {
                    cancelled.value = 1
                    throw error
                }
            }
            XCTFail("stalled")
        } catch {
            XCTAssertTrue(error is StallWatch.Stalled)
        }
        XCTAssertLessThan(Date().timeIntervalSince(started), 2)
        try? await Task.sleep(for: .milliseconds(100))
        XCTAssertEqual(cancelled.value, 1, "the operation was told to stop")
    }

    func testANilReadingPausesTheWatch() async throws {
        // Core ML's compile reports nothing for longer than the limit; it isn't a stall.
        let value = try await StallWatch.run(limit: .milliseconds(100), poll: .milliseconds(20), progress: { nil }) {
            try await Task.sleep(for: .milliseconds(400))
            return 7
        }
        XCTAssertEqual(value, 7)
    }

    func testCancellingTheCallerEndsTheWait() async {
        let task = Task {
            try await StallWatch.run(limit: .seconds(30), progress: { nil }) {
                try await Task.sleep(for: .seconds(30))
            }
        }
        try? await Task.sleep(for: .milliseconds(100))
        task.cancel()
        let result = await task.result
        if case .success = result { XCTFail("cancelled") }
    }
}

/// Real Apple Speech on real clips, through the app's `AppleSpeechTranscriber`. Skipped unless
/// `HEX_SPEECH_CLIPS` names a folder of `<language>.wav` files (`nb-NO.wav`, made with
/// `say -v Nora`); it downloads each language's model, so it needs the network. It skips the
/// Speech Recognition prompt, which a test can't answer (simctl can't grant it either).
final class AppleSpeechClipTests: XCTestCase {
    func testTranscribesEachClipInItsLanguage() async throws {
        guard let folder = ProcessInfo.processInfo.environment["HEX_SPEECH_CLIPS"] else {
            throw XCTSkip("Set HEX_SPEECH_CLIPS to a folder of <language>.wav clips")
        }
        guard #available(iOS 26.0, *) else { throw XCTSkip("Apple Speech needs iOS 26") }
        let clips = try FileManager.default.contentsOfDirectory(atPath: folder)
            .filter { $0.hasSuffix(".wav") }
            .sorted()
        XCTAssertFalse(clips.isEmpty)
        // Run it with Speech Recognition turned off too (`simctl privacy <udid> revoke
        // speech-recognition`): Apple Speech never needs it.
        let access = switch SFSpeechRecognizer.authorizationStatus() {
        case .authorized: "allowed"
        case .denied: "denied"
        case .restricted: "restricted"
        case .notDetermined: "not asked"
        @unknown default: "unknown"
        }
        var report = ["Speech Recognition access: \(access)"]
        for clip in clips {
            let identifier = String(clip.dropLast(4))
            let suite = "hex.tests.\(UUID().uuidString)"
            let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
            defer { defaults.removePersistentDomain(forName: suite) }
            let settings = LanguageSettings(defaults: defaults)
            settings.dictationLanguage = identifier
            guard let language = await AppleSpeechLanguages.resolve(settings: settings),
                  Locale(identifier: language.identifier).language.languageCode
                    == Locale(identifier: identifier).language.languageCode else {
                report.append("\(identifier): unsupported by SpeechTranscriber and DictationTranscriber")
                continue
            }
            let transcriber = AppleSpeechTranscriber(language: language)
            do {
                try await transcriber.prepare { _ in }
                let text = try await transcriber.transcribe(URL(fileURLWithPath: folder).appendingPathComponent(clip))
                report.append("\(identifier) [\(language.module.rawValue) \(language.identifier)]: \(text)")
            } catch {
                report.append("\(identifier) [\(language.module.rawValue) \(language.identifier)]: FAILED \(error.localizedDescription)")
            }
        }
        let joined = report.joined(separator: "\n")
        print("HEX-SPEECH-CLIPS\n\(joined)\nHEX-SPEECH-CLIPS-END")
        try? joined.write(toFile: (folder as NSString).appendingPathComponent("transcripts.txt"), atomically: true, encoding: .utf8)
    }
}

private final class FakeAudioSession: AudioSessionControlling {
    enum Call: Equatable {
        case category(AVAudioSession.Category, AVAudioSession.Mode, AVAudioSession.CategoryOptions)
        case preferredInput(AVAudioSession.Port?)
        case active(Bool, AVAudioSession.SetActiveOptions)
    }

    var calls: [Call] = []
    var isOtherAudioPlaying = false
    private(set) var category = AVAudioSession.Category.soloAmbient
    private(set) var mode = AVAudioSession.Mode.default
    private(set) var categoryOptions: AVAudioSession.CategoryOptions = []
    /// What iOS offers: Bluetooth hands-free inputs only in a category that allows them.
    var connected: [Microphone] = []
    private var allowsBluetooth = false

    var microphones: [Microphone] {
        connected.filter { allowsBluetooth || ($0.port != .bluetoothHFP && $0.port != .bluetoothLE) }
    }

    func setCategory(
        _ category: AVAudioSession.Category,
        mode: AVAudioSession.Mode,
        options: AVAudioSession.CategoryOptions
    ) throws {
        calls.append(.category(category, mode, options))
        self.category = category
        self.mode = mode
        // iOS adds the active mode's implicit options: `.voiceChat` records
        // Bluetooth hands-free itself, whether or not it was asked for.
        let implicit: AVAudioSession.CategoryOptions = mode == .voiceChat ? [.allowBluetoothHFP] : []
        categoryOptions = options.union(implicit)
        allowsBluetooth = categoryOptions.contains(.allowBluetoothHFP)
    }

    func prefer(_ microphone: Microphone?) throws {
        calls.append(.preferredInput(microphone?.port))
    }

    func setActive(_ active: Bool, options: AVAudioSession.SetActiveOptions) throws {
        calls.append(.active(active, options))
    }
}

private let iPhoneMic = Microphone(uid: "Built-In Microphone", name: "iPhone Microphone", port: .builtInMic)
private let airPods = Microphone(uid: "A0:B1:C2:D3:E4:F5-tsco", name: "AirPods Pro", port: .bluetoothHFP)
private let bluetoothCar = Microphone(uid: "10:20:30:40:50:60-tsco", name: "My Car", port: .bluetoothHFP)
private let carPlay = Microphone(uid: "CarPlay-mic", name: "CarPlay", port: .carAudio)
private let wiredHeadset = Microphone(uid: "Wired Headset Microphone", name: "Headset Microphone", port: .headsetMic)

@MainActor
final class AudioSessionSetupTests: XCTestCase {
    /// TestFlight build 129 (iOS 27.3): a warm session's dictation began in the background
    /// with the category set again, unchanged, and iOS 27 refused it ("Another app is using
    /// audio"). The category is set only when it changes; a choice that needs another one
    /// (Bluetooth) still sets it.
    func testTheCategoryIsSetOnlyWhenItChanges() throws {
        let session = FakeAudioSession()
        let audio = SystemAudioManager(session: session)
        try audio.configureAudioSession(recordingFrom: nil)
        try audio.configureAudioSession(recordingFrom: .automatic)
        let categoryCalls = session.calls.filter { if case .category = $0 { true } else { false } }
        XCTAssertEqual(categoryCalls.count, 1, "the keepalive's category serves the dictation")
        try audio.configureAudioSession(recordingFrom: .bluetooth)
        XCTAssertEqual(session.calls.filter { if case .category = $0 { true } else { false } }.count, 2)
    }

    /// Experiments › Voice isolation: on, Hex records in `.voiceChat` mode, whose voice
    /// processing lets Control Center's Mic Mode offer Voice Isolation; off, `.measurement`
    /// exactly as before. The options stay today's either way, and the category (with the
    /// mode) is still set only when it changes, even though `.voiceChat` adds
    /// `.allowBluetoothHFP` itself.
    func testVoiceIsolationRecordsInVoiceChatModeAndOffKeepsMeasurement() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = VoiceIsolationSettings(defaults: defaults)

        let offSession = FakeAudioSession()
        try SystemAudioManager(session: offSession, voiceIsolation: settings)
            .configureAudioSession(recordingFrom: .automatic)
        XCTAssertEqual(offSession.mode, .measurement, "off keeps today's mode")
        XCTAssertEqual(
            offSession.calls.first,
            .category(.playAndRecord, .measurement, [.mixWithOthers, .allowBluetoothA2DP, .defaultToSpeaker])
        )

        settings.isEnabled = true
        let onSession = FakeAudioSession()
        let audio = SystemAudioManager(session: onSession, voiceIsolation: settings)
        try audio.configureAudioSession(recordingFrom: .automatic)
        XCTAssertEqual(onSession.mode, .voiceChat)
        guard case .category(.playAndRecord, .voiceChat, let options)? = onSession.calls.first else {
            return XCTFail("the category and mode are set together")
        }
        XCTAssertTrue(options.contains(.mixWithOthers), "the experiment keeps today's options")
        XCTAssertFalse(options.contains(.duckOthers))
        XCTAssertTrue(
            onSession.categoryOptions.contains(.allowBluetoothHFP),
            "iOS adds hands-free Bluetooth itself in .voiceChat"
        )

        try audio.configureAudioSession(recordingFrom: nil)
        XCTAssertEqual(
            onSession.calls.filter { if case .category = $0 { true } else { false } }.count,
            1,
            "the implicit option doesn't set the category again while the mode stays the same"
        )
    }

    func testRecordingMixesWithOtherAppsAudioWhateverTheMicrophone() throws {
        let choices: [MicrophoneChoice?] = [nil] + MicrophoneChoice.allCases.map { $0 }
        for choice in choices {
            let session = FakeAudioSession()
            let audio = SystemAudioManager(session: session)
            try audio.configureAudioSession(recordingFrom: choice)

            guard case .category(let category, let mode, let options)? = session.calls.first else {
                return XCTFail("the category is set first")
            }
            XCTAssertEqual(category, .playAndRecord)
            XCTAssertEqual(mode, .measurement)
            XCTAssertTrue(options.contains(.mixWithOthers), "never interrupts another app's video or music")
            XCTAssertFalse(options.contains(.duckOthers))
            XCTAssertTrue(options.contains(.allowBluetoothA2DP))
            XCTAssertEqual(
                options.contains(.allowBluetoothHFP), choice == .bluetooth,
                "Bluetooth hands-free microphones only for a dictation with Bluetooth chosen, never the keepalive"
            )
            XCTAssertEqual(session.calls[1], .active(true, []))
        }
    }

    /// Ryan's car: in CarPlay, Automatic (the default) records from the car's microphone, not
    /// the iPhone in its cupholder hearing the speakers. AirPods keep the iPhone's microphone
    /// (music stays at full quality, nothing waits for call audio); Bluetooth takes theirs.
    func testEachChoiceRecordsFromItsMicrophoneForTheRoute() {
        let routes: [(String, [Microphone], [MicrophoneChoice: Microphone])] = [
            ("nothing connected", [iPhoneMic], [.automatic: iPhoneMic, .iPhone: iPhoneMic, .bluetooth: iPhoneMic]),
            ("CarPlay", [iPhoneMic, carPlay], [.automatic: carPlay, .iPhone: iPhoneMic, .bluetooth: carPlay]),
            ("AirPods", [iPhoneMic, airPods], [.automatic: iPhoneMic, .iPhone: iPhoneMic, .bluetooth: airPods]),
            ("a Bluetooth car kit", [iPhoneMic, bluetoothCar], [.automatic: iPhoneMic, .iPhone: iPhoneMic, .bluetooth: bluetoothCar]),
            ("a wired headset", [iPhoneMic, wiredHeadset], [.automatic: wiredHeadset, .iPhone: iPhoneMic, .bluetooth: wiredHeadset]),
        ]
        for (route, inputs, expected) in routes {
            for (choice, input) in expected {
                XCTAssertEqual(MicrophoneRoute.capture(choice, from: inputs), input, "\(choice) with \(route)")
            }
            XCTAssertEqual(MicrophoneRoute.keepalive(from: inputs), iPhoneMic, "the keepalive with \(route)")
        }
    }

    func testTheSessionPrefersTheChosenInputAndTheKeepaliveTheIPhones() throws {
        let cases: [(MicrophoneChoice?, [Microphone], AVAudioSession.Port)] = [
            (.automatic, [iPhoneMic, carPlay], .carAudio),
            (.iPhone, [iPhoneMic, carPlay], .builtInMic),
            (.bluetooth, [iPhoneMic, airPods], .bluetoothHFP),
            (.automatic, [iPhoneMic, airPods], .builtInMic),
            // A warm session between dictations never holds the car or headset in call audio.
            (nil, [iPhoneMic, carPlay], .builtInMic),
            (nil, [iPhoneMic, airPods], .builtInMic),
        ]
        for (choice, connected, port) in cases {
            let session = FakeAudioSession()
            session.connected = connected
            try SystemAudioManager(session: session).configureAudioSession(recordingFrom: choice)
            XCTAssertEqual(session.calls.last, .preferredInput(port), "\(String(describing: choice)) with \(connected.map(\.name))")
        }
    }

    func testTheMicrophoneChoiceDefaultsToAutomaticAndKeepsAnOldBluetoothSwitch() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let bridge = KeyboardBridge(defaults: defaults)
        XCTAssertEqual(bridge.microphone, .automatic, "Always use iPhone microphone left on, its default")

        defaults.set(true, forKey: "settings.allowBluetoothMic")
        XCTAssertEqual(bridge.microphone, .bluetooth, "Always use iPhone microphone turned off")

        bridge.microphone = .iPhone
        XCTAssertEqual(KeyboardBridge(defaults: defaults).microphone, .iPhone)
    }

    /// `.playAndRecord` runs the microphone and the output at one rate: a preferred 16 kHz
    /// moved everything the iPhone played and recorded, a screen recording's audio too, to
    /// 16 kHz while Hex held the session (the simulator went from 48 kHz to 16 kHz).
    func testRecordingLeavesTheHardwareAtItsOwnSampleRate() throws {
        let session = FakeAudioSession()
        try SystemAudioManager(session: session).configureAudioSession(recordingFrom: .automatic)
        XCTAssertEqual(session.calls, [
            .category(.playAndRecord, .measurement, [.mixWithOthers, .allowBluetoothA2DP, .defaultToSpeaker]),
            .active(true, []),
            .preferredInput(nil),
        ], "no preferred sample rate")

        let shared = AVAudioSession.sharedInstance()
        let before = shared.sampleRate
        do {
            try SystemAudioManager().configureAudioSession(recordingFrom: .automatic)
        } catch {
            throw XCTSkip("No audio session to activate here: \(error)")
        }
        defer { try? shared.setActive(false, options: .notifyOthersOnDeactivation) }
        XCTAssertEqual(shared.preferredSampleRate, 0)
        XCTAssertEqual(shared.sampleRate, before, "the session records at the rate the hardware already ran")
    }

    func testEndingTheSessionTellsInterruptedAppsTheyMayResume() {
        let session = FakeAudioSession()
        let audio = SystemAudioManager(session: session)
        audio.deactivateSession()
        XCTAssertEqual(session.calls, [.active(false, .notifyOthersOnDeactivation)])
        XCTAssertFalse(audio.isKeepaliveRunning)
    }

    func testTheKeyboardAsksAudioItsCaptureStoppedToResume() throws {
        XCTAssertTrue(InterruptedAudio.shouldAskToResume(playingAtStart: true, playingNow: false))
        XCTAssertFalse(InterruptedAudio.shouldAskToResume(playingAtStart: true, playingNow: true), "audio that mixed kept playing")
        XCTAssertFalse(InterruptedAudio.shouldAskToResume(playingAtStart: false, playingNow: false), "nothing was playing")

        let session = FakeAudioSession()
        try InterruptedAudio.askToResume(session)
        XCTAssertEqual(session.calls, [
            .category(.playback, .default, []),
            .active(true, []),
            .active(false, .notifyOthersOnDeactivation),
        ])
    }
}

final class VoiceIsolationSettingsTests: XCTestCase {
    /// Off by default, and turned on in the shared App Group so the keyboard reads it too.
    func testVoiceIsolationIsOffByDefaultAndPersists() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        XCTAssertFalse(VoiceIsolationSettings(defaults: defaults).isEnabled)
        XCTAssertNil(defaults.object(forKey: "experiments.voiceIsolation"), "reading writes nothing")

        VoiceIsolationSettings(defaults: defaults).isEnabled = true
        XCTAssertEqual(defaults.object(forKey: "experiments.voiceIsolation") as? Bool, true)
        XCTAssertTrue(VoiceIsolationSettings(defaults: defaults).isEnabled)
    }

    func testTheLaunchArgumentTurnsVoiceIsolationOn() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        let settings = VoiceIsolationSettings(defaults: defaults)
        settings.applyLaunchArguments([VoiceIsolationSettings.launchArgument])
        XCTAssertTrue(settings.isEnabled)
    }
}

/// Counts Speech Recognition requests and answers each with `answer`.
private final class CountingSpeechAccess: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    private var status: SFSpeechRecognizerAuthorizationStatus = .notDetermined
    let answer: SFSpeechRecognizerAuthorizationStatus

    init(answer: SFSpeechRecognizerAuthorizationStatus) {
        self.answer = answer
    }

    var requests: Int { lock.withLock { count } }

    var access: SpeechRecognitionAccess {
        SpeechRecognitionAccess(
            status: { self.lock.withLock { self.status } },
            request: {
                self.lock.withLock {
                    self.count += 1
                    self.status = self.answer
                    return self.answer
                }
            }
        )
    }
}

private final class DetailLog: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [String] = []

    func append(_ value: String) { lock.withLock { stored.append(value) } }
    var values: [String] { lock.withLock { stored } }
}
