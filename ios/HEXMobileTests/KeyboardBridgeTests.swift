import XCTest
@testable import HEX

@MainActor
final class KeyboardBridgeTests: XCTestCase {
    func testSnapshotAvailabilityRequiresFreshHeartbeatAndActiveSession() {
        let now = 1_000.0
        let available = snapshot(state: .ready, heartbeat: now - 2.9, expiresAt: now + 1)
        let stale = snapshot(state: .ready, heartbeat: now - 3.0, expiresAt: now + 1)
        let expired = snapshot(state: .ready, heartbeat: now, expiresAt: now)
        let transcribingAfterSessionEnd = snapshot(state: .transcribing, heartbeat: now - 0.5, expiresAt: 0)

        XCTAssertTrue(available.isAvailable(at: now))
        XCTAssertFalse(stale.isAvailable(at: now))
        XCTAssertFalse(expired.isAvailable(at: now))
        XCTAssertTrue(transcribingAfterSessionEnd.isAvailable(at: now))
    }

    func testDeliverableResultSurvivesWarmSessionShutdownUntilTTL() {
        let now = 5_000.0
        let closedWithResult = KeyboardSnapshot(
            state: .offline,
            heartbeat: 0,
            expiresAt: 0,
            resultID: "res-1",
            resultExpiresAt: now + 25,
            transcript: "Ship it."
        )
        XCTAssertFalse(closedWithResult.isAvailable(at: now))
        XCTAssertTrue(closedWithResult.hasDeliverableResult(at: now))
        XCTAssertFalse(closedWithResult.hasDeliverableResult(at: now + 26))
    }

    /// TestFlight build 56: dictate in the keyboard, dismiss it, dictate again (Hex opens and
    /// records), swipe back. Hex ended mid-job and its last snapshot still said recording, so
    /// every keyboard after showed a recording nothing would finish: Done and Cancel went to
    /// no one, and no text came until Hex was opened again.
    func testAJobHexStoppedHeartbeatingForNoLongerHoldsTheKeyboard() {
        let t0 = 10_000.0
        let recording = KeyboardSnapshot(
            state: .recording,
            heartbeat: t0,
            expiresAt: t0 + 900,
            recordingStartedAt: t0 - 3,
            audioLevel: 0.4,
            jobID: "handoff"
        )
        let model = KeyboardViewModel()
        func show(_ snapshot: KeyboardSnapshot, at now: TimeInterval, on model: KeyboardViewModel) {
            model.applyBridgeState(
                hasFullAccess: true,
                snapshot: snapshot,
                isModelInstalled: true,
                style: .polished,
                now: now,
                animated: false
            )
        }

        show(recording, at: t0, on: model)
        XCTAssertEqual(model.stagePhase, .recording)
        show(recording, at: t0 + 5, on: model)
        XCTAssertEqual(model.stagePhase, .recording, "a late heartbeat isn't a Hex that ended")

        // Done, sent to a Hex that will never read it.
        model.setPendingCommand(
            KeyboardViewModel.PendingCommand(kind: .stop, jobID: "handoff", sentAt: t0 + 6),
            now: t0 + 6,
            animated: false
        )
        XCTAssertEqual(model.stagePhase, .transcribing)
        show(recording, at: t0 + 7, on: model)
        XCTAssertEqual(model.stagePhase, .idle, "the job is over: the Hex key dictates again")
        XCTAssertNil(model.pendingCommand)
        XCTAssertNil(model.currentSnapshot.jobID)
        XCTAssertFalse(model.isWarmAvailable)

        // A keyboard opened later, in this process or a new one, reads the same snapshot the same way.
        let later = KeyboardViewModel()
        show(recording, at: t0 + 600, on: later)
        XCTAssertEqual(later.stagePhase, .idle)
        let transcribing = KeyboardSnapshot(state: .transcribing, heartbeat: t0, expiresAt: 0, jobID: "handoff")
        show(transcribing, at: t0 + 2, on: later)
        XCTAssertEqual(later.stagePhase, .transcribing)
        show(transcribing, at: t0 + 7, on: later)
        XCTAssertEqual(later.stagePhase, .idle)
        XCTAssertTrue(
            KeyboardSnapshot(state: .recording, heartbeat: 0, expiresAt: 0, jobID: "handoff").isOrphanedJob(at: t0),
            "never heartbeated"
        )

        // A result the snapshot carries still arrives, and a Hex heartbeating again is live again.
        let finished = KeyboardSnapshot(
            state: .transcribing,
            heartbeat: t0,
            expiresAt: 0,
            jobID: "handoff",
            resultID: "result",
            resultExpiresAt: t0 + 30,
            transcript: "Ship it."
        )
        XCTAssertTrue(finished.droppingOrphanedJob(at: t0 + 7).hasDeliverableResult(at: t0 + 7))
        let back = KeyboardSnapshot(state: .recording, heartbeat: t0 + 600, expiresAt: t0 + 900, jobID: "next")
        show(back, at: t0 + 600, on: later)
        XCTAssertEqual(later.stagePhase, .recording)
    }

    /// TestFlight: "The another app is using audio message seems inaccurate cause it's showing
    /// for me all the time although it's working fine", and "telling me another app is using
    /// the microphone". Hex writes its snapshot only while it runs, so a failure stayed in the
    /// App Group: every keyboard after showed it, and the full keyboard's Hex key sent its
    /// dictations to Hex's warm session, which ignored them while marked failed. A message now
    /// shows for a few seconds after Hex wrote it; one with no time (an earlier build's) never.
    func testAFailureHexWroteAWhileAgoIsOver() {
        let t0 = 20_000.0
        let busy = "Another app is using the microphone. Try again."
        func show(_ snapshot: KeyboardSnapshot, at now: TimeInterval, on model: KeyboardViewModel) {
            model.applyBridgeState(
                hasFullAccess: true,
                snapshot: snapshot,
                isModelInstalled: true,
                style: .polished,
                now: now,
                animated: false
            )
        }

        // A warm session's dictation failed just now: the keyboard says so and dictates itself.
        let model = KeyboardViewModel()
        show(
            KeyboardSnapshot(state: .failed, heartbeat: t0 + 1, expiresAt: t0 + 900, message: busy, messageAt: t0),
            at: t0 + 1,
            on: model
        )
        XCTAssertEqual(model.stagePhase, .failed)
        XCTAssertEqual(model.statusMessage, busy)
        XCTAssertTrue(model.isWarmAvailable)
        XCTAssertFalse(model.dictatesThroughHex, "the Hex key dictates in the keyboard, as Dictate does")

        // Seconds later, with the session still up, it's over and the Hex key uses the session.
        let later = KeyboardSnapshot(state: .failed, heartbeat: t0 + 9, expiresAt: t0 + 900, message: busy, messageAt: t0)
        XCTAssertTrue(later.hasStaleMessage(at: t0 + 9))
        show(later, at: t0 + 9, on: model)
        XCTAssertEqual(model.stagePhase, .idle)
        XCTAssertNil(model.statusMessage)
        XCTAssertTrue(model.dictatesThroughHex)

        // Hex suspended since, or an earlier build's snapshot with no time: never shown.
        for stale in [
            KeyboardSnapshot(state: .failed, heartbeat: 0, expiresAt: 0, message: busy, messageAt: t0),
            KeyboardSnapshot(state: .failed, heartbeat: 0, expiresAt: 0, message: busy),
        ] {
            let next = KeyboardViewModel()
            show(stale, at: t0 + 3_600, on: next)
            XCTAssertEqual(next.stagePhase, .idle)
            XCTAssertNil(next.statusMessage)
        }

        // A note beside a live session ("Interrupted") goes the same way, the session staying.
        let noted = KeyboardViewModel()
        show(KeyboardSnapshot(state: .ready, heartbeat: t0 + 1, expiresAt: t0 + 900, message: "Interrupted", messageAt: t0), at: t0 + 1, on: noted)
        XCTAssertEqual(noted.statusMessage, "Interrupted")
        show(KeyboardSnapshot(state: .ready, heartbeat: t0 + 9, expiresAt: t0 + 900, message: "Interrupted", messageAt: t0), at: t0 + 9, on: noted)
        XCTAssertNil(noted.statusMessage)
        XCTAssertTrue(noted.dictatesThroughHex)

        // The keyboard's own failure keeps its snapshot live for its 6 s, which is no session:
        // the full keyboard's Hex key tries again in the keyboard instead of sending to no one.
        let own = KeyboardViewModel()
        show(
            KeyboardSnapshot(state: .failed, heartbeat: t0 + 1, expiresAt: t0 + 6, message: busy, messageAt: t0 + 1),
            at: t0 + 1,
            on: own
        )
        XCTAssertEqual(own.stagePhase, .failed)
        XCTAssertTrue(own.isWarmAvailable)
        XCTAssertFalse(own.dictatesThroughHex)

        let encoded = KeyboardSnapshot(state: .failed, heartbeat: t0, expiresAt: 0, message: busy, messageAt: t0)
        XCTAssertEqual(
            try JSONDecoder().decode(KeyboardSnapshot.self, from: JSONEncoder().encode(encoded)).messageAt,
            t0
        )
    }

    /// The same keyboard's own dictation: one still recording or transcribing when the keyboard
    /// was dismissed finishes without it, goes back into its field when the keyboard returns
    /// there while it's fresh, and is offered as Paste dictation anywhere else or later. A
    /// capture that went quiet or a transcription that never returns resets the keyboard.
    func testADictationTheKeyboardFinishedAfterItHidReachesTheNextKeyboard() {
        let suiteName = "hex.tests.heldDictation.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let bridge = KeyboardBridge(defaults: defaults)
        let field = UUID()
        let held = KeyboardHeldDictation(
            id: "held",
            text: "Meet at ten.",
            heard: "meet at ten",
            document: field,
            finishedAt: 1_000
        )

        XCTAssertNil(bridge.heldDictation)
        bridge.heldDictation = held
        XCTAssertEqual(KeyboardBridge(defaults: defaults).heldDictation, held, "a new keyboard process finds it")
        XCTAssertEqual(held.delivery(in: field, at: 1_010), .insert)
        XCTAssertEqual(held.delivery(in: nil, at: 1_010), .insert, "a field UIKit hasn't named yet")
        XCTAssertEqual(held.delivery(in: UUID(), at: 1_010), .offer, "another field")
        XCTAssertEqual(held.delivery(in: field, at: 1_031), .offer, "back after its time to go in by itself")
        XCTAssertEqual(held.delivery(in: field, at: 1_000 + KeyboardSnapshot.resultOfferWindow), .expired)
        bridge.heldDictation = nil
        XCTAssertNil(KeyboardBridge(defaults: defaults).heldDictation)

        XCTAssertFalse(KeyboardJobWatchdog.isStalled(.recording, lastProgress: 100, now: 105), "a capture still starting")
        XCTAssertTrue(KeyboardJobWatchdog.isStalled(.recording, lastProgress: 100, now: 107), "no audio for 7 s")
        XCTAssertFalse(KeyboardJobWatchdog.isStalled(.transcribing, lastProgress: 100, now: 160), "a long dictation")
        XCTAssertTrue(KeyboardJobWatchdog.isStalled(.transcribing, lastProgress: 100, now: 230))
    }

    /// Hex's text that came after its time to go in by itself (Done in Hex, then back late)
    /// is offered as Paste dictation for ten minutes rather than lost.
    func testAResultPastItsTimeToGoInIsOfferedForTenMinutes() {
        let snap = KeyboardSnapshot(
            state: .offline,
            heartbeat: 0,
            expiresAt: 0,
            resultID: "result",
            resultExpiresAt: 5_030,
            transcript: "Ship it."
        )
        XCTAssertTrue(snap.hasDeliverableResult(at: 5_029))
        XCTAssertFalse(snap.hasDeliverableResult(at: 5_031))
        XCTAssertTrue(snap.hasOfferableResult(at: 5_031))
        XCTAssertFalse(snap.hasOfferableResult(at: 5_030 + KeyboardSnapshot.resultOfferWindow))
        XCTAssertFalse(KeyboardSnapshot.offline.hasOfferableResult(at: 5_031))
    }

    func testKeyboardMessagesRoundTripThroughJSON() throws {
        let command = KeyboardCommand(
            id: "command",
            kind: .stop,
            jobID: "job",
            style: .casual,
            sentAt: 42.5
        )
        let snap = snapshot(state: .recording, heartbeat: 10, expiresAt: 20)
        let encoder = JSONEncoder()
        let decoder = JSONDecoder()

        XCTAssertEqual(
            try decoder.decode(KeyboardCommand.self, from: encoder.encode(command)),
            command
        )
        XCTAssertEqual(
            try decoder.decode(KeyboardSnapshot.self, from: encoder.encode(snap)),
            snap
        )
    }

    func testKeyboardRecordingLaunchRoundTripsMetadataAndEnforcesTenSecondWindow() {
        let t0 = 2_000.0
        let request = KeyboardLaunchRequest.startRecording(
            jobID: "job with spaces",
            requestedAt: t0,
            style: .casual
        )

        let decoded = KeyboardLaunchRequest(url: request.url)
        XCTAssertEqual(decoded, request)
        XCTAssertFalse(request.isHandoffExpired(at: t0 + 9.9))
        XCTAssertTrue(request.isHandoffExpired(at: t0 + 10.1))
    }

    func testKeyboardSessionLaunchDoesNotRequestRecording() {
        let request = KeyboardLaunchRequest.startSession(requestedAt: 100)
        XCTAssertEqual(KeyboardLaunchRequest(url: request.url), request)
        // A keyboard from before the return-app setting went away may still send `return=`.
        XCTAssertEqual(
            KeyboardLaunchRequest(url: URL(string: "hex-dictation://keyboard/start?ts=100&return=messages")!),
            request
        )
    }

    /// Hex's result tells the keyboard why it went in as spoken, through the App Group, and a
    /// snapshot from a build without the field still reads.
    func testSnapshotCarriesWhyATranslationWentInAsSpoken() throws {
        let defaults = UserDefaults(suiteName: "hex.tests.\(UUID().uuidString)")!
        let bridge = KeyboardBridge(defaults: defaults)
        let failure = TranslationFailure(
            .failed,
            source: Locale.Language(identifier: "zh-CN"),
            target: Locale.Language(identifier: "en"),
            detail: "TranslationErrorDomain 16"
        )
        bridge.publish(KeyboardSnapshot(
            state: .ready, heartbeat: 1, expiresAt: 2, resultID: "r", transcript: "我们明天见。", translationFailure: failure
        ))
        XCTAssertEqual(bridge.snapshot().translationFailure, failure)

        let old = #"{"state":"ready","heartbeat":1,"expiresAt":2,"audioLevel":0,"resultID":"r","transcript":"hi"}"#
        let decoded = try JSONDecoder().decode(KeyboardSnapshot.self, from: Data(old.utf8))
        XCTAssertNil(decoded.translationFailure)
        XCTAssertEqual(decoded.transcript, "hi")
    }

    func testPasteSettingsLaunchOpensAPageWithoutAHandoff() {
        let request = KeyboardLaunchRequest.pasteSettings
        XCTAssertEqual(request.url.absoluteString, "hex-dictation://keyboard/paste-settings")
        XCTAssertEqual(KeyboardLaunchRequest(url: request.url), request)
        XCTAssertTrue(request.opensPage)
        XCTAssertNil(request.requestedAt)
        XCTAssertFalse(request.isHandoffExpired(at: 1_000_000))
    }

    func testTranslationSettingsLaunchOpensAPageWithoutAHandoff() {
        let request = KeyboardLaunchRequest.translationSettings
        XCTAssertEqual(request.url.absoluteString, "hex-dictation://keyboard/translation")
        XCTAssertEqual(KeyboardLaunchRequest(url: request.url), request)
        XCTAssertTrue(request.opensPage)
        XCTAssertFalse(KeyboardLaunchRequest.startSession(requestedAt: nil).opensPage)
    }

    func testKeyboardSetupLaunchRoundTripsWithoutAHandoff() {
        let request = KeyboardLaunchRequest.setup
        XCTAssertEqual(request.url.absoluteString, "hex-dictation://keyboard/setup")
        XCTAssertEqual(KeyboardLaunchRequest(url: request.url), request)
        XCTAssertFalse(request.isHandoffExpired(at: 1_000_000))
    }

    @MainActor
    func testFullAccessSeenAtIsStoredInTheAppGroup() {
        let suiteName = "hex.tests.fullAccess.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let bridge = KeyboardBridge(defaults: defaults)

        XCTAssertNil(bridge.fullAccessSeenAt)
        bridge.fullAccessSeenAt = 1_234
        XCTAssertEqual(KeyboardBridge(defaults: defaults).fullAccessSeenAt, 1_234)
    }

    @MainActor
    func testFullAccessReportsAreStoredInTheAppGroup() {
        let suiteName = "hex.tests.fullAccessReports.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let bridge = KeyboardBridge(defaults: defaults)

        XCTAssertNil(bridge.fullAccessOnReport)
        XCTAssertNil(bridge.fullAccessOffReport)
        bridge.fullAccessSeenAt = 50
        XCTAssertEqual(
            bridge.fullAccessOnReport,
            KeyboardFullAccessReport(isOn: true, at: 50, launchedAt: 0),
            "an earlier keyboard's word counts, as from the oldest process"
        )

        let on = KeyboardFullAccessReport(isOn: true, at: 100, launchedAt: 90)
        let off = KeyboardFullAccessReport(isOn: false, at: 200, launchedAt: 190)
        bridge.recordFullAccess(on)
        bridge.recordFullAccess(off)
        let reread = KeyboardBridge(defaults: defaults)
        XCTAssertEqual(reread.fullAccessOnReport, on)
        XCTAssertEqual(reread.fullAccessOffReport, off)
        XCTAssertEqual(reread.fullAccessSeenAt, 100, "an on still sets the key earlier builds read")
    }

    func testFullAccessTrustsTheNewestReportFromANewerKeyboard() {
        func report(_ isOn: Bool, at: TimeInterval, launched: TimeInterval) -> KeyboardFullAccessReport {
            KeyboardFullAccessReport(isOn: isOn, at: at, launchedAt: launched)
        }
        let resolve = KeyboardFullAccess.resolve
        XCTAssertEqual(resolve(nil, nil, 0), .unconfirmed)
        XCTAssertEqual(resolve(report(true, at: 10, launched: 5), nil, 1_000), .on, "an on stays on")
        XCTAssertEqual(resolve(nil, report(false, at: 120, launched: 110), 100), .off, "seen off since Hex came back")
        XCTAssertEqual(
            resolve(nil, report(false, at: 80, launched: 70), 100), .unconfirmed,
            "seen off before Hex came back from Settings: Full Access may have been turned on since"
        )
        XCTAssertEqual(
            resolve(report(true, at: 150, launched: 140), report(false, at: 120, launched: 110), 100), .on,
            "the grant is newer"
        )
        XCTAssertEqual(
            resolve(report(true, at: 150, launched: 140), report(false, at: 160, launched: 110), 100), .on,
            "a keyboard started before the grant still says off: stale"
        )
        XCTAssertEqual(
            resolve(report(true, at: 150, launched: 140), report(false, at: 160, launched: 140), 100), .off,
            "the same keyboard saw it turned off"
        )
        XCTAssertEqual(
            resolve(report(true, at: 50, launched: 40), report(false, at: 80, launched: 70), 100), .unconfirmed,
            "turned off after the last on, then Hex went to the background: it may be on again"
        )
    }

    @MainActor
    func testSetupMonitorFollowsTheKeyboardsReports() {
        let suiteName = "hex.tests.setupMonitor.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let bridge = KeyboardBridge(defaults: defaults)
        let monitor = KeyboardSetupMonitor(bridge: bridge, activeSince: 1_000)
        XCTAssertEqual(monitor.fullAccess, .unconfirmed)

        bridge.recordFullAccess(KeyboardFullAccessReport(isOn: false, at: 1_010, launchedAt: 1_005))
        monitor.refresh()
        XCTAssertEqual(monitor.fullAccess, .off, "the keyboard said off while Hex was up")
        monitor.cameToForeground(at: 1_020)
        XCTAssertEqual(monitor.fullAccess, .unconfirmed, "back from Settings: off is no longer known")

        // No refresh call: the keyboard's Darwin notification brings the report in.
        bridge.recordFullAccess(KeyboardFullAccessReport(isOn: true, at: 1_030, launchedAt: 1_025))
        let deadline = Date().addingTimeInterval(3)
        while monitor.fullAccess != .on, Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        }
        XCTAssertEqual(monitor.fullAccess, .on)
        XCTAssertTrue(monitor.fullAccessOn)
    }

    @MainActor
    func testEmptyKeyboardDictationIsNotedInTheAppGroup() {
        let suiteName = "hex.tests.emptyDictation.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let bridge = KeyboardBridge(defaults: defaults)

        XCTAssertNil(bridge.lastEmptyDictationAt)
        bridge.lastEmptyDictationAt = 2_000
        XCTAssertEqual(KeyboardBridge(defaults: defaults).lastEmptyDictationAt, 2_000)
        bridge.lastEmptyDictationAt = nil
        XCTAssertNil(KeyboardBridge(defaults: defaults).lastEmptyDictationAt)
    }

    func testHexKeyboardIsShownFromAppearingUntilItHides() {
        XCTAssertFalse(KeyboardPresence.isShown(shownAt: nil, hiddenAt: nil, heartbeat: nil, now: 1_000), "never shown")
        XCTAssertTrue(KeyboardPresence.isShown(shownAt: 999, hiddenAt: 990, heartbeat: nil, now: 1_000))
        XCTAssertFalse(KeyboardPresence.isShown(shownAt: 990, hiddenAt: 999, heartbeat: nil, now: 1_000), "another keyboard or iOS's dictation took over")
        XCTAssertFalse(KeyboardPresence.isShown(shownAt: 980, hiddenAt: nil, heartbeat: 985, now: 1_000), "no sign of life: the process ended")
        XCTAssertTrue(KeyboardPresence.isShown(shownAt: 900, hiddenAt: 890, heartbeat: 995, now: 1_000), "the Full Access report it repeats while shown")

        let suiteName = "hex.tests.presence.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let bridge = KeyboardBridge(defaults: defaults)
        let now = Date().timeIntervalSince1970
        XCTAssertFalse(bridge.isKeyboardShown(now: now))
        bridge.recordKeyboardShown(at: now - 1)
        XCTAssertTrue(KeyboardBridge(defaults: defaults).isKeyboardShown(now: now))
        bridge.recordKeyboardHidden(at: now)
        XCTAssertFalse(KeyboardBridge(defaults: defaults).isKeyboardShown(now: now))
    }

    func testOnboardingPracticeCountsADictationOrAFewTypedWords() {
        XCTAssertFalse(OnboardingPractice.isDone(text: "S", dictated: false, typedWithHex: 1, fullAccessOn: true), "one letter")
        XCTAssertFalse(OnboardingPractice.isDone(text: "See you", dictated: false, typedWithHex: 2, fullAccessOn: true))
        XCTAssertTrue(OnboardingPractice.isDone(text: "See you at", dictated: false, typedWithHex: 3, fullAccessOn: true))
        XCTAssertFalse(OnboardingPractice.isDone(text: "See you at ten", dictated: false, typedWithHex: 4, fullAccessOn: false), "not the Hex keyboard")
        XCTAssertFalse(OnboardingPractice.isDone(text: "See you at ten", dictated: false, typedWithHex: 0, fullAccessOn: true), "iOS's dictation or another keyboard typed it")
        XCTAssertFalse(OnboardingPractice.isDone(text: "See you at ten", dictated: false, typedWithHex: 2, fullAccessOn: true), "two words from Hex, the rest from elsewhere")
        XCTAssertTrue(OnboardingPractice.isDone(text: "", dictated: true, typedWithHex: 0, fullAccessOn: false), "a dictation arrived")
        XCTAssertEqual(OnboardingPractice.wordsAdded(from: "See yo", to: "See you"), 0, "a letter inside a word")
        XCTAssertEqual(OnboardingPractice.wordsAdded(from: "See you ", to: "See you a"), 1)
        XCTAssertEqual(OnboardingPractice.wordsAdded(from: "", to: "See you at ten"), 4, "a dictation's words at once")
        XCTAssertEqual(OnboardingPractice.wordsAdded(from: "See you at", to: "See you"), 0, "a deletion")
        XCTAssertGreaterThanOrEqual(OnboardingPractice.words(in: "我们十点见"), 2, "Chinese splits into words without spaces")
    }

    func testOnboardingIsReadyOnlyOnceHexAndTheHexKeyModelsAre() {
        typealias Stage = OnboardingModel.Stage
        XCTAssertEqual(OnboardingModel.combine(.ready, .ready), .ready)
        XCTAssertEqual(OnboardingModel.combine(.ready, .downloading(0.4)), .downloading(0.7), "Parakeet ready, the keyboard's Apple Speech still downloading")
        XCTAssertEqual(OnboardingModel.combine(.downloading(0.2), .ready), .downloading(0.6))
        XCTAssertEqual(OnboardingModel.combine(.ready, .starting), .downloading(0.5))
        XCTAssertEqual(OnboardingModel.combine(.starting, .starting), .starting)
        XCTAssertEqual(OnboardingModel.combine(.ready, .failed("no")), .failed("no"))
        XCTAssertEqual(OnboardingModel.combine(.failed("app"), .failed("keys")), .failed("app"))
        XCTAssertEqual(OnboardingModel.combine(.downloading(0.2), nil), .downloading(0.2), "nothing for the keyboard to download")
        XCTAssertEqual(OnboardingModel.combine(.ready, nil), .ready)
    }

    func testOnboardingResumesPagesEarlierBuildsSaved() {
        XCTAssertEqual(OnboardingPage(saved: "welcome"), .setup)
        XCTAssertEqual(OnboardingPage(saved: "allow"), .permissions)
        XCTAssertEqual(OnboardingPage(saved: "try"), .practice)
        XCTAssertEqual(OnboardingPage(saved: "waiting"), .waiting)
        XCTAssertNil(OnboardingPage(saved: "nope"))
        XCTAssertEqual(OnboardingPage.allCases, [.setup, .demo, .permissions, .waiting, .practice, .ready])
        XCTAssertEqual(OnboardingPage.allCases.filter(\.isOptional), [.demo, .practice])
    }

    func testKeyboardRecordingLaunchRequiresAJob() {
        let url = URL(string: "hex-dictation://keyboard/record")!
        XCTAssertNil(KeyboardLaunchRequest(url: url))
    }

    func testTranscriptStylesFormatDeterministically() {
        let raw = "um let's ship the ios build tonight"
        XCTAssertEqual(
            TranscriptFormatter.format(raw, style: .polished),
            "Let's ship the ios build tonight."
        )
        XCTAssertEqual(
            TranscriptFormatter.format("Um, sounds good.", style: .casual),
            "sounds good"
        )
        XCTAssertEqual(
            TranscriptFormatter.format("um keep raw words", style: .verbatim),
            "um keep raw words"
        )
        XCTAssertEqual(
            TranscriptFormatter.format("First item bullet check signing bullet test keyboard", style: .notes),
            "First item\n- Check signing\n- Test keyboard"
        )
    }

    func testFillerTrimmingFollowsTheDictationLanguage() {
        // "er" is Norwegian for "is"; English trims it as a filler, Norwegian must not.
        XCTAssertEqual(
            TranscriptFormatter.format("det er bra, eh, vi ses", style: .polished, language: "nb-NO"),
            "Det er bra, vi ses."
        )
        XCTAssertEqual(
            TranscriptFormatter.format("er I think so", style: .polished, language: "en-US"),
            "I think so."
        )
        // Portuguese "um" is "a"; a language without a filler list trims nothing.
        XCTAssertEqual(
            TranscriptFormatter.format("um café, por favor", style: .casual, language: "pt-BR"),
            "um café, por favor"
        )
        XCTAssertEqual(
            TranscriptFormatter.format("um let's go", style: .polished),
            "Let's go.",
            "no language is English, as before"
        )
    }

    func testSmartInsertionSpacingRespectsSurroundingContext() {
        XCTAssertEqual(
            TranscriptFormatter.insertionText(for: "world.", beforeContext: "Hello", afterContext: nil),
            " world."
        )
        XCTAssertEqual(
            TranscriptFormatter.insertionText(for: "world.", beforeContext: "Hello ", afterContext: nil),
            "world."
        )
        XCTAssertEqual(
            TranscriptFormatter.insertionText(for: "Hello", beforeContext: nil, afterContext: "world"),
            "Hello "
        )
    }

    func testDictationContinuingASentenceStartsLowercase() {
        let words: Set<String> = ["that's", "the", "self", "a", "don't"]
        func insert(_ text: String, after before: String?) -> String {
            TranscriptFormatter.insertionText(for: text, beforeContext: before, afterContext: nil, isWord: words.contains)
        }
        XCTAssertEqual(insert("That's great.", after: "I think"), " that's great.")
        XCTAssertEqual(insert("The plan works.", after: "and so "), "the plan works.")
        XCTAssertEqual(insert("Self-driving cars.", after: "I like"), " self-driving cars.")
        XCTAssertEqual(insert("A dog.", after: "We saw"), " a dog.")

        // A sentence start keeps its capital.
        for before in [nil, "", "   ", "Hello.", "Hello. ", "Really?", "Yes!", "Wait…", "Line one\n", "He said \"stop.\" "] {
            XCTAssertTrue(TranscriptFormatter.startsSentence(after: before), String(describing: before))
            XCTAssertEqual(insert("The end.", after: before).trimmingCharacters(in: .whitespaces), "The end.")
        }
        XCTAssertFalse(TranscriptFormatter.startsSentence(after: "Note: "))
        XCTAssertFalse(TranscriptFormatter.startsSentence(after: "see (this)"))

        // I, its contractions, acronyms, inner capitals and names keep theirs.
        XCTAssertEqual(insert("I think so.", after: "and"), " I think so.")
        XCTAssertEqual(insert("I'm here.", after: "and"), " I'm here.")
        XCTAssertEqual(insert("I’ll go.", after: "and"), " I’ll go.")
        XCTAssertEqual(insert("NASA launched it.", after: "and"), " NASA launched it.")
        XCTAssertEqual(insert("McDonald's is open.", after: "and"), " McDonald's is open.")
        XCTAssertEqual(insert("Sarah said so.", after: "and"), " Sarah said so.", "not a word in lowercase: a name")
        XCTAssertEqual(insert("iPhone works.", after: "my"), " iPhone works.")

        // Without a dictionary the case is left alone.
        XCTAssertEqual(
            TranscriptFormatter.insertionText(for: "That's it.", beforeContext: "and", afterContext: nil),
            " That's it."
        )
    }

    func testDictationSpacingNeverDoublesOrSplitsAnOpeningMark() {
        XCTAssertEqual(TranscriptFormatter.insertionText(for: " world", beforeContext: "Hello ", afterContext: nil), "world")
        XCTAssertEqual(TranscriptFormatter.insertionText(for: " world", beforeContext: "Hello", afterContext: nil), " world")
        XCTAssertEqual(TranscriptFormatter.insertionText(for: "world", beforeContext: "Hello\t", afterContext: nil), "world")
        for opening in ["(", "[", "\"", "“", "‘", "«", "¿", "¡"] {
            XCTAssertEqual(
                TranscriptFormatter.insertionText(for: "Hola", beforeContext: "Dijo " + opening, afterContext: nil),
                "Hola",
                opening
            )
        }
        XCTAssertEqual(TranscriptFormatter.insertionText(for: ", right", beforeContext: "Okay", afterContext: nil), ", right")
    }

    /// TestFlight build 121 (zh): "a dictation inserted mid-sentence gets a space before and
    /// after; Chinese between Chinese needs none". Chinese, Japanese and full-width marks take
    /// no space on either side; Korean and Latin text are spaced as before.
    func testDictationIntoChineseTextTakesNoSpaces() {
        func insert(_ text: String, _ before: String?, _ after: String?) -> String {
            TranscriptFormatter.insertionText(for: text, beforeContext: before, afterContext: after, isWord: { _ in true })
        }
        // Between two Han characters.
        XCTAssertEqual(insert("天气", "今天", "很好"), "天气")
        XCTAssertEqual(insert("我们明天去公园", "我觉得", "你说呢"), "我们明天去公园")
        // After a full-width comma, before a full-width period.
        XCTAssertEqual(insert("然后回家做饭", "我想去超市，", "。"), "然后回家做饭")
        XCTAssertEqual(insert("买点水果", "我想去超市", "。"), "买点水果")
        XCTAssertEqual(insert("你好", "他说：「", "」"), "你好")
        // Han beside a Latin word or a number: none, as Apple's Chinese keyboard and dictation write it.
        XCTAssertEqual(insert("iPhone", "我用", "拍照"), "iPhone")
        XCTAssertEqual(insert("拍照", "我用iPhone", "了"), "拍照")
        XCTAssertEqual(insert("三点", "会议改到 3", "开"), "三点")
        // At the start and the end of the field.
        XCTAssertEqual(insert("你好。", nil, nil), "你好。")
        XCTAssertEqual(insert("你好。", "", "我是小王"), "你好。")
        XCTAssertEqual(insert("我是小王。", "你好。", nil), "我是小王。")
        // A selection replaced: the text on either side of it.
        XCTAssertEqual(insert("后天", "我们", "见"), "后天")
        // Japanese.
        XCTAssertEqual(insert("ありがとう", "本当に", "ございます"), "ありがとう")
        // A sentence that goes on after the cursor never gets the dictation's "。" inside it.
        XCTAssertEqual(insert("如果天气好。", "我想", "去超市"), "如果天气好")
        XCTAssertEqual(insert("买点水果。", "我想去超市", "，然后回家"), "买点水果")
        XCTAssertEqual(insert("买点水果。", "我想去超市", "。"), "买点水果")
        XCTAssertEqual(insert("等一下。", "我到了。", "你在哪里"), "等一下。", "a sentence between two keeps its 。")
        XCTAssertEqual(insert("等一下。", "我到了。", nil), "等一下。")
        XCTAssertEqual(insert("等一下？", "我想", "去超市"), "等一下？", "a question keeps its mark")
        XCTAssertEqual(insert("black.", "the quick", "fox"), " black. ", "Latin text as before")
        // Korean spaces its words; Latin text is spaced as before.
        XCTAssertEqual(insert("감사합니다", "정말", "여러분"), " 감사합니다 ")
        XCTAssertEqual(insert("brown", "the quick", "fox"), " brown ")
        XCTAssertEqual(insert("Café", "Vamos al", "ahora"), " café ")
    }

    func testDictationDictionaryFollowsTheDictationLanguage() {
        let available = ["en_US", "nb", "de_DE", "fr_FR", "es"]
        XCTAssertEqual(KeyboardLanguage.spellCheckLanguage(forDictation: "nb-NO", available: available), "nb")
        XCTAssertEqual(KeyboardLanguage.spellCheckLanguage(forDictation: "de-DE", available: available), "de_DE")
        XCTAssertEqual(KeyboardLanguage.spellCheckLanguage(forDictation: "es-419", available: available), "es")
        XCTAssertEqual(KeyboardLanguage.spellCheckLanguage(forDictation: "fr-CA", available: available), "fr_FR")
        XCTAssertEqual(KeyboardLanguage.spellCheckLanguage(forDictation: "en-GB", available: available), "en_US")
        XCTAssertNil(KeyboardLanguage.spellCheckLanguage(forDictation: "he-IL", available: available))

        // German capitalizes every noun, and its dictionary takes them in lowercase too.
        XCTAssertFalse(TranscriptFormatter.lowercasesContinuations(in: "de-DE"))
        XCTAssertFalse(TranscriptFormatter.lowercasesContinuations(in: "de_CH"))
        XCTAssertTrue(TranscriptFormatter.lowercasesContinuations(in: "en-US"))
        XCTAssertTrue(TranscriptFormatter.lowercasesContinuations(in: "nb-NO"))
    }

    func testAppIntentMetadataContainsNoApplePlatformNames() throws {
        var texts = [
            String(localized: DictateWithHexIntent.title),
            String(localized: WarmHexKeyboardIntent.title),
            String(localized: ToggleHexControlIntent.title),
            String(localized: StopHexDictationIntent.title),
            String(localized: CancelHexDictationIntent.title),
            String(localized: DictateWithHexIntent.description.descriptionText),
            String(localized: WarmHexKeyboardIntent.description.descriptionText),
            String(localized: ToggleHexControlIntent.description.descriptionText),
            String(localized: StopHexDictationIntent.description.descriptionText),
            String(localized: CancelHexDictationIntent.description.descriptionText),
        ]

        // Read user-facing strings from compiled Metadata.appintents inside HEX.app and HEXWidgets.appex,
        // plus source literals when #filePath is not CAS prefix-mapped (/^wt).
        let userKeys: Set<String> = ["title", "shortTitle", "description", "key", "phrases", "basePhrases", "template"]
        func collectUserStrings(_ node: Any) {
            if let dict = node as? [String: Any] {
                for (k, v) in dict {
                    if userKeys.contains(k) {
                        if let s = v as? String {
                            texts.append(s)
                        } else if let arr = v as? [String] {
                            texts.append(contentsOf: arr)
                        } else {
                            collectUserStrings(v)
                        }
                    } else {
                        collectUserStrings(v)
                    }
                }
            } else if let arr = node as? [Any] {
                for item in arr {
                    collectUserStrings(item)
                }
            }
        }

        var metadataFilesRead = 0
        var metadataRoots: [URL] = [
            Bundle.main.bundleURL.appendingPathComponent("Metadata.appintents"),
        ]
        if let plugIns = Bundle.main.builtInPlugInsURL {
            metadataRoots.append(
                plugIns.appendingPathComponent("HEXWidgets.appex/Metadata.appintents")
            )
        }
        for root in metadataRoots where FileManager.default.fileExists(atPath: root.path) {
            if let enumerator = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil) {
                for case let url as URL in enumerator {
                    guard ["json", "actionsdata", "phrasesdata"].contains(url.pathExtension),
                          let data = try? Data(contentsOf: url),
                          let obj = try? JSONSerialization.jsonObject(with: data) else {
                        continue
                    }
                    metadataFilesRead += 1
                    collectUserStrings(obj)
                }
            }
        }
        XCTAssertGreaterThan(metadataFilesRead, 0, "Expected compiled Metadata.appintents in host bundle")

        let ios = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
        if FileManager.default.fileExists(atPath: ios.appendingPathComponent("HEXMobile/HexIntents.swift").path) {
            for file in [
                "HEXMobile/HexIntents.swift",
                "HEXActivity/HexActivityAttributes.swift",
                "HEXActivity/HexControlIntents.swift",
                "HEXWidgets/HexDictationControl.swift",
            ] {
                let source = try String(contentsOf: ios.appendingPathComponent(file), encoding: .utf8)
                for line in source.split(separator: "\n") where !line.trimmingCharacters(in: .whitespaces).hasPrefix("//") {
                    texts.append(contentsOf: line.split(separator: "\"", omittingEmptySubsequences: false)
                        .enumerated().filter { $0.offset % 2 == 1 }.map { String($0.element) })
                }
            }
        }

        let forbidden = ["iphone", "ipad", "apple", "siri", "macos", "ios", "watch", "airpods", "vision"]
        for text in texts {
            let lower = text.lowercased()
            for word in forbidden {
                XCTAssertFalse(lower.contains(word), "Intent string '\(text)' contains forbidden word '\(word)'")
            }
        }
    }

    /// A keyboard's optional launch work (language lists, the capture page) runs only after
    /// its first frame, and never once memory is short (TestFlight build 56's blank panel).
    func testOptionalLaunchWorkWaitsForTheFirstFrameAndForMemory() {
        let budget = KeyboardLaunchWork.optionalHeadroomNeeded
        XCTAssertGreaterThan(KeyboardLaunchWork.afterFirstFrame, 0, "never in the turn that draws the keys")
        XCTAssertGreaterThan(KeyboardLaunchWork.optionalWorkDelay, KeyboardLaunchWork.afterFirstFrame)
        XCTAssertTrue(KeyboardLaunchWork.allowsOptionalWork(headroom: 0, memoryWarned: false), "iOS doesn't say")
        XCTAssertTrue(KeyboardLaunchWork.allowsOptionalWork(headroom: budget, memoryWarned: false))
        XCTAssertTrue(KeyboardLaunchWork.allowsOptionalWork(headroom: 200 * 1_048_576, memoryWarned: false))
        XCTAssertFalse(KeyboardLaunchWork.allowsOptionalWork(headroom: budget - 1, memoryWarned: false))
        XCTAssertFalse(KeyboardLaunchWork.allowsOptionalWork(headroom: 8 * 1_048_576, memoryWarned: false))
        XCTAssertFalse(KeyboardLaunchWork.allowsOptionalWork(headroom: 0, memoryWarned: true), "after a memory warning")
        XCTAssertFalse(KeyboardLaunchWork.allowsOptionalWork(headroom: 200 * 1_048_576, memoryWarned: true))
    }

    private func snapshot(
        state: KeyboardDictationState,
        heartbeat: TimeInterval,
        expiresAt: TimeInterval
    ) -> KeyboardSnapshot {
        KeyboardSnapshot(
            state: state,
            heartbeat: heartbeat,
            expiresAt: expiresAt,
            recordingStartedAt: heartbeat - 2,
            audioLevel: 0.5,
            jobID: "job",
            resultID: "result",
            resultExpiresAt: heartbeat + 30,
            transcript: "hello",
            message: nil
        )
    }

    /// Keep mic warm defaults to Off where the keyboard dictates by itself, else to 15 minutes:
    /// before iOS 26, and while the keyboard hands its dictations to Hex. A choice stays.
    func testKeepMicWarmDefaultFollowsWhetherTheKeyboardDictatesItself() {
        let suiteName = "hex.tests.warmDefault.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let bridge = KeyboardBridge(defaults: defaults)
        let record = KeyboardSelfDictation(defaults: defaults)
        let now = Date(timeIntervalSince1970: 2_000_000_000)

        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .off)
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 27, now: now), .off)
        for version in [18, 25] {
            XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: version, now: now), .fifteenMinutes, "iOS \(version)")
        }

        record.recordHandedToHex(.memory, at: now.addingTimeInterval(-3_600))
        XCTAssertEqual(record.handedToHex?.reason, .memory)
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .fifteenMinutes, "short of memory an hour ago")
        XCTAssertEqual(
            bridge.defaultWarmDuration(osMajorVersion: 26, now: now.addingTimeInterval(KeyboardSelfDictation.recentWindow)),
            .off,
            "a week on"
        )
        record.clearHandedToHex()
        XCTAssertNil(record.handedToHex)
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .off, "the keyboard dictated by itself again")

        let languages = LanguageSettings(defaults: defaults)
        languages.markKeyboardNeedsHex(at: now)
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .fifteenMinutes, "no speech model in the keyboard")
        languages.clearKeyboardNeedsHex()

        languages.chooseDictationLanguage("nl-NL")
        let multilingual = MultilingualParakeetSettings(defaults: defaults)
        multilingual.isEnabled = true
        multilingual.isModelReady = true
        if MultilingualParakeetSettings.fits() {
            XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .fifteenMinutes, "only Hex loads the multilingual model")
        }
        KeyboardParakeetSettings(defaults: defaults).isEnabled = true
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .off, "Speech model in keyboard loads it there")
        multilingual.isEnabled = false

        bridge.warmDuration = .oneHour
        XCTAssertEqual(bridge.warmDuration, .oneHour)
        bridge.warmDuration = .off
        record.recordHandedToHex(.memory)
        XCTAssertEqual(bridge.warmDuration, .off, "Off chosen stays Off")
    }

    /// An install from before the default followed the keyboard: a stored 15 minutes (tapping
    /// the default to warm the mic stored it) reads as not chosen, once, on iOS 26 where the
    /// keyboard dictates by itself. 5 minutes, 1 hour and Off stay.
    func testAStoredFifteenMinutesIsReviewedOnceWhereTheKeyboardDictatesItself() {
        let suiteName = "hex.tests.warmReview.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let bridge = KeyboardBridge(defaults: defaults)
        let now = Date(timeIntervalSince1970: 2_000_000_000)

        bridge.warmDuration = .fifteenMinutes
        bridge.reviewWarmDurationForKeyboardDictation(osMajorVersion: 25, now: now)
        XCTAssertEqual(bridge.warmDuration, .fifteenMinutes, "iOS 25 needs it, and isn't reviewed")
        bridge.reviewWarmDurationForKeyboardDictation(osMajorVersion: 26, now: now)
        XCTAssertNil(defaults.string(forKey: "settings.warmDuration"))
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .off)

        bridge.warmDuration = .fifteenMinutes
        bridge.reviewWarmDurationForKeyboardDictation(osMajorVersion: 26, now: now)
        XCTAssertEqual(bridge.warmDuration, .fifteenMinutes, "once: a 15 minutes picked since stays")

        for kept in [WarmSessionDuration.off, .fiveMinutes, .oneHour] {
            let suite = "hex.tests.warmReview.\(UUID().uuidString)"
            let other = UserDefaults(suiteName: suite)!
            defer { other.removePersistentDomain(forName: suite) }
            let otherBridge = KeyboardBridge(defaults: other)
            otherBridge.warmDuration = kept
            otherBridge.reviewWarmDurationForKeyboardDictation(osMajorVersion: 26, now: now)
            XCTAssertEqual(otherBridge.warmDuration, kept)
        }

        let suite = "hex.tests.warmReview.\(UUID().uuidString)"
        let short = UserDefaults(suiteName: suite)!
        defer { short.removePersistentDomain(forName: suite) }
        let shortBridge = KeyboardBridge(defaults: short)
        shortBridge.warmDuration = .fifteenMinutes
        KeyboardSelfDictation(defaults: short).recordHandedToHex(.memory, at: now)
        shortBridge.reviewWarmDurationForKeyboardDictation(osMajorVersion: 26, now: now)
        XCTAssertEqual(shortBridge.warmDuration, .fifteenMinutes, "the keyboard needs Hex here")
    }
}

/// The keyboard's quick dictation-language switch.
final class DictationLanguageSwitchTests: XCTestCase {
    private let supported = ["en-US", "en-GB", "es-ES", "es-US", "es-MX", "de-DE", "nb-NO", "he-IL", "fr-FR"]

    func testOffersTheCurrentThenRecentThenPreferredLanguages() {
        XCTAssertEqual(
            DictationLanguageSwitch.choices(
                current: "de-DE",
                recent: ["he-IL", "de-DE"],
                preferred: ["nb-NO", "en-US"],
                supported: supported
            ),
            ["de-DE", "he-IL", "nb-NO", "en-US"]
        )
    }

    func testMatchesPreferredLanguagesToOnesAppleTranscribes() {
        let choices = DictationLanguageSwitch.choices(
            current: nil,
            recent: [],
            preferred: ["nn-NO", "es-419", "es", "en-AU", "tlh", "fr-CA", "en-GB", "de-AT"],
            supported: supported,
            region: "US"
        )
        // Nynorsk is Bokmål; "es-419" finds the Americas' Spanish (es-US); "es" adds no
        // second Spanish; Klingon has none; one variant per language.
        XCTAssertEqual(choices, ["nb-NO", "es-US", "en-US", "fr-FR", "de-DE"])
        XCTAssertEqual(choices.count, DictationLanguageSwitch.maxChoices)
    }

    /// Apple Speech has three Spanish variants: Spain's es-ES, and es-US and es-MX for the
    /// Americas. A Latin American or US iPhone that speaks Spanish speaks Latin American
    /// Spanish, so its dictation is es-US where Apple has it, es-MX next, and never Spain's.
    func testSpanishFindsTheAmericasVariant() {
        let all = ["en-US", "es-ES", "es-MX", "es-US"]
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-419", in: all), "es-US")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-CO", in: all), "es-US")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-AR", in: all), "es-US")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-US", in: all), "es-US", "exact")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-MX", in: all), "es-MX", "exact")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-ES", in: all), "es-ES", "exact")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-GQ", in: all), "es-ES", "Equatorial Guinea is Spain's")
        XCTAssertEqual(
            DictationLanguageSwitch.supportedMatch(for: "es-419", in: ["en-US", "es-ES", "es-MX"]),
            "es-MX",
            "es-US missing"
        )
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-419", in: ["en-US", "es-ES"]), "es-ES")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-419", in: []), "es-419", "not listed yet")
    }

    /// Plain "es" names no region, so Spanish follows the iPhone's Region: Mexico's own
    /// Spanish, the United States' and Latin America's es-US, and Spain's es-ES elsewhere.
    func testPlainSpanishFollowsTheIPhonesRegion() {
        let all = ["en-US", "es-ES", "es-MX", "es-US"]
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es", in: all, region: "MX"), "es-MX")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es", in: all, region: "US"), "es-US")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es", in: all, region: "PR"), "es-US")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es", in: all, region: "CO"), "es-US")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es", in: all, region: "ES"), "es-ES")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es", in: all, region: "FR"), "es-ES")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es", in: all, region: nil), "es-ES")
        XCTAssertEqual(
            DictationLanguageSwitch.supportedMatch(for: "es", in: ["en-US", "es-MX"], region: "US"),
            "es-MX",
            "es-US missing"
        )
    }

    /// One chip per language: the current variant first, then the first of the recents, then
    /// the preferred languages, so es-US and es-ES never both appear.
    func testOneVariantPerLanguageInTheChoices() {
        XCTAssertEqual(
            DictationLanguageSwitch.choices(
                current: "es-MX",
                recent: ["es-US", "en-GB", "es-ES"],
                preferred: ["fr-FR", "en-US"],
                supported: ["en-US", "en-GB", "es-ES", "es-MX", "es-US", "fr-FR"],
                region: "US"
            ),
            ["es-MX", "en-GB", "fr-FR"]
        )
    }

    /// A picked variant drops its siblings from the recents; Norwegian's nb/nn/no aliases and
    /// Hinglish's Hindi stay one language each.
    func testPickingAVariantDropsItsSiblingsFromTheRecents() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = LanguageSettings(defaults: defaults)
        settings.chooseDictationLanguage("es-US")
        settings.chooseDictationLanguage("en-GB")
        settings.chooseDictationLanguage("es-MX")
        XCTAssertEqual(settings.recentDictationLanguages, ["es-MX", "en-GB"])
        settings.chooseDictationLanguage("nn-NO")
        settings.chooseDictationLanguage("nb-NO")
        XCTAssertEqual(settings.recentDictationLanguages, ["nb-NO", "es-MX", "en-GB"])
        settings.chooseDictationLanguage("hi-IN")
        XCTAssertEqual(settings.recentDictationLanguages, ["hi-IN", "nb-NO", "es-MX", "en-GB"])
    }

    /// Chinese is more than one spoken language: Mandarin's zh-CN, Taiwan's zh-TW and
    /// Cantonese's zh-HK are told apart by script and region, so each keeps a chip and
    /// picking one leaves the others in the recents.
    func testChineseVariantsStayApart() throws {
        let chinese = ["en-US", "zh-CN", "zh-TW", "zh-HK"]
        XCTAssertEqual(
            DictationLanguageSwitch.choices(
                current: "zh-CN",
                recent: ["zh-HK"],
                preferred: ["zh-TW"],
                supported: chinese,
                region: "US"
            ),
            ["zh-CN", "zh-HK", "zh-TW"],
            "one chip per language, but not one for all of Chinese"
        )
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = LanguageSettings(defaults: defaults)
        settings.chooseDictationLanguage("zh-CN")
        settings.chooseDictationLanguage("zh-HK")
        XCTAssertEqual(settings.recentDictationLanguages, ["zh-HK", "zh-CN"])
        settings.chooseDictationLanguage("zh-TW")
        XCTAssertEqual(settings.recentDictationLanguages, ["zh-TW", "zh-HK", "zh-CN"])
    }

    /// A language with more than one variant in Apple Speech names its region, as Apple's own
    /// keyboards do; one with a single variant keeps its plain name.
    func testNamesTheRegionOnlyWhereAppleHasMoreThanOneVariant() {
        let supported = ["en-US", "en-GB", "es-ES", "es-MX", "es-US", "nb-NO", "fr-FR"]
        XCTAssertEqual(
            DictationLanguageSwitch.nativeNames(for: ["es-US", "en-GB", "nb-NO"], supported: supported),
            ["es-US": "Español (US)", "en-GB": "English (GB)", "nb-NO": "Norsk"]
        )
        XCTAssertEqual(
            DictationLanguageSwitch.nativeName(for: "es-ES", supported: supported),
            "Español (ES)"
        )
        XCTAssertEqual(
            DictationLanguageSwitch.nativeNames(for: ["es-US", "en-GB", "nb-NO"]),
            ["es-US": "Español", "en-GB": "English", "nb-NO": "Norsk"],
            "without Apple's list, one variant each"
        )
    }

    func testWithoutAListEveryLanguageIsOffered() {
        XCTAssertEqual(
            DictationLanguageSwitch.choices(current: nil, recent: [], preferred: ["nb_NO", "en"], supported: []),
            ["nb-NO", "en"]
        )
    }

    func testNamesEachLanguageInItself() {
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "nb-NO"), "Norsk")
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "en-US"), "English")
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "es-ES"), "Español")
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "de-DE"), "Deutsch")
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "he-IL"), "עברית")
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "ru-RU"), "Русский")
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "sr-Latn-RS"), "Srpski")
        XCTAssertEqual(
            DictationLanguageSwitch.nativeNames(for: ["en-US", "en-GB", "nb-NO"]),
            ["en-US": "English (US)", "en-GB": "English (GB)", "nb-NO": "Norsk"]
        )
    }

    /// Experiments › Hinglish offers Hindi in the Hex key's language menu, named Hinglish.
    func testHinglishOffersHindiByThatName() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let hinglish = HinglishSettings(defaults: defaults)
        let supported = self.supported + ["hi-IN"]
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "hi-IN", hinglish: hinglish), "हिन्दी")
        XCTAssertEqual(
            DictationLanguageSwitch.choices(current: "de-DE", recent: ["he-IL"], preferred: ["en-US"], supported: supported),
            ["de-DE", "he-IL", "en-US"]
        )
        hinglish.isEnabled = true
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "hi-IN", hinglish: hinglish), "Hinglish")
        XCTAssertEqual(
            DictationLanguageSwitch.nativeNames(for: ["hi-IN", "en-US"], hinglish: hinglish),
            ["hi-IN": "Hinglish", "en-US": "English"]
        )
        XCTAssertEqual(
            DictationLanguageSwitch.choices(
                current: "de-DE", recent: ["he-IL"], preferred: ["en-US"], supported: supported, hinglish: true
            ),
            ["de-DE", "he-IL", "hi-IN", "en-US"]
        )
        XCTAssertEqual(
            DictationLanguageSwitch.choices(current: nil, recent: [], preferred: ["en-US"], supported: ["en-US"], hinglish: true),
            ["en-US"],
            "not offered where Apple has no Hindi"
        )
    }

    /// Experiments › Greeklish offers Greek in the Hex key's language menu, named Greeklish,
    /// the way Hinglish offers Hindi.
    func testGreeklishOffersGreekByThatName() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let greeklish = GreeklishSettings(defaults: defaults)
        let supported = self.supported + ["el-GR"]
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "el-GR", greeklish: greeklish), "Ελληνικά")
        XCTAssertEqual(
            DictationLanguageSwitch.choices(current: "de-DE", recent: ["he-IL"], preferred: ["en-US"], supported: supported),
            ["de-DE", "he-IL", "en-US"]
        )
        greeklish.isEnabled = true
        XCTAssertEqual(DictationLanguageSwitch.nativeName(for: "el-GR", greeklish: greeklish), "Greeklish")
        XCTAssertEqual(
            DictationLanguageSwitch.nativeNames(for: ["el-GR", "en-US"], greeklish: greeklish),
            ["el-GR": "Greeklish", "en-US": "English"]
        )
        XCTAssertEqual(
            DictationLanguageSwitch.choices(
                current: "de-DE", recent: ["he-IL"], preferred: ["en-US"], supported: supported, greeklish: true
            ),
            ["de-DE", "he-IL", "el-GR", "en-US"]
        )
        XCTAssertEqual(
            DictationLanguageSwitch.choices(current: nil, recent: [], preferred: ["en-US"], supported: ["en-US"], greeklish: true),
            ["en-US"],
            "not offered where Apple has no Greek"
        )
    }

    func testAChoiceMovesToTheFrontOfTheRecents() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = LanguageSettings(defaults: defaults)
        for id in ["nb-NO", "es-ES", "de-DE", "he-IL", "fr-FR", "en-GB", "es-ES"] {
            settings.chooseDictationLanguage(id)
        }
        XCTAssertEqual(settings.dictationLanguage, "es-ES")
        XCTAssertEqual(settings.recentDictationLanguages, ["es-ES", "en-GB", "fr-FR", "he-IL", "de-DE"])
        settings.chooseDictationLanguage(nil)
        XCTAssertNil(settings.dictationLanguage, "Automatic")
        XCTAssertEqual(settings.recentDictationLanguages.first, "es-ES", "Automatic isn't a recent")
    }

    /// TestFlight build 56: a language couldn't be taken off the keyboard's switch. Removed, a
    /// recent or one of the iPhone's own languages stays off until it is picked again; the one
    /// set goes back to Automatic.
    func testARemovedLanguageStaysOffUntilPickedAgain() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = LanguageSettings(defaults: defaults)
        settings.supportedDictationLanguages = supported
        settings.chooseDictationLanguage("he-IL")
        settings.chooseDictationLanguage("es-ES")
        let preferred = ["zh-Hans-CN", "nb-NO", "en-US"]
        XCTAssertEqual(
            settings.keyboardLanguageChoices(preferred: preferred, hinglish: false, region: "ES"),
            ["es-ES", "he-IL", "nb-NO", "en-US"]
        )

        settings.removeDictationLanguage("es-ES")
        XCTAssertNil(settings.dictationLanguage, "the one set goes back to Automatic")
        XCTAssertEqual(settings.recentDictationLanguages, ["he-IL"])
        settings.removeDictationLanguage("nb-NO")
        XCTAssertEqual(
            settings.keyboardLanguageChoices(preferred: preferred, hinglish: false, region: "ES"),
            ["he-IL", "en-US"]
        )

        settings.chooseDictationLanguage("nb-NO")
        XCTAssertEqual(settings.removedDictationLanguages, ["es-ES"])
        XCTAssertEqual(
            settings.keyboardLanguageChoices(preferred: preferred, hinglish: false, region: "ES"),
            ["nb-NO", "he-IL", "en-US"]
        )
        XCTAssertEqual(
            DictationLanguageSwitch.choices(current: "es-ES", recent: [], preferred: [], supported: supported, removed: ["es-ES"]),
            ["es-ES"],
            "the one set always shows"
        )
    }

    /// More languages… lists every other language Apple Speech has, the iPhone's own first
    /// (with their other regions), then the rest by name.
    func testMoreLanguagesPutsTheIPhonesOwnFirst() {
        let supported = ["en-US", "en-GB", "es-ES", "de-DE", "zh-CN", "zh-TW", "fr-FR", "nb-NO"]
        XCTAssertEqual(
            DictationLanguageSwitch.more(choices: ["en-US", "de-DE"], preferred: ["zh-Hans-CN", "en-US"], supported: supported),
            ["zh-CN", "zh-TW", "en-GB", "es-ES", "fr-FR", "nb-NO"]
        )
        XCTAssertEqual(DictationLanguageSwitch.more(choices: [], preferred: ["en-US"], supported: []), [], "no list yet")
    }
}
