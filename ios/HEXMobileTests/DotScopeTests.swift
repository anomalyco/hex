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
}
