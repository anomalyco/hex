import { useCallback, useEffect, useRef, useState, type RefObject } from "react"
import { setGlow } from "./glow"

// The hero show lives in ./shout (plain JS, WebGPU with a Canvas 2D fallback). The prerendered
// page shows a still of the idle head; the engine loads when the browser is idle or the visitor
// reaches for the key, takes over the same box, and only fetches audio once they press it.

type Engine = {
  renderer: string
  pressKey?(pressed: boolean, rect?: DOMRect | null): void
  explodeKey?(rect?: DOMRect | null): void
  start(options: { context: AudioContext | null; muted: boolean; keyRect?: DOMRect | null }): Promise<void>
  setMuted(muted: boolean, context?: AudioContext | null): void
  destroy(): void
}
export type Phase = "idle" | "live" | "starting" | "started"

const MUTED_KEY = "hex-shout-muted"
const readMuted = () => {
  try {
    return localStorage.getItem(MUTED_KEY) === "1"
  } catch {
    return false
  }
}
const newContext = () => {
  try {
    return new AudioContext({ latencyHint: "interactive" })
  } catch {
    return null
  }
}

export function useShout(root: RefObject<HTMLElement | null>, canvas: RefObject<HTMLCanvasElement | null>, head: RefObject<HTMLElement | null>) {
  const [phase, setPhase] = useState<Phase>("idle")
  const [muted, setMutedState] = useState(false)
  const engine = useRef<Promise<Engine | null> | null>(null)
  const live = useRef<Engine | null>(null)
  const reduced = useRef(false)

  const load = useCallback(() => {
    if (!engine.current) {
      const params = new URLSearchParams(location.search)
      engine.current = import("./shout/main.js")
        .then(({ mountShout }) =>
          mountShout({
            root: root.current,
            canvas: canvas.current,
            head: head.current,
            reduced: reduced.current,
            forceCanvas2d: params.has("canvas2d"),
          }),
        )
        .then((e: Engine) => {
          live.current = e
          // The canvas has drawn the head; the still can go.
          root.current?.setAttribute("data-live", "")
          setPhase((p) => (p === "idle" ? "live" : p))
          return e
        })
        .catch(() => null)
    }
    return engine.current
  }, [root, canvas, head])

  useEffect(() => {
    setMutedState(readMuted())
    reduced.current = matchMedia("(prefers-reduced-motion: reduce)").matches
    // Reduced motion and phones keep the still until the visitor asks for the show: building
    // the renderer is a long task on a phone CPU.
    if (reduced.current || matchMedia("(pointer: coarse), (max-width: 40rem)").matches) return
    const idle = "requestIdleCallback" in window ? requestIdleCallback(() => void load(), { timeout: 2500 }) : setTimeout(() => void load(), 1200)
    return () => {
      if ("cancelIdleCallback" in window) cancelIdleCallback(idle as number)
      else clearTimeout(idle as ReturnType<typeof setTimeout>)
    }
  }, [load])

  useEffect(() => () => live.current?.destroy(), [])

  const context = useRef<AudioContext | null>(null)
  // Building an AudioContext blocks the main thread for tens of milliseconds, so it is made when the
  // visitor reaches for the key; the click still resumes it, inside the gesture.
  const intent = useCallback(() => {
    if (!readMuted()) context.current ??= newContext()
    void load()
  }, [load])

  const pressKey = useCallback((down: boolean, el?: HTMLElement | null) => {
    live.current?.pressKey?.(down, el?.getBoundingClientRect() ?? null)
  }, [])

  // Must run inside the click: that gesture is what lets the browser play audio.
  const start = useCallback(
    (el?: HTMLElement | null) => {
      if (phase === "starting" || phase === "started") return
      const quiet = readMuted()
      if (!quiet) context.current ??= newContext()
      const resumed = context.current?.resume()
      const keyRect = el?.getBoundingClientRect() ?? null
      setPhase("starting")
      if (live.current && !reduced.current) {
        root.current?.setAttribute("data-key-exploded", "")
        live.current.explodeKey?.(keyRect)
      }
      void load().then(async (e) => {
        if (!e) return setPhase("idle")
        if (!reduced.current) root.current?.setAttribute("data-key-exploded", "")
        // A context made before the gesture is still suspended; the engine only sounds once it runs.
        if (resumed && context.current?.state !== "running") await Promise.race([resumed.catch(() => {}), new Promise((r) => setTimeout(r, 250))])
        await e.start({ context: context.current, muted: quiet, keyRect })
        setPhase("started")
      })
    },
    [phase, load, root],
  )

  const toggleMuted = useCallback(() => {
    const next = !muted
    setMutedState(next)
    try {
      localStorage.setItem(MUTED_KEY, next ? "1" : "0")
    } catch {}
    if (!next) {
      context.current ??= newContext()
      void context.current?.resume()
    }
    live.current?.setMuted(next, context.current)
  }, [muted])

  return { phase, muted, start, intent, pressKey, toggleMuted }
}

export function OptionKey({
  phase,
  onStart,
  onIntent,
  onPress,
}: {
  phase: Phase
  onStart(el?: HTMLElement | null): void
  onIntent(): void
  onPress?(down: boolean, el?: HTMLElement | null): void
}) {
  const ref = useRef<HTMLButtonElement>(null)
  const [pressed, setPressed] = useState(false)
  const gone = phase === "started"
  // The glow pulses (./glow.ts) from here until the show's engine draws the head and takes it over. A timer waits
  // out each 1/30 s and the value is set in the next frame callback, so it lands on that vsync's frame.
  const pulsing = phase === "idle"
  useEffect(() => {
    const el = ref.current
    if (!el || !pulsing || matchMedia("(prefers-reduced-motion: reduce)").matches) return
    let timer = 0
    let raf = 0
    let onScreen = true
    const frame = (now: number) => {
      raf = 0
      setGlow(el, now)
      // A frame and a half early: asked for from idle, a frame's slot is already under way (see shout/main.js).
      if (!document.hidden && onScreen) timer = window.setTimeout(next, 1000 / 30 - 10)
    }
    const next = () => {
      timer = 0
      raf = requestAnimationFrame(frame)
    }
    const resume = () => {
      if (!timer && !raf && !document.hidden && onScreen) next()
    }
    const io = new IntersectionObserver(([e]) => {
      onScreen = e.isIntersecting
      resume()
    })
    io.observe(el)
    document.addEventListener("visibilitychange", resume)
    next()
    return () => {
      clearTimeout(timer)
      cancelAnimationFrame(raf)
      io.disconnect()
      document.removeEventListener("visibilitychange", resume)
    }
  }, [pulsing])

  const setDown = (down: boolean) => {
    setPressed(down)
    onPress?.(down, ref.current)
  }

  return (
    <button
      ref={ref}
      type="button"
      className="keycap"
      data-pressed={pressed || phase === "starting" || undefined}
      data-gone={gone || undefined}
      aria-label="Play the demo with sound"
      tabIndex={gone ? -1 : undefined}
      aria-hidden={gone || undefined}
      onPointerEnter={onIntent}
      onFocus={onIntent}
      onPointerDown={() => {
        onIntent()
        setDown(true)
      }}
      onPointerUp={() => setDown(false)}
      onPointerLeave={() => {
        if (pressed) setDown(false)
      }}
      onPointerCancel={() => setDown(false)}
      onBlur={() => {
        if (pressed) setDown(false)
      }}
      onKeyDown={(e) => {
        if ((e.key === " " || e.key === "Enter") && !e.repeat) {
          onIntent()
          setDown(true)
        }
      }}
      onKeyUp={(e) => {
        if (e.key === " " || e.key === "Enter") setDown(false)
      }}
      onClick={() => onStart(ref.current)}
    >
      <span className="keycap__glow" aria-hidden="true" />
      <span className="keycap__well" aria-hidden="true" />
      <span className="keycap__cap" aria-hidden="true">
        <span className="keycap__dish">
          <svg viewBox="0 0 24 18" className="keycap__glyph">
            <path d="M2.5 3.5h5.2l6.2 11h7.6M14.2 3.5h7.3" />
          </svg>
          <span className="keycap__label">option</span>
        </span>
      </span>
      <span className="keycap__hint" aria-hidden="true">
        <span className="keycap__hint-fine">Click to play</span>
        <span className="keycap__hint-touch">Tap to play</span>
      </span>
    </button>
  )
}

export function SoundToggle({ muted, onToggle }: { muted: boolean; onToggle(): void }) {
  return (
    <button type="button" className="sound" aria-pressed={!muted} aria-label={muted ? "Unmute" : "Mute"} onClick={onToggle}>
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <path d="M3 8v4h3l4 3V5L6 8z" />
        {muted ? <path d="m13.5 8 4 4m0-4-4 4" fill="none" /> : <path d="M13 7.5a3.5 3.5 0 0 1 0 5M15.2 5.5a6.5 6.5 0 0 1 0 9" fill="none" />}
      </svg>
    </button>
  )
}
