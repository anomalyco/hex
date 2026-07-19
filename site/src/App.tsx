import * as m from "motion/react-m"

const DOWNLOAD_URL =
  "https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/releases/HEX-latest-arm64.dmg"

const reveal = {
  hidden: { opacity: 0, y: 12 },
  visible: { opacity: 1, y: 0 },
}

const voiceBars = [0.42, 0.7, 1, 0.58, 0.34]

function HexMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 28 28" className="wordmark__icon">
      <path d="M6 5v18M22 5v18M6 14h16" />
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

function VoiceOrb() {
  return (
    <m.div
      className="orb"
      variants={reveal}
      transition={{ duration: 0.7, ease: [0.16, 1, 0.3, 1] }}
    >
      <m.div
        className="orb__halo"
        animate={{ opacity: [0.28, 0.48, 0.28], scale: [0.98, 1.025, 0.98] }}
        transition={{ duration: 5.5, repeat: Infinity, ease: "easeInOut" }}
      />
      <div className="orb__shell">
        <div className="orb__light" />
        <div className="orb__core">
          <div className="wave" aria-hidden="true">
            {voiceBars.map((height, index) => (
              <m.span
                key={height}
                animate={{ scaleY: [height, Math.min(1, height + 0.28), height] }}
                transition={{
                  duration: 2.4,
                  delay: index * 0.13,
                  repeat: Infinity,
                  ease: "easeInOut",
                }}
              />
            ))}
          </div>
        </div>
      </div>
    </m.div>
  )
}

export default function App() {
  return (
    <main className="page">
      <m.header
        className="header"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.5 }}
      >
        <a className="wordmark" href="/" aria-label="HEX home">
          <HexMark />
          <span>HEX</span>
        </a>
        <span className="header__note">Local voice dictation</span>
      </m.header>

      <m.section
        className="hero"
        initial="hidden"
        animate="visible"
        transition={{ staggerChildren: 0.09, delayChildren: 0.08 }}
      >
        <VoiceOrb />

        <m.div className="hero__copy" variants={reveal} transition={{ duration: 0.55 }}>
          <h1>Speak. It appears.</h1>
          <p>
            Private, local dictation for Mac. Hold Option, say what you mean,
            and HEX pastes it where you are.
          </p>
        </m.div>

        <m.div className="hero__action" variants={reveal} transition={{ duration: 0.55 }}>
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
          <p className="requirements">Apple silicon&nbsp;&nbsp;&middot;&nbsp;&nbsp;macOS 15+</p>
        </m.div>
      </m.section>

      <m.footer
        className="footer"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.6, delay: 0.45 }}
      >
        <span>On-device transcription</span>
        <span className="footer__dot" aria-hidden="true" />
        <span>Signed automatic updates</span>
      </m.footer>
    </main>
  )
}
