import { useLayoutEffect, useState } from "react"
import ChatteringTeeth from "./ChatteringTeeth"
import Download, { PLATFORM_NAMES, SHORTCUTS } from "./Download"
import { guessPlatform, refinePlatform, type Platform } from "./platform"

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

  return (
    <main className="page">
      <header className="header">
        <a className="wordmark" href="/" aria-label="Hex home">
          <HexMark />
          <span>HEX</span>
        </a>
        <a className="header__github" href="https://github.com/anomalyco/hex">
          <GitHubMark />
          <span>GitHub</span>
        </a>
      </header>

      <section className="hero">
        <div className="hero__copy">
          <div className="hero__title">
            <h1>Speak. It appears.</h1>
            <ChatteringTeeth />
          </div>
          <p>
            Local voice dictation for {PLATFORM_NAMES[platform]}. Hold {SHORTCUTS[platform]}, say what you mean, and
            Hex pastes it where you are.
          </p>
        </div>

        <Download platform={platform} />
      </section>
    </main>
  )
}
