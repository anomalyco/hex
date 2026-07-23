import * as m from "motion/react-m"
import ChatteringTeeth from "./ChatteringTeeth"

const DOWNLOAD_URL =
  "https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/releases/HEX-latest-arm64.dmg"

function HexMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 64 64" className="wordmark__icon">
      <path d="M32 4 57 18.5v27L32 60 7 45.5v-27Z" />
      <path d="M32 15 48 24.25v15.5L32 49l-16-9.25v-15.5Z" />
      <path className="wordmark__core" d="m32 24 7.5 4.25v8.5L32 41l-7.5-4.25v-8.5Z" />
    </svg>
  )
}

function AppleMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="download__apple">
      <path d="M16.68 12.9c.02 2.3 2.02 3.06 2.04 3.07-.02.06-.32 1.1-1.05 2.17-.63.92-1.29 1.84-2.32 1.86-1.01.02-1.34-.6-2.5-.6-1.15 0-1.51.58-2.47.62-.99.04-1.74-.99-2.38-1.91-1.3-1.88-2.3-5.32-.96-7.65a3.7 3.7 0 0 1 3.15-1.91c.98-.02 1.91.66 2.5.66.6 0 1.72-.82 2.9-.7.49.02 1.88.2 2.77 1.5-.07.04-1.65.96-1.68 2.89ZM14.81 7.27c.53-.64.89-1.53.79-2.41-.76.03-1.68.51-2.23 1.15-.49.57-.92 1.47-.8 2.34.85.07 1.71-.43 2.24-1.08Z" />
    </svg>
  )
}

function LinuxMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="download__linux">
      <path d="M5 5.5h14v13H5zM8 9l2.5 2.5L8 14M12.5 14H16" />
    </svg>
  )
}

export default function App() {
  const isLinux = /\bLinux\b/i.test(navigator.userAgent) && !/\bAndroid\b/i.test(navigator.userAgent)

  return (
    <m.main
      className="page"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.35, ease: "easeOut" }}
    >
      <header className="header">
        <a className="wordmark" href="/" aria-label="HEX home">
          <HexMark />
          <span>HEX</span>
        </a>
      </header>

      <section className="hero">
        <div className="hero__copy">
          <div className="hero__title">
            <h1>Speak. It appears.</h1>
            <ChatteringTeeth />
          </div>
          <p>
            Private, local dictation for {isLinux ? "Linux." : "Mac."} Hold Option, say what you mean,
            and HEX pastes it where you are.
          </p>
        </div>

        <div className="hero__action">
          {isLinux ? (
            <div className="download download--disabled" role="status">
              <LinuxMark />
              <span>Linux download coming soon</span>
              <span className="download__arrow" aria-hidden="true">&#8943;</span>
            </div>
          ) : (
            <m.a
              className="download"
              href={DOWNLOAD_URL}
              whileHover={{ y: -1 }}
              whileTap={{ scale: 0.985 }}
            >
              <AppleMark />
              <span>Download for Mac</span>
              <span className="download__arrow" aria-hidden="true">&#8595;</span>
            </m.a>
          )}
          <p className="requirements">
            {isLinux ? "x86_64  ·  Arch Linux beta" : "Apple silicon  ·  macOS 15+"}
          </p>
        </div>
      </section>
    </m.main>
  )
}
