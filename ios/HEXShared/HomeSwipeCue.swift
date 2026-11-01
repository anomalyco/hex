import Metal
import QuartzCore
import simd
import SwiftUI
import UIKit

// MARK: - `DotSwipeCueView` (Full-Bottom-Width Home-Indicator Return Cue)

@MainActor
final class DotSwipeCueHostView: UIView {
    override static var layerClass: AnyClass { CAMetalLayer.self }

    private static let fadeSpec = DotSpringSpec(duration: 0.34, bounce: 0.0)

    private let proxy = DisplayLinkProxy()
    private var displayLink: CADisplayLink?
    private var lastTargetTimestamp: CFTimeInterval?
    /// Starts mid-glide so initial paint and snapshot stills immediately read as a rightward sweep.
    private var elapsed: Float = 0.86
    private var visibilitySpring = DotSpring(at: 0.0)
    private var hasSeededVisibility = false

    var reduceMotion = false {
        didSet {
            if reduceMotion != oldValue {
                updateAnimatingState()
                renderCurrentFrame()
            }
        }
    }

    var isActive = true {
        didSet {
            if isActive != oldValue {
                updateAnimatingState()
                if displayLink?.isPaused != false {
                    renderCurrentFrame()
                }
            }
        }
    }

    var isSceneActive = true {
        didSet {
            if isSceneActive != oldValue {
                updateAnimatingState()
                if displayLink?.isPaused != false {
                    renderCurrentFrame()
                }
            }
        }
    }

    var debugElapsedTime: Float { elapsed }
    var debugVisibility: Float { visibilitySpring.x }
    var debugIsPaused: Bool { displayLink?.isPaused ?? true }

    private var metalLayer: CAMetalLayer {
        // swiftlint:disable:next force_cast
        layer as! CAMetalLayer
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
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

        proxy.tickHandler = { [weak self] link in
            self?.stepAndRender(timestamp: link.timestamp, targetTimestamp: link.targetTimestamp)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil {
            seedVisibilityIfNeeded()
            ensureDisplayLink()
            updateAnimatingState()
            renderCurrentFrame()
        } else {
            displayLink?.invalidate()
            displayLink = nil
            lastTargetTimestamp = nil
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        seedVisibilityIfNeeded()
        let scale = max(1.0, traitCollection.displayScale)
        metalLayer.contentsScale = scale
        let pixelSize = CGSize(width: (bounds.width * scale).rounded(), height: (bounds.height * scale).rounded())
        if metalLayer.drawableSize != pixelSize {
            metalLayer.drawableSize = pixelSize
        }
        updateAnimatingState()
        renderCurrentFrame()
    }

    private func seedVisibilityIfNeeded() {
        guard !hasSeededVisibility else { return }
        hasSeededVisibility = true
        visibilitySpring.jump(isActive ? 1.0 : 0.0)
    }

    private func ensureDisplayLink() {
        guard displayLink == nil else { return }
        let link = CADisplayLink(target: proxy, selector: #selector(DisplayLinkProxy.tick(_:)))
        if #available(iOS 15.0, *) {
            link.preferredFrameRateRange = CAFrameRateRange(minimum: 80, maximum: 120, preferred: 120)
        } else {
            link.preferredFramesPerSecond = 60
        }
        link.isPaused = true
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    private var wantsAnimationFrames: Bool {
        guard window != nil, isSceneActive else { return false }
        let target: Float = isActive ? 1.0 : 0.0
        let fading = abs(visibilitySpring.x - target) > 0.002 || abs(visibilitySpring.v) > 0.002
        if reduceMotion {
            return fading
        }
        return isActive || fading
    }

    private func updateAnimatingState() {
        let shouldRun = wantsAnimationFrames
        if displayLink?.isPaused == shouldRun {
            lastTargetTimestamp = nil
        }
        displayLink?.isPaused = !shouldRun
    }

    func stepAndRender(timestamp: CFTimeInterval, targetTimestamp: CFTimeInterval) {
        seedVisibilityIfNeeded()
        let dt: Float
        if let prevTarget = lastTargetTimestamp {
            dt = Float(min(0.05, max(1.0 / 240.0, targetTimestamp - prevTarget)))
        } else {
            dt = Float(min(0.05, max(1.0 / 240.0, targetTimestamp - timestamp)))
        }
        lastTargetTimestamp = targetTimestamp

        let target: Float = isActive ? 1.0 : 0.0
        let spec: DotSpringSpec = reduceMotion ? .reducedFade : Self.fadeSpec
        visibilitySpring.step(to: target, spec: spec, dt: dt)
        if !isActive, visibilitySpring.x < 0.002, abs(visibilitySpring.v) < 0.005 {
            visibilitySpring.jump(0.0)
        }

        if !reduceMotion {
            elapsed += dt
        }

        renderCurrentFrame()
        updateAnimatingState()
    }

    private func renderCurrentFrame() {
        guard bounds.width > 4, bounds.height > 4,
              let gpu = DotScopeMetalPipeline.shared,
              let swipePipeline = gpu.swipePipeline,
              let drawable = metalLayer.nextDrawable(),
              let commandBuffer = gpu.queue.makeCommandBuffer() else {
            return
        }

        let scale = Float(max(1.0, metalLayer.contentsScale))
        let resX = Float(metalLayer.drawableSize.width)
        let resY = Float(metalLayer.drawableSize.height)
        let pxPitch = max(3.0, (2.15 * scale).rounded())
        let pitch = pxPitch / scale
        let cols = max(15, Int(floor(resX / pxPitch)) - 1)
        let rows = 5
        let opacity = min(1.0, max(0.0, visibilitySpring.x))

        var uniforms = DotSwipeCueMetalUniforms(
            metrics: SIMD4<Float>(resX, resY, scale, pitch),
            grid: SIMD4<Float>(Float(cols), Float(rows), elapsed, reduceMotion ? 1.0 : 0.0),
            base: SIMD4<Float>(0.86, 0.83, 0.77, opacity),
            core: SIMD4<Float>(0.99, 0.97, 0.93, 42.0)
        )

        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = drawable.texture
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].storeAction = .store
        pass.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 0)

        if let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: pass) {
            encoder.setRenderPipelineState(swipePipeline)
            encoder.setFragmentBytes(
                &uniforms,
                length: MemoryLayout<DotSwipeCueMetalUniforms>.stride,
                index: 0
            )
            encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
            encoder.endEncoding()
        }

        commandBuffer.present(drawable)
        commandBuffer.commit()
    }
}

struct DotSwipeCueView: UIViewRepresentable {
    var isActive: Bool = true

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var scenePhase

    func makeUIView(context: Context) -> DotSwipeCueHostView {
        let view = DotSwipeCueHostView()
        view.reduceMotion = reduceMotion
        view.isActive = isActive
        view.isSceneActive = scenePhase == .active
        return view
    }

    func updateUIView(_ uiView: DotSwipeCueHostView, context: Context) {
        uiView.reduceMotion = reduceMotion
        uiView.isActive = isActive
        uiView.isSceneActive = scenePhase == .active
    }
}
