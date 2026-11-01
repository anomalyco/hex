import XCTest
@testable import HEX

@MainActor
final class DotScopeTests: XCTestCase {
    func testAnalyticalSpringIsFrameRateIndependent() {
        let spec = DotSpringSpec(duration: 0.30, bounce: 0.15)
        var fast = DotSpring(at: 0)
        var slow = DotSpring(at: 0)

        for _ in 0..<240 {
            fast.step(to: 1.0, spec: spec, dt: 1.0 / 240.0)
        }
        for _ in 0..<30 {
            slow.step(to: 1.0, spec: spec, dt: 1.0 / 30.0)
        }

        XCTAssertEqual(fast.x, slow.x, accuracy: 1e-4)
        XCTAssertEqual(fast.v, slow.v, accuracy: 1e-3)
        XCTAssertEqual(fast.x, 1.0, accuracy: 1e-3)
    }

    func testAutoLevelNormalizesSpeechAboveRoomFloor() {
        var auto = DotScopeAutoLevel()
        var silentLevel: Float = 1.0
        for _ in 0..<120 {
            silentLevel = auto.step(loudness: -62.0, dt: 1.0 / 240.0, release: 3.0, noiseFloor: -60.0)
        }
        XCTAssertLessThan(silentLevel, 0.05)

        var speechLevel: Float = 0.0
        for _ in 0..<60 {
            speechLevel = auto.step(loudness: -28.0, dt: 1.0 / 240.0, release: 3.0, noiseFloor: -60.0)
        }
        XCTAssertGreaterThan(speechLevel, 0.65)
    }

    func testSimulationLightsBraidedDotsDuringRecordingAndCoolsDuringProcessing() {
        let sim = DotScopeSimulation(tuning: .keyboard)
        sim.restart(reduceMotion: false)

        for _ in 0..<60 {
            sim.advance(
                dt: 1.0 / 60.0,
                stage: .recording,
                visible: true,
                loudness: DotScopeSimulation.demoLoudness(at: sim.elapsedTime),
                reduceMotion: false
            )
        }

        let recordingPose = sim.pose
        XCTAssertGreaterThan(recordingPose.opacity, 0.95)
        XCTAssertEqual(recordingPose.cool, 0.0, accuracy: 0.01)
        XCTAssertGreaterThan(recordingPose.live, 0.70)

        let peakDot = sim.dots.map(\.x).max() ?? 0
        XCTAssertGreaterThan(peakDot, 0.45)

        let warmPalette = DotScopePalette.dark(pose: recordingPose)
        XCTAssertGreaterThan(warmPalette.base.x, warmPalette.base.z)

        for _ in 0..<45 {
            sim.advance(
                dt: 1.0 / 60.0,
                stage: .processing,
                visible: true,
                loudness: -120.0,
                reduceMotion: false
            )
        }

        let processingPose = sim.pose
        XCTAssertLessThan(processingPose.width, recordingPose.width)
        XCTAssertGreaterThan(processingPose.cool, 0.90)

        let coolPalette = DotScopePalette.dark(pose: processingPose)
        XCTAssertGreaterThan(coolPalette.base.z, coolPalette.base.x)
    }

    func testSimulationSettlesWhenHidden() {
        let sim = DotScopeSimulation(tuning: .hud)
        sim.restart(reduceMotion: false)
        sim.advance(dt: 0.4, stage: .recording, visible: true, loudness: -30.0, reduceMotion: false)
        XCTAssertFalse(sim.settled)

        for _ in 0..<60 {
            sim.advance(dt: 1.0 / 60.0, stage: .done, visible: false, loudness: -120.0, reduceMotion: false)
        }
        XCTAssertTrue(sim.settled)
    }

    func testIdleStageSettlesForZeroIdlePower() {
        let sim = DotScopeSimulation(tuning: .keyboard)
        sim.restart(reduceMotion: false)
        XCTAssertFalse(sim.isIdleSettled)

        for _ in 0..<36 {
            sim.advance(dt: 1.0 / 60.0, stage: .idle, visible: true, loudness: -120.0, reduceMotion: false)
        }
        XCTAssertTrue(sim.isIdleSettled)
        XCTAssertGreaterThan(sim.pose.opacity, 0.99)
        XCTAssertEqual(sim.pose.width, DotScopeTuning.keyboard.idleWidth, accuracy: 0.25)
    }

    func testBarTuningFitsFullWidthDrawableWithPixelSnappedPitchAndOddRows() {
        let fitted = DotScopeTuning.bar(leadingInset: 49, trailingInset: 79, fade: 16, spacing: 3.2)
            .fitted(pixelWidth: 373 * 3, pixelHeight: 52 * 3, scale: 3.0)

        XCTAssertTrue(fitted.fitsBounds)
        XCTAssertEqual(fitted.width, 373.0, accuracy: 0.01)
        XCTAssertEqual(fitted.height, 52.0, accuracy: 0.01)
        XCTAssertGreaterThan(fitted.columns, 100)
        XCTAssertEqual(fitted.rows % 2, 1)

        let pxPitch = fitted.spacing * 3.0
        XCTAssertEqual(pxPitch, pxPitch.rounded(), accuracy: 1e-4)

        let sim = DotScopeSimulation(tuning: fitted)
        sim.restart(reduceMotion: false)
        for _ in 0..<48 {
            sim.advance(dt: 1.0 / 60.0, stage: .recording, visible: true, loudness: -29.0, reduceMotion: false)
        }

        let pose = sim.pose
        XCTAssertEqual(pose.width, 373.0, accuracy: 0.01)
        XCTAssertEqual(pose.activeSpan, 373.0 - 49.0 - 79.0, accuracy: 0.5)
        XCTAssertEqual(pose.activeCenter, (49.0 - 79.0) * 0.5, accuracy: 0.25)

        // When recording finishes and buttons clear, processing spans the full bar end to end.
        let fullBar = DotScopeTuning.bar(leadingInset: 0, trailingInset: 0, fade: 16, spacing: 3.2)
            .fitted(pixelWidth: 373 * 3, pixelHeight: 52 * 3, scale: 3.0)
        sim.tune(fullBar)
        for _ in 0..<48 {
            sim.advance(dt: 1.0 / 60.0, stage: .processing, visible: true, loudness: -120.0, reduceMotion: false)
        }
        XCTAssertEqual(sim.pose.activeSpan, 373.0, accuracy: 1.0)
        XCTAssertEqual(sim.pose.activeCenter, 0.0, accuracy: 0.5)
    }

    func testBarIdleSettlesAndRewakesWhenInsetsChange() {
        let idleBar = DotScopeTuning.bar(leadingInset: 0, trailingInset: 105, fade: 16, spacing: 3.2)
            .fitted(pixelWidth: 373 * 3, pixelHeight: 52 * 3, scale: 3.0)
        let sim = DotScopeSimulation(tuning: idleBar)
        sim.restart(reduceMotion: false)

        for _ in 0..<36 {
            sim.advance(dt: 1.0 / 60.0, stage: .idle, visible: true, loudness: -120.0, reduceMotion: false)
        }
        XCTAssertTrue(sim.isIdleSettled)

        var widerTrigger = idleBar
        widerTrigger.trailingInset = 142
        sim.tune(widerTrigger)
        XCTAssertFalse(sim.isIdleSettled)

        for _ in 0..<48 {
            sim.advance(dt: 1.0 / 60.0, stage: .idle, visible: true, loudness: -120.0, reduceMotion: false)
        }
        XCTAssertTrue(sim.isIdleSettled)
    }

    func testBarInsetUpdatesGlideSmoothlyInsteadOfJumping() {
        let idleBar = DotScopeTuning.bar(leadingInset: 0, trailingInset: 105)
            .fitted(pixelWidth: 373 * 3, pixelHeight: 44 * 3, scale: 3.0)
        let sim = DotScopeSimulation(tuning: idleBar)
        sim.restart(reduceMotion: false)

        for _ in 0..<42 {
            sim.advance(dt: 1.0 / 60.0, stage: .idle, visible: true, loudness: -120.0, reduceMotion: false)
        }
        XCTAssertEqual(sim.pose.leadingInset, 0.0, accuracy: 0.25)
        XCTAssertEqual(sim.pose.trailingInset, 105.0, accuracy: 0.25)

        // Transitioning to recording retargets both insets on the 0.28s critical spring:
        // after one 60 Hz frame, neither inset may jump all the way to its target.
        let recBar = DotScopeTuning.bar(leadingInset: 45, trailingInset: 77)
            .fitted(pixelWidth: 373 * 3, pixelHeight: 44 * 3, scale: 3.0)
        sim.tune(recBar, jumpLayout: false)
        sim.advance(dt: 1.0 / 60.0, stage: .recording, visible: true, loudness: -30.0, reduceMotion: false)

        XCTAssertGreaterThan(sim.pose.leadingInset, 0.2)
        XCTAssertLessThan(sim.pose.leadingInset, 15.0)
        XCTAssertLessThan(sim.pose.trailingInset, 104.8)
        XCTAssertGreaterThan(sim.pose.trailingInset, 90.0)

        for _ in 0..<48 {
            sim.advance(dt: 1.0 / 60.0, stage: .recording, visible: true, loudness: -30.0, reduceMotion: false)
        }
        XCTAssertEqual(sim.pose.leadingInset, 45.0, accuracy: 0.35)
        XCTAssertEqual(sim.pose.trailingInset, 77.0, accuracy: 0.35)
    }

    func testHostViewLoudnessUpdatesNeverResetSimulationClock() {
        var liveDB: Float = -34.0
        let host = DotScopeHostView(tuning: .bar(leadingInset: 45, trailingInset: 77))
        host.frame = CGRect(x: 0, y: 0, width: 373, height: 44)
        host.useDemoVoice = true
        host.loudnessProvider = { liveDB }
        host.phase = .recording

        host.stepAndRender(timestamp: 10.0, targetTimestamp: 10.0 + 1.0 / 60.0)
        host.stepAndRender(timestamp: 10.0 + 1.0 / 60.0, targetTimestamp: 10.0 + 2.0 / 60.0)
        let tBefore = host.debugElapsedTime
        XCTAssertGreaterThan(tBefore, 0.01)

        // Updating live loudness 10 times during `.recording` must never reset `elapsedTime`.
        for step in 1...10 {
            liveDB = -34.0 + Float(step) * 0.8
            host.phase = .recording
            let ts = 10.0 + Double(step + 1) / 60.0
            host.stepAndRender(timestamp: ts, targetTimestamp: ts + 1.0 / 60.0)
        }
        XCTAssertGreaterThan(host.debugElapsedTime, tBefore + 0.14)
    }

    func testHomeSwipeCueKeepsGlidingWhileCoolingOutInsteadOfFreezingMidFade() {
        let cue = DotSwipeCueHostView(frame: CGRect(x: 0, y: 0, width: 402, height: 21))
        cue.isActive = true
        cue.stepAndRender(timestamp: 20.0, targetTimestamp: 20.0 + 1.0 / 120.0)
        XCTAssertEqual(cue.debugVisibility, 1.0, accuracy: 0.01)
        let elapsedAtDeactivate = cue.debugElapsedTime

        // Deactivating cools `visibilitySpring` smoothly while the wave keeps moving rightward.
        cue.isActive = false
        cue.stepAndRender(timestamp: 20.0 + 1.0 / 120.0, targetTimestamp: 20.0 + 2.0 / 120.0)
        XCTAssertGreaterThan(cue.debugVisibility, 0.85)
        XCTAssertLessThan(cue.debugVisibility, 1.0)
        XCTAssertGreaterThan(cue.debugElapsedTime, elapsedAtDeactivate)

        for frame in 2...60 {
            let t0 = 20.0 + Double(frame) / 120.0
            cue.stepAndRender(timestamp: t0, targetTimestamp: t0 + 1.0 / 120.0)
        }
        XCTAssertEqual(cue.debugVisibility, 0.0, accuracy: 0.005)
        XCTAssertGreaterThan(cue.debugElapsedTime, elapsedAtDeactivate + 0.45)
    }
}
