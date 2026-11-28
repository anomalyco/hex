// What the comparison pages (src/compare.ts) and the Linux page (src/linux.ts)
// share: their styles, the visible questions with their FAQPage twin, and the
// app's structured data. Every word is in the HTML a crawler fetches; the only
// script on these pages is the copy buttons' (src/mcp.ts).
import { withBase } from "./base"
import { escape } from "./labs"

export const MONO = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"

// One visible question and its answer. The answer may carry inline links and
// code; the structured copy is the same words without the markup.
export type Question = { question: string; answer: string }

export const STYLE = `
.intent { --line: #1d1e21; --edge: rgba(242, 242, 239, 0.1); }
.intent a { text-decoration-color: #4a4b50; text-underline-offset: 3px; }
.intent a:hover { text-decoration-color: currentColor; }
.intent a:focus-visible, .intent .copy:focus-visible { outline: 2px solid #a9b9ff; outline-offset: 3px; }
.intent code { font-size: 13px; }
.intent .lede { margin: 18px 0 0; max-width: 32em; color: #b4b5ba; font-size: 20px; line-height: 1.45; text-wrap: pretty; }
.intent h2 { margin: 64px 0 0; font-size: 20px; font-weight: 600; letter-spacing: -0.015em; line-height: 1.2; }
.intent .sub { margin: 8px 0 0; max-width: 38em; color: #8a8b90; font-size: 15px; text-wrap: pretty; }
.intent p.text { margin: 14px 0 0; max-width: 38em; color: #b4b5ba; text-wrap: pretty; }
.intent .note { margin: 16px 0 0; max-width: 40em; color: #6f7075; font-size: 14px; text-wrap: pretty; }
.intent .note code { font-size: 12px; }
.compare { width: 100%; margin: 24px 0 0; border-bottom: 1px solid var(--line); border-collapse: collapse; table-layout: fixed; }
.compare col.compare__label { width: 9.5rem; }
.compare th, .compare td { padding: 13px 18px 13px 0; border-top: 1px solid var(--line); font-size: 15px; line-height: 1.5; text-align: left; vertical-align: top; text-wrap: pretty; }
.compare td:last-child { padding-right: 0; }
.compare thead th { padding-top: 0; border-top: 0; color: #8a8b90; font-size: 13px; font-weight: 500; letter-spacing: 0.01em; }
.compare thead th:nth-child(2) { color: #f2f2ef; }
.compare tbody th { color: #f2f2ef; font-weight: 500; }
.compare td { color: #a3a4a9; }
.compare td.hex { color: #d6d7db; }
.compare code { white-space: normal; overflow-wrap: anywhere; }
.points { margin: 18px 0 0; padding: 0; list-style: none; border-bottom: 1px solid var(--line); }
.points li { padding: 13px 0; border-top: 1px solid var(--line); color: #b4b5ba; font-size: 15px; text-wrap: pretty; }
.points li strong { color: #f2f2ef; font-weight: 500; }
.rows { margin: 20px 0 0; border-bottom: 1px solid var(--line); }
.row { display: grid; grid-template-columns: 8.5rem minmax(0, 1fr); align-items: baseline; gap: 0 16px; padding: 13px 0; border-top: 1px solid var(--line); color: #b4b5ba; font-size: 15px; }
.row--copy { grid-template-columns: 8.5rem minmax(0, 1fr) 36px; align-items: center; }
.row__label { color: #f2f2ef; font-weight: 500; }
.row__value { display: grid; min-width: 0; gap: 4px; text-wrap: pretty; }
.row__value small { color: #8a8b90; font-size: 14px; }
.cmd { display: block; overflow-x: auto; padding: 0; border: 0; background: none; color: #d6d7db; font: 13px/1.6 ${MONO}; white-space: pre; scrollbar-width: none; }
.copy { display: inline-grid; width: 36px; height: 36px; place-items: center; padding: 0; border: 1px solid rgba(242, 242, 239, 0.14); border-radius: 9px; color: #f2f2ef; background: transparent; cursor: pointer; transition: background-color 160ms ease, border-color 160ms ease; }
.copy[hidden] { display: none; }
.copy:hover { border-color: rgba(242, 242, 239, 0.3); background: rgba(242, 242, 239, 0.06); }
.copy svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.copy svg + svg, .copy[data-copied] svg:first-child { display: none; }
.copy[data-copied] svg + svg { display: block; }
.block { margin: 20px 0 0; overflow-x: auto; padding: 16px 18px; border: 1px solid var(--edge); border-radius: 13px; background: rgba(242, 242, 239, 0.025); color: #d6d7db; font: 13px/1.7 ${MONO}; }
.block code { padding: 0; border: 0; background: none; font: inherit; white-space: pre; }
.block .dim { color: #6f7075; }
.questions h3 { margin: 26px 0 6px; font-size: 16px; font-weight: 600; letter-spacing: -0.01em; }
.questions p { margin: 0; max-width: 38em; color: #b4b5ba; text-wrap: pretty; }
.intent .about { margin-top: 64px; font-size: 14px; }
@media (max-width: 640px) {
  .compare, .compare tbody, .compare tr, .compare th, .compare td { display: block; width: auto; }
  .compare colgroup, .compare thead { display: none; }
  .compare tr { padding: 13px 0; border-top: 1px solid var(--line); }
  .compare tbody th, .compare td { padding: 0; border: 0; }
  .compare tbody th { margin: 0 0 6px; }
  .compare td { display: grid; grid-template-columns: 6.5rem minmax(0, 1fr); gap: 12px; padding: 2px 0; }
  .compare td::before { content: attr(data-label); color: #6f7075; font-size: 13px; line-height: 1.75; }
  .row, .row--copy { grid-template-columns: minmax(0, 1fr) auto; gap: 6px 16px; }
  .row__label { grid-column: 1 / -1; }
}
`

const plain = (html: string) =>
  html
    .replace(/<[^>]+>/g, "")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&")

export function questions(list: Question[], heading = "Questions"): string {
  const items = list.map((entry) => `<h3>${escape(entry.question)}</h3>\n<p>${entry.answer}</p>`).join("\n")
  return `<section class="questions">\n<h2>${escape(heading)}</h2>\n${items}\n</section>`
}

// The FAQPage carries the visible questions word for word (intent.test.ts).
export function faqPage(url: string, list: Question[]) {
  return {
    "@type": "FAQPage",
    "@id": `${url}#faq`,
    mainEntity: list.map((entry) => ({
      "@type": "Question",
      name: entry.question,
      acceptedAnswer: { "@type": "Answer", text: plain(entry.answer) },
    })),
  }
}

export function breadcrumbs(origin: string, url: string, name: string) {
  return {
    "@type": "BreadcrumbList",
    itemListElement: [
      { "@type": "ListItem", position: 1, name: "Hex", item: `${origin}/` },
      { "@type": "ListItem", position: 2, name, item: url },
    ],
  }
}

// Hex itself, as the home page's graph describes it (index.html).
export function hexApp(origin: string, extra: Record<string, unknown> = {}) {
  return {
    "@type": "SoftwareApplication",
    "@id": `${origin}/#app`,
    name: "Hex",
    url: `${origin}/`,
    applicationCategory: "UtilitiesApplication",
    applicationSubCategory: "Voice dictation",
    operatingSystem: "macOS 15, Windows 10, Windows 11, Linux, iOS 18, Android 12",
    softwareVersion: "__HEX_VERSION__",
    isAccessibleForFree: true,
    license: "https://github.com/anomalyco/hex/blob/dev/LICENSE",
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
    sameAs: ["https://github.com/anomalyco/hex"],
    ...extra,
  }
}

export const GET_HEX = `<div class="actions"><a class="button button--primary" href="${withBase("/")}">Get Hex</a><a class="button button--secondary" href="https://github.com/anomalyco/hex">Source on GitHub</a></div>`

// The other pages, linked at the foot of each.
export function about(current: string): string {
  const links: [href: string, label: string][] = [
    [withBase("/"), "Hex for Mac, Windows, and Linux"],
    [withBase("/meetings/"), "Meetings"],
    [withBase("/mcp"), "MCP"],
    [withBase("/linux"), "Linux"],
    [withBase("/vs-wispr-flow"), "Hex vs Wispr Flow"],
    [withBase("/vs-superwhisper"), "Hex vs Superwhisper"],
  ]
  return `<section class="about">\n<p>${links
    .filter(([href]) => href !== current)
    .map(([href, label]) => `<a href="${href}">${label}</a>`)
    .join(" · ")}</p>\n</section>`
}
