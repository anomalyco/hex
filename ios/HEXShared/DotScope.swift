import Metal
import QuartzCore
import SwiftUI
import UIKit

// MARK: - Analytical Spring (`src/spring.rs`)

nonisolated struct DotSpringSpec: Equatable, Sendable {
    var duration: Float
    var bounce: Float

    static let reducedFade = DotSpringSpec(duration: 0.16, bounce: 0.0)

    var omega: Float {
        (2.0 * .pi) / max(duration, 0.01)
    }

    var zeta: Float {
        if bounce >= 0 {
            return max(1.0 - bounce, 0.0)
        }
        return 1.0 / max(1.0 + bounce, 0.01)
    }
}

nonisolated struct DotSpring: Equatable, Sendable {
    private static let criticalTolerance: Float = 1e-4

    var x: Float
    var v: Float = 0
    var target: Float

    init(at value: Float) {
        self.x = value
        self.v = 0
        self.target = value
    }

    mutating func jump(_ value: Float) {
        x = value
        v = 0
        target = value
    }

    mutating func step(to newTarget: Float, spec: DotSpringSpec, dt: Float) {
        target = newTarget
        guard dt > 0, dt.isFinite else { return }

        let omega = spec.omega
        let zeta = spec.zeta
        let x0 = x - target
        let v0 = v

        if zeta < 1.0 - Self.criticalTolerance {
            let decay = zeta * omega
            let omegaD = omega * sqrt(1.0 - zeta * zeta)
            let expTerm = exp(-decay * dt)
            let s = sin(omegaD * dt)
            let c = cos(omegaD * dt)
            let sinOverD = s / omegaD
            x = target + expTerm * (x0 * (c + decay * sinOverD) + v0 * sinOverD)
            v = expTerm * (-x0 * omega * omega * sinOverD + v0 * (c - decay * sinOverD))
        } else if zeta > 1.0 + Self.criticalTolerance {
            let root = sqrt(zeta * zeta - 1.0)
            let rootSum = zeta + root
            let slowRoot = -omega / rootSum
            let fastRoot = -omega * rootSum
            let denom = slowRoot - fastRoot
            let slowExp = exp(slowRoot * dt)
            let fastExp = exp(fastRoot * dt)
            let p00 = (-fastRoot * slowExp + slowRoot * fastExp) / denom
            let p01 = (slowExp - fastExp) / denom
            let p10 = slowRoot * fastRoot * (fastExp - slowExp) / denom
            let p11 = (slowRoot * slowExp - fastRoot * fastExp) / denom
            x = target + p00 * x0 + p01 * v0
            v = p10 * x0 + p11 * v0
        } else {
            let expTerm = exp(-omega * dt)
            let c = v0 + omega * x0
            x = target + (x0 + c * dt) * expTerm
            v = (v0 - omega * c * dt) * expTerm
        }
    }
}

// MARK: - Auto Level (`src/dot_scope.rs`)

nonisolated struct DotScopeAutoLevel: Equatable, Sendable {
    private static let minSpan: Float = 16.0
    private static let range: Float = 24.0
    private static let roomMargin: Float = 5.0
    private static let attack: Float = 0.04
    private static let roomFall: Float = 0.30
    private static let roomRise: Float = 6.0

    var peak: Float = -30.0
    var room: Float = -55.0

    mutating func step(loudness: Float, dt: Float, release: Float, noiseFloor: Float) -> Float {
        let db = max(loudness, -120.0)
        func follow(_ value: inout Float, _ target: Float, _ time: Float) {
            value += (target - value) * (1.0 - exp(-dt / max(time, 1e-3)))
        }
        follow(&room, db, db < room ? Self.roomFall : Self.roomRise)
        let lowest = max(room, noiseFloor) + Self.minSpan
        if db > peak {
            follow(&peak, db, Self.attack)
        } else {
            follow(&peak, lowest, release)
        }
        peak = max(peak, lowest)
        let low = max(max(room, noiseFloor) + Self.roomMargin, peak - Self.range)
        return min(1.0, max(0.0, (db - low) / max(peak - low, 1.0)))
    }
}

// MARK: - Tuning & Stage (`src/dot_scope.rs`, `src/dot_scope/scope.rs`)

nonisolated enum DotScopeStage: Equatable, Sendable {
    case idle
    case recording
    case processing
    case done
    case failed
}

nonisolated struct DotScopeTuning: Equatable, Sendable {
    static let maxColumns = 96
    static let maxRows = 15
    static let maxDots = 1024

    var columns: Int = 43
    var rows: Int = 9
    var spacing: Float = 2.6
    var width: Float = 112.0
    var idleWidth: Float = 112.0
    var processingWidth: Float = 76.0
    var height: Float = 26.0
    var braidSpeed: Float = 1.0
    var helixCycles: Float = 1.6
    var gain: Float = 1.0
    var response: Float = 6.5
    var release: Float = 0.15
    var historySpeed: Float = 150.0
    var phosphor: Float = 0.60
    var glow: Float = 1.0
    var saturation: Float = 1.0
    var shimmerPass: Float = 0.55
    var shimmer: Float = 1.0
    var transition: Float = 0.30
    var bounce: Float = 0.15
    var autoLevel: Bool = true
    var autoRelease: Float = 3.0
    var noiseFloor: Float = -60.0

    static let hud = DotScopeTuning()

    /// Even-margin sizing matching `dictation_indicator::fill_island`.
    static func sized(
        width targetWidth: Float,
        idleWidth targetIdle: Float = 120,
        processingWidth targetProcessing: Float,
        height: Float = 30.0,
        rows: Int = 9,
        spacing: Float = 2.75,
        gain: Float = 1.18
    ) -> DotScopeTuning {
        var outer = (height - Float(rows - 1) * spacing) * 0.5
        if outer < 3.0 && rows > 2 {
            outer += spacing
        }
        let maxCols = min(maxColumns, maxDots / max(rows, 1))
        let cols = max(11, min(maxCols, Int(floor((targetWidth - 2.0 * outer) / spacing)) + 1))
        let snappedWidth = Float(cols - 1) * spacing + 2.0 * outer
        var t = DotScopeTuning()
        t.columns = cols
        t.rows = rows
        t.spacing = spacing
        t.width = snappedWidth
        t.idleWidth = min(snappedWidth, max(height, targetIdle))
        t.processingWidth = min(snappedWidth, max(height, targetProcessing))
        t.height = height
        t.gain = gain
        return t
    }

    static let keyboard = DotScopeTuning.sized(width: 152, idleWidth: 122, processingWidth: 96, height: 30, rows: 9, spacing: 2.75)
    static let handoff = DotScopeTuning.sized(width: 162, idleWidth: 126, processingWidth: 100, height: 30, rows: 9, spacing: 2.8)
}

nonisolated struct DotScopePose: Equatable, Sendable {
    var width: Float
    var height: Float
    var opacity: Float
    var scale: Float
    var softness: Float
    var cool: Float
    var fail: Float
    var glow: Float
    var grid: Float
    var live: Float
}

nonisolated struct DotScopePalette: Equatable, Sendable {
    var base: SIMD3<Float>
    var core: SIMD3<Float>
    var plate: SIMD4<Float>
    var rim: SIMD4<Float>
    var shadow: SIMD4<Float>

    static func dark(pose: DotScopePose, saturation: Float = 1.0) -> DotScopePalette {
        let you = SIMD3<Float>(0.86, 0.83, 0.77)
        let youCore = SIMD3<Float>(0.98, 0.96, 0.92)
        let agent = SIMD3<Float>(0.70, 0.78, 0.90)
        let agentCore = SIMD3<Float>(0.92, 0.95, 1.00)
        let coral = SIMD3<Float>(1.00, 0.38, 0.31)
        let coralCore = SIMD3<Float>(1.00, 0.66, 0.60)

        func mix3(_ a: SIMD3<Float>, _ b: SIMD3<Float>, _ t: Float) -> SIMD3<Float> {
            a + (b - a) * t
        }

        func sat(_ c: SIMD3<Float>, _ amount: Float) -> SIMD3<Float> {
            let luma = 0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
            let out = SIMD3<Float>(luma, luma, luma) + (c - SIMD3<Float>(luma, luma, luma)) * amount
            return SIMD3<Float>(
                min(1.0, max(0.0, out.x)),
                min(1.0, max(0.0, out.y)),
                min(1.0, max(0.0, out.z))
            )
        }

        let base = mix3(mix3(you, agent, pose.cool), coral, pose.fail)
        let core = mix3(mix3(youCore, agentCore, pose.cool), coralCore, pose.fail)

        return DotScopePalette(
            base: sat(base, saturation),
            core: sat(core, saturation),
            plate: SIMD4<Float>(17.0 / 255.0, 17.0 / 255.0, 20.0 / 255.0, 0.94),
            rim: SIMD4<Float>(1.0, 1.0, 1.0, 0.11),
            shadow: SIMD4<Float>(0.0, 0.0, 0.0, 0.50)
        )
    }
}

// MARK: - 240 Hz Braided Oscilloscope Simulation (`src/dot_scope/scope.rs`)

nonisolated final class DotScopeSimulation: @unchecked Sendable {
    private static let stepDt: Float = 1.0 / 240.0
    private static let historyCount = 512
    private static let samplesPerColumn = 3
    private static let speeds: (Float, Float, Float, Float) = (0.8, 1.25, 1.85, 2.6)
    private static let baseWeights: (Float, Float, Float, Float) = (0.55, 0.42, 0.30, 0.20)
    private static let spatialFreqs: (Float, Float, Float, Float) = (1.4, 2.4, 3.8, 5.6)
    private static let startWidth: Float = 30.0
    private static let enterDuration: Float = 0.42
    private static let wipeDuration: Float = 0.30
    private static let phosphorHead: Float = 0.15
    private static let breathPeriod: Float = 5.5
    private static let hiddenScale: Float = 0.90
    private static let hiddenSoftness: Float = 3.0
    private static let workHelix: Float = 2.6
    private static let flowSpacingFactor: Float = 0.70
    private static let carrierRest: Float = 0.14
    private static let tau: Float = 2.0 * .pi

    private struct Voice {
        var level: Float = 0
        var reservoir: Float = 0
        var energy: Float = 0
        var kick: Float = 0
        var kickVelocity: Float = 0
        var band: Float = 0

        mutating func step(raw: Float, h: Float) {
            let previous = level
            let tLevel: Float = raw > level ? 0.035 : 0.095
            level += (raw - level) * (1.0 - exp(-h / tLevel))
            let rise = max(0.0, (level - previous) / h)
            reservoir += (1.75 * level * level - reservoir / 1.8) * h
            energy = 1.0 - exp(-reservoir * 1.45)
            let w: Float = 2.0 * .pi * 1.3
            let z: Float = 0.22
            kickVelocity += (-w * w * kick - 2.0 * z * w * kickVelocity + 15.0 * rise + 2.2 * level) * h
            kick += kickVelocity * h
            let tBand: Float = raw > band ? 0.025 : 0.14
            band += (raw - band) * (1.0 - exp(-h / tBand))
        }
    }

    private struct Trace {
        var envelope: Float = 0
        var amplitude: Float = 0
        var amplitudeVelocity: Float = 0
        var glowFast: Float = 0
        var glowSpring: Float = 0
        var glowSpringVelocity: Float = 0
        var glowSlow: Float = 0
        var glow: Float = 0
        var phases: SIMD4<Float>
        var history: [Float]
        var head: Int = 0

        init(phases: SIMD4<Float>) {
            self.phases = phases
            self.history = Array(repeating: 0, count: DotScopeSimulation.historyCount * 5)
        }

        mutating func tick(voice: Voice, live: Float, speed: Float, tuning: DotScopeTuning, h: Float) {
            func follow(_ value: inout Float, _ target: Float, _ rise: Float, _ fall: Float) {
                let t = target > value ? rise : fall
                value += (target - value) * (1.0 - exp(-h / max(t, 1e-4)))
            }
            follow(&envelope, voice.level, 0.022, tuning.release)
            let w = DotScopeSimulation.tau * tuning.response
            let z: Float = 0.52
            amplitudeVelocity += (-w * w * (amplitude - envelope) - 2.0 * z * w * amplitudeVelocity) * h
            amplitude += amplitudeVelocity * h

            follow(&glowFast, voice.level, 0.045, 0.13)
            let gw = DotScopeSimulation.tau * 3.6
            let gz: Float = 0.50
            glowSpringVelocity += (-gw * gw * (glowSpring - glowFast) - 2.0 * gz * gw * glowSpringVelocity) * h
            glowSpring += glowSpringVelocity * h
            follow(&glowSlow, voice.level, 0.15, 0.70)

            let swell = 0.8 * max(0.0, glowSpring - 0.25) + 0.04 * max(-0.5, voice.kick)
            glow = min(1.15, max(0.0, swell) + 0.42 * glowSlow + 0.20 * voice.energy) * live

            let go = 0.6 + 1.1 * min(1.0, envelope * 1.6)
            let s = DotScopeSimulation.speeds
            let stepFactors = SIMD4<Float>(s.0, s.1, s.2, s.3) * (h * DotScopeSimulation.tau * go * speed)
            phases += stepFactors
            for lane in 0..<4 where phases[lane] >= DotScopeSimulation.tau {
                phases[lane] = phases[lane].truncatingRemainder(dividingBy: DotScopeSimulation.tau)
            }

            let emphasis = 0.35 + 0.65 * min(2.0, voice.band / (voice.band + 0.06))
            let offset = head * 5
            let amp = max(0.0, amplitude) * live
            let b = DotScopeSimulation.baseWeights
            history[offset] = amp
            history[offset + 1] = amp * b.0 * emphasis
            history[offset + 2] = amp * b.1 * emphasis
            history[offset + 3] = amp * b.2 * emphasis
            history[offset + 4] = amp * b.3 * emphasis
            head = (head + 1) % DotScopeSimulation.historyCount
        }

        func past(delay: Float, channel: Int) -> Float {
            let count = DotScopeSimulation.historyCount
            let d = min(delay / DotScopeSimulation.stepDt, Float(count - 2))
            let i = Int(floor(d))
            let k = d - Float(i)
            let a = (head + 2 * count - 1 - i) % count
            let b = (a + count - 1) % count
            return history[a * 5 + channel] * (1.0 - k) + history[b * 5 + channel] * k
        }
    }

    private(set) var tuning: DotScopeTuning
    private(set) var stage: DotScopeStage = .recording
    private var elapsed: Float = 0
    private var speedAtDone: Float = 0
    private var doneFrom: Float = 0
    private var flowOffset: Float = 0
    private var flowTurn: Float = 0
    private var auto = DotScopeAutoLevel()
    private(set) var level: Float = 0
    private var voices = (Voice(), Voice())
    private var traces = (
        Trace(phases: SIMD4<Float>(0.1, 1.7, 3.3, 4.6)),
        Trace(phases: SIMD4<Float>(2.4, 0.6, 5.1, 1.9))
    )
    private var live: Float = 0
    private var carrier: Float = 0
    private var time: Float = 0
    private var steps: UInt64 = 0
    private var pending: Float = 0
    private var reduceMotion = false

    private(set) var columns: Int = 0
    private(set) var rows: Int = 0
    private var head0: [Float] = []
    private var head1: [Float] = []
    private var tail0: [Float] = []
    private var tail1: [Float] = []
    private var ys0: [Float] = []
    private var ys1: [Float] = []
    private var bright0: [Float] = []
    private var bright1: [Float] = []
    private(set) var dots: [SIMD2<Float>] = []
    private(set) var packedDots: [SIMD2<UInt16>] = []

    private var grid: Float = 0
    private var widthSpring = DotSpring(at: startWidth)
    private var opacitySpring = DotSpring(at: 0.0)
    private var scaleSpring = DotSpring(at: hiddenScale)
    private var softnessSpring = DotSpring(at: hiddenSoftness)
    private var coolSpring = DotSpring(at: 0.0)
    private var failSpring = DotSpring(at: 0.0)

    init(tuning: DotScopeTuning = .hud) {
        self.tuning = tuning
        resizeIfNeeded()
    }

    /// Synthetic speech envelope from `src/dot_scope/scope.rs` (`demo_loudness`) for previews/tests.
    static func demoLoudness(at time: Float) -> Float {
        let phraseDur: Float = 2.4
        let spokenDur: Float = 1.7
        let phrase = floor(time / phraseDur)
        let t = time - phrase * phraseDur
        let level: Float
        if t > spokenDur {
            level = 0.04
        } else {
            let shape = pow(max(0.0, sin(.pi * t / spokenDur)), 0.4)
            let rate = tau * (4.3 + 0.6 * sin(phrase * 1.7))
            let syllable = 0.5 + 0.5 * sin(rate * t + phrase)
            let stress = 0.75 + 0.25 * sin(tau * 1.1 * t + 2.0 * phrase)
            level = min(1.0, max(0.0, 0.08 + 0.82 * shape * pow(syllable, 1.6) * stress))
        }
        return -58.0 + 28.0 * level
    }

    static func loudness(fromNormalized level: Float) -> Float {
        let clamped = min(1.0, max(0.0, level))
        return clamped <= 0.001 ? -90.0 : (-54.0 + 34.0 * clamped)
    }

    func restart(reduceMotion: Bool) {
        let savedAuto = auto
        let currentTuning = tuning
        stage = .recording
        elapsed = 0
        speedAtDone = 0
        doneFrom = 0
        flowOffset = 0
        flowTurn = 0
        auto = savedAuto
        level = 0
        voices = (Voice(), Voice())
        traces = (
            Trace(phases: SIMD4<Float>(0.1, 1.7, 3.3, 4.6)),
            Trace(phases: SIMD4<Float>(2.4, 0.6, 5.1, 1.9))
        )
        live = 0
        carrier = 0
        time = 0
        steps = 0
        pending = 0
        self.reduceMotion = reduceMotion
        enterFromHidden = true
        grid = 0
        widthSpring = DotSpring(at: Self.startWidth)
        opacitySpring = DotSpring(at: 0.0)
        scaleSpring = DotSpring(at: Self.hiddenScale)
        softnessSpring = DotSpring(at: Self.hiddenSoftness)
        coolSpring = DotSpring(at: 0.0)
        failSpring = DotSpring(at: 0.0)
        columns = 0
        rows = 0
        tuning = currentTuning
        resizeIfNeeded()
        if reduceMotion {
            scaleSpring.jump(1.0)
            softnessSpring.jump(0.0)
            widthSpring.jump(tuning.width)
        }
    }

    func tune(_ newTuning: DotScopeTuning) {
        guard tuning != newTuning else { return }
        tuning = newTuning
        resizeIfNeeded()
    }

    private var enterFromHidden = true

    var settled: Bool {
        opacitySpring.target == 0.0 && opacitySpring.x < 0.002
    }

    var isIdleSettled: Bool {
        stage == .idle
            && elapsed >= 0.45
            && abs(widthSpring.x - tuning.idleWidth) < 0.25
            && abs(opacitySpring.x - 1.0) < 0.01
    }

    var elapsedTime: Float {
        time
    }

    var pose: DotScopePose {
        DotScopePose(
            width: max(widthSpring.x, tuning.height),
            height: tuning.height,
            opacity: min(1.0, max(0.0, opacitySpring.x)),
            scale: max(0.01, scaleSpring.x),
            softness: min(Self.hiddenSoftness, max(0.0, softnessSpring.x)),
            cool: min(1.0, max(0.0, coolSpring.x)),
            fail: min(1.0, max(0.0, failSpring.x)),
            glow: ((traces.0.glow + traces.1.glow) * 0.5) * tuning.glow,
            grid: grid,
            live: live
        )
    }

    func advance(
        dt: Float,
        stage targetStage: DotScopeStage,
        visible: Bool,
        loudness: Float,
        reduceMotion: Bool
    ) {
        if targetStage != stage {
            if stage == .idle {
                enterFromHidden = false
            }
            if targetStage == .done {
                armDone()
            }
            if targetStage == .processing {
                flowOffset = -widthSpring.x * 0.5 - 4.0
            }
            stage = targetStage
            elapsed = 0
        }
        self.reduceMotion = reduceMotion
        pending += min(0.25, max(0.0, dt))
        while pending >= Self.stepDt {
            pending -= Self.stepDt
            stepOnce(visible: visible, loudness: loudness, reduceMotion: reduceMotion)
        }
    }

    private func resizeIfNeeded() {
        let cols = max(1, min(DotScopeTuning.maxColumns, tuning.columns))
        let rws = max(1, min(DotScopeTuning.maxRows, min(tuning.rows, DotScopeTuning.maxDots / cols)))
        guard cols != columns || rws != rows else { return }
        columns = cols
        rows = rws
        let dotCount = cols * rws
        let sampleCount = (cols - 1) * Self.samplesPerColumn + 1
        head0 = Array(repeating: 0, count: dotCount)
        head1 = Array(repeating: 0, count: dotCount)
        tail0 = Array(repeating: 0, count: dotCount)
        tail1 = Array(repeating: 0, count: dotCount)
        ys0 = Array(repeating: 0, count: sampleCount)
        ys1 = Array(repeating: 0, count: sampleCount)
        bright0 = Array(repeating: 0, count: sampleCount)
        bright1 = Array(repeating: 0, count: sampleCount)
        dots = Array(repeating: .zero, count: dotCount)
        packedDots = Array(repeating: .zero, count: dotCount)
    }

    private var workPeriod: Float {
        2.0 * max(0.05, tuning.shimmerPass)
    }

    private var flowSpacing: Float {
        Self.flowSpacingFactor * tuning.processingWidth
    }

    private var flowSpeed: Float {
        flowSpacing / workPeriod
    }

    private var flowTurnHz: Float {
        flowSpeed * tuning.helixCycles / max(1.0, widthSpring.x)
    }

    private var pulseSpeed: Float {
        (tuning.processingWidth * 0.5 + 12.0) / (1.4 * workPeriod)
    }

    private func armDone() {
        speedAtDone = 1.5 * pulseSpeed
        doneFrom = stage == .processing ? elapsed : 0.0
    }

    private func stepOnce(visible: Bool, loudness: Float, reduceMotion: Bool) {
        let h = Self.stepDt
        let isRecording = stage == .recording && visible
        let inputLevel: Float
        if isRecording {
            if tuning.autoLevel {
                level = auto.step(
                    loudness: loudness,
                    dt: h,
                    release: tuning.autoRelease,
                    noiseFloor: tuning.noiseFloor
                )
            } else {
                level = min(1.0, max(0.0, (loudness + 40.0) / 30.0))
            }
            inputLevel = min(1.0, max(0.0, level * tuning.gain))
        } else {
            level = 0
            inputLevel = 0
        }

        voices.0.step(raw: inputLevel, h: h)
        voices.1.step(raw: inputLevel * 0.85, h: h)

        let liveTarget: Float = if isRecording {
            1.0
        } else if stage == .idle && visible {
            0.26
        } else {
            0.0
        }
        live += (liveTarget - live) * (1.0 - exp(-h / 0.55))

        let speed: Float = reduceMotion ? 0.0 : tuning.braidSpeed
        traces.0.tick(voice: voices.0, live: live, speed: speed, tuning: tuning, h: h)
        traces.1.tick(voice: voices.1, live: live, speed: speed, tuning: tuning, h: h)

        let energy = max(voices.0.energy, voices.1.energy)
        let working = (stage == .processing || stage == .done)
        flowTurn += ((working ? 1.0 : 0.0) - flowTurn) * (1.0 - exp(-h / 0.25))
        if working {
            flowOffset += flowSpeed * h
        }

        let rest = Self.carrierRest + 0.22 * energy
        let turn = rest + (flowTurnHz - rest) * flowTurn
        carrier = (carrier + h * Self.tau * turn * speed).truncatingRemainder(dividingBy: Self.tau)

        let targetWidth: Float = switch stage {
        case .idle: tuning.idleWidth
        case .recording: tuning.width
        case .processing, .done: tuning.processingWidth
        case .failed: widthSpring.target
        }
        let coolTarget: Float = (stage == .processing || stage == .done) ? 1.0 : 0.0
        let failTarget: Float = stage == .failed ? 1.0 : 0.0
        let shownTarget: Float = visible ? 1.0 : 0.0

        let fade = DotSpringSpec(duration: tuning.transition, bounce: 0.0)
        let bouncy = DotSpringSpec(duration: tuning.transition, bounce: tuning.bounce)

        if reduceMotion {
            widthSpring.jump(targetWidth)
            scaleSpring.jump(1.0)
            softnessSpring.jump(0.0)
            opacitySpring.step(to: shownTarget, spec: .reducedFade, dt: h)
            coolSpring.step(to: coolTarget, spec: .reducedFade, dt: h)
            failSpring.step(to: failTarget, spec: .reducedFade, dt: h)
        } else {
            widthSpring.step(to: targetWidth, spec: bouncy, dt: h)
            opacitySpring.step(to: shownTarget, spec: fade, dt: h)
            scaleSpring.step(to: visible ? 1.0 : Self.hiddenScale, spec: bouncy, dt: h)
            softnessSpring.step(to: visible ? 0.0 : Self.hiddenSoftness, spec: fade, dt: h)
            coolSpring.step(to: coolTarget, spec: fade, dt: h)
            failSpring.step(to: failTarget, spec: fade, dt: h)
        }

        elapsed += h
        time += h
        steps &+= 1
        if steps.isMultiple(of: 2) {
            beam(hb: 2.0 * h)
        }
    }

    @inline(__always)
    private static func smoothstep(_ a: Float, _ b: Float, _ x: Float) -> Float {
        let t = min(1.0, max(0.0, (x - a) / (b - a)))
        return t * t * (3.0 - 2.0 * t)
    }

    @inline(__always)
    private static func bump(_ d: Float, _ w: Float) -> Float {
        let r = d / w
        return exp(-r * r)
    }

    @inline(__always)
    private static func hermite(start: Float, speed: Float, end: Float, t rawT: Float) -> Float {
        let t = min(1.0, max(0.0, rawT))
        let t2 = t * t
        let t3 = t2 * t
        return (2.0 * t3 - 3.0 * t2 + 1.0) * start
            + (t3 - 2.0 * t2 + t) * wipeDuration * speed
            + (3.0 * t2 - 2.0 * t3) * end
    }

    @inline(__always)
    private static func remEuclid(_ a: Float, _ b: Float) -> Float {
        let r = a.truncatingRemainder(dividingBy: b)
        return r < 0 ? r + b : r
    }

    private func flowLight(x: Float) -> Float {
        let spacing = flowSpacing
        let d = Self.remEuclid(x - flowOffset + spacing * 0.5, spacing) - spacing * 0.5
        return Self.bump(d, 7.0)
    }

    private func beam(hb: Float) {
        let cols = columns
        let rws = rows
        let sub = Self.samplesPerColumn
        let sampleCount = (cols - 1) * sub + 1
        let pitch = tuning.spacing
        let width = max(widthSpring.x, tuning.height)
        let halfWidth = width * 0.5
        let e = elapsed

        let scanHelix: Float
        let scanDecay: Float
        let scanTail: Float
        let scanGrid: Float

        enum Mode {
            case still
            case enter(reach: Float, head: Float, unfold: Float)
            case work(head: Float, swell: Float)
            case clear(front: Float, head: Float, glow: Float)
        }
        let mode: Mode

        if stage == .failed || stage == .idle || reduceMotion {
            scanHelix = stage == .idle ? 0.85 : 1.0
            scanDecay = 1.0; scanTail = 1.0; scanGrid = 1.0
            mode = .still
        } else {
            switch stage {
            case .idle:
                scanHelix = 0.85; scanDecay = 1.0; scanTail = 1.0; scanGrid = 1.0
                mode = .still

            case .recording:
                scanHelix = 1.0; scanDecay = 1.0; scanTail = 1.0
                if enterFromHidden {
                    scanGrid = min(1.0, e / 0.30)
                    let p = min(1.0, max(0.0, e / Self.enterDuration))
                    let passPos = 0.5 - 0.5 * cos(.pi * p)
                    let reach = (halfWidth + 12.0) * passPos
                    let head = 2.8 * (1.0 - Self.smoothstep(0.08, Self.enterDuration, e))
                    let unfold = Self.smoothstep(0.0, 0.10, e)
                    mode = .enter(reach: reach, head: head, unfold: unfold)
                } else {
                    scanGrid = 1.0
                    mode = .still
                }

            case .processing:
                let enter = Self.smoothstep(0.0, 0.35, e)
                scanHelix = 1.0 + (Self.workHelix - 1.0) * enter
                scanDecay = 1.0; scanTail = 1.0; scanGrid = 1.0
                let head = 2.5 * Self.smoothstep(0.05, 0.30, e) * tuning.shimmer
                mode = .work(head: head, swell: 0.9 * enter)

            case .done:
                let target = halfWidth + 8.0
                let front = Self.hermite(start: 0.0, speed: speedAtDone, end: target, t: e / Self.wipeDuration)
                scanHelix = Self.workHelix
                scanDecay = 1.0 - 0.6 * Self.smoothstep(0.0, 0.12, e)
                scanTail = 1.0 - Self.smoothstep(0.0, 0.15, e)
                scanGrid = 1.0 - Self.smoothstep(0.20, 0.40, e)
                let head = 2.8 * Self.smoothstep(0.0, 0.04, e) * (1.0 - Self.smoothstep(0.22, 0.32, e))
                let glow = 2.5 * tuning.shimmer * (1.0 - Self.smoothstep(0.0, 0.18, e))
                mode = .clear(front: front, head: head, glow: glow)

            case .failed:
                scanHelix = 1.0; scanDecay = 1.0; scanTail = 1.0; scanGrid = 1.0
                mode = .still
            }
        }

        grid = scanGrid
        let mid = Float(rws - 1) * 0.5
        let half = mid - 0.55
        let breath: Float = reduceMotion ? 0.5 : (0.5 + 0.5 * sin(time * Self.tau / Self.breathPeriod))
        let rest = (0.55 + 0.35 * breath) * (0.4 + 0.6 * live) * scanHelix
        let sf = Self.spatialFreqs

        func scanAt(_ x: Float) -> (Float, Float, Float) {
            switch mode {
            case .still:
                return (1.0, 1.0, 0.0)
            case .enter(let reach, let head, let unfold):
                let d = abs(x)
                let drawn = unfold * (1.0 - Self.smoothstep(max(0.0, reach - 14.0), max(0.001, reach), d))
                let amp = drawn * (1.0 - Self.smoothstep(max(0.0, reach - 22.0), max(0.001, reach - 3.0), d))
                let beamLight = head * Self.bump(d - reach, 6.0)
                let swell = 1.8 * unfold * (head / 2.8) * Self.bump(d - max(0.0, reach - 14.0), 14.0)
                return (amp, amp + beamLight, swell)
            case .work(let head, let swell):
                let light = flowLight(x: x)
                return (1.0, 1.0 + head * light, swell * light)
            case .clear(let front, let head, let glow):
                let d = abs(x)
                let keep = Self.smoothstep(front - 12.0, front - 2.0, d)
                let light = flowLight(x: x)
                return (
                    keep,
                    keep * (1.0 + glow * light) + head * Self.bump(d - front, 6.0),
                    keep * 0.9 * light
                )
            }
        }

        func fillSamples(trace: Trace, sign: Float, ys: inout [Float], brightness: inout [Float]) {
            let p = trace.phases
            for s in 0..<sampleCount {
                let x = (Float(s) / Float(sub) - Float(cols - 1) * 0.5) * pitch
                if abs(x) > halfWidth {
                    ys[s] = mid
                    brightness[s] = 0
                    continue
                }
                let (amp, bright, swell) = scanAt(x)
                let own = abs(x)
                let xn = (x + halfWidth) / width
                let ends = min(
                    Self.smoothstep(-halfWidth + 3.0, -halfWidth + 17.0, x),
                    Self.smoothstep(-halfWidth + 3.0, -halfWidth + 17.0, -x)
                )
                let reach = 1.0 - 0.8 * Self.smoothstep(14.0, halfWidth, own)
                let delay = own / tuning.historySpeed
                let a = trace.past(delay: delay, channel: 0)
                var spread: Float = 0
                spread += trace.past(delay: delay, channel: 1) * sin(Self.tau * sf.0 * xn - p[0])
                spread += trace.past(delay: delay, channel: 2) * sin(Self.tau * sf.1 * xn - p[1])
                spread += trace.past(delay: delay, channel: 3) * sin(Self.tau * sf.2 * xn - p[2])
                spread += trace.past(delay: delay, channel: 4) * sin(Self.tau * sf.3 * xn - p[3])

                let helix = (rest + swell + half * 0.55 * a * reach)
                    * sin(Self.tau * tuning.helixCycles * xn - carrier)
                let y = half * tanh((helix + half * 0.7 * spread * reach) / max(half, 0.01)) * ends * amp
                ys[s] = mid - sign * y
                let spatial = (0.3 + 0.7 * min(1.0, a * 1.8)) * (0.45 + 0.55 * ends)
                brightness[s] = spatial * amp + 0.36 * max(0.0, bright - amp)
            }
            for s in 0..<sampleCount {
                let nextIdx = min(sampleCount - 1, s + 1)
                let prevIdx = max(0, s - 1)
                let slope = (ys[nextIdx] - ys[prevIdx]) * Float(sub) * 0.5
                brightness[s] *= 0.45 + 0.55 / sqrt(1.0 + 0.22 * slope * slope)
            }
        }

        fillSamples(trace: traces.0, sign: 1.0, ys: &ys0, brightness: &bright0)
        fillSamples(trace: traces.1, sign: -1.0, ys: &ys1, brightness: &bright1)

        let headKeep = exp(-hb / max(1e-4, Self.phosphorHead * scanDecay))
        let tailKeep = exp(-hb / max(1e-4, tuning.phosphor * scanDecay))
        let dx = 1.0 / Float(sub)
        let dx2 = dx * dx

        for i in 0..<cols {
            let s0 = max(0, i - 3) * sub
            let s1 = min(sampleCount - 1, (i + 3) * sub)
            let fi = Float(i)
            for j in 0..<rws {
                let fj = Float(j)
                let n = j * cols + i

                func traceLight(
                    ys: [Float],
                    bright: [Float],
                    headArr: inout [Float],
                    tailArr: inout [Float]
                ) -> (instant: Float, light: Float) {
                    var best = Float.greatestFiniteMagnitude
                    var nearest = i * sub
                    for s in s0..<s1 {
                        let ay = ys[s]
                        let dy = ys[s + 1] - ay
                        let px = fi - Float(s) * dx
                        let py = fj - ay
                        let t = min(1.0, max(0.0, (px * dx + py * dy) / (dx2 + dy * dy)))
                        let ex = px - t * dx
                        let ey = py - t * dy
                        let d2 = ex * ex + ey * ey
                        if d2 < best {
                            best = d2
                            nearest = s
                        }
                    }
                    let b = bright[nearest]
                    let onBeam = exp(-best / 0.25)
                    let instant = min(1.2, onBeam + 0.5 * exp(-sqrt(best) / 0.8)) * b
                    let core = onBeam * b
                    let nextHead = max(headArr[n] * headKeep, core)
                    let nextTail = max(tailArr[n] * tailKeep, core)
                    headArr[n] = nextHead
                    tailArr[n] = nextTail
                    let light = max(instant, max(nextHead, 0.55 * scanTail * nextTail))
                    return (instant, light)
                }

                let t0 = traceLight(ys: ys0, bright: bright0, headArr: &head0, tailArr: &tail0)
                let t1 = traceLight(ys: ys1, bright: bright1, headArr: &head1, tailArr: &tail1)
                let nowSum = t0.instant + t1.instant
                let totalSum = t0.light + t1.light
                let power = min(1.0, max(0.0, 1.0 - (1.0 - min(1.0, t0.light)) * (1.0 - min(1.0, t1.light))))
                let fresh = min(1.0, max(0.0, nowSum / max(totalSum, 1e-4)))

                dots[n] = SIMD2<Float>(power, fresh)
                packedDots[n] = SIMD2<UInt16>(
                    UInt16((power * 65535.0).rounded()),
                    UInt16((fresh * 65535.0).rounded())
                )
            }
        }
    }
}

// MARK: - Metal GPU View (`DotScopeView`)

enum DotScopePhase: Equatable, Sendable {
    case hidden
    case idle
    case recording(loudnessDB: Float)
    case processing
    case failed
}

private struct DotScopeMetalUniforms {
    var metrics0: SIMD4<Float>
    var metrics1: SIMD4<Float>
    var metrics2: SIMD4<Float>
    var state: SIMD4<Float>
    var base: SIMD4<Float>
    var core: SIMD4<Float>
    var plate: SIMD4<Float>
    var rim: SIMD4<Float>
    var shadow: SIMD4<Float>
}

@MainActor
private final class DotScopeMetalPipeline {
    static let shared: DotScopeMetalPipeline? = DotScopeMetalPipeline()

    let device: MTLDevice
    let queue: MTLCommandQueue
    let pipeline: MTLRenderPipelineState

    private init?() {
        guard let device = MTLCreateSystemDefaultDevice(),
              let queue = device.makeCommandQueue() else {
            return nil
        }
        let bundle = Bundle(for: DotScopeHostView.self)
        guard let library = (try? device.makeDefaultLibrary(bundle: bundle)) ?? device.makeDefaultLibrary(),
              let vertex = library.makeFunction(name: "dot_scope_vertex"),
              let fragment = library.makeFunction(name: "dot_scope_fragment") else {
            return nil
        }

        let descriptor = MTLRenderPipelineDescriptor()
        descriptor.label = "Hex DotScope"
        descriptor.vertexFunction = vertex
        descriptor.fragmentFunction = fragment
        descriptor.colorAttachments[0].pixelFormat = .bgra8Unorm

        guard let pipeline = try? device.makeRenderPipelineState(descriptor: descriptor) else {
            return nil
        }

        self.device = device
        self.queue = queue
        self.pipeline = pipeline
    }
}

private final class DisplayLinkProxy: NSObject {
    weak var owner: DotScopeHostView?

    @MainActor
    @objc func tick(_ link: CADisplayLink) {
        owner?.stepAndRender(timestamp: link.timestamp, targetTimestamp: link.targetTimestamp)
    }
}

@MainActor
final class DotScopeHostView: UIView {
    override static var layerClass: AnyClass { CAMetalLayer.self }

    private let simulation: DotScopeSimulation
    private let dotBuffer: MTLBuffer?
    private let proxy = DisplayLinkProxy()
    private var displayLink: CADisplayLink?
    private var lastTimestamp: CFTimeInterval?
    private var ringIndex = 0
    private var hasSeededSnapshot = false

    var phase: DotScopePhase = .hidden {
        didSet {
            guard phase != oldValue else { return }
            handlePhaseChange(from: oldValue, to: phase)
        }
    }

    var drawPlate = true {
        didSet {
            if drawPlate != oldValue {
                renderCurrentFrame()
            }
        }
    }

    var reduceMotion = false
    var useDemoVoice = false

    private var metalLayer: CAMetalLayer {
        // swiftlint:disable:next force_cast
        layer as! CAMetalLayer
    }

    init(tuning: DotScopeTuning) {
        self.simulation = DotScopeSimulation(tuning: tuning)
        if let gpu = DotScopeMetalPipeline.shared {
            self.dotBuffer = gpu.device.makeBuffer(
                length: DotScopeTuning.maxDots * MemoryLayout<SIMD2<UInt16>>.stride * 3,
                options: .storageModeShared
            )
        } else {
            self.dotBuffer = nil
        }
        super.init(frame: .zero)

        isOpaque = false
        backgroundColor = .clear
        isUserInteractionEnabled = false

        if let gpu = DotScopeMetalPipeline.shared {
            metalLayer.device = gpu.device
            metalLayer.pixelFormat = .bgra8Unorm
            metalLayer.isOpaque = false
            metalLayer.framebufferOnly = true
            metalLayer.colorspace = CGColorSpace(name: CGColorSpace.sRGB)
        }

        proxy.owner = self
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func updateTuning(_ tuning: DotScopeTuning) {
        simulation.tune(tuning)
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil {
            ensureDisplayLink()
            updateAnimatingState()
            renderCurrentFrame()
        } else {
            displayLink?.invalidate()
            displayLink = nil
            lastTimestamp = nil
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let scale = max(1.0, traitCollection.displayScale)
        metalLayer.contentsScale = scale
        let pixelSize = CGSize(width: bounds.width * scale, height: bounds.height * scale)
        if metalLayer.drawableSize != pixelSize {
            metalLayer.drawableSize = pixelSize
        }
        seedForSnapshotIfNeeded()
        renderCurrentFrame()
    }

    private func ensureDisplayLink() {
        guard displayLink == nil else { return }
        let link = CADisplayLink(target: proxy, selector: #selector(DisplayLinkProxy.tick(_:)))
        if #available(iOS 15.0, *) {
            link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        }
        link.isPaused = true
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    private func handlePhaseChange(from old: DotScopePhase, to next: DotScopePhase) {
        if case .hidden = old, next != .hidden {
            simulation.restart(reduceMotion: reduceMotion)
            if case .idle = next {
                // Settle resting frame immediately so idle starts at 0% CPU/GPU with zero flash.
                for _ in 0..<36 {
                    simulation.advance(
                        dt: 1.0 / 60.0,
                        stage: .idle,
                        visible: true,
                        loudness: -120.0,
                        reduceMotion: reduceMotion
                    )
                }
            }
        }
        lastTimestamp = nil
        seedForSnapshotIfNeeded()
        updateAnimatingState()
        renderCurrentFrame()
    }

    private func seedForSnapshotIfNeeded() {
        guard useDemoVoice, !hasSeededSnapshot else { return }
        switch phase {
        case .hidden, .idle:
            return
        case .recording, .processing, .failed:
            break
        }
        hasSeededSnapshot = true
        simulation.restart(reduceMotion: false)
        // Step into an expressive mid-phrase peak so static screenshots show crisp braided loops.
        let dt: Float = 1.0 / 60.0
        for _ in 0..<48 {
            simulation.advance(
                dt: dt,
                stage: .recording,
                visible: true,
                loudness: -28.0 + 4.5 * sin(simulation.elapsedTime * 22.0),
                reduceMotion: false
            )
        }
        if case .processing = phase {
            for _ in 0..<30 {
                simulation.advance(
                    dt: dt,
                    stage: .processing,
                    visible: true,
                    loudness: -120.0,
                    reduceMotion: false
                )
            }
        }
    }

    private func updateAnimatingState() {
        let wantsFrames: Bool = switch phase {
        case .hidden:
            !simulation.settled
        case .idle:
            !simulation.isIdleSettled
        case .recording, .processing, .failed:
            true
        }
        displayLink?.isPaused = !wantsFrames
    }

    func stepAndRender(timestamp: CFTimeInterval, targetTimestamp: CFTimeInterval) {
        let dt: Float
        if let prev = lastTimestamp {
            dt = Float(min(0.1, max(1.0 / 240.0, timestamp - prev)))
        } else {
            dt = Float(max(1.0 / 120.0, targetTimestamp - timestamp))
        }
        lastTimestamp = timestamp

        let (stage, visible, loudness) = simulationInputs()
        simulation.advance(
            dt: dt,
            stage: stage,
            visible: visible,
            loudness: loudness,
            reduceMotion: reduceMotion
        )
        renderCurrentFrame()
        updateAnimatingState()
    }

    private func simulationInputs() -> (DotScopeStage, Bool, Float) {
        switch phase {
        case .hidden:
            return (.done, false, -120.0)
        case .idle:
            return (.idle, true, -120.0)
        case .recording(let db):
            let effectiveDB = useDemoVoice
                ? (-29.0 + 5.0 * sin(simulation.elapsedTime * 21.0))
                : db
            return (.recording, true, effectiveDB)
        case .processing:
            return (.processing, true, -120.0)
        case .failed:
            return (.failed, true, -120.0)
        }
    }

    private func renderCurrentFrame() {
        guard bounds.width > 1, bounds.height > 1,
              let gpu = DotScopeMetalPipeline.shared,
              let dotBuffer,
              let drawable = metalLayer.nextDrawable(),
              let commandBuffer = gpu.queue.makeCommandBuffer() else {
            return
        }

        let pose = simulation.pose
        let palette = DotScopePalette.dark(pose: pose, saturation: simulation.tuning.saturation)
        let scale = Float(max(1.0, metalLayer.contentsScale))
        let resX = Float(metalLayer.drawableSize.width)
        let resY = Float(metalLayer.drawableSize.height)

        var uniforms = DotScopeMetalUniforms(
            metrics0: SIMD4<Float>(resX, resY, scale, simulation.tuning.spacing),
            metrics1: SIMD4<Float>(pose.width, pose.height, pose.opacity, pose.scale),
            metrics2: SIMD4<Float>(
                pose.softness,
                Float(simulation.columns),
                Float(simulation.rows),
                pose.height * 0.5
            ),
            state: SIMD4<Float>(pose.glow, pose.grid, pose.live, pose.cool),
            base: SIMD4<Float>(palette.base.x, palette.base.y, palette.base.z, drawPlate ? 1.0 : 0.0),
            core: SIMD4<Float>(palette.core.x, palette.core.y, palette.core.z, 0.0),
            plate: palette.plate,
            rim: palette.rim,
            shadow: palette.shadow
        )

        let slotStride = DotScopeTuning.maxDots * MemoryLayout<SIMD2<UInt16>>.stride
        let slotOffset = ringIndex * slotStride
        ringIndex = (ringIndex + 1) % 3

        let packed = simulation.packedDots
        if !packed.isEmpty {
            let dst = dotBuffer.contents().advanced(by: slotOffset)
            packed.withUnsafeBytes { raw in
                if let baseAddr = raw.baseAddress {
                    memcpy(dst, baseAddr, min(slotStride, raw.count))
                }
            }
        }

        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = drawable.texture
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].storeAction = .store
        pass.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 0)

        if let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: pass) {
            encoder.setRenderPipelineState(gpu.pipeline)
            encoder.setFragmentBytes(
                &uniforms,
                length: MemoryLayout<DotScopeMetalUniforms>.stride,
                index: 0
            )
            encoder.setFragmentBuffer(dotBuffer, offset: slotOffset, index: 1)
            encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
            encoder.endEncoding()
        }

        commandBuffer.present(drawable)
        commandBuffer.commit()
    }
}

struct DotScopeView: UIViewRepresentable {
    var phase: DotScopePhase
    var tuning: DotScopeTuning = .hud
    var drawPlate: Bool = true
    var demoVoice: Bool = ProcessInfo.processInfo.arguments.contains("--snap-state")

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeUIView(context: Context) -> DotScopeHostView {
        let view = DotScopeHostView(tuning: tuning)
        view.reduceMotion = reduceMotion
        view.useDemoVoice = demoVoice
        view.drawPlate = drawPlate
        view.phase = phase
        return view
    }

    func updateUIView(_ uiView: DotScopeHostView, context: Context) {
        uiView.reduceMotion = reduceMotion
        uiView.useDemoVoice = demoVoice
        uiView.updateTuning(tuning)
        uiView.drawPlate = drawPlate
        uiView.phase = phase
    }
}
