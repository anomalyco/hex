// Below the hero: one spec sheet, the questions, and the footer, on the install
// stack's width, hairlines, and text spine. Every word is in the prerendered
// HTML, and the questions are native <details>, so nothing here needs scripts.
// index.html's FAQPage carries the same questions and answers (seo.test.ts).
import type { ReactNode } from "react"
import { withBase } from "./base"
import signature from "./footer-signature.svg"

const FACTS = [
  { term: "Open source", detail: "MIT licensed" },
  { term: "Offline", detail: "No network needed" },
  { term: "Private", detail: "Audio stays local" },
  { term: "Free", detail: "No account" },
]

const FEATURES: { id: string; title: string; line: string; tags: ReactNode[] }[] = [
  {
    id: "dictation",
    title: "Offline dictation",
    line: "Hold a key, speak, and release. A local model types what you said wherever your cursor is.",
    tags: ["Parakeet", "Whisper", "Apple Speech"],
  },
  {
    id: "meetings",
    title: "Meetings without bots",
    line: "Hex records your mic and system audio, and writes the transcript locally. No bot joins the call.",
    tags: ["Chapters", "Notes", <a href={withBase("/meetings/")}>Meetings ↗</a>],
  },
  {
    id: "annotate",
    title: "Annotate & transforms",
    line: "Send a screenshot, window, or iPhone capture to OpenCode. Rewrite dictated text with your own JavaScript.",
    tags: ["Screenshots", <a href={withBase("/docs/transforms")}>hex.config.ts ↗</a>],
  },
  {
    id: "cli",
    title: "CLI & MCP",
    line: "Drive Hex from a terminal, transcribe a file, or let your coding agent read your meetings.",
    tags: ["hex transcribe", <a href={withBase("/mcp")}>hex mcp ↗</a>],
  },
]

const QUESTIONS: { question: string; answer: ReactNode }[] = [
  {
    question: "Is Hex free?",
    answer: "Yes. Hex is free and open source under the MIT license, with no account, subscription, or word limit.",
  },
  {
    question: "Does Hex work offline?",
    answer: "Yes. After a one-time model download, dictation and meeting transcription run entirely on your machine.",
  },
  {
    question: "Which speech models does Hex run?",
    answer: "Local Parakeet and Whisper models, Apple Speech on the Mac, or your own GGUF model.",
  },
  {
    question: "Does a bot join my meetings?",
    answer: "No. Hex records your microphone and system audio itself, so there is no bot in the call.",
  },
  {
    question: "Can my coding agent read my meetings?",
    answer: (
      <>
        Yes. <a href={withBase("/mcp")}>hex mcp</a> is a read-only MCP server for OpenCode, Claude Code, Claude Desktop, and
        Cursor.
      </>
    ),
  },
  {
    question: "Where does my data go?",
    answer: "Nowhere. Audio, transcripts, and history stay on your machine.",
  },
]

function PlusIcon() {
  return (
    <svg aria-hidden="true" className="faq__mark" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round">
      <path d="M12 5.5v13M5.5 12h13" />
    </svg>
  )
}

export default function Below() {
  return (
    <>
      <section className="details">
        <div className="stack">
          <ul className="facts">
            {FACTS.map((fact) => (
              <li key={fact.term}>
                <strong>{fact.term}</strong>
                <span>{fact.detail}</span>
              </li>
            ))}
          </ul>
          <div className="features">
            {FEATURES.map((feature) => (
              <section key={feature.id} className="feature" aria-labelledby={feature.id}>
                <h2 id={feature.id}>{feature.title}</h2>
                <p>{feature.line}</p>
                <ul className="feature__tags">
                  {feature.tags.map((tag, index) => (
                    <li key={index}>{tag}</li>
                  ))}
                </ul>
              </section>
            ))}
          </div>
        </div>

        <section className="stack faq" aria-labelledby="faq">
          <h2 className="stack__title" id="faq">
            Questions
          </h2>
          {QUESTIONS.map((entry) => (
            <details key={entry.question}>
              <summary>
                <span>{entry.question}</span>
                <PlusIcon />
              </summary>
              <p>{entry.answer}</p>
            </details>
          ))}
        </section>
      </section>

      <footer className="site-footer">
        <nav className="site-footer__links" aria-label="More about Hex">
          <a href={withBase("/changelog")}>Changelog</a>
          <a href={withBase("/meetings/")}>Meetings</a>
          <a href={withBase("/mcp")}>MCP</a>
          <a href={withBase("/linux")}>Linux</a>
          <a href={withBase("/iphone")}>iOS</a>
          <a href={withBase("/android")}>Android</a>
          <a href="https://github.com/anomalyco/hex">GitHub</a>
          <a href={withBase("/llms.txt")}>llms.txt</a>
        </nav>
        <nav className="site-footer__links site-footer__links--compare" aria-label="Compare Hex">
          <a href={withBase("/vs-wispr-flow")}>Hex vs Wispr Flow</a>
          <a href={withBase("/vs-superwhisper")}>Hex vs Superwhisper</a>
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