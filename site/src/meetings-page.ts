// The crawlable half of /hex/meetings/. The showcase in public/meetings/ is
// imported as it ships (scripts/meetings/import.ts) and is mostly a show, so
// the build (scripts/meetings/build.ts) adds this below it: how Hex records a
// meeting, where it runs, the CLI and MCP, and the questions, with a title,
// description, and structured data that say the same. Checked against
// docs/features/meetings.md: Windows and Linux are beta, and titles and
// summaries come from Apple Intelligence on a Mac and from OpenCode, only when
// turned on, elsewhere.
import { withBase } from "./base"
import { faqPage, hexApp, MONO, type Question } from "./intent"
import { escape } from "./labs"

export const MEETINGS_TITLE = "Hex Meetings: local meeting transcription with no bot"
export const MEETINGS_DESCRIPTION =
  "Hex records your microphone and system audio itself, so no bot joins the call, and transcribes the meeting on your computer. Free and open source, on Mac, with Windows and Linux in beta. Your agent reads it with hex mcp."

const HOW: [label: string, value: string][] = [
  ["No bot", "Hex records the call from your computer. Nothing joins the meeting, and there's no one to admit from the waiting room."],
  ["Two tracks", "Your microphone is You and the system audio is Them, recorded side by side. Everyone on the far end shares Them."],
  ["Local transcript", "A speech model on your computer writes the transcript as the call goes, with no network."],
  ["Notes", "On a Mac with Apple Intelligence, Hex writes a title and a short summary on the Mac. Elsewhere they're off unless you send them to your own OpenCode model."],
  ["Export", "Each meeting is saved as Markdown in Documents › Hex Meetings, or a folder you choose. Copy it as Markdown, SRT, or VTT."],
  ["Retention", "Meetings stay 7 days by default, or 24 hours, 30 days, or forever. Expired ones go to the Trash. Exported Markdown is never deleted."],
]

const WHERE: [os: string, audio: string, status: string][] = [
  ["macOS 15, Apple silicon", "A Core Audio process tap, or ScreenCaptureKit with Screen Recording", "Stable"],
  ["Windows 10 and 11", "WASAPI loopback of the outputs the call plays on", "Beta"],
  ["Linux", "PipeWire, recording each output's monitor", "Beta"],
]

export const MEETINGS_QUESTIONS: Question[] = [
  { question: "Does a bot join my call?", answer: "No. Hex records your microphone and system audio on your computer, so nothing joins the meeting." },
  { question: "Does meeting transcription work offline?", answer: "Yes. A local speech model transcribes the meeting on your computer, with no network once the model has downloaded." },
  {
    question: "Which call apps does it work with?",
    answer: "Any app that plays the call through your computer, in a browser or on the desktop, because Hex records the system audio.",
  },
  { question: "Is Hex Meetings free?", answer: "Yes. Meetings is part of Hex, which is free and open source under the MIT license, with no account." },
  {
    question: "Can my coding agent read my meetings?",
    answer: `Yes. <a href="${withBase("/mcp")}">hex mcp</a> is a local, read-only MCP server for OpenCode, Claude Code, Claude Desktop, and Cursor.`,
  },
]

// Scoped to .mx, on the showcase's black and Inter, at the home page's sheet width.
const STYLE = `<style>
.mx { max-width: 42.5rem; margin: 0 auto; padding: clamp(3rem, 8vw, 5rem) 1.25rem 1rem; color: #f2f2ef; font-size: 16px; line-height: 1.55; }
.mx h2 { margin: 3.5rem 0 0; font-size: 1.25rem; font-weight: 620; letter-spacing: -0.015em; line-height: 1.2; }
.mx h2:first-child { margin-top: 0; }
.mx h3 { margin: 1.6rem 0 0.35rem; font-size: 1rem; font-weight: 600; letter-spacing: -0.01em; }
.mx p { margin: 0; color: #b4b5ba; text-wrap: pretty; }
.mx .mx__sub { margin-top: 0.5rem; color: #8a8b90; font-size: 0.9375rem; }
.mx a { color: inherit; text-decoration: underline; text-decoration-color: #4a4b50; text-underline-offset: 3px; }
.mx a:hover { text-decoration-color: currentColor; }
.mx a:focus-visible { outline: 2px solid #a9b9ff; outline-offset: 3px; }
.mx code { color: #d6d7db; font: 0.8125rem ${MONO}; }
.mx dl { margin: 1.25rem 0 0; border-bottom: 1px solid #1d1e21; }
.mx dl div { display: grid; grid-template-columns: 8.5rem minmax(0, 1fr); gap: 0 1rem; padding: 0.8rem 0; border-top: 1px solid #1d1e21; font-size: 0.9375rem; }
.mx dt { color: #f2f2ef; font-weight: 500; }
.mx dd { margin: 0; color: #b4b5ba; text-wrap: pretty; }
.mx table { width: 100%; margin: 1.25rem 0 0; border-bottom: 1px solid #1d1e21; border-collapse: collapse; }
.mx th, .mx td { padding: 0.8rem 1rem 0.8rem 0; border-top: 1px solid #1d1e21; font-size: 0.9375rem; text-align: left; vertical-align: top; }
.mx thead th { padding-top: 0; border-top: 0; color: #8a8b90; font-size: 0.8125rem; font-weight: 500; }
.mx tbody th { width: 11rem; color: #f2f2ef; font-weight: 500; }
.mx td { color: #b4b5ba; }
.mx td:last-child { padding-right: 0; white-space: nowrap; }
.mx .pill { display: inline-block; padding: 0 0.45rem; border: 1px solid rgba(242, 242, 239, 0.16); border-radius: 999px; color: #a3a4a9; font-size: 0.6875rem; font-weight: 500; letter-spacing: 0.03em; line-height: 1.1rem; }
.mx tr:first-child .pill { border-color: rgba(242, 242, 239, 0.3); color: #e6e7e9; }
.mx pre { margin: 1.25rem 0 0; overflow-x: auto; padding: 1rem 1.125rem; border: 1px solid rgba(242, 242, 239, 0.1); border-radius: 0.8rem; background: rgba(242, 242, 239, 0.025); color: #d6d7db; font: 0.8125rem/1.7 ${MONO}; }
.mx pre code { font: inherit; }
.mx pre span { color: #6f7075; }
.mx__links { margin-top: 3.5rem !important; color: #8a8b90 !important; font-size: 0.875rem; }
@media (max-width: 36rem) {
  .mx dl div { grid-template-columns: minmax(0, 1fr); gap: 0.25rem; }
  .mx tbody th { width: auto; }
  .mx thead { display: none; }
  .mx tr { display: grid; padding: 0.8rem 0; border-top: 1px solid #1d1e21; }
  .mx th, .mx td { padding: 0; border: 0; }
  .mx td:last-child { white-space: normal; }
}
</style>`

export function meetingsDetails(): string {
  const how = HOW.map(([label, value]) => `<div><dt>${label}</dt><dd>${value}</dd></div>`).join("\n")
  const where = WHERE.map(([os, audio, status]) => `<tr><th scope="row">${os}</th><td>${audio}</td><td><span class="pill">${status}</span></td></tr>`).join("\n")
  const questions = MEETINGS_QUESTIONS.map((entry) => `<h3>${escape(entry.question)}</h3>\n<p>${entry.answer}</p>`).join("\n")
  return `${STYLE}
<section class="mx" aria-labelledby="mx-how">
<h2 id="mx-how">How Hex records a meeting</h2>
<p class="mx__sub">A meeting recorder with no bot, built into Hex. It's off until you turn it on in the Meetings tab.</p>
<dl>
${how}
</dl>

<h2>Where it runs</h2>
<table>
<thead><tr><th scope="col">System</th><th scope="col">System audio</th><th scope="col">Status</th></tr></thead>
<tbody>
${where}
</tbody>
</table>
<p class="mx__sub">Windows and Linux use the same recorder, transcript, and Meetings pane as the Mac, with no permissions to grant.</p>

<h2>Read it from a terminal or an agent</h2>
<pre><code>hex meetings list --since 7d          <span># newest first</span>
hex meetings show latest --md         <span># the transcript as Markdown</span>
hex meetings show latest --from 10:30 <span># from 10:30 into the call</span>
hex meetings search 'release schedule'
hex mcp                               <span># the same meetings, over MCP</span></code></pre>
<p class="mx__sub">These commands read files and never start Hex. <a href="${withBase("/mcp")}">Set up hex mcp</a> for OpenCode, Claude Code, Claude Desktop, or Cursor.</p>

<h2>Questions</h2>
${questions}

<p class="mx__links"><a href="${withBase("/")}">Get Hex</a> · <a href="${withBase("/linux")}">Hex for Linux</a> · <a href="${withBase("/vs-wispr-flow")}">Hex vs Wispr Flow</a> · <a href="${withBase("/vs-superwhisper")}">Hex vs Superwhisper</a> · <a href="https://github.com/anomalyco/hex/blob/dev/docs/transcript-cli.md">Transcript CLI docs</a></p>
</section>
`
}

export function meetingsJsonLd(origin: string): string {
  const url = `${origin}/meetings/`
  const graph = {
    "@context": "https://schema.org",
    "@graph": [
      hexApp(origin, {
        "@id": `${url}#app`,
        name: "Hex Meetings",
        url,
        description: MEETINGS_DESCRIPTION,
        applicationSubCategory: "Meeting transcription",
        operatingSystem: "macOS 15, Windows 10, Windows 11, Linux",
        isPartOf: { "@id": `${origin}/#app` },
      }),
      faqPage(url, MEETINGS_QUESTIONS),
    ],
  }
  return `<script type="application/ld+json">${JSON.stringify(graph)}</script>`
}

// The built page: the search title and description, the structured data, and
// the section above the showcase's footer.
export function finishMeetings(html: string, origin: string): string {
  const footer = '<footer class="site-footer">'
  for (const [what, found] of [
    ["<title>", /<title>[^<]*<\/title>/.test(html)],
    ['<meta name="description">', /<meta name="description" content="[^"]*">/.test(html)],
    [footer, html.includes(footer)],
  ] as const) {
    if (!found) throw new Error(`meetings/index.html has no ${what}`)
  }
  return html
    .replace(/<title>[^<]*<\/title>/, `<title>${escape(MEETINGS_TITLE)}</title>`)
    .replace(/<meta name="description" content="[^"]*">/, `<meta name="description" content="${escape(MEETINGS_DESCRIPTION)}">`)
    .replace("</head>", `${meetingsJsonLd(origin)}\n</head>`)
    .replace(footer, `${meetingsDetails()}${footer}`)
}
