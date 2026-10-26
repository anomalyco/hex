import { useLayoutEffect, useRef, useState } from "react"
import Download, { PLATFORM_NAMES, SHORTCUTS } from "./Download"
import { guessPlatform, refinePlatform, type Platform } from "./platform"
import { OptionKey, SoundToggle, useShout } from "./Shout"
import signature from "./footer-signature.svg"

function HexMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 64 64" className="wordmark__icon">
      <path d="M32 4 57 18.5v27L32 60 7 45.5v-27Z" />
      <path d="M32 15 48 24.25v15.5L32 49l-16-9.25v-15.5Z" />
      <path className="wordmark__core" d="m32 24 7.5 4.25v8.5L32 41l-7.5-4.25v-8.5Z" />
    </svg>
  )
}

function GitHubMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 16 16" className="header__github-icon">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  )
}

export default function App() {
  // The prerendered HTML is the Mac page; switch before the first paint, then
  // refine with client hints.
  const [platform, setPlatform] = useState<Platform>("mac")
  useLayoutEffect(() => {
    let current = true
    const guess = guessPlatform()
    setPlatform(guess)
    void refinePlatform(guess).then((refined) => {
      if (current && refined !== guess) setPlatform(refined)
    })
    return () => {
      current = false
    }
  }, [])

  const root = useRef<HTMLElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const head = useRef<HTMLDivElement>(null)
  const shout = useShout(root, canvas, head)

  return (
    <>
      <main className="page" ref={root} data-phase={shout.phase}>
        <canvas ref={canvas} className="stage" aria-hidden="true" />
        <header className="header">
          <a className="wordmark" href={import.meta.env.BASE_URL} aria-label="Hex home" data-avoid>
            <HexMark />
            <span>HEX</span>
          </a>
          <div className="header__tools" data-avoid>
            {shout.phase === "started" && <SoundToggle muted={shout.muted} onToggle={shout.toggleMuted} />}
            <a className="header__github" href="https://github.com/anomalyco/hex">
              <GitHubMark />
              <span>GitHub</span>
            </a>
          </div>
        </header>

        <div className="show">
          <div className="show__head" ref={head}>
            <img src={`${import.meta.env.BASE_URL}shout-idle.webp`} alt="" width="640" height="640" decoding="async" fetchPriority="high" />
          </div>
          <OptionKey phase={shout.phase} onStart={shout.start} onIntent={shout.load} onPress={shout.pressKey} />
        </div>

        <section className="hero">
          <div className="hero__copy" data-avoid>
            <h1 aria-label="Speak. It appears.">
              <span className="hero__word" data-title-word="0">
                Speak.
              </span>{" "}
              <span className="hero__word" data-title-word="1">
                It
              </span>{" "}
              <span className="hero__word" data-title-word="2">
                appears.
              </span>
            </h1>
            <p>
              Local voice dictation for {PLATFORM_NAMES[platform]}. Hold {SHORTCUTS[platform]}, say what you mean, and
              Hex pastes it where you are.
            </p>
          </div>

          <Download platform={platform} />
        </section>
      </main>
      <footer className="site-footer">
        <div className="copyright">
          <img src={signature} alt="" aria-hidden="true" width={163} height={64} />
          <p>
            ©2026 <a href="https://anoma.ly">Anomaly</a>. All rights reserved.
          </p>
        </div>
      </footer>
    </>
  )
}
