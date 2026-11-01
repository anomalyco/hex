// The option key's glow pulse, set as --glow-pulse (0 dim, 1 bright) on the key 30 times a second. As a CSS
// animation it restyled the page on every vsync, 165 times a second on a 165 Hz display, for a change too slow
// to see between them. Before the show's engine runs, OptionKey sets it; once the engine draws the idle head, the
// engine sets it in the same frame. Both read one clock, so the hand-over is seamless.

export const GLOW_PERIOD_MS = 2800

// cubic-bezier(0.42, 0, 0.58, 1), CSS ease-in-out, at x: Newton on the curve's x, then its y.
function easeInOut(x: number) {
  let s = x
  for (let i = 0; i < 8; i++) {
    const bx = 3 * (1 - s) * (1 - s) * s * 0.42 + 3 * (1 - s) * s * s * 0.58 + s * s * s
    const dx = 3 * (1 - s) * (1 - s) * 0.42 + 6 * (1 - s) * s * 0.16 + 3 * s * s * 0.42
    if (dx < 1e-6) break
    s -= (bx - x) / dx
  }
  return 3 * (1 - s) * s * s + s * s * s
}

// Up for half a period and down for the other, each eased, as the keyframes it replaces.
export function glowPulse(nowMs: number) {
  const cycles = nowMs / GLOW_PERIOD_MS
  const u = cycles - Math.floor(cycles)
  return easeInOut(u < 0.5 ? u * 2 : (1 - u) * 2)
}

export function setGlow(key: HTMLElement, nowMs: number) {
  key.style.setProperty("--glow-pulse", glowPulse(nowMs).toFixed(3))
}
