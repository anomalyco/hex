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

    var onSystemEvent: ((AudioSystemEvent) -> Void)?
    var onUnexpectedCaptureEnd: ((URL, Bool) -> Void)?

    func requestPermission() async -> Bool {
        permissionGranted
    }

    func startKeepalive(allowBluetooth: Bool) throws {
        if keepaliveShouldThrow {
            isKeepaliveRunning = false
            throw keepaliveError
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
        allowBluetooth: Bool
    ) throws -> any ActiveAudioCapture {
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

private actor ControlledTranscriber: LocalTranscribing {
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
    nonisolated let displayName = "Apple Speech"
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
            learnedDictationCorrections: { learned }
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
            learnedDictationCorrections: { [DictationCorrection(heard: "ship", meant: "send")] }
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
            "Parakeet V2 is not ready."
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
            "Parakeet V2 didn't download. Check your connection and try again."
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
        XCTAssertFalse(AppleSpeechTranscriber.shouldUse(dictation, parakeetFits: true), "Parakeet where it fits, as before")
        XCTAssertTrue(AppleSpeechTranscriber.shouldUse(dictation, parakeetFits: false), "an iPhone 11 has 4 GB")
        let speech = SpeechLanguage(locale: Locale(identifier: "en-US"), module: .speech)
        XCTAssertTrue(AppleSpeechTranscriber.shouldUse(speech, parakeetFits: true))
    }

    func testAParakeetLoadThatEndedHexIsNotRepeatedAtLaunch() {
        let loads = ParakeetLoadGuard(defaults: defaults)
        loads.noteLaunch()
        XCTAssertFalse(loads.lastLoadWasInterrupted)

        loads.begin()
        // iOS ends Hex while Core ML compiles; the next launch finds the load still marked.
        let nextLaunch = ParakeetLoadGuard(defaults: defaults)
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
        XCTAssertEqual(controller.phase, .failed("Parakeet V2 is not ready."))
        XCTAssertEqual(bridge.snapshot().message, "Parakeet V2 is not ready.")
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

    private func settle() async throws {
        for _ in 0..<8 {
            try await Task.sleep(for: .milliseconds(25))
        }
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
        speechTranscriber: Bool = true
    ) async -> SpeechLanguage? {
        await SpeechLanguagePicker.pick(
            explicit: explicit,
            candidates: SpeechLanguagePicker.automaticCandidates(preferred: preferred, current: current),
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
        // picked nothing: "Apple Speech doesn't have your language on this iPhone." It tries
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
        var downloaded: [SpeechModuleKind] = []

        var steps: SpeechModelInstall.Steps {
            SpeechModelInstall.Steps(
                dictationEquivalent: { [self] locale in dictationHas ? locale : nil },
                status: { [self] language in lock.withLock { status[language.module] ?? .unsupported } },
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

    func testASpeechTranscriberAssetThatNeverArrivesFallsBackToDictationTranscriber() async throws {
        let assets = FakeAssets()
        assets.downloads[.speech] = .fails
        let installed = try await run(assets)
        XCTAssertEqual(installed.module, .dictation)
        XCTAssertEqual(installed.identifier, "ru-RU")
        XCTAssertEqual(assets.downloaded, [.speech, .dictation])
        XCTAssertTrue(settings.speechModuleFailed("ru-RU"), "the next dictation goes straight to DictationTranscriber")
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
            let shown = SpeechModelNeedsHex.keyboardError(for: error, language: "en-US", settings: settings)
            XCTAssertEqual(shown as? SpeechModelNeedsHex, SpeechModelNeedsHex(language: "en-US"), "\(error)")
            XCTAssertEqual(UserFacingError.message(for: shown, context: .transcription), "Open Hex to download English.")
            XCTAssertTrue(settings.keyboardNeedsHex(), "the Hex key opens Hex, which downloads it")
        }
    }

    func testTheKeyboardKeepsAMoveToAnotherLanguageAndACancel() {
        let switched = SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US")
        let offer = SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US", automatic: false)
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
        case preferredInput
        case active(Bool, AVAudioSession.SetActiveOptions)
    }

    var calls: [Call] = []
    var isOtherAudioPlaying = false
    var availableInputs: [AVAudioSessionPortDescription]? { nil }

    func setCategory(
        _ category: AVAudioSession.Category,
        mode: AVAudioSession.Mode,
        options: AVAudioSession.CategoryOptions
    ) throws {
        calls.append(.category(category, mode, options))
    }

    func setPreferredInput(_ inPort: AVAudioSessionPortDescription?) throws {
        calls.append(.preferredInput)
    }

    func setActive(_ active: Bool, options: AVAudioSession.SetActiveOptions) throws {
        calls.append(.active(active, options))
    }
}

@MainActor
final class AudioSessionSetupTests: XCTestCase {
    func testRecordingMixesWithOtherAppsAudioWithOrWithoutTheBluetoothMicrophone() throws {
        for allowBluetooth in [false, true] {
            let session = FakeAudioSession()
            let audio = SystemAudioManager(session: session)
            try audio.configureAudioSession(allowBluetooth: allowBluetooth)

            guard case .category(let category, let mode, let options)? = session.calls.first else {
                return XCTFail("the category is set first")
            }
            XCTAssertEqual(category, .playAndRecord)
            XCTAssertEqual(mode, .measurement)
            XCTAssertTrue(options.contains(.mixWithOthers), "never interrupts another app's video or music")
            XCTAssertFalse(options.contains(.duckOthers))
            XCTAssertTrue(options.contains(.allowBluetoothA2DP))
            XCTAssertEqual(options.contains(.allowBluetoothHFP), allowBluetooth, "the Bluetooth microphone only when allowed")
            XCTAssertEqual(session.calls.last, .active(true, []))
        }
    }

    /// `.playAndRecord` runs the microphone and the output at one rate: a preferred 16 kHz
    /// moved everything the iPhone played and recorded, a screen recording's audio too, to
    /// 16 kHz while Hex held the session (the simulator went from 48 kHz to 16 kHz).
    func testRecordingLeavesTheHardwareAtItsOwnSampleRate() throws {
        let session = FakeAudioSession()
        try SystemAudioManager(session: session).configureAudioSession(allowBluetooth: false)
        XCTAssertEqual(session.calls, [
            .category(.playAndRecord, .measurement, [.mixWithOthers, .allowBluetoothA2DP, .defaultToSpeaker]),
            .active(true, []),
        ], "no preferred sample rate")

        let shared = AVAudioSession.sharedInstance()
        let before = shared.sampleRate
        do {
            try SystemAudioManager().configureAudioSession(allowBluetooth: false)
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
