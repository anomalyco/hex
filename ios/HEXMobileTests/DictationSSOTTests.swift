import XCTest
@testable import HEX

final class DictationSSOTTests: XCTestCase {
    // MARK: - DictationRoute

    func testDictationRoutePrecedenceTable() {
        // Full Access off always routes to setUp.
        for modelInstalled in [false, true] {
            for warm in [false, true] {
                for local in [false, true] {
                    XCTAssertEqual(
                        DictationRoute.resolve(
                            hasFullAccess: false,
                            isModelInstalled: modelInstalled,
                            isWarmAvailable: warm,
                            localAvailable: local,
                            opensHexReason: .memory
                        ),
                        .setUp
                    )
                }
            }
        }

        // Full Access on, Hex's model not installed: the keyboard still dictates by itself where
        // it can (it uses its own speech model), else needsModel.
        for warm in [false, true] {
            for local in [false, true] {
                XCTAssertEqual(
                    DictationRoute.resolve(
                        hasFullAccess: true,
                        isModelInstalled: false,
                        isWarmAvailable: warm,
                        localAvailable: local,
                        opensHexReason: .speechModel
                    ),
                    local ? .inKeyboard : .needsModel
                )
            }
        }

        // Warm beats local and opensHex.
        for local in [false, true] {
            let route = DictationRoute.resolve(
                hasFullAccess: true,
                isModelInstalled: true,
                isWarmAvailable: true,
                localAvailable: local,
                opensHexReason: .memory
            )
            XCTAssertEqual(route, .warm)
            XCTAssertTrue(route.dictatesInPlace)
        }

        // Local beats opensHex when warm is off.
        let localRoute = DictationRoute.resolve(
            hasFullAccess: true,
            isModelInstalled: true,
            isWarmAvailable: false,
            localAvailable: true,
            opensHexReason: nil
        )
        XCTAssertEqual(localRoute, .inKeyboard)
        XCTAssertTrue(localRoute.dictatesInPlace)

        // Otherwise opensHex carries the reason (or nil when unsupported).
        for reason: KeyboardSelfDictation.Reason? in [nil, .memory, .speechModel, .multilingual, .openedHex] {
            let route = DictationRoute.resolve(
                hasFullAccess: true,
                isModelInstalled: true,
                isWarmAvailable: false,
                localAvailable: false,
                opensHexReason: reason
            )
            XCTAssertEqual(route, .opensHex(reason))
            XCTAssertFalse(route.dictatesInPlace)
        }
    }

    @MainActor
    func testFailedWarmSessionRetriesWarmWithBarAndFullKeyboardParity() {
        let t0 = 40_000.0
        let errorText = "Hex couldn't start the microphone. Try again."
        let model = KeyboardViewModel()
        var warmTaps = 0
        var localTaps = 0
        var opened: [URL] = []
        model.onDictateTap = { warmTaps += 1 }
        model.onLocalDictateTap = { localTaps += 1 }
        model.onOpenURL = { opened.append($0) }

        let warmFailed = KeyboardSnapshot(
            state: .failed,
            heartbeat: t0,
            expiresAt: t0 + 900,
            message: errorText,
            messageAt: t0
        )
        model.applyBridgeState(
            hasFullAccess: true,
            snapshot: warmFailed,
            isModelInstalled: true,
            style: .polished,
            now: t0 + 1,
            animated: false
        )

        XCTAssertEqual(model.stagePhase, .failed)
        XCTAssertTrue(model.isWarmAvailable)
        XCTAssertEqual(model.route, .warm)
        XCTAssertTrue(model.dictatesThroughHex)
        XCTAssertTrue(model.canPushToTalk)
        XCTAssertEqual(model.statusSlot, .message(errorText, isFailure: true), "failure wins over warm countdown")
        XCTAssertEqual(model.failureMessage, errorText)

        let full = FullKeyboardView(model: model, target: FixtureTextTarget())
        full.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        full.layoutIfNeeded()
        XCTAssertTrue(full.hexKey.isWarm)
        full.hexKeyHeld()
        XCTAssertEqual(warmTaps, 1, "full keyboard Hex key retries warm session")
        XCTAssertEqual(localTaps, 0)
        XCTAssertTrue(opened.isEmpty)
    }

    @MainActor
    func testLocalFailureStaysInKeyboardAndOneShotFailureRemainsVisibleInStatusSlot() throws {
        let t0 = 41_000.0
        let errorText = "Hex couldn't start the microphone. Try again."

        // Local keyboard failure: heartbeat/expiresAt are 0, so route stays inKeyboard and both surfaces show it.
        let localModel = KeyboardViewModel()
        var warmTaps = 0
        var localTaps = 0
        localModel.onDictateTap = { warmTaps += 1 }
        localModel.onLocalDictateTap = { localTaps += 1 }
        let localSnap = try XCTUnwrap(
            KeyboardLocalCapturePhase.failed(message: errorText, until: t0 + 6)
                .snapshot(now: t0, loudnessDB: -62)
        )
        XCTAssertEqual(localSnap.heartbeat, 0)
        XCTAssertEqual(localSnap.expiresAt, 0)
        localModel.applyBridgeState(
            hasFullAccess: true,
            snapshot: localSnap,
            isModelInstalled: true,
            style: .polished,
            now: t0,
            animated: false
        )
        XCTAssertFalse(localModel.isWarmAvailable)
        XCTAssertEqual(localModel.route, .inKeyboard)
        XCTAssertTrue(localModel.canPushToTalk)
        XCTAssertEqual(localModel.statusSlot, .message(errorText, isFailure: true))
        XCTAssertEqual(localModel.failureMessage, errorText)

        let full = FullKeyboardView(model: localModel, target: FixtureTextTarget())
        full.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        full.layoutIfNeeded()
        XCTAssertFalse(full.hexKey.isWarm)
        full.hexKeyHeld()
        XCTAssertEqual(localTaps, 1, "local failure retries in the keyboard")
        XCTAssertEqual(warmTaps, 0)

        // One-shot failure (warm off, local unavailable): bar StatusSlot still shows the failure.
        let oneShot = KeyboardViewModel()
        oneShot.opensHexReason = .memory
        oneShot.applyBridgeState(
            hasFullAccess: true,
            snapshot: KeyboardSnapshot(state: .failed, heartbeat: 0, expiresAt: 0, message: errorText, messageAt: t0),
            isModelInstalled: true,
            style: .polished,
            now: t0 + 1,
            animated: false
        )
        XCTAssertEqual(oneShot.route, .opensHex(.memory))
        XCTAssertFalse(oneShot.canPushToTalk)
        XCTAssertEqual(oneShot.statusSlot, .message(errorText, isFailure: true))
        XCTAssertEqual(oneShot.failureMessage, errorText)
    }

    @MainActor
    func testStatusSlotIsMutuallyExclusiveAcrossStages() {
        let t0 = 42_000.0
        let model = KeyboardViewModel()
        let note = TranslationFailure(.notDownloaded, source: Locale.Language(identifier: "es"), target: Locale.Language(identifier: "en"), detail: nil)

        // No Full Access always shows noAccess.
        model.applyBridgeState(
            hasFullAccess: false,
            snapshot: KeyboardSnapshot(state: .failed, heartbeat: 0, expiresAt: 0, message: "Oops", messageAt: t0),
            isModelInstalled: true,
            style: .polished,
            now: t0,
            animated: false
        )
        XCTAssertEqual(model.statusSlot, .noAccess)
        XCTAssertNil(model.failureMessage)

        // Starting (PR #2478) -> .starting.
        model.applyBridgeState(
            hasFullAccess: true,
            snapshot: KeyboardSnapshot(state: .starting, heartbeat: t0, expiresAt: t0 + 60, jobID: "j1"),
            isModelInstalled: true,
            style: .polished,
            now: t0,
            animated: false
        )
        XCTAssertEqual(model.statusSlot, .starting)

        // Recording -> .recording.
        model.applyBridgeState(
            hasFullAccess: true,
            snapshot: KeyboardSnapshot(state: .recording, heartbeat: t0, expiresAt: t0 + 60, recordingStartedAt: t0, jobID: "j1"),
            isModelInstalled: true,
            style: .polished,
            now: t0,
            animated: false
        )
        XCTAssertEqual(model.statusSlot, .recording)

        // Transcribing -> .none.
        model.applyBridgeState(
            hasFullAccess: true,
            snapshot: KeyboardSnapshot(state: .transcribing, heartbeat: t0, expiresAt: t0 + 60, jobID: "j1"),
            isModelInstalled: true,
            style: .polished,
            now: t0,
            animated: false
        )
        XCTAssertEqual(model.statusSlot, .none)

        // Idle + warm + translationNote -> translationNote wins over warm.
        model.translationNote = note
        model.applyBridgeState(
            hasFullAccess: true,
            snapshot: KeyboardSnapshot(state: .ready, heartbeat: t0, expiresAt: t0 + 900),
            isModelInstalled: true,
            style: .polished,
            now: t0,
            animated: false
        )
        XCTAssertEqual(model.statusSlot, .translationNote(note))

        // Idle + warm + non-failure message -> .message(isFailure: false), failureMessage nil.
        model.translationNote = nil
        model.applyBridgeState(
            hasFullAccess: true,
            snapshot: KeyboardSnapshot(state: .ready, heartbeat: t0, expiresAt: t0 + 900, message: "Interrupted", messageAt: t0),
            isModelInstalled: true,
            style: .polished,
            now: t0,
            animated: false
        )
        XCTAssertEqual(model.statusSlot, .message("Interrupted", isFailure: false))
        XCTAssertNil(model.failureMessage)

        // Idle + warm + no message -> .warm.
        model.applyBridgeState(
            hasFullAccess: true,
            snapshot: KeyboardSnapshot(state: .ready, heartbeat: t0, expiresAt: t0 + 900),
            isModelInstalled: true,
            style: .polished,
            now: t0,
            animated: false
        )
        XCTAssertEqual(model.statusSlot, .warm)
    }

    // MARK: - DictationEngine

    func testDictationEngineDisplayNamesAndCapabilities() {
        XCTAssertEqual(DictationEngine.undecided.displayName, "Local speech")
        XCTAssertEqual(DictationEngine.appleSpeech.displayName, "Apple Speech")
        XCTAssertEqual(DictationEngine.onDeviceRecognizer.displayName, "Apple Speech")
        XCTAssertEqual(DictationEngine.parakeetV2.displayName, "Parakeet V2")
        XCTAssertEqual(DictationEngine.parakeetUltra.displayName, "Parakeet Ultra")

        XCTAssertFalse(DictationEngine.undecided.isAppleSpeech)
        XCTAssertTrue(DictationEngine.appleSpeech.isAppleSpeech)
        XCTAssertTrue(DictationEngine.onDeviceRecognizer.isAppleSpeech)
        XCTAssertFalse(DictationEngine.parakeetV2.isAppleSpeech)
        XCTAssertFalse(DictationEngine.parakeetUltra.isAppleSpeech)

        XCTAssertTrue(DictationEngine.undecided.downloads)
        XCTAssertTrue(DictationEngine.appleSpeech.downloads)
        XCTAssertFalse(DictationEngine.onDeviceRecognizer.downloads)
        XCTAssertTrue(DictationEngine.parakeetV2.downloads)
        XCTAssertTrue(DictationEngine.parakeetUltra.downloads)

        XCTAssertFalse(DictationEngine.undecided.keyboardSpeechFollowsApp(keyboardNeedsHex: false))
        XCTAssertFalse(DictationEngine.undecided.keyboardSpeechFollowsApp(keyboardNeedsHex: true))
        XCTAssertFalse(DictationEngine.parakeetV2.keyboardSpeechFollowsApp(keyboardNeedsHex: false))
        XCTAssertFalse(DictationEngine.parakeetV2.keyboardSpeechFollowsApp(keyboardNeedsHex: true))
        XCTAssertTrue(DictationEngine.parakeetUltra.keyboardSpeechFollowsApp(keyboardNeedsHex: false))
        XCTAssertTrue(DictationEngine.parakeetUltra.keyboardSpeechFollowsApp(keyboardNeedsHex: true))
        XCTAssertTrue(DictationEngine.appleSpeech.keyboardSpeechFollowsApp(keyboardNeedsHex: false))
        XCTAssertFalse(DictationEngine.appleSpeech.keyboardSpeechFollowsApp(keyboardNeedsHex: true))
        XCTAssertTrue(DictationEngine.onDeviceRecognizer.keyboardSpeechFollowsApp(keyboardNeedsHex: false))
        XCTAssertFalse(DictationEngine.onDeviceRecognizer.keyboardSpeechFollowsApp(keyboardNeedsHex: true))
    }

    // MARK: - KeyboardParakeetPlan & Language Normalization

    func testKeyboardParakeetPlanTable() {
        let mb = KeyboardParakeetBudget.megabyte
        let largeLimit = 177 * mb
        let smallLimit = 100 * mb

        func plan(
            enabled: Bool = true,
            fits: Bool = true,
            twoLanguage: Bool = false,
            multilingual: String? = nil,
            wanted: String? = "en-US",
            limit: Int? = largeLimit,
            v2Downloaded: Bool = false,
            ultraDownloaded: Bool = false
        ) -> KeyboardParakeetPlan {
            KeyboardParakeetPlan.resolve(
                isEnabled: enabled,
                fitsDevice: fits,
                twoLanguageEnabled: twoLanguage,
                multilingualLanguage: multilingual,
                wantedLanguage: wanted,
                keyboardLimit: limit,
                v2Downloaded: v2Downloaded,
                ultraDownloaded: ultraDownloaded
            )
        }

        XCTAssertEqual(plan(enabled: false), .off)
        XCTAssertEqual(plan(fits: false), .off)
        XCTAssertNil(plan(enabled: false).modelChoice)

        XCTAssertEqual(plan(twoLanguage: true), .twoLanguage)
        XCTAssertEqual(KeyboardParakeetDownload.hold(plan: .twoLanguage, unmetered: true, freeBytes: 8_000_000_000), .otherEngine)

        // Multilingual downloaded + fits -> parakeetUltra choice; too small -> nil choice (no drift!).
        let ultraReady = plan(multilingual: "nl-NL", limit: largeLimit, ultraDownloaded: true)
        XCTAssertEqual(ultraReady, .multilingual(language: "nl-NL", downloaded: true, fitsKeyboardLimit: true))
        XCTAssertEqual(ultraReady.modelChoice?.model, .parakeetUltra)
        XCTAssertEqual(ultraReady.modelChoice?.language, "nl-NL")
        XCTAssertFalse(ultraReady.awaitsV2Download)
        XCTAssertEqual(KeyboardParakeetDownload.hold(plan: ultraReady, unmetered: true, freeBytes: 8_000_000_000), .otherEngine)

        let ultraTooSmall = plan(multilingual: "nl-NL", limit: smallLimit, ultraDownloaded: true)
        XCTAssertEqual(ultraTooSmall, .multilingual(language: "nl-NL", downloaded: true, fitsKeyboardLimit: false))
        XCTAssertNil(ultraTooSmall.modelChoice, "measured keyboard limit below requiredLimit prevents loading")

        let ultraMissing = plan(multilingual: "nl-NL", limit: largeLimit, ultraDownloaded: false)
        XCTAssertNil(ultraMissing.modelChoice)

        // Non-English and nil wanted language.
        XCTAssertEqual(plan(wanted: "es-ES"), .notEnglish)
        XCTAssertEqual(plan(wanted: nil), .notEnglish)
        XCTAssertEqual(KeyboardParakeetDownload.hold(plan: .notEnglish, unmetered: true, freeBytes: 8_000_000_000), .notEnglish)

        // Unmeasured limit holds V2 auto-download and hides notice, but permits V2 choice if downloaded (e.g. simulator).
        let unmeasuredEmpty = plan(limit: nil, v2Downloaded: false)
        XCTAssertEqual(unmeasuredEmpty, .unmeasured(language: "en-US", v2Downloaded: false))
        XCTAssertNil(unmeasuredEmpty.modelChoice)
        XCTAssertFalse(unmeasuredEmpty.awaitsV2Download)
        XCTAssertEqual(KeyboardParakeetDownload.hold(plan: unmeasuredEmpty, unmetered: true, freeBytes: 8_000_000_000), .keyboardNotMeasured)

        let unmeasuredDownloaded = plan(limit: nil, v2Downloaded: true)
        XCTAssertEqual(unmeasuredDownloaded.modelChoice?.model, .parakeetV2)
        XCTAssertFalse(unmeasuredDownloaded.awaitsV2Download)

        // Too small keyboard blocks V2 load, notice, and auto-download consistently.
        let tooSmall = plan(limit: smallLimit, v2Downloaded: true)
        XCTAssertEqual(tooSmall, .tooSmall)
        XCTAssertNil(tooSmall.modelChoice, "measured small keyboard never attempts V2 load")
        XCTAssertFalse(tooSmall.awaitsV2Download)
        XCTAssertEqual(KeyboardParakeetDownload.hold(plan: tooSmall, unmetered: true, freeBytes: 8_000_000_000), .keyboardTooSmall)

        // Awaiting V2 download shows notice and allows auto-download on Wi-Fi with space.
        let awaiting = plan(limit: largeLimit, v2Downloaded: false)
        XCTAssertEqual(awaiting, .awaitingV2Download(language: "en-US"))
        XCTAssertNil(awaiting.modelChoice)
        XCTAssertTrue(awaiting.awaitsV2Download)
        XCTAssertNil(KeyboardParakeetDownload.hold(plan: awaiting, unmetered: true, freeBytes: 8_000_000_000))
        XCTAssertEqual(KeyboardParakeetDownload.hold(plan: awaiting, unmetered: false, freeBytes: 8_000_000_000), .notOnWiFi)

        // Ready V2 selects Parakeet V2 and hides notice.
        let ready = plan(limit: largeLimit, v2Downloaded: true)
        XCTAssertEqual(ready, .readyV2(language: "en-US"))
        XCTAssertEqual(ready.modelChoice?.model, .parakeetV2)
        XCTAssertEqual(ready.modelChoice?.language, "en-US")
        XCTAssertFalse(ready.awaitsV2Download)
    }

    func testWantedDictationLanguagePrecedenceAndSelfDictationHandoffReason() throws {
        let suite = "hex.tests.ssot.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        let languages = LanguageSettings(defaults: defaults)
        XCTAssertEqual(languages.wantedDictationLanguage(preferred: ["fr-FR", "en-US"]), "fr-FR")
        XCTAssertNil(languages.wantedDictationLanguage(preferred: []))

        languages.dictationLanguage = "es-MX"
        XCTAssertEqual(languages.wantedDictationLanguage(preferred: ["fr-FR"]), "es-MX")

        let selfDict = KeyboardSelfDictation(defaults: defaults)
        XCTAssertEqual(selfDict.handoffReason(osMajorVersion: 18), .openedHex)
        XCTAssertNil(selfDict.handoffReason(osMajorVersion: 26, hasMultilingualModelInKeyboard: false, hasMemoryToStart: true, includeRecentHandoff: false))
        XCTAssertEqual(selfDict.handoffReason(osMajorVersion: 26, hasMultilingualModelInKeyboard: false, hasMemoryToStart: false, includeRecentHandoff: false), .memory)

        languages.markKeyboardNeedsHex()
        XCTAssertEqual(selfDict.handoffReason(osMajorVersion: 26, hasMultilingualModelInKeyboard: false, hasMemoryToStart: true, includeRecentHandoff: false), .speechModel)
        XCTAssertFalse(selfDict.dictatesItself(osMajorVersion: 26))
        languages.clearKeyboardNeedsHex()

        selfDict.recordHandedToHex(.memory)
        XCTAssertEqual(selfDict.handoffReason(osMajorVersion: 26, includeRecentHandoff: true), .memory)
        XCTAssertNil(selfDict.handoffReason(osMajorVersion: 26, hasMultilingualModelInKeyboard: false, hasMemoryToStart: true, includeRecentHandoff: false))
    }

    // MARK: - DictationPipeline.Preferences

    @MainActor
    func testDictationPipelinePreferencesSnapshotIsIsolatedFromLaterSettingChanges() async throws {
        let suite = "hex.tests.pipeline-prefs.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        let lowercase = DictationLowercaseSettings(defaults: defaults)
        let period = DictationPeriodSettings(defaults: defaults)
        lowercase.isEnabled = true
        period.isEnabled = false

        let snapshot = DictationPipeline.Preferences.current(
            style: .polished,
            vocabulary: CustomVocabularySettings(defaults: defaults),
            corrections: [],
            lowercase: lowercase,
            lists: DictationListSettings(defaults: defaults),
            punctuation: DictationPunctuationSettings(defaults: defaults),
            numbers: DictationNumberSettings(defaults: defaults),
            hinglish: HinglishSettings(defaults: defaults),
            greeklish: GreeklishSettings(defaults: defaults),
            period: period
        )

        // Flip settings after capture started; the snapshot keeps what was active at start.
        lowercase.isEnabled = false
        period.isEnabled = true

        let pipeline = DictationPipeline(preferences: snapshot, language: "en-US")
        let stages = await pipeline.run("Hello world")
        XCTAssertEqual(stages.formatted, "hello world")
    }
}
