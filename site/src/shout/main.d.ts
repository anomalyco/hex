export const HEAD_BOX: number

export function mountShout(options: {
  root: HTMLElement | null
  canvas: HTMLCanvasElement | null
  head: HTMLElement | null
  reduced: boolean
  forceCanvas2d?: boolean
}): Promise<{
  renderer: "webgpu" | "canvas2d"
  start(options: { context: AudioContext | null; muted: boolean }): Promise<void>
  setMuted(muted: boolean, context?: AudioContext | null): void
  destroy(): void
}>
