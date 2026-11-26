// WebGPU renderer. One uniform buffer holds the whole frame (cast.js G); the draw path allocates
// nothing but the frame's own command encoder and texture view.
//
//   veins   once, at start: the vessels texture every living surface reads (vessels.js)
//   flesh   once, at start: what is under the lacquer (flesh.wgsl), baked on the GPU with its
//           smaller copies, so a peeled face costs two texture reads and no noise
//   scene   on the notes, first, their backdrop (backdrop.wgsl: one triangle, its own small
//           uniform buffer, ambient.js fills it); then
//           the desktop and its two windows → tiles and faces → the one mass the four become
//           (one quad, drawn once) → filaments → motes, into a
//           float target so light can be brighter than white, and beside it, only on frames
//           where a face, a tile or a filament is on the move, a second target: how far each
//           pixel moves while the shutter is open
//   bloom   what is brighter than white, at a quarter size, blurred both ways
//   post    the shutter's blur, shockwave refraction, chromatic aberration, grain, grade → canvas
import { MAX, G } from './cast.js'
import { BODY, HEADS, TENDRILS, PARTICLES, BACKDROP, FLESH, BLOOM_DOWN, BLOOM_BLUR, POST } from './shaders.js'
import { BACKDROP_FLOATS } from './ambient.js'
import { VESSELS, vesselCanvas } from './vessels.js'

export const FRAME_FLOATS = G.FLOATS
const HDR = 'rgba16float'
const VEL = 'rg16float'
const PER_BURST = 96
const BURSTS = 4
const SEGMENT_VERTS = 44 * 6
const FLESH_SIZE = 1024
const FLESH_MIPS = 4
// The passes, in order, as the timers name them.
export const PASSES = ['scene', 'bloomDown', 'blurH', 'blurV', 'post']

export class GpuRenderer {
  static async create(canvas) {
    let device
    let ready = false
    GpuRenderer.failures = []
    try {
      if (!navigator.gpu) return null
      const adapter = await navigator.gpu.requestAdapter()
      if (!adapter) return null
      let timing = adapter.features.has('timestamp-query')
      try {
        device = await adapter.requestDevice({ requiredFeatures: timing ? ['timestamp-query'] : [] })
      } catch (error) {
        if (!timing) throw error
        // Timing must never prevent pictures, even if an advertised optional feature fails.
        timing = false
        const plain = await navigator.gpu.requestAdapter()
        if (!plain) throw error
        device = await plain.requestDevice()
      }
      let reportedError = false
      device.addEventListener('uncapturederror', event => {
        event.preventDefault()
        if (reportedError) return
        reportedError = true
        console.error('Hex Meetings: WebGPU error. ' + event.error.message)
      })
      const ctx = canvas.getContext('webgpu')
      if (!ctx) return null
      const format = navigator.gpu.getPreferredCanvasFormat()
      // The canvas is written whole every frame, so the compositor never blends it.
      ctx.configure({ device, format, alphaMode: 'opaque' })

      const failures = []
      const compile = async (name, code) => {
        const module = device.createShaderModule({ code })
        const info = await module.getCompilationInfo()
        for (const m of info.messages) if (m.type === 'error') failures.push(name + ' ' + m.lineNum + ':' + m.linePos + ' ' + m.message)
        return module
      }
      const mods = {
        body: await compile('body', BODY), heads: await compile('heads', HEADS), tendrils: await compile('tendrils', TENDRILS), particles: await compile('particles', PARTICLES), backdrop: await compile('backdrop', BACKDROP),
        flesh: await compile('flesh', FLESH), down: await compile('bloom-down', BLOOM_DOWN), blur: await compile('bloom-blur', BLOOM_BLUR), post: await compile('post', POST),
      }
      if (failures.length) {
        console.error('Hex Meetings: shaders did not compile.\n' + failures.join('\n'))
        GpuRenderer.failures = failures
        return null
      }

      device.pushErrorScope('validation')
      const V = GPUShaderStage.VERTEX, FR = GPUShaderStage.FRAGMENT
      const frameLayout = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: V | FR, buffer: { type: 'uniform' } }] })
      const tex1 = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: FR, texture: {} }, { binding: 1, visibility: FR, sampler: {} }] })
      const living = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: FR, texture: {} }, { binding: 1, visibility: FR, sampler: {} }, { binding: 2, visibility: FR, texture: {} }] })
      // The notes' backdrop: its own uniforms, and the vessels texture.
      const backLayout = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: V | FR, buffer: { type: 'uniform' } }, { binding: 1, visibility: FR, texture: {} }, { binding: 2, visibility: FR, sampler: {} }] })
      const tex3 = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: FR, texture: {} }, { binding: 1, visibility: FR, texture: {} }, { binding: 2, visibility: FR, texture: {} }, { binding: 3, visibility: FR, sampler: {} }] })
      const over = { color: { srcFactor: 'one', dstFactor: 'one-minus-src-alpha' }, alpha: { srcFactor: 'one', dstFactor: 'one-minus-src-alpha' } }
      const add = { color: { srcFactor: 'one', dstFactor: 'one' }, alpha: { srcFactor: 'zero', dstFactor: 'one' } }
      // motion: 1 writes the pixel's velocity beside its colour, 0 draws into the scene pass
      // without touching the velocity target, undefined is a pass with one target.
      const make = (module, layouts, target, blend, fc, motion, vs = 'vs', fs = 'fs') =>
        device.createRenderPipelineAsync({
          layout: device.createPipelineLayout({ bindGroupLayouts: layouts }),
          vertex: { module, entryPoint: vs },
          fragment: { module, entryPoint: fs, targets: motion === undefined ? [{ format: target, blend }] : [{ format: target, blend }, motion ? { format: VEL, blend: over } : { format: VEL, writeMask: 0 }], constants: fc },
          primitive: { topology: 'triangle-list' },
        })
      const both = async (module, layouts, blend, motion, vs, fs) => [await make(module, layouts, HDR, blend, undefined, undefined, vs, fs), await make(module, layouts, HDR, blend, undefined, motion, vs, fs)]
      const pipes = {
        body: await both(mods.body, [frameLayout, living], undefined, 0),
        heads: await both(mods.heads, [frameLayout, living], over, 1),
        mass: await both(mods.heads, [frameLayout, living], over, 1, 'vsMass', 'fsMass'),
        tendrils: await both(mods.tendrils, [frameLayout], over, 1),
        particles: await both(mods.particles, [frameLayout], add, 0),
        backdrop: await both(mods.backdrop, [frameLayout, backLayout], undefined, 0),
        down: await make(mods.down, [tex1], HDR),
        blurH: await make(mods.blur, [tex1], HDR, undefined, { DX: 1, DY: 0 }),
        blurV: await make(mods.blur, [tex1], HDR, undefined, { DX: 0, DY: 1 }),
        post: await make(mods.post, [frameLayout, tex3], format),
      }
      const bake = (entryPoint, layouts) =>
        device.createRenderPipelineAsync({
          layout: device.createPipelineLayout({ bindGroupLayouts: layouts }),
          vertex: { module: mods.flesh, entryPoint: 'vs' },
          fragment: { module: mods.flesh, entryPoint, targets: [{ format: 'rgba8unorm' }] },
          primitive: { topology: 'triangle-list' },
        })
      pipes.flesh = await bake('fs', [])
      pipes.fleshDown = await bake('down', [tex1])
      const failed = await device.popErrorScope()
      if (failed) {
        console.error('Hex Meetings: WebGPU pipeline refused. ' + failed.message)
        GpuRenderer.failures = [failed.message]
        return null
      }
      const renderer = new GpuRenderer(canvas, ctx, device, pipes, { frameLayout, tex1, tex3, living, backLayout }, timing, format)
      ready = true
      return renderer
    } catch (e) {
      console.error('Hex Meetings: WebGPU unavailable. ' + (e?.message ?? e))
      GpuRenderer.failures = [String(e?.message ?? e)]
      return null
    } finally {
      if (!ready) device?.destroy()
    }
  }

  constructor(canvas, ctx, device, pipes, layouts, timing, format) {
    this.kind = 'WebGPU'
    this.canvas = canvas
    this.ctx = ctx
    this.format = format
    this.device = device
    this.pipes = pipes
    this.layouts = layouts
    this.lost = false
    device.lost.then(info => {
      if (this.lost) return
      this.lost = true
      this.onLost?.(info)
    })
    this.buffer = device.createBuffer({ size: FRAME_FLOATS * 4, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST })
    this.frameBind = device.createBindGroup({ layout: layouts.frameLayout, entries: [{ binding: 0, resource: { buffer: this.buffer } }] })
    this.sampler = device.createSampler({ magFilter: 'linear', minFilter: 'linear', addressModeU: 'clamp-to-edge', addressModeV: 'clamp-to-edge' })
    // The vessels: drawn once, now, and only read from here on.
    const veins = device.createTexture({ size: [VESSELS, VESSELS], format: 'rgba8unorm', usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_DST | GPUTextureUsage.TEXTURE_BINDING })
    const tile = device.createSampler({ magFilter: 'linear', minFilter: 'linear', mipmapFilter: 'linear', addressModeU: 'repeat', addressModeV: 'repeat' })
    device.queue.copyExternalImageToTexture({ source: vesselCanvas() }, { texture: veins }, [VESSELS, VESSELS])
    // The flesh: drawn once too, then each smaller copy from the one above it.
    const flesh = device.createTexture({ size: [FLESH_SIZE, FLESH_SIZE], mipLevelCount: FLESH_MIPS, format: 'rgba8unorm', usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING })
    const enc = device.createCommandEncoder()
    for (let level = 0; level < FLESH_MIPS; level++) {
      const p = enc.beginRenderPass({ colorAttachments: [{ view: flesh.createView({ baseMipLevel: level, mipLevelCount: 1 }), loadOp: 'clear', storeOp: 'store', clearValue: { r: 0, g: 0, b: 0.5, a: 0.5 } }] })
      if (level === 0) p.setPipeline(pipes.flesh)
      else {
        p.setPipeline(pipes.fleshDown)
        p.setBindGroup(0, device.createBindGroup({ layout: layouts.tex1, entries: [{ binding: 0, resource: flesh.createView({ baseMipLevel: level - 1, mipLevelCount: 1 }) }, { binding: 1, resource: tile }] }))
      }
      p.draw(3)
      p.end()
    }
    device.queue.submit([enc.finish()])
    this.veinBind = device.createBindGroup({ layout: layouts.living, entries: [{ binding: 0, resource: veins.createView() }, { binding: 1, resource: tile }, { binding: 2, resource: flesh.createView() }] })
    this.backBuffer = device.createBuffer({ size: BACKDROP_FLOATS * 4, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST })
    this.backBind = device.createBindGroup({ layout: layouts.backLayout, entries: [{ binding: 0, resource: { buffer: this.backBuffer } }, { binding: 1, resource: veins.createView() }, { binding: 2, resource: tile }] })
    const none = { r: 0, g: 0, b: 0, a: 0 }
    const page = { r: 7 / 255, g: 7 / 255, b: 7 / 255, a: 1 }
    const pass = (clear) => ({ colorAttachments: [{ view: null, clearValue: clear, loadOp: 'clear', storeOp: 'store' }] })
    const pass2 = (clear) => ({ colorAttachments: [{ view: null, clearValue: clear, loadOp: 'clear', storeOp: 'store' }, { view: null, clearValue: none, loadOp: 'clear', storeOp: 'store' }] })
    this.pass = { scene: pass2(page), still: pass(page), down: pass(none), blurH: pass(none), blurV: pass(none), post: pass(page) }
    this.w = 1
    this.h = 1
    this.dpr = 1
    this.tw = 0
    this.th = 0
    this.gpuMs = 0
    this.samples = null
    this.timing = false
    this.query = null
    if (timing) {
      // One pair of timestamps for each pass: scene, bloom down, blur across, blur down, post.
      this.query = device.createQuerySet({ type: 'timestamp', count: PASSES.length * 2 })
      this.resolve = device.createBuffer({ size: PASSES.length * 16, usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC })
      this.readback = device.createBuffer({ size: PASSES.length * 16, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ })
      this.reading = false
      this.stamps = PASSES.map((_, n) => ({ querySet: this.query, beginningOfPassWriteIndex: n * 2, endOfPassWriteIndex: n * 2 + 1 }))
      this.passMs = PASSES.map(() => 0)
      this.onRead = () => {
        const ns = new BigInt64Array(this.readback.getMappedRange())
        // The frame: from the start of the first pass to the end of the last.
        const ms = Number(ns[PASSES.length * 2 - 1] - ns[0]) / 1e6
        if (ms > 0 && ms < 1000) {
          this.gpuMs = this.gpuMs ? this.gpuMs * 0.9 + ms * 0.1 : ms
          this.samples?.push(ms) // the benchmark sets this to an array to keep every reading
          // (From the end of the pass before to the end of this one: on Metal every pass's
          // start reads as the start of the frame, so end minus start would be a running total.)
          const each = PASSES.map((_, n) => Number(ns[n * 2 + 1] - (n ? ns[n * 2 - 1] : ns[0])) / 1e6)
          for (let n = 0; n < each.length; n++) this.passMs[n] = this.passMs[n] ? this.passMs[n] * 0.9 + each[n] * 0.1 : each[n]
          this.passSamples?.push(each) // and this, to keep every pass of every reading
        }
        this.readback.unmap()
        this.reading = false
      }
      this.onReadFail = () => (this.reading = false)
    }
  }

  // Ask for GPU pass times (?perf). Costs one tiny buffer read a frame, so it is off otherwise.
  setTiming(on) {
    this.timing = !!on && !!this.query
  }

  resize(w, h, dpr) {
    if (this.lost) return
    this.w = w
    this.h = h
    const limit = this.device.limits.maxTextureDimension2D
    this.dpr = Math.min(dpr, limit / Math.max(1, w, h))
    const cw = Math.min(limit, Math.max(1, Math.round(w * this.dpr)))
    const ch = Math.min(limit, Math.max(1, Math.round(h * this.dpr)))
    if (this.canvas.width !== cw) this.canvas.width = cw
    if (this.canvas.height !== ch) this.canvas.height = ch
    if (cw === this.tw && ch === this.th) return
    // A resize discards the current canvas texture. Reaffirm the same configuration before
    // rebuilding attachments; no configure work is done on ordinary frames.
    this.ctx.configure({ device: this.device, format: this.format, alphaMode: 'opaque' })
    this.tw = cw
    this.th = ch
    const d = this.device
    for (const t of this.textures ?? []) t.destroy()
    const T = GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING
    const tex = (tw, th) => d.createTexture({ size: [Math.max(1, tw), Math.max(1, th)], format: HDR, usage: T })
    const scene = tex(cw, ch)
    const q1 = tex(cw >> 2, ch >> 2)
    const q2 = tex(cw >> 2, ch >> 2)
    const vel = d.createTexture({ size: [cw, ch], format: VEL, usage: T })
    this.textures = [scene, q1, q2, vel]
    const v = { scene: scene.createView(), q1: q1.createView(), q2: q2.createView(), vel: vel.createView() }
    this.pass.still.colorAttachments[0].view = v.scene
    this.pass.scene.colorAttachments[0].view = v.scene
    this.pass.scene.colorAttachments[1].view = v.vel
    this.pass.down.colorAttachments[0].view = v.q1
    this.pass.blurH.colorAttachments[0].view = v.q2
    this.pass.blurV.colorAttachments[0].view = v.q1
    const one = (view) => d.createBindGroup({ layout: this.layouts.tex1, entries: [{ binding: 0, resource: view }, { binding: 1, resource: this.sampler }] })
    this.bind = {
      scene: one(v.scene), q1: one(v.q1), q2: one(v.q2),
      post: d.createBindGroup({ layout: this.layouts.tex3, entries: [{ binding: 0, resource: v.scene }, { binding: 1, resource: v.q1 }, { binding: 2, resource: v.vel }, { binding: 3, resource: this.sampler }] }),
    }
  }

  // frame: the G layout from cast.js. fx says which optional draws have anything in them this
  // frame: { body, tendrils (how many), motes }. back: the notes' backdrop (ambient.js), or null.
  draw(frame, count, fx, back = null) {
    if (this.lost || !this.bind) return
    const P = this.pipes
    frame[G.VIEW] = this.w
    frame[G.VIEW + 1] = this.h
    frame[G.MISC + 1] = count
    frame[G.MISC + 2] = this.dpr
    this.device.queue.writeBuffer(this.buffer, 0, frame, 0, fx.tendrils ? G.FLOATS : G.TENDRILS)
    const enc = this.device.createCommandEncoder()
    const timed = this.timing && !this.reading
    // Is a face, a tile or a filament on the move this frame? Only then does the scene write
    // the second target. (vfx.js decides; the post pass reads the same flag, and when it is 1
    // smears for the lens alone without reading that target.)
    const m = frame[G.BLUR + 3] > 1.5 ? 1 : 0
    const scene = m ? this.pass.scene : this.pass.still
    const stamp = (pass, n) => (pass.timestampWrites = timed ? this.stamps[n] : undefined)
    stamp(scene, 0)
    stamp(this.pass.down, 1)
    stamp(this.pass.blurH, 2)
    stamp(this.pass.blurV, 3)
    stamp(this.pass.post, 4)

    let p = enc.beginRenderPass(scene)
    p.setBindGroup(0, this.frameBind)
    if (back) {
      this.device.queue.writeBuffer(this.backBuffer, 0, back)
      p.setPipeline(P.backdrop[m])
      p.setBindGroup(1, this.backBind)
      p.draw(3)
    }
    p.setBindGroup(1, this.veinBind)
    if (fx.body) {
      p.setPipeline(P.body[m])
      p.draw(3)
    }
    if (count > 0) {
      p.setPipeline(P.heads[m])
      p.draw(6, Math.min(count, MAX))
    }
    if (fx.mass) {
      p.setPipeline(P.mass[m])
      p.draw(6, 1)
    }
    if (fx.tendrils) {
      p.setPipeline(P.tendrils[m])
      p.draw(SEGMENT_VERTS, fx.tendrils)
    }
    if (fx.motes) {
      p.setPipeline(P.particles[m])
      p.draw(6, BURSTS * PER_BURST)
    }
    p.end()

    p = enc.beginRenderPass(this.pass.down)
    p.setPipeline(P.down)
    p.setBindGroup(0, this.bind.scene)
    p.draw(3)
    p.end()
    p = enc.beginRenderPass(this.pass.blurH)
    p.setPipeline(P.blurH)
    p.setBindGroup(0, this.bind.q1)
    p.draw(3)
    p.end()
    p = enc.beginRenderPass(this.pass.blurV)
    p.setPipeline(P.blurV)
    p.setBindGroup(0, this.bind.q2)
    p.draw(3)
    p.end()

    this.pass.post.colorAttachments[0].view = this.ctx.getCurrentTexture().createView()
    p = enc.beginRenderPass(this.pass.post)
    p.setPipeline(P.post)
    p.setBindGroup(0, this.frameBind)
    p.setBindGroup(1, this.bind.post)
    p.draw(3)
    p.end()
    if (timed) {
      enc.resolveQuerySet(this.query, 0, PASSES.length * 2, this.resolve, 0)
      enc.copyBufferToBuffer(this.resolve, 0, this.readback, 0, PASSES.length * 16)
    }
    this.device.queue.submit([enc.finish()])
    if (timed) {
      this.reading = true
      this.readback.mapAsync(GPUMapMode.READ).then(this.onRead, this.onReadFail)
    }
  }

  // Resolves when the GPU has finished everything submitted so far.
  done() {
    return this.device.queue.onSubmittedWorkDone()
  }
}
