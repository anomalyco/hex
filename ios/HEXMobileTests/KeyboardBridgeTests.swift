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

        // A warm session's dictation failed just now: the keyboard says so and retries through Hex.
        let model = KeyboardViewModel()
        show(
            KeyboardSnapshot(state: .failed, heartbeat: t0 + 1, expiresAt: t0 + 900, message: busy, messageAt: t0),
            at: t0 + 1,
            on: model
        )
        XCTAssertEqual(model.stagePhase, .failed)
        XCTAssertEqual(model.statusMessage, busy)
        XCTAssertTrue(model.isWarmAvailable)
        XCTAssertEqual(model.route, .warm)
        XCTAssertTrue(model.dictatesThroughHex, "a live warm session retries through Hex")

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

        // The keyboard's own failure snapshot has no warm session (heartbeat/expiresAt 0):
        // both keyboards show the message and retry in the keyboard instead of sending to Hex.
        let own = KeyboardViewModel()
        own.onLocalDictateTap = {}
        let localFailed = KeyboardLocalCapturePhase.failed(message: busy, until: t0 + 6)
            .snapshot(now: t0 + 1, loudnessDB: -62)!
        show(localFailed, at: t0 + 1, on: own)
        XCTAssertEqual(own.stagePhase, .failed)
        XCTAssertEqual(own.failureMessage, busy)
        XCTAssertFalse(own.isWarmAvailable)
        XCTAssertEqual(own.route, .inKeyboard)
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

    /// The screen locked mid-dictation: the keyboard stops and keeps what was said, and leaves
    /// the recording for the next keyboard in case iOS ends this one (it freezes a keyboard
    /// seconds after the lock) before it has transcribed it. The text, once held, says for a
    /// few seconds beside Paste dictation where it stopped and why.
    func testARecordingTheKeyboardClosedDuringWaitsForTheNextKeyboard() throws {
        let suiteName = "hex.tests.interruptedRecording.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let field = UUID()
        let record = KeyboardInterruptedRecording(
            jobID: "a", fileName: "keyboard-\(UUID().uuidString).wav", document: field, seconds: 44.6, cutShort: true, at: 1_000
        )
        try Data([1, 2, 3]).write(to: record.url)

        XCTAssertNil(KeyboardInterruptedRecording.load(defaults: defaults))
        record.save(defaults: defaults)
        XCTAssertEqual(KeyboardInterruptedRecording.load(defaults: defaults), record, "a new keyboard process finds it")
        XCTAssertFalse(record.isExpired(at: 1_000 + 60 * 60), "an hour's drive later")
        XCTAssertTrue(record.isExpired(at: 1_000 + KeyboardInterruptedRecording.keptFor + 1))
        KeyboardInterruptedRecording.clear(jobID: "b", defaults: defaults)
        XCTAssertEqual(KeyboardInterruptedRecording.load(defaults: defaults), record, "another job's text leaves it")
        XCTAssertTrue(FileManager.default.fileExists(atPath: record.url.path))
        KeyboardInterruptedRecording.clear(jobID: "a", defaults: defaults)
        XCTAssertNil(KeyboardInterruptedRecording.load(defaults: defaults))
        XCTAssertFalse(FileManager.default.fileExists(atPath: record.url.path), "its recording goes with it")

        XCTAssertEqual(KeyboardInterruptedRecording.note(stoppedAfter: 44.6).title, "Stopped at 0:44")
        XCTAssertEqual(KeyboardInterruptedRecording.note(stoppedAfter: 79.5).title, "Stopped at 1:19")

        let bridge = KeyboardBridge(defaults: defaults)
        bridge.heldDictation = KeyboardHeldDictation(
            id: "held", text: "Meet at ten.", heard: nil, document: field, finishedAt: 1_000, stoppedAfter: 44.6
        )
        XCTAssertEqual(bridge.heldDictation?.stoppedAfter, 44.6)
        defaults.set(Data(#"{"id":"old","text":"Hi.","finishedAt":1000}"#.utf8), forKey: "keyboard.heldDictation")
        XCTAssertEqual(bridge.heldDictation?.id, "old", "held by an earlier build")
        XCTAssertNil(bridge.heldDictation?.stoppedAfter)

        let model = KeyboardViewModel()
        model.configureFixture(snapshot: .offline, now: 1_000)
        model.offeredDictation = "Meet at ten."
        model.showStoppedNote(44.6)
        XCTAssertEqual(model.stoppedNote?.detail, "The screen locked or you left the app")
        XCTAssertEqual(model.statusSlot, .message("Stopped at 0:44. The screen locked or you left the app", isFailure: false))
        model.offeredDictation = nil
        XCTAssertNil(model.stoppedNote, "pasted: the note goes with it")
        model.offeredDictation = "Meet at ten."
        model.showStoppedNote(nil)
        XCTAssertEqual(model.statusSlot, .none, "a dictation that wasn't cut short says nothing")
    }

    /// A notification takes the audio session for a second or two: the page says its capture is
    /// interrupted, that wait isn't counted as quiet, and once the page says it's back the same
    /// dictation carries on, its quiet counted from then. Interrupted longer than
    /// `captureInterruption` (a call, an alarm), it finishes with what was said.
    func testAnInterruptedCaptureWaitsForItsPageAndTheSameDictationCarriesOn() {
        func page(_ context: String, muted: Bool = false, ended: Bool = false) -> KeyboardCapturePageState {
            KeyboardCapturePageState(cause: "test", context: context, muted: muted, ended: ended, at: 0)
        }
        XCTAssertFalse(page("running").isInterrupted)
        XCTAssertTrue(page("interrupted").isInterrupted)
        XCTAssertTrue(page("suspended").isInterrupted)
        XCTAssertTrue(page("running", muted: true).isInterrupted, "a muted track sends only silence")
        XCTAssertTrue(page("running", ended: true).isInterrupted)

        // Audio until 103, a notification from 103.2 to 108 with no audio at all, then audio
        // again: polled 30 times a second, the watchdog never ends it.
        var watch = KeyboardCaptureWatch(startedAt: 100)
        var stalledAt: [TimeInterval] = []
        for tick in 0...(30 * 20) {
            let now = 100 + Double(tick) / 30
            if now < 103 || now >= 108.5 { watch.heardAudio(at: now) }
            if abs(now - 103.2) < 0.5 / 30 { XCTAssertEqual(watch.page(page("interrupted", muted: true), at: now), .interrupted) }
            if abs(now - 108) < 0.5 / 30 {
                guard case .resumed(let after) = watch.page(page("running"), at: now) else { return XCTFail("not resumed") }
                XCTAssertEqual(after, 4.8, accuracy: 0.01)
            }
            if watch.isStalled(now: now) { stalledAt.append(now) }
        }
        XCTAssertEqual(stalledAt, [], "5 s without audio, but interrupted for all of it")
        XCTAssertEqual(watch.interruptedSince, nil)

        // Back at 108, the page sends nothing more: quiet counts from 108, not from 103.
        var quiet = KeyboardCaptureWatch(startedAt: 100)
        quiet.heardAudio(at: 103)
        _ = quiet.page(page("suspended"), at: 103.5)
        _ = quiet.page(page("running"), at: 108)
        XCTAssertFalse(quiet.isStalled(now: 113.9))
        XCTAssertTrue(quiet.isStalled(now: 114.1), "6 s after it came back")

        // Muted, the page still sends silence: the interruption's limit ends it all the same.
        var call = KeyboardCaptureWatch(startedAt: 100)
        XCTAssertEqual(call.page(page("running", muted: true), at: 101), .interrupted)
        XCTAssertNil(call.page(page("interrupted", muted: true), at: 102), "still the same interruption")
        call.heardAudio(at: 115)
        XCTAssertFalse(call.isStalled(now: 101 + KeyboardJobWatchdog.captureInterruption - 0.1))
        XCTAssertTrue(call.isStalled(now: 101 + KeyboardJobWatchdog.captureInterruption + 0.1), "finishes with what was said")
        XCTAssertTrue(KeyboardJobWatchdog.captureInterruption > KeyboardJobWatchdog.captureSilence)
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
        record.recordOpenedHex(at: now)
        XCTAssertEqual(record.handedToHex?.reason, .openedHex)
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .fifteenMinutes, "Hex was opened for a dictation")
        record.recordHandedToHex(.memory, at: now)
        record.recordOpenedHex(at: now.addingTimeInterval(60))
        XCTAssertEqual(record.handedToHex?.reason, .memory, "Hex keeps the keyboard's reason")
        record.clearHandedToHex()
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .off, "the keyboard dictated by itself again")

        let languages = LanguageSettings(defaults: defaults)
        languages.markKeyboardNeedsHex(at: now)
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .fifteenMinutes, "no speech model in the keyboard")
        languages.clearKeyboardNeedsHex()

        // The multilingual model never takes dictation away from the keyboard: it dictates
        // that model's languages itself, with Apple's speech recognition until it has the model.
        languages.chooseDictationLanguage("nl-NL")
        let multilingual = MultilingualParakeetSettings(defaults: defaults)
        multilingual.isEnabled = true
        let keyboardParakeet = KeyboardParakeetSettings(defaults: defaults)
        XCTAssertTrue(keyboardParakeet.isEnabled, "Speech model in keyboard is on by default")
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .off, "only Hex has the multilingual model loaded")
        keyboardParakeet.setKeyboardState(.preparing, multilingual: true)
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .off, "the keyboard is still loading it")
        keyboardParakeet.setKeyboardState(.ready, multilingual: true)
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .off, "the keyboard has it loaded")
        keyboardParakeet.isEnabled = false
        XCTAssertEqual(bridge.defaultWarmDuration(osMajorVersion: 26, now: now), .off, "Speech model in keyboard off")
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

    /// The keyboard's New speech model notice: only where the keyboard would dictate with Hex's
    /// model and it isn't downloaded, at most once a day and three times, never after Open Hex.
    func testTheNewSpeechModelNoticeShowsOnlyWhereItHelpsAndNotOften() throws {
        let suite = "anomaly.hex.tests.model-notice"
        for name in [suite, suite + ".unmeasured", suite + ".small", suite + ".opened"] {
            UserDefaults(suiteName: name)?.removePersistentDomain(forName: name)
        }
        let settings = KeyboardParakeetSettings(defaults: UserDefaults(suiteName: suite))
        settings.recordKeyboardLimit(177 * KeyboardParakeetBudget.megabyte)
        func awaits(_ settings: KeyboardParakeetSettings, english: Bool = true, other: Bool = false, downloaded: Bool = false) -> Bool {
            KeyboardParakeetPlan.resolve(
                isEnabled: settings.isEnabled,
                fitsDevice: true,
                twoLanguageEnabled: other,
                multilingualLanguage: nil,
                wantedLanguage: english ? "en-US" : "fr-FR",
                keyboardLimit: settings.keyboardLimit,
                v2Downloaded: downloaded,
                ultraDownloaded: false
            ).awaitsV2Download
        }
        XCTAssertTrue(awaits(settings), "English, the switch on, an iPhone and a keyboard with the memory, not downloaded")
        XCTAssertFalse(awaits(settings, downloaded: true), "downloaded")
        XCTAssertFalse(awaits(settings, english: false), "not English")
        XCTAssertFalse(awaits(settings, other: true), "Two-language dictation or the multilingual model")
        let small = KeyboardParakeetSettings(defaults: UserDefaults(suiteName: suite + ".small"))
        small.recordKeyboardLimit(100 * KeyboardParakeetBudget.megabyte)
        XCTAssertFalse(awaits(small), "a phone whose keyboard can't use it")
        XCTAssertFalse(awaits(KeyboardParakeetSettings(defaults: UserDefaults(suiteName: suite + ".unmeasured"))), "a keyboard not measured yet")
        settings.isEnabled = false
        XCTAssertFalse(awaits(settings), "switched off")

        var utc = Calendar(identifier: .gregorian)
        utc.timeZone = TimeZone(identifier: "UTC")!
        let day = Date(timeIntervalSince1970: 1_795_000_000)
        let notice = KeyboardParakeetNotice(defaults: UserDefaults(suiteName: suite))
        XCTAssertTrue(notice.mayShow(now: day, calendar: utc))
        notice.recordShown(now: day, calendar: utc)
        XCTAssertFalse(notice.mayShow(now: day.addingTimeInterval(3_600), calendar: utc), "the same day twice")
        XCTAssertTrue(notice.mayShow(now: day.addingTimeInterval(86_400), calendar: utc))
        notice.recordShown(now: day.addingTimeInterval(86_400), calendar: utc)
        notice.recordShown(now: day.addingTimeInterval(3 * 86_400), calendar: utc)
        XCTAssertFalse(notice.mayShow(now: day.addingTimeInterval(9 * 86_400), calendar: utc), "three showings in all")
        let opened = KeyboardParakeetNotice(defaults: UserDefaults(suiteName: suite + ".opened"))
        opened.recordOpened()
        XCTAssertFalse(opened.mayShow(now: day, calendar: utc), "never again once Hex was opened from it")
    }

    /// The notice opens Hex, starting nothing, and never shows in a password or one-time-code
    /// field.
    @MainActor
    func testTheModelNoticeOpensHexAndNeverShowsInAPasswordField() throws {
        let group = try XCTUnwrap(UserDefaults(suiteName: KeyboardBridge.appGroup))
        let keys = [KeyboardParakeetNotice.shownKey, KeyboardParakeetNotice.dayKey, KeyboardParakeetNotice.openedKey, KeyboardParakeetNotice.dismissedKey]
        keys.forEach { group.removeObject(forKey: $0) }
        defer { keys.forEach { group.removeObject(forKey: $0) } }
        let model = KeyboardViewModel()
        model.configureFixture(snapshot: .offline, now: 10_000)
        var opened: [URL] = []
        model.onOpenURL = { opened.append($0) }
        func keyboard(_ target: FixtureTextTarget) -> FullKeyboardView {
            let view = FullKeyboardView(model: model, target: target)
            view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
            view.layoutIfNeeded()
            view.offerModelNotice(awaitsDownload: true)
            return view
        }
        let password = FixtureTextTarget(text: "")
        password.isSecureTextEntry = true
        XCTAssertNil(keyboard(password).modelNotice)
        let code = FixtureTextTarget(text: "")
        code.textContentType = .oneTimeCode
        XCTAssertNil(keyboard(code).modelNotice)
        XCTAssertEqual(group.integer(forKey: KeyboardParakeetNotice.shownKey), 0, "not counted where it didn't show")

        let view = keyboard(FixtureTextTarget(text: ""))
        XCTAssertEqual(view.modelNotice, KeyboardStripNotice(title: "New speech model", detail: "Downloads in Hex on Wi-Fi", button: "Open Hex", dismissible: true))
        view.stripView.onOpenNotice?()
        XCTAssertEqual(opened, [KeyboardParakeetNotice.opensHex])
        XCTAssertNil(KeyboardLaunchRequest(url: KeyboardParakeetNotice.opensHex), "Hex opens as it is: no page, no session")
        XCTAssertNil(view.modelNotice, "gone once tapped")
        XCTAssertFalse(KeyboardParakeetNotice().mayShow(now: Date().addingTimeInterval(7 * 86_400)), "and never again")
    }

    /// Ryan: "make sure that ... the notice bar is able to be turned off." Its ✕ puts it away at
    /// once and for good on this iPhone, whatever the counts; it opens nothing and isn't Open Hex.
    @MainActor
    func testTheModelNoticesDismissPutsItAwayForGood() throws {
        let group = try XCTUnwrap(UserDefaults(suiteName: KeyboardBridge.appGroup))
        let keys = [KeyboardParakeetNotice.shownKey, KeyboardParakeetNotice.dayKey, KeyboardParakeetNotice.openedKey, KeyboardParakeetNotice.dismissedKey]
        keys.forEach { group.removeObject(forKey: $0) }
        defer { keys.forEach { group.removeObject(forKey: $0) } }
        let model = KeyboardViewModel()
        model.configureFixture(snapshot: .offline, now: 10_000)
        var opened: [URL] = []
        model.onOpenURL = { opened.append($0) }
        let view = FullKeyboardView(model: model, target: FixtureTextTarget(text: ""))
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        view.offerModelNotice(awaitsDownload: true)
        XCTAssertEqual(view.modelNotice?.dismissible, true)
        view.layoutIfNeeded()
        let dismiss = try XCTUnwrap(dismissControl(in: view.stripView), "the notice's ✕")
        XCTAssertGreaterThanOrEqual(dismiss.bounds.width, 34)
        XCTAssertGreaterThanOrEqual(dismiss.bounds.height, 28, "a real touch target")
        let open = try XCTUnwrap(control(in: view.stripView, titled: "Open Hex"))
        XCTAssertFalse(dismiss.convert(dismiss.bounds, to: view).intersects(open.convert(open.bounds, to: view)), "a miss on ✕ never hits Open Hex")
        dismiss.sendActions(for: .touchUpInside)
        XCTAssertNil(view.modelNotice, "gone at once")
        XCTAssertEqual(opened, [], "opens nothing")
        XCTAssertFalse(group.bool(forKey: KeyboardParakeetNotice.openedKey), "not Open Hex")
        XCTAssertEqual(group.integer(forKey: KeyboardParakeetNotice.shownKey), 1, "under three showings")
        let notice = KeyboardParakeetNotice()
        XCTAssertFalse(notice.mayShow(now: Date().addingTimeInterval(86_400)), "not the next day")
        XCTAssertFalse(notice.mayShow(now: Date().addingTimeInterval(30 * 86_400)), "nor ever")
        view.offerModelNotice(awaitsDownload: true)
        XCTAssertNil(view.modelNotice, "nor when the keyboard appears again")
        let again = FullKeyboardView(model: model, target: FixtureTextTarget(text: ""))
        again.offerModelNotice(now: Date().addingTimeInterval(2 * 86_400), awaitsDownload: true)
        XCTAssertNil(again.modelNotice, "nor in a new keyboard on a new day")
    }

    private func dismissControl(in view: UIView) -> UIButton? {
        for sub in view.subviews {
            if let button = sub as? UIButton, !button.isHidden, button.accessibilityLabel == "Dismiss" { return button }
            if let found = dismissControl(in: sub) { return found }
        }
        return nil
    }

    func testLocalCapturePhaseProjectsSnapshotsAndCaptureOwnership() {
        let t0 = 50_000.0

        // Idle and armed stay unseen (nil snapshot); only non-failed armed captures match jobID.
        let idle: KeyboardLocalCapturePhase = .idle
        XCTAssertFalse(idle.isActive)
        XCTAssertFalse(idle.isCapturing("job-1"))
        XCTAssertEqual(idle.visibleStage, .idle)
        XCTAssertNil(idle.snapshot(now: t0, loudnessDB: -62))

        let armed: KeyboardLocalCapturePhase = .armed(jobID: "job-1")
        XCTAssertTrue(armed.isActive)
        XCTAssertTrue(armed.isCapturing("job-1"))
        XCTAssertFalse(armed.isCapturing("other"))
        XCTAssertEqual(armed.visibleStage, .idle)
        XCTAssertNil(armed.snapshot(now: t0 + 0.05, loudnessDB: -30), "armed stays unseen")

        let armedFailed: KeyboardLocalCapturePhase = .armed(jobID: "job-1", failed: true)
        XCTAssertTrue(armedFailed.isActive)
        XCTAssertFalse(armedFailed.isCapturing("job-1"), "failed armed capture accepts no further events")
        XCTAssertEqual(armedFailed.visibleStage, .idle)
        XCTAssertNil(armedFailed.snapshot(now: t0 + 0.10, loudnessDB: -62))

        // While starting, the snapshot is .starting with no recording clock yet.
        let starting: KeyboardLocalCapturePhase = .starting(jobID: "job-1")
        XCTAssertTrue(starting.isActive)
        XCTAssertTrue(starting.isCapturing("job-1"))
        XCTAssertFalse(starting.isCapturing("other"))
        XCTAssertEqual(starting.visibleStage, .starting)
        let startingSnap = starting.snapshot(now: t0 + 0.15, loudnessDB: -25)
        XCTAssertEqual(startingSnap?.state, .starting)
        XCTAssertEqual(startingSnap?.jobID, "job-1")
        XCTAssertNil(startingSnap?.recordingStartedAt)

        // Recording projects startedAt and loudnessDB so the elapsed clock starts at first audio.
        let recording: KeyboardLocalCapturePhase = .recording(jobID: "job-1", startedAt: t0 + 0.25)
        XCTAssertTrue(recording.isActive)
        XCTAssertTrue(recording.isCapturing("job-1"))
        XCTAssertEqual(recording.visibleStage, .recording)
        let recSnap = recording.snapshot(now: t0 + 1.25, loudnessDB: -28)
        XCTAssertEqual(recSnap?.state, .recording)
        XCTAssertEqual(recSnap?.recordingStartedAt, t0 + 0.25)
        XCTAssertEqual(recSnap?.loudnessDB, -28)
        XCTAssertEqual(recSnap?.recordingElapsed(at: t0 + 1.25) ?? -1, 1.0, accuracy: 0.001)

        // Transcribing is active but no longer capturing.
        let transcribing: KeyboardLocalCapturePhase = .transcribing(jobID: "job-1")
        XCTAssertTrue(transcribing.isActive)
        XCTAssertFalse(transcribing.isCapturing("job-1"))
        XCTAssertEqual(transcribing.visibleStage, .transcribing)
        XCTAssertEqual(transcribing.snapshot(now: t0 + 1.5, loudnessDB: -62)?.state, .transcribing)

        // Finished projects the KeyboardResult payload; failed expires once until <= now.
        let result = KeyboardResult(id: "res-1", transcript: "Ship it.", heard: "ship it")
        let finished: KeyboardLocalCapturePhase = .finished(jobID: "job-1", result: result)
        XCTAssertFalse(finished.isActive)
        XCTAssertEqual(finished.visibleStage, .finished)
        let finSnap = finished.snapshot(now: t0 + 2.0, loudnessDB: -62)
        XCTAssertEqual(finSnap?.state, .ready)
        XCTAssertEqual(finSnap?.resultID, "res-1")
        XCTAssertEqual(finSnap?.transcript, "Ship it.")
        XCTAssertEqual(finSnap?.heard, "ship it")

        let failed: KeyboardLocalCapturePhase = .failed(message: "Mic busy", until: t0 + 8.0)
        XCTAssertFalse(failed.isActive)
        XCTAssertEqual(failed.visibleStage, .failed)
        let failSnap = failed.snapshot(now: t0 + 7.9, loudnessDB: -62)
        XCTAssertEqual(failSnap?.state, .failed)
        XCTAssertEqual(failSnap?.heartbeat, 0)
        XCTAssertEqual(failSnap?.expiresAt, 0)
        XCTAssertEqual(failSnap?.message, "Mic busy")
        XCTAssertNil(failed.snapshot(now: t0 + 8.0, loudnessDB: -62))
    }

    func testKeyboardViewModelShowsStartingUntilAudioFlowsAndHandlesEarlyCancelAndFailure() {
        let t0 = 70_000.0
        let model = KeyboardViewModel()
        func show(_ snap: KeyboardSnapshot, at now: TimeInterval) {
            model.applyBridgeState(
                hasFullAccess: true,
                snapshot: snap,
                isModelInstalled: true,
                style: .polished,
                now: now,
                animated: false
            )
        }

        // Starting snapshot keeps UI in .starting with no running clock and quiet scope.
        let starting = KeyboardSnapshot(state: .starting, heartbeat: t0, expiresAt: t0 + 60, jobID: "j1")
        show(starting, at: t0)
        XCTAssertEqual(model.stagePhase, .starting)
        XCTAssertEqual(model.dotScopePhase, .starting)
        XCTAssertEqual(model.liveLoudnessDB, -62.0)
        XCTAssertEqual(model.clock.recordingText, "0:00")

        // First audio arrives -> .recording, and clock advances from first audio.
        let recording = KeyboardSnapshot(
            state: .recording,
            heartbeat: t0 + 2.25,
            expiresAt: t0 + 60,
            recordingStartedAt: t0 + 0.25,
            loudnessDB: -26,
            jobID: "j1"
        )
        show(recording, at: t0 + 2.25)
        XCTAssertEqual(model.stagePhase, .recording)
        XCTAssertEqual(model.dotScopePhase, .recording)
        XCTAssertEqual(model.liveLoudnessDB, -26)
        XCTAssertEqual(model.clock.recordingText, "0:02")

        // Early cancel while starting returns cleanly to idle.
        let startingAgain = KeyboardSnapshot(state: .starting, heartbeat: t0 + 10, expiresAt: t0 + 70, jobID: "j2")
        show(startingAgain, at: t0 + 10)
        XCTAssertEqual(model.stagePhase, .starting)
        model.setPendingCommand(
            KeyboardViewModel.PendingCommand(kind: .cancel, jobID: "j2", sentAt: t0 + 10.05),
            now: t0 + 10.05,
            animated: false
        )
        XCTAssertEqual(model.stagePhase, .idle)
        show(.offline, at: t0 + 10.1)
        XCTAssertEqual(model.stagePhase, .idle)
        XCTAssertNil(model.pendingCommand)

        // Early stop while starting also returns cleanly to idle (never transcribing).
        let startingThird = KeyboardSnapshot(state: .starting, heartbeat: t0 + 20, expiresAt: t0 + 80, jobID: "j3")
        show(startingThird, at: t0 + 20)
        model.setPendingCommand(
            KeyboardViewModel.PendingCommand(kind: .stop, jobID: "j3", sentAt: t0 + 20.05),
            now: t0 + 20.05,
            animated: false
        )
        XCTAssertEqual(model.stagePhase, .idle)

        // Startup failure shows failure briefly and returns cleanly to idle once expired.
        let failed = KeyboardSnapshot(
            state: .failed,
            heartbeat: t0 + 30,
            expiresAt: t0 + 36,
            message: UserFacingError.Context.microphone.fallback,
            messageAt: t0 + 30
        )
        show(failed, at: t0 + 30)
        XCTAssertEqual(model.stagePhase, .failed)
        XCTAssertEqual(model.statusMessage, UserFacingError.Context.microphone.fallback)
        show(failed, at: t0 + 39)
        XCTAssertEqual(model.stagePhase, .idle)
        XCTAssertNil(model.statusMessage)

        // Orphaned starting job drops cleanly to idle.
        XCTAssertTrue(starting.isOrphanedJob(at: t0 + KeyboardSnapshot.orphanedJobTimeout + 1))
        show(starting, at: t0 + KeyboardSnapshot.orphanedJobTimeout + 1)
        XCTAssertEqual(model.stagePhase, .idle)

        // Recording panel keeps spinner in Done's spot while starting, then shows checkmark when live.
        let panel = HexKeyRecordingPanel(loudness: { -62 })
        panel.update(phase: .starting, style: .polished)
        XCTAssertEqual(panel.scope.phase, .starting)
        XCTAssertEqual(panel.debugMotionSample()["doneSymbol"], 0)
        panel.update(phase: .recording, style: .polished)
        XCTAssertEqual(panel.scope.phase, .recording)
        XCTAssertEqual(panel.debugMotionSample()["doneSymbol"], 1)
        panel.tearDown()
    }

    private func control(in view: UIView, titled title: String) -> UIButton? {
        for sub in view.subviews {
            if let button = sub as? UIButton, !button.isHidden, button.configuration?.title == title { return button }
            if let found = control(in: sub, titled: title) { return found }
        }
        return nil
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
