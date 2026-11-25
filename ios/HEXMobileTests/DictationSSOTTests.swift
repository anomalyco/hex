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
        for reason: KeyboardSelfDictation.Reason? in [nil, .memory, .speechModel, .languageUnavailable, .openedHex] {
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

    /// From iOS 26 nothing a dictation tap does opens Hex: where the keyboard can't dictate it
    /// says why, and only Open Hex in that line opens Hex. Before iOS 26 it opens Hex, as always.
    func testADictationTapThatCantDictateHereSaysWhyFromiOS26AndOpensHexOnlyBefore() {
        let routes: [(DictationRoute, KeyboardDictationBlock?)] = [
            (.needsModel, .setUp),
            (.opensHex(.speechModel), .speechModel(language: "de-DE")),
            (.opensHex(.memory), .memory),
            (.opensHex(.languageUnavailable), .languageUnavailable(language: "de-DE")),
            (.setUp, nil),
            (.warm, nil),
            (.inKeyboard, nil),
        ]
        for (route, block) in routes {
            XCTAssertEqual(KeyboardDictationBlock.resolve(route: route, language: "de-DE", osMajorVersion: 26), block, "\(route)")
            XCTAssertNil(KeyboardDictationBlock.resolve(route: route, language: "de-DE", osMajorVersion: 18), "iOS 18 unchanged: \(route)")
        }
        XCTAssertNil(KeyboardDictationBlock.resolve(route: .opensHex(nil), language: nil, osMajorVersion: 18))
        XCTAssertEqual(KeyboardDictationBlock.resolve(route: .opensHex(.speechModel), language: nil, osMajorVersion: 27), .speechModel(language: "en-US"))
        XCTAssertEqual(KeyboardDictationBlock.speechModel(language: "en-US").line, "Download English in Hex to dictate")
        XCTAssertEqual(KeyboardDictationBlock.memory.line, "Not enough memory to dictate here")
        XCTAssertEqual(KeyboardDictationBlock.setUp.line, "Finish setting up Hex to dictate")
        // Apple has no model for it here: downloading in Hex can't help, so no Open Hex.
        XCTAssertEqual(KeyboardDictationBlock.languageUnavailable(language: "mt-MT").line, "Maltese isn't available on this iPhone yet")
        XCTAssertFalse(KeyboardDictationBlock.languageUnavailable(language: "mt-MT").opensHex)
        XCTAssertTrue(KeyboardDictationBlock.speechModel(language: "en-US").opensHex)
    }

    @MainActor
    func testTheDictationBlockOpensHexOnlyFromItsOpenHexAndPutsItselfAway() throws {
        try XCTSkipIf(ProcessInfo.processInfo.operatingSystemVersion.majorVersion < KeyboardSelfDictation.firstVersion)
        let model = KeyboardViewModel()
        var opened: [URL] = []
        model.onOpenURL = { opened.append($0) }
        model.configureFixture(snapshot: .offline, now: 50_000)
        model.automaticLanguage = "en-US"
        model.opensHexReason = .memory
        XCTAssertEqual(model.route, .opensHex(.memory))
        XCTAssertFalse(model.dictationTapOpensHex)

        model.dictateWhereUnavailable()
        XCTAssertEqual(model.dictationBlock, .memory)
        XCTAssertEqual(model.statusSlot, .dictationBlock(.memory), "the bar's line")
        XCTAssertTrue(opened.isEmpty, "a dictation tap opens nothing")
        model.dismissDictationBlock()
        XCTAssertNil(model.dictationBlock, "its ✕")
        XCTAssertTrue(opened.isEmpty)

        model.dictateWhereUnavailable()
        model.openHexForDictationBlock()
        XCTAssertEqual(opened, [model.coldStartURL], "Open Hex records in Hex")
        XCTAssertNil(model.dictationBlock)

        // The second failure to get the speech model says so as a tap would; dictating here again puts it away.
        model.opensHexReason = .speechModel
        model.localDictationChanged(available: false, was: true, reason: .speechModel)
        XCTAssertEqual(model.dictationBlock, .speechModel(language: "en-US"))
        model.localDictationChanged(available: true, was: false, reason: nil)
        XCTAssertNil(model.dictationBlock)
        model.localDictationChanged(available: false, was: nil, reason: .speechModel)
        XCTAssertNil(model.dictationBlock, "a keyboard that opens without the model says nothing until a tap")
        XCTAssertEqual(opened.count, 1)
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
        // The keyboard dictates the multilingual model's languages itself, with Apple's model
        // until it has the multilingual one loaded: Hex gets that model ready for it.
        XCTAssertFalse(DictationEngine.parakeetUltra.keyboardSpeechFollowsApp(keyboardNeedsHex: false))
        XCTAssertFalse(DictationEngine.parakeetUltra.keyboardSpeechFollowsApp(keyboardNeedsHex: true))
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

        // Settings › Dictation › Model switched: the model picked before keeps dictating English
        // until the new one is here (it goes only once the new one has loaded once).
        let multilingualPickedEnglishStill = plan(multilingual: "en-US", v2Downloaded: true, ultraDownloaded: false)
        XCTAssertEqual(multilingualPickedEnglishStill, .readyV2(language: "en-US"))
        XCTAssertEqual(multilingualPickedEnglishStill.modelChoice?.model, .parakeetV2)
        XCTAssertEqual(plan(multilingual: "en-US", v2Downloaded: true, ultraDownloaded: true).modelChoice?.model, .parakeetUltra)
        XCTAssertNil(plan(multilingual: "nl-NL", v2Downloaded: true, ultraDownloaded: false).modelChoice, "English's model only takes English")
        let englishPickedUltraStill = plan(v2Downloaded: false, ultraDownloaded: true)
        XCTAssertEqual(englishPickedUltraStill, .awaitingV2DownloadUsingUltra(language: "en-US"))
        XCTAssertEqual(englishPickedUltraStill.modelChoice?.model, .parakeetUltra)
        XCTAssertNil(
            KeyboardParakeetDownload.hold(plan: englishPickedUltraStill, unmetered: true, freeBytes: 8_000_000_000),
            "English's model still downloads by itself"
        )
        XCTAssertEqual(plan(v2Downloaded: true, ultraDownloaded: true).modelChoice?.model, .parakeetV2)

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

        // Settings' row: the keyboard's "ready" is about English. Dictating Hinglish after the
        // download, Apple Speech takes it, and the row says so rather than "Ready".
        let now = Date()
        func line(_ hold: KeyboardParakeetDownload.Hold?) -> String? {
            KeyboardParakeetStatus.line(state: .idle, isDownloaded: true, hold: hold, keyboardState: (.ready, now), now: now)
        }
        XCTAssertEqual(line(nil), "Ready in the keyboard")
        XCTAssertEqual(line(.notEnglish), "Only for English dictation.")
        XCTAssertEqual(
            KeyboardParakeetStatus.line(state: .idle, isDownloaded: false, hold: .notEnglish, keyboardState: nil, now: now),
            "Only for English dictation."
        )

        // Model: Multilingual. Its download speaks for itself; once on the iPhone, what the
        // keyboard does with it, or why it doesn't.
        func multilingual(
            downloaded: Bool = true,
            takes: Bool = true,
            fits: Bool = true,
            _ state: KeyboardParakeetSettings.KeyboardState?
        ) -> String? {
            KeyboardParakeetStatus.multilingualLine(
                isDownloaded: downloaded,
                takesLanguage: takes,
                fitsKeyboard: fits,
                keyboardState: state.map { ($0, now) },
                now: now
            )
        }
        XCTAssertNil(multilingual(downloaded: false, .ready))
        XCTAssertEqual(multilingual(.ready), "Ready in the keyboard")
        XCTAssertEqual(
            multilingual(.preparing),
            "Getting ready in the keyboard. After a download or an update, this takes a little longer."
        )
        XCTAssertEqual(multilingual(nil), "Downloaded. The keyboard gets it ready the next time it opens.")
        XCTAssertEqual(
            multilingual(fits: false, nil),
            "This iPhone's keyboard doesn't have the memory for it, so it uses Apple's speech recognition."
        )
        XCTAssertEqual(
            multilingual(takes: false, .ready),
            "Not for this dictation language, so the keyboard uses Apple's speech recognition."
        )
        // Only the picked model stays, and the other goes only once the picked one is downloaded
        // and has loaded once, and never while Hex dictates with it.
        func unpicked(
            _ picked: SpeechModelChoice,
            english: (Bool, Bool) = (true, true),
            multilingual: (Bool, Bool) = (true, true),
            hex: KeyboardParakeetPlan.Model? = nil
        ) -> KeyboardParakeetPlan.Model? {
            SpeechModelChoice.unpicked(
                picked: picked,
                englishDownloaded: english.0,
                englishLoaded: english.1,
                multilingualDownloaded: multilingual.0,
                multilingualLoaded: multilingual.1,
                hexDictatesWith: hex
            )
        }
        XCTAssertEqual(unpicked(.multilingual), .parakeetV2)
        XCTAssertNil(unpicked(.multilingual, multilingual: (false, false)), "still downloading: English's stays and dictates")
        XCTAssertNil(unpicked(.multilingual, multilingual: (true, false)), "downloaded, not loaded yet")
        XCTAssertNil(unpicked(.multilingual, english: (false, false)), "nothing to remove")
        XCTAssertNil(unpicked(.multilingual, hex: .parakeetV2), "Hex dictates with English's")
        XCTAssertEqual(unpicked(.english), .parakeetUltra)
        XCTAssertNil(unpicked(.english, english: (true, false)))
        XCTAssertNil(unpicked(.english, english: (false, false)), "a download that failed leaves the multilingual one")
        XCTAssertNil(unpicked(.english, hex: .parakeetUltra))

        let names = SpeechModelChoice.multilingualLanguages
        XCTAssertTrue(names.contains("Dutch") && names.contains("Ukrainian") && names.contains("English"), names)
    }

    /// Settings › Dictation's speech model line once the keyboard couldn't use the model: it used
    /// to say "The keyboard can't use it right now" for six causes, and kept saying it after the
    /// cause had passed (a post on X, 2026-11-25). Each cause now says why and what happens
    /// next, and a load that failed gets Try again.
    @MainActor
    func testTheKeyboardsSpeechModelLineSaysWhyItCouldntUseItAndWhatHappensNext() throws {
        let suite = "hex.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = KeyboardParakeetSettings(defaults: defaults)
        let now = Date()

        // What the keyboard writes, and what Hex reads back.
        settings.setKeyboardState(.unavailable, reason: .shortOfMemory)
        XCTAssertEqual(settings.unavailableReason, .shortOfMemory)
        settings.setKeyboardState(.preparing)
        XCTAssertNil(settings.unavailableReason, "only .unavailable carries a reason")
        XCTAssertNil(defaults.string(forKey: KeyboardParakeetSettings.keyboardStateReasonKey))
        settings.setKeyboardState(.unavailable)
        XCTAssertNil(settings.unavailableReason, "an older keyboard's word, without a reason")

        // An older keyboard's `.unavailable` without a reason says nothing true: Hex forgets it
        // at launch (TestFlight build 153, an iPhone 14: the page showed it with no reason).
        XCTAssertTrue(settings.forgetUnavailableWithoutReason())
        XCTAssertNil(settings.keyboardState)
        settings.setKeyboardState(.unavailable, reason: .loadFailed)
        XCTAssertFalse(settings.forgetUnavailableWithoutReason(), "one with a reason stays")
        XCTAssertEqual(settings.unavailableReason, .loadFailed)
        settings.setKeyboardState(.ready)
        XCTAssertFalse(settings.forgetUnavailableWithoutReason())
        XCTAssertEqual(settings.keyboardState?.state, .ready)

        func line(
            _ reason: KeyboardParakeetSettings.UnavailableReason?,
            state: KeyboardParakeetSettings.KeyboardState = .unavailable
        ) -> String? {
            KeyboardParakeetStatus.line(
                state: .idle, isDownloaded: true, hold: nil, keyboardState: (state, now), reason: reason, now: now
            )
        }
        XCTAssertEqual(
            line(.shortOfMemory),
            "Not enough free memory when the keyboard last opened, so it used Apple's speech recognition. It tries again the next time it opens."
        )
        XCTAssertEqual(
            line(.noNeuralEngine),
            "This iPhone can't run it in the keyboard, so the keyboard uses Apple's speech recognition."
        )
        XCTAssertEqual(line(.loadFailed), "The keyboard couldn't load it, so it uses Apple's speech recognition.")
        XCTAssertEqual(line(.notOnNeuralEngine), line(.loadFailed))
        XCTAssertEqual(
            line(nil),
            "Downloaded. The keyboard gets it ready the next time it opens.",
            "no reason only ever means the keyboard hasn't said anything since"
        )
        XCTAssertEqual(line(.loadFailed, state: .ready), "Ready in the keyboard", "a reason only counts with .unavailable")
        XCTAssertEqual(
            KeyboardParakeetStatus.line(state: .idle, isDownloaded: false, hold: .keyboardTooSmall, keyboardState: nil, now: now),
            "This iPhone's keyboard doesn't have the memory for it, so it uses Apple's speech recognition.",
            "a limit under 131 MB never changes, so it doesn't say right now"
        )
        XCTAssertEqual(
            KeyboardParakeetStatus.line(
                state: .idle, isDownloaded: true, hold: .keyboardTooSmall,
                keyboardState: (.unavailable, now), reason: nil, now: now
            ),
            "This iPhone's keyboard doesn't have the memory for it, so it uses Apple's speech recognition.",
            "downloaded anyway (turned on by hand), it still never loads, whatever the keyboard last said"
        )
        XCTAssertEqual(
            KeyboardParakeetStatus.multilingualLine(
                isDownloaded: true, takesLanguage: true, fitsKeyboard: true,
                keyboardState: (.unavailable, now), reason: .shortOfMemory, now: now
            ),
            line(.shortOfMemory)
        )
        let reasons: [KeyboardParakeetSettings.UnavailableReason?] = [.shortOfMemory, .noNeuralEngine, .notOnNeuralEngine, .loadFailed, nil]
        for reason in reasons {
            XCTAssertFalse(line(reason)?.contains("right now") ?? true)
        }

        // Try again, beside a load that failed: the keyboard drops its compiled copies and tries
        // again the next time it opens, even in the process that set it aside.
        settings.setKeyboardState(.unavailable, reason: .loadFailed)
        KeyboardParakeetStatus.retryInKeyboard(settings)
        XCTAssertTrue(settings.purgesCompiled)
        XCTAssertNil(settings.keyboardState)
        XCTAssertEqual(
            KeyboardParakeetStatus.line(
                state: .idle, isDownloaded: true, hold: nil, keyboardState: settings.keyboardState,
                reason: settings.unavailableReason, now: now
            ),
            "Downloaded. The keyboard gets it ready the next time it opens."
        )
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
        XCTAssertNil(selfDict.handoffReason(osMajorVersion: 26, hasMemoryToStart: true, includeRecentHandoff: false))
        XCTAssertEqual(selfDict.handoffReason(osMajorVersion: 26, hasMemoryToStart: false, includeRecentHandoff: false), .memory)

        languages.markKeyboardNeedsHex()
        XCTAssertEqual(selfDict.handoffReason(osMajorVersion: 26, hasMemoryToStart: true, includeRecentHandoff: false), .speechModel)
        XCTAssertFalse(selfDict.dictatesItself(osMajorVersion: 26))
        languages.markKeyboardNeedsHex(unavailable: true)
        XCTAssertEqual(
            selfDict.handoffReason(osMajorVersion: 26, hasMemoryToStart: true, includeRecentHandoff: false),
            .languageUnavailable,
            "Hex found Apple has no model for the language here"
        )
        languages.clearKeyboardNeedsHex()
        XCTAssertFalse(languages.keyboardLanguageUnavailable)

        selfDict.recordHandedToHex(.memory)
        XCTAssertEqual(selfDict.handoffReason(osMajorVersion: 26, includeRecentHandoff: true), .memory)
        XCTAssertNil(selfDict.handoffReason(osMajorVersion: 26, hasMemoryToStart: true, includeRecentHandoff: false))
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
