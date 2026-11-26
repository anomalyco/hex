import Foundation
import SwiftUI

extension DictationController {
    // MARK: - Launch Fixtures for Simulator Verification (`--snap-state <name>`)

    static func applyLaunchFixtureIfPresent(to controller: DictationController) -> Bool {
        let args = ProcessInfo.processInfo.arguments
        guard let flagIndex = args.firstIndex(of: "--snap-state"),
              flagIndex + 1 < args.count else {
            return false
        }
        let scene = args[flagIndex + 1]
        controller.engine = .appleSpeech
        controller.transcriptionLanguage = "English"
        controller.transcriptionLanguageID = "en-US"
        controller.keyboardBridge.setModelInstalled(true)
        controller.isModelReady = true

        switch scene {
        case "setup", "setup-sheet", "setup-sheet-microphone", "setup-sheet-noaccess", "paste-setting", _ where scene.hasPrefix("onboarding-") || scene.hasPrefix("settings-") || scene.hasPrefix("model-card-"):
            controller.applyFixturePhase(.ready)
            controller.warmDuration = .fifteenMinutes
            controller.setWarmSession(.cold)
            if scene.hasPrefix("model-card-") { KeyboardParakeetDownload.shared.applyFixture(scene) }
            if scene.hasPrefix("settings-dictation-model-") { controller.applySpeechModelFixture(scene) }

        case "language-unavailable":
            // Russian picked by hand on an iPhone whose Apple Speech can't install it.
            let unavailable = SpeechModelUnavailable(
                language: "ru-RU",
                reason: .notOnThisIPhone,
                fallback: "en-US",
                automatic: false
            )
            controller.isModelReady = false
            controller.engine = .appleSpeech
            controller.applyFixturePhase(.failed(UserFacingError.message(for: unavailable, context: .model)))
            controller.languageOffer = "en-US"
            controller.languageOfferLine = unavailable.lineBesideOffer

        case "language-switched":
            // Automatic moved on from Russian to English.
            let unavailable = SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US")
            controller.applyFixturePhase(.ready)
            controller.languageNotice = UserFacingError.message(for: unavailable, context: .model)

        case "ready":
            controller.applyFixturePhase(.ready)
            controller.warmDuration = .off
            controller.setWarmSession(.cold)
            controller.transcript = "Meet at ten on Thursday and bring the updated design notes."

        case "warm":
            controller.applyFixturePhase(.ready)
            controller.warmDuration = .fifteenMinutes
            controller.applyFixtureWarmSession(remaining: 14 * 60 + 42)
            controller.transcript = "Meet at ten on Thursday and bring the updated design notes."

        case "recording":
            controller.applyFixturePhase(.recording)
            controller.warmDuration = .fifteenMinutes
            controller.audioLevel = 0.74
            controller.loudnessDB = -31.0
            controller.recordingDuration = 6.4
            controller.applyFixtureWarmSession(remaining: 14 * 60 + 54)

        case "transcribing":
            controller.applyFixturePhase(.transcribing)
            controller.warmDuration = .fifteenMinutes
            controller.applyFixtureWarmSession(remaining: 14 * 60 + 52)

        case "handoff-listening", "handoff-no-home-bar":
            controller.applyFixturePhase(.recording)
            controller.audioLevel = 0.68
            controller.loudnessDB = -32.5
            controller.recordingDuration = 3.2
            controller.applyFixtureWarmSession(remaining: 14 * 60 + 57)
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
            controller.applyFixturePhase(.recording)
            controller.audioLevel = 0.68
            controller.loudnessDB = -32.0
            controller.recordingDuration = 0.0
            controller.applyFixtureWarmSession(remaining: 14 * 60 + 58)
            Task { @MainActor in
                let boot = Date().timeIntervalSince1970
                var simTime: Float = 0.0
                while !Task.isCancelled {
                    let elapsed = Date().timeIntervalSince1970 - boot
                    if elapsed < 3.4 {
                        if controller.phase != .recording {
                            withAnimation(HexMotion.stage) {
                                controller.applyFixturePhase(.recording)
                            }
                        }
                        controller.loudnessDB = DotScopeSimulation.demoLoudness(at: simTime) + 4.5
                        controller.recordingDuration = elapsed
                        simTime += 0.033
                    } else if elapsed < 4.7 {
                        if controller.phase != .transcribing {
                            withAnimation(HexMotion.stage) {
                                controller.applyFixturePhase(.transcribing)
                            }
                        }
                        controller.loudnessDB = -120.0
                    } else {
                        if controller.phase != .ready {
                            withAnimation(HexMotion.stage) {
                                controller.applyFixturePhase(.ready)
                            }
                        }
                    }
                    try? await Task.sleep(for: .milliseconds(33))
                }
            }

        case "hosted-keyboard-warm":
            controller.applyFixturePhase(.ready)
            controller.warmDuration = .fifteenMinutes
            controller.applyFixtureWarmSession(remaining: 14 * 60 + 42)
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
            controller.applyFixturePhase(.ready)
            controller.warmDuration = .fifteenMinutes
            controller.applyFixtureWarmSession(remaining: 14 * 60 + 55)
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
            controller.applyFixturePhase(.ready)
            controller.warmDuration = .fifteenMinutes
            controller.applyFixtureWarmSession(remaining: 14 * 60 + 58)
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

    private func applyFixtureWarmSession(remaining: TimeInterval) {
        setWarmSession(.warm(expiresAt: Date().timeIntervalSince1970 + remaining), remaining: remaining)
    }

    /// `settings-dictation-model-<english|multilingual>-<waiting|downloading|ready>`: Settings ›
    /// Dictation › Model with that model picked and its download in that state. Waiting is the
    /// real rule: pass `--simulate-cellular` and the keyboard has measured room for the model,
    /// so only the connection holds it. The other states leave the keyboard unmeasured, so
    /// nothing really downloads on the simulator's Wi-Fi. English also has what the keyboard
    /// said about a downloaded model, preparing|short|noengine|failed (short with 41 MB free),
    /// and toosmall, a keyboard limit of 118 MB, under KeyboardParakeetBudget.requiredLimit;
    /// toosmalldownloaded has English's model on the iPhone all the same (a sparse file of its
    /// size), as a switch turned on by hand used to download it, for Remove.
    private func applySpeechModelFixture(_ scene: String) {
        let multilingual = scene.contains("-multilingual-")
        let step = scene.split(separator: "-").last.map(String.init) ?? "waiting"
        let keyboardSaid: [String: (KeyboardParakeetSettings.KeyboardState, KeyboardParakeetSettings.UnavailableReason?)] = [
            "preparing": (.preparing, nil),
            "short": (.unavailable, .shortOfMemory),
            "noengine": (.unavailable, .noNeuralEngine),
            "failed": (.unavailable, .loadFailed),
        ]
        let said = multilingual ? nil : keyboardSaid[step]
        let card = multilingual ? "waiting" : said == nil && step != "toosmalldownloaded" ? step : "ready"
        KeyboardParakeetDownload.shared.applyFixture("model-card-\(card)")
        let megabyte = KeyboardParakeetBudget.megabyte
        if step == "waiting" {
            KeyboardParakeetSettings().recordKeyboardLimit(KeyboardParakeetBudget.requiredLimit)
        } else if step.hasPrefix("toosmall") {
            KeyboardParakeetSettings().recordKeyboardLimit(118 * megabyte)
        }
        if step == "toosmalldownloaded" {
            // Hex itself dictates with Apple's speech recognition, as on an iPhone 14, so the
            // model is the keyboard's alone and Remove shows.
            if #available(iOS 26.0, *) {
                transcriber = AppleSpeechTranscriber(language: SpeechLanguage(locale: Locale(identifier: "en-US"), module: .speech))
            }
            let file = ParakeetTranscriber.cacheDirectory.appendingPathComponent("fixture.bin")
            FileManager.default.createFile(atPath: file.path, contents: nil)
            let handle = try? FileHandle(forWritingTo: file)
            try? handle?.truncate(atOffset: UInt64(KeyboardParakeetDownload.bytes))
            try? handle?.close()
        }
        if let said {
            KeyboardParakeetSettings().setKeyboardState(said.0, reason: said.1, free: step == "short" ? 41 * megabyte : nil)
        }
        MultilingualParakeetSettings().isEnabled = multilingual
        downloadNetwork.forgetMobileData()
        guard multilingual else { return }
        ParakeetUltraModel.remove()
        switch step {
        case "downloading":
            multilingualDownload.applyFixture(share: 0.42)
        case "ready":
            // Files of the right sizes (sparse), so it reads as on the iPhone; nothing loads them.
            for file in ParakeetUltraModel.files {
                let url = ParakeetUltraModel.directory.appendingPathComponent(file.path)
                try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
                FileManager.default.createFile(atPath: url.path, contents: nil)
                let handle = try? FileHandle(forWritingTo: url)
                try? handle?.truncate(atOffset: UInt64(file.size))
                try? handle?.close()
            }
            KeyboardParakeetSettings().setKeyboardState(.ready, multilingual: true)
        default:
            startMultilingualDownloadIfWanted()
        }
    }
}
