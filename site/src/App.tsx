import { useLayoutEffect, useRef, useState } from "react"
import Download, { OS_KEY_LEGENDS, OS_NAMES, OS_SHORTCUTS, osOf, type Os } from "./Download"
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

  // The install tabs and the hero's key are one choice: clicking a tab takes
  // the hero to that OS, until then it follows the detected platform.
  const [chosenOs, setChosenOs] = useState<Os>()
  const os = chosenOs ?? osOf(platform)

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
            <span>Hex</span>
          </a>
          <div className="header__tools" data-avoid>
            {shout.phase === "started" && <SoundToggle muted={shout.muted} onToggle={shout.toggleMuted} />}
            <a className="header__link" href={`${import.meta.env.BASE_URL}meetings/`}>
              Meetings
            </a>
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
          <OptionKey
            os={os}
            phase={shout.phase}
            onStart={(el) => shout.start(el, OS_KEY_LEGENDS[os])}
            onIntent={shout.intent}
            onPress={shout.pressKey}
          />
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
            <p className="hero__device">
              {os === "mobile"
                ? "Local voice dictation for iOS and Android. Dictate right inside the keyboard, in any app, on your phone."
                : `Local voice dictation for ${OS_NAMES[os]}. Hold ${OS_SHORTCUTS[os]}, say what you mean, and Hex pastes it where you are.`}
            </p>
            {/* Scripts off: the line above is only the page's first guess, so name
                every platform; index.html's <noscript> stylesheet swaps them. */}
            <p className="hero__any">
              Local voice dictation for Mac, Windows, Linux, iOS, and Android. Hold your
              shortcut or dictate from your phone&apos;s keyboard, and Hex types what you said.
            </p>
          </div>

          <Download platform={platform} os={os} onChooseOs={setChosenOs} />
        </section>
      </main>
      <section className="details">
        <div className="details__inner">
          <section aria-labelledby="dictation">
            <h2 id="dictation">Voice dictation that stays on your machine.</h2>
            <p>
              Hex is free, open-source voice dictation for Mac, Windows, and Linux. Hold the shortcut,
              say what you mean, and release: a local Parakeet or Whisper model turns the audio into
              text and Hex pastes it wherever your cursor is. Audio never leaves the machine, and
              there is no account, no subscription, and no word limit. Windows and Linux builds are
              in beta; the Mac build is stable. On the Mac you can also choose Apple Speech.
            </p>
          </section>

          <section aria-labelledby="meetings">
            <h2 id="meetings">Meetings without a bot.</h2>
            <p>
              Hex Meetings records your microphone and your Mac&apos;s system audio at the same time, so
              the other side of the call is captured without an invite, a join link, or a participant
              that shows up in the list. It transcribes while you talk, keeps chapters as the call
              moves, and stores a short summary when the Mac can write one. Recordings stay in a
              folder you own, expire on a schedule you choose, and can be read from the app, the
              CLI, or <a href={`${import.meta.env.BASE_URL}mcp`}>hex mcp</a>. Nothing is uploaded.
            </p>
          </section>

          <section aria-labelledby="tools">
            <h2 id="tools">Screenshots, transforms, and the command line.</h2>
            <p>
              Annotate captures a screen, a window, a region, or a connected iPhone and opens the
              capture in an OpenCode session. Transforms are JavaScript in{" "}
              <code>~/.config/hex/hex.config.ts</code> that rewrite dictated text deterministically
              before it is pasted, from a name you always misspell to a snippet that calls a script
              you trust. The <code>hex</code> command drives the app from a terminal: status,
              dictation, settings, history, meetings, and <code>hex transcribe</code> for an audio
              file. <a href={`${import.meta.env.BASE_URL}mcp`}>hex mcp</a> serves meeting
              transcripts, search, chapters, and stored notes to AI coding agents, read-only, over
              stdio.
            </p>
          </section>

          <section className="faq" aria-labelledby="faq">
            <h2 id="faq">Questions people ask.</h2>
            <h3>Is Hex really free?</h3>
            <p>
              Yes. Hex is open source under the MIT license. There is no account, no subscription, and
              no word limit.
            </p>
            <h3>Does Hex work offline?</h3>
            <p>
              Yes. Hex downloads a speech model once, then turns speech into text on your own machine.
              Dictation and meeting transcription keep working with no internet connection.
            </p>
            <h3>Which speech models does Hex run?</h3>
            <p>
              Local Parakeet and Whisper models, plus Apple Speech on the Mac. Download a catalog model
              or add your own GGUF model in Settings.
            </p>
            <h3>Does a bot join my meetings?</h3>
            <p>
              No. Hex records your microphone and your Mac&apos;s system audio itself, so there is no
              participant to invite and no bot in the call. The transcript and notes are written
              locally.
            </p>
            <h3>Can my coding agent read my meetings?</h3>
            <p>
              Yes, through <code>hex mcp</code>, a read-only MCP server over stdio. It serves
              transcripts, search, chapters, and stored notes to OpenCode, Claude Code, Claude Desktop,
              Cursor, and other MCP clients.
            </p>
            <h3>Where does my data go?</h3>
            <p>
              Nowhere. Audio and text stay on your machine. Hex keeps no dictation audio unless you
              turn on Keep recordings, and meeting transcripts stay in a folder you can delete.
            </p>
          </section>
        </div>
      </section>

      <footer className="site-footer">
        <nav className="site-footer__links" aria-label="More about Hex">
          <a href={`${import.meta.env.BASE_URL}meetings/`}>Meetings</a>
          <a href={`${import.meta.env.BASE_URL}mcp`}>MCP</a>
          <a href={`${import.meta.env.BASE_URL}changelog`}>Changelog</a>
        </nav>
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
