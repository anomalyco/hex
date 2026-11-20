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
        controller.engineName = "Apple Speech"
        controller.transcriptionLanguage = "English"
        controller.transcriptionLanguageID = "en-US"
        controller.keyboardBridge.setModelInstalled(true)
        controller.isModelReady = true

        switch scene {
        case "setup", "setup-sheet", "setup-sheet-microphone", _ where scene.hasPrefix("onboarding-") || scene.hasPrefix("settings-"):
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = false

        case "language-unavailable":
            // Russian picked by hand on an iPhone whose Apple Speech can't install it.
            let unavailable = SpeechModelUnavailable(
                language: "ru-RU",
                reason: .notOnThisIPhone,
                fallback: "en-US",
                automatic: false
            )
            controller.isModelReady = false
            controller.engineName = "Apple Speech"
            controller.phase = .failed(UserFacingError.message(for: unavailable, context: .model))
            controller.languageOffer = "en-US"

        case "language-switched":
            // Automatic moved on from Russian to English.
            let unavailable = SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US")
            controller.phase = .ready
            controller.languageNotice = UserFacingError.message(for: unavailable, context: .model)

        case "ready":
            controller.phase = .ready
            controller.warmDuration = .off
            controller.keyboardSessionEnabled = false
            controller.transcript = "Meet at ten on Thursday and bring the updated design notes."

        case "warm":
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 42
            controller.keyboardSessionExpiresAt = Date().timeIntervalSince1970 + 14 * 60 + 42
            controller.transcript = "Meet at ten on Thursday and bring the updated design notes."

        case "recording":
            controller.phase = .recording
            controller.warmDuration = .fifteenMinutes
            controller.audioLevel = 0.74
            controller.loudnessDB = -31.0
            controller.recordingDuration = 6.4
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 54

        case "transcribing":
            controller.phase = .transcribing
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 52

        case "handoff-listening", "handoff-no-home-bar":
            controller.phase = .recording
            controller.audioLevel = 0.68
            controller.loudnessDB = -32.5
            controller.recordingDuration = 3.2
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 57
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
            controller.phase = .recording
            controller.audioLevel = 0.68
            controller.loudnessDB = -32.0
            controller.recordingDuration = 0.0
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 58
            Task { @MainActor in
                let boot = Date().timeIntervalSince1970
                var simTime: Float = 0.0
                while !Task.isCancelled {
                    let elapsed = Date().timeIntervalSince1970 - boot
                    if elapsed < 3.4 {
                        if controller.phase != .recording {
                            withAnimation(HexMotion.stage) {
                                controller.phase = .recording
                            }
                        }
                        controller.loudnessDB = DotScopeSimulation.demoLoudness(at: simTime) + 4.5
                        controller.recordingDuration = elapsed
                        simTime += 0.033
                    } else if elapsed < 4.7 {
                        if controller.phase != .transcribing {
                            withAnimation(HexMotion.stage) {
                                controller.phase = .transcribing
                            }
                        }
                        controller.loudnessDB = -120.0
                    } else {
                        if controller.phase != .ready {
                            withAnimation(HexMotion.stage) {
                                controller.phase = .ready
                            }
                        }
                    }
                    try? await Task.sleep(for: .milliseconds(33))
                }
            }

        case "hosted-keyboard-warm":
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 42
            controller.keyboardSessionExpiresAt = Date().timeIntervalSince1970 + 14 * 60 + 42
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
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 55
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
            controller.phase = .ready
            controller.warmDuration = .fifteenMinutes
            controller.keyboardSessionEnabled = true
            controller.keyboardSessionRemaining = 14 * 60 + 58
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
}
