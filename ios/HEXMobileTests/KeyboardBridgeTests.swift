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

    func testOnboardingPracticeCountsADictationOrAFewTypedWords() {
        XCTAssertFalse(OnboardingPractice.isDone(text: "S", dictated: false, fullAccessOn: true), "one letter")
        XCTAssertFalse(OnboardingPractice.isDone(text: "See you", dictated: false, fullAccessOn: true))
        XCTAssertTrue(OnboardingPractice.isDone(text: "See you at", dictated: false, fullAccessOn: true))
        XCTAssertFalse(OnboardingPractice.isDone(text: "See you at ten", dictated: false, fullAccessOn: false), "not the Hex keyboard")
        XCTAssertTrue(OnboardingPractice.isDone(text: "", dictated: true, fullAccessOn: false), "a dictation arrived")
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
}

/// The keyboard's quick dictation-language switch.
final class DictationLanguageSwitchTests: XCTestCase {
    private let supported = ["en-US", "en-GB", "es-ES", "es-MX", "de-DE", "nb-NO", "he-IL", "fr-FR"]

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
            supported: supported
        )
        // Nynorsk is Bokmål; "es-419" and "es" find Spain's Spanish; Klingon has none.
        XCTAssertEqual(choices, ["nb-NO", "es-ES", "en-US", "fr-FR", "en-GB"])
        XCTAssertEqual(choices.count, DictationLanguageSwitch.maxChoices)
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
}
