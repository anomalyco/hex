import { useEffect, useRef, useState } from "react"
import * as THREE from "three"
import { OutlineEffect } from "three/examples/jsm/effects/OutlineEffect.js"
import { RoundedBoxGeometry } from "three/examples/jsm/geometries/RoundedBoxGeometry.js"

type MicrophoneState = "idle" | "requesting" | "listening" | "error"

function createJaw(upper: boolean, toothMaterial: THREE.Material, shellMaterial: THREE.Material) {
  const jaw = new THREE.Group()
  const shell = new THREE.Mesh(
    new RoundedBoxGeometry(4.5, 0.92, 1.18, 8, 0.3),
    shellMaterial,
  )
  shell.position.set(0, upper ? 0.78 : -0.78, 0.34)
  jaw.add(shell)

  const toothGeometry = new RoundedBoxGeometry(0.46, 0.72, 0.38, 7, 0.17)
  const count = 11

  for (let index = 0; index < count; index += 1) {
    const amount = index / (count - 1)
    const normalized = amount * 2 - 1
    const angle = normalized * 1.04
    const side = Math.abs(normalized)
    const tooth = new THREE.Mesh(toothGeometry, toothMaterial)

    tooth.position.set(
      Math.sin(angle) * 2.02,
      upper ? 0.34 : -0.34,
      0.72 + Math.cos(angle) * 0.52,
    )
    tooth.rotation.y = -angle * 0.92
    tooth.rotation.z = upper ? normalized * 0.025 : -normalized * 0.025
    tooth.scale.set(1 - side * 0.24, 1 - side * 0.13, 1 + side * 0.32)
    jaw.add(tooth)
  }

  return jaw
}

export default function ChatteringTeeth() {
  const [state, setState] = useState<MicrophoneState>("idle")
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const streamRef = useRef<MediaStream | null>(null)
  const audioContextRef = useRef<AudioContext | null>(null)
  const analyserRef = useRef<AnalyserNode | null>(null)
  const listeningRef = useRef(false)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return

    const renderer = new THREE.WebGLRenderer({
      canvas,
      alpha: true,
      antialias: true,
      powerPreference: "high-performance",
    })
    renderer.outputColorSpace = THREE.SRGBColorSpace
    renderer.toneMapping = THREE.ACESFilmicToneMapping
    renderer.toneMappingExposure = 1
    const outline = new OutlineEffect(renderer, {
      defaultThickness: 0.008,
      defaultColor: [0.012, 0.014, 0.018],
      defaultAlpha: 0.9,
      defaultKeepAlive: true,
    })

    const scene = new THREE.Scene()
    scene.fog = new THREE.FogExp2(0x08090a, 0.055)

    const camera = new THREE.PerspectiveCamera(36, 1, 0.1, 40)
    camera.position.set(0, 0, 9)

    const gradientMap = new THREE.DataTexture(
      new Uint8Array([48, 112, 184, 255]),
      4,
      1,
      THREE.RedFormat,
    )
    gradientMap.minFilter = THREE.NearestFilter
    gradientMap.magFilter = THREE.NearestFilter
    gradientMap.needsUpdate = true

    const toothMaterial = new THREE.MeshToonMaterial({
      color: 0xffffff,
      gradientMap,
    })
    const shellMaterial = new THREE.MeshToonMaterial({
      color: 0xf12549,
      gradientMap,
    })
    const eyeMaterial = new THREE.MeshToonMaterial({
      color: 0xf6f4ee,
      gradientMap,
    })
    const pupilMaterial = new THREE.MeshToonMaterial({
      color: 0x020203,
      gradientMap,
    })
    const innerMaterial = new THREE.MeshToonMaterial({
      color: 0x21040b,
      gradientMap,
    })
    const metalMaterial = new THREE.MeshToonMaterial({
      color: 0x8ca3b3,
      gradientMap,
    })

    const root = new THREE.Group()
    const rig = new THREE.Group()
    const upperJaw = createJaw(true, toothMaterial, shellMaterial)
    const lowerJaw = createJaw(false, toothMaterial, shellMaterial)

    const eyeGeometry = new THREE.SphereGeometry(0.64, 40, 28)
    const pupilGeometry = new THREE.SphereGeometry(0.27, 32, 20)
    const pupils: THREE.Mesh[] = []
    for (const x of [-0.7, 0.7]) {
      const eye = new THREE.Mesh(eyeGeometry, eyeMaterial)
      eye.position.set(x, 1.7, 0.54)
      upperJaw.add(eye)

      const pupil = new THREE.Mesh(pupilGeometry, pupilMaterial)
      pupil.position.set(x, 1.7, 1.11)
      pupil.scale.z = 0.58
      pupil.userData.homeX = x
      pupils.push(pupil)
      upperJaw.add(pupil)
    }

    const inner = new THREE.Mesh(
      new RoundedBoxGeometry(3.72, 0.7, 0.48, 6, 0.2),
      innerMaterial,
    )
    inner.position.set(0, -0.02, 0.34)

    const supportGeometry = new RoundedBoxGeometry(0.24, 1.35, 0.22, 4, 0.08)
    for (const x of [-0.72, 0.72]) {
      const support = new THREE.Mesh(supportGeometry, shellMaterial)
      support.position.set(x, -0.08, 0.66)
      rig.add(support)
    }

    const axle = new THREE.Mesh(new THREE.CylinderGeometry(0.09, 0.09, 4.25, 20), metalMaterial)
    axle.rotation.z = Math.PI / 2
    axle.position.set(0, -0.08, 0.52)

    const crank = new THREE.Group()
    const crankShaft = new THREE.Mesh(new THREE.CylinderGeometry(0.08, 0.08, 0.78, 18), metalMaterial)
    crankShaft.rotation.z = Math.PI / 2
    crankShaft.position.x = -2.56
    const crankHandle = new THREE.Mesh(
      new RoundedBoxGeometry(0.32, 0.7, 0.28, 5, 0.11),
      eyeMaterial,
    )
    crankHandle.position.x = -2.95
    crank.add(crankShaft, crankHandle)

    const hingeGeometry = new THREE.CylinderGeometry(0.25, 0.25, 0.24, 28)
    for (const x of [-2.22, 2.22]) {
      const hinge = new THREE.Mesh(hingeGeometry, shellMaterial)
      hinge.rotation.x = Math.PI / 2
      hinge.position.set(x, -0.08, 0.47)
      rig.add(hinge)
    }

    rig.add(inner, axle, crank, upperJaw, lowerJaw)
    root.add(rig)
    scene.add(root)

    scene.add(new THREE.AmbientLight(0x526073, 1.35))
    const keyLight = new THREE.DirectionalLight(0xfff5e7, 3.2)
    keyLight.position.set(-3.5, 4.5, 6)
    scene.add(keyLight)
    const fillLight = new THREE.DirectionalLight(0x62bde8, 1.25)
    fillLight.position.set(4, -2, 3)
    scene.add(fillLight)

    const timer = new THREE.Timer()
    timer.connect(document)
    const frequencyData = new Uint8Array(64)
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)")
    let frame = 0
    let smoothedLevel = 0
    let chatterPhase = 0

    function resize() {
      const width = window.innerWidth
      const height = window.innerHeight
      camera.aspect = width / height
      camera.updateProjectionMatrix()
      renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2))
      outline.setSize(width, height, false)

      const rootScale = THREE.MathUtils.clamp(width / 1800, 0.24, 0.38)
      root.scale.setScalar(rootScale)
      root.position.set(0, width < 640 ? 1.98 : 1.58, 0)
    }

    function microphoneLevel() {
      const analyser = analyserRef.current
      if (!analyser) return 0
      analyser.getByteFrequencyData(frequencyData)

      let energy = 0
      for (let index = 1; index < 34; index += 1) energy += frequencyData[index]
      const average = energy / 33
      return THREE.MathUtils.clamp((average - 4) / 62, 0, 1)
    }

    function animate() {
      timer.update()
      const delta = Math.min(timer.getDelta(), 0.04)
      const elapsed = timer.getElapsed()
      const rawLevel = listeningRef.current ? microphoneLevel() : 0
      smoothedLevel = smoothedLevel * 0.58 + rawLevel * 0.42
      const idleCycle = elapsed % 3.8
      const idleChatter = !reducedMotion.matches && idleCycle < 0.82
        ? Math.pow((Math.sin(elapsed * 48) + 1) / 2, 1.7) * 0.3
        : 0

      if (listeningRef.current) chatterPhase += delta * (42 + smoothedLevel * 42)
      const chatter = Math.pow((Math.sin(chatterPhase) + 1) / 2, 1.5)
      const jawOpen = listeningRef.current
        ? 0.04 + chatter * (0.38 + smoothedLevel * 0.62)
        : 0.025 + idleChatter
      const impact = listeningRef.current ? Math.pow(1 - chatter, 12) * (0.02 + smoothedLevel * 0.04) : 0

      upperJaw.position.y = jawOpen * 0.06
      lowerJaw.position.y = -jawOpen * 0.94
      lowerJaw.rotation.x = -jawOpen * 0.16
      crank.rotation.x = chatterPhase * 0.7
      for (const pupil of pupils) {
        pupil.position.x = pupil.userData.homeX + Math.sin(elapsed * 0.7) * 0.045
        pupil.position.y = 1.7 + Math.cos(elapsed * 0.55) * 0.025 - impact
      }
      rig.position.y = -impact
      rig.rotation.z = Math.sin(chatterPhase * 0.5) * smoothedLevel * 0.025
      rig.rotation.y = Math.sin(chatterPhase * 0.37) * smoothedLevel * 0.02
      keyLight.intensity = 3.2 + smoothedLevel * 0.9

      if (!reducedMotion.matches) {
        root.rotation.y = Math.sin(elapsed * 0.62) * 0.5
        root.rotation.x = -0.04 + Math.cos(elapsed * 0.38) * 0.055
        root.rotation.z = Math.sin(elapsed * 0.31) * 0.035
      }

      outline.render(scene, camera)
      frame = requestAnimationFrame(animate)
    }

    resize()
    window.addEventListener("resize", resize)
    frame = requestAnimationFrame(animate)

    return () => {
      cancelAnimationFrame(frame)
      window.removeEventListener("resize", resize)
      scene.traverse((object) => {
        if (object instanceof THREE.Mesh) object.geometry.dispose()
      })
      toothMaterial.dispose()
      shellMaterial.dispose()
      eyeMaterial.dispose()
      pupilMaterial.dispose()
      innerMaterial.dispose()
      metalMaterial.dispose()
      gradientMap.dispose()
      renderer.dispose()
      timer.dispose()
    }
  }, [])

  useEffect(() => {
    return () => {
      listeningRef.current = false
      streamRef.current?.getTracks().forEach((track) => track.stop())
      void audioContextRef.current?.close()
    }
  }, [])

  function stopListening() {
    listeningRef.current = false
    streamRef.current?.getTracks().forEach((track) => track.stop())
    streamRef.current = null
    analyserRef.current = null
    void audioContextRef.current?.close()
    audioContextRef.current = null
    setState("idle")
  }

  async function toggleMicrophone() {
    if (listeningRef.current) {
      stopListening()
      return
    }

    if (!navigator.mediaDevices?.getUserMedia) {
      setState("error")
      return
    }

    setState("requesting")
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      const audioContext = new AudioContext()
      const analyser = audioContext.createAnalyser()
      analyser.fftSize = 128
      analyser.smoothingTimeConstant = 0.48
      audioContext.createMediaStreamSource(stream).connect(analyser)

      streamRef.current = stream
      audioContextRef.current = audioContext
      analyserRef.current = analyser
      listeningRef.current = true
      setState("listening")
    } catch {
      setState("error")
    }
  }

  const label = state === "listening" ? "Stop chattering teeth" : "Make the teeth speak"
  const status =
    state === "requesting"
      ? "Waiting for microphone permission"
      : state === "listening"
        ? "Listening. The teeth are following your voice."
        : state === "error"
          ? "Microphone access is unavailable."
          : ""

  return (
    <>
      <canvas ref={canvasRef} className="teeth-canvas" aria-hidden="true" />
      <div className="mic-control">
        <button
          className={`mic-button mic-button--${state}`}
          type="button"
          aria-label={label}
          aria-pressed={state === "listening"}
          aria-describedby="microphone-status"
          disabled={state === "requesting"}
          onClick={toggleMicrophone}
        >
          <span className="mic-button__glyph" aria-hidden="true" />
        </button>
        <span id="microphone-status" className="visually-hidden" aria-live="polite">
          {status}
        </span>
      </div>
    </>
  )
}
