// The comparison pages: /hex/vs-wispr-flow and /hex/vs-superwhisper. Hex's
// side is checked against docs/ and llms-full.txt; the other app's side
// against its own public pages, which each page links under the table, read on
// CHECKED. Keep to what those pages say, and update the date with the facts.
// scripts/pages.ts writes the pages and their cards at build.
import { withBase } from "./base"
import { about, breadcrumbs, faqPage, GET_HEX, hexApp, questions, STYLE, type Question } from "./intent"
import { escape, MARK, shell } from "./labs"

export const CHECKED = "November 28, 2026"
const CHECKED_ISO = "2026-11-28"

export type Comparison = {
  id: string
  // The other app's name.
  name: string
  path: string
  title: string
  description: string
  lede: string
  // Label, Hex, the other app. Cells may carry inline code.
  rows: [label: string, hex: string, other: string][]
  // Only in Hex, said without claiming the other app lacks it.
  also?: string
  // When the other app may be the better choice.
  fits: string[]
  questions: Question[]
  sources: [label: string, url: string][]
  card: { title: string; line: string }
  // The header art, above the title. Set it and the page draws it; nothing else changes.
  art?: { src: string; alt: string; width: number; height: number }
}

const COMPUTERS_HEX = "Mac with Apple silicon (macOS 15+)<br>Windows 10 and 11<br>Linux"
const PHONES_HEX = "iPhone, and Android through Hex Preview"
const MEETINGS_HEX = "Mac, Windows, and Linux. Transcribed and saved on your computer"

export const WISPR_FLOW: Comparison = {
  id: "wispr-flow",
  name: "Wispr Flow",
  path: withBase("/vs-wispr-flow"),
  title: "Hex vs Wispr Flow: a free, local, open-source alternative",
  description:
    "Hex and Wispr Flow both type what you say into any app. Hex transcribes on your computer, works offline, is free and open source, and runs on Linux. Wispr Flow transcribes in the cloud.",
  lede: "Both type what you say into the app you're in. Hex runs the speech model on your computer, so it works offline and your audio stays put. Wispr Flow sends your audio to its servers.",
  rows: [
    ["Speech to text", "On your computer", "On Wispr's servers"],
    ["Offline", "Yes, after one model download", "No. Transcription needs an internet connection"],
    ["Price", "Free, with no word limit", "Free plan with limited dictation. Pro is $15 a month or $144 a year"],
    ["Account", "None", "Required"],
    ["Source code", "Open source (MIT)", "Closed"],
    ["Computers", COMPUTERS_HEX, "Mac with Apple silicon or Intel (macOS 12+), Windows 10 and 11 on x64"],
    ["Phones", PHONES_HEX, "iPhone and Android"],
    ["Linux", "X11 and Wayland, on x86_64 and ARM64", "Not supported"],
    ["Meetings without a bot", MEETINGS_HEX, "Notetaker on Mac, Windows, and iPhone. Recordings upload to Wispr"],
    ["MCP", "<code>hex mcp</code> runs on your computer. No account or API key", "Remote server for Notetaker meetings, through your Wispr account"],
    ["Speech models", "Parakeet, Whisper, Apple Speech on the Mac, or your own GGUF file", "Wispr's cloud models"],
    ["Text rules", "Word replacements, plus JavaScript transforms in <code>hex.config.ts</code>", "Dictionary and snippets"],
  ],
  fits: [
    "You're on an Intel Mac or a macOS older than 15. Hex needs Apple silicon and macOS 15.",
    "You want meeting notes on your iPhone. Hex Meetings is desktop only.",
    "You'd rather not download a speech model or spend battery running one. Wispr Flow does that on its servers.",
    "Your team wants a shared dictionary, central billing, and admin controls. Wispr Flow's team plans have them.",
  ],
  questions: [
    {
      question: "Is Hex a free alternative to Wispr Flow?",
      answer:
        "Yes. Hex is free and open source under the MIT license, with no account, subscription, or word limit. Hold a shortcut, speak, and the text appears where your cursor is.",
    },
    {
      question: "Does Hex work offline?",
      answer:
        "Yes. After a one-time model download, dictation and meeting transcription run on your computer with no network. Wispr Flow's help center says it needs an internet connection to transcribe.",
    },
    {
      question: "Does Hex run on Linux?",
      answer: `Yes, on x86_64 and ARM64. It works on X11 and Wayland, including GNOME, KDE Plasma, Hyprland, and Sway. See <a href="${withBase("/linux")}">Hex for Linux</a>. Wispr Flow doesn't support Linux.`,
    },
    {
      question: "Does Hex record meetings without a bot?",
      answer: `Yes. <a href="${withBase("/meetings/")}">Hex Meetings</a> records your microphone and system audio and transcribes them on your computer, so nothing joins the call. Turn it on in the Meetings tab.`,
    },
  ],
  sources: [
    ["Pricing", "https://wisprflow.ai/pricing"],
    ["What is Wispr Flow", "https://docs.wisprflow.ai/articles/2772472373-what-is-flow"],
    ["Supported devices", "https://docs.wisprflow.ai/articles/1036674442-supported-devices-and-system-requirements"],
    ["MCP", "https://docs.wisprflow.ai/articles/9551372685-connect-an-mcp-client-to-wispr-flow"],
  ],
  card: { title: "Hex vs Wispr Flow", line: "Dictation that runs on your computer. Free, open source, offline, and on Linux." },
}

export const SUPERWHISPER: Comparison = {
  id: "superwhisper",
  name: "Superwhisper",
  path: withBase("/vs-superwhisper"),
  title: "Hex vs Superwhisper: a free, open-source alternative",
  description:
    "Hex and Superwhisper both run speech models on your computer. Hex is free and open source, with Parakeet included, meetings on Mac, Windows, and Linux, and X11 support on Linux.",
  lede: "Both run a speech model on your computer and type into any app. Hex is free and open source, with nothing behind a paywall. Superwhisper is free with local Whisper models, and Pro adds Parakeet, cloud models, and AI modes.",
  rows: [
    ["Source code", "Open source (MIT)", "Closed"],
    ["Price", "Free, everything included", "Free with local Whisper models. Pro is $8.49 a month, $84.99 a year, or $249.99 once"],
    ["Speech to text", "Always on your computer", "Local models, plus cloud models in Pro"],
    ["Parakeet", "Free", "Pro"],
    ["Computers", COMPUTERS_HEX, "Mac with Apple silicon or Intel (macOS 14+), Windows 10 and 11, Linux"],
    ["Linux desktops", "X11 and Wayland: GNOME, KDE Plasma, Hyprland, Sway, and other wlroots compositors", "Wayland only: Hyprland, KDE Plasma, or GNOME, with systemd"],
    ["Phones", PHONES_HEX, "iPhone, and Android with cloud transcription"],
    ["Meetings without a bot", MEETINGS_HEX, "Mac only"],
    ["CLI and MCP", "Mac, Windows, and Linux. <code>hex mcp</code> reads your meetings", "Mac only, for dictations and meetings"],
  ],
  also: `Hex also runs a compatible GGUF speech model you add yourself, rewrites dictated text with your own JavaScript in <a href="${withBase("/docs/transforms")}"><code>hex.config.ts</code></a>, and sends a screenshot or window to OpenCode with Annotate.`,
  fits: [
    "You're on an Intel Mac or macOS 14. Hex needs Apple silicon and macOS 15.",
    "You want cloud models, or AI modes that rewrite and translate, with priority support in one paid app.",
    "You want each person in a meeting labeled. Hex labels your microphone You and everyone else Them.",
  ],
  questions: [
    {
      question: "Is Hex a free alternative to Superwhisper?",
      answer:
        "Yes. Hex is free and open source under the MIT license. Parakeet and Whisper models, meetings, the command line, and hex mcp are all included, and there's no paid plan.",
    },
    {
      question: "Does Hex run on Linux and Windows?",
      answer: `Yes. On Linux, Hex works on X11 and Wayland, on x86_64 and ARM64 (see <a href="${withBase("/linux")}">Hex for Linux</a>). On Windows, it runs on 10 and 11, x64 and ARM64.`,
    },
    {
      question: "Does Hex record meetings without a bot?",
      answer: `Yes. <a href="${withBase("/meetings/")}">Hex Meetings</a> records your microphone and system audio and transcribes them on your computer. It runs on the Mac, Windows, and Linux.`,
    },
    {
      question: "Can I use Parakeet for free?",
      answer: "Yes. Parakeet is in Hex's model catalog at no cost, along with Whisper, Apple Speech on the Mac, and any compatible GGUF file you add.",
    },
  ],
  sources: [
    ["Plans and pricing", "https://superwhisper.com/docs/billing/plans"],
    ["Product facts", "https://superwhisper.com/llms-full.txt"],
    ["Linux", "https://superwhisper.com/linux"],
    ["Meetings", "https://superwhisper.com/docs/meetings/overview"],
    ["CLI and MCP", "https://superwhisper.com/docs/get-started/cli"],
  ],
  card: { title: "Hex vs Superwhisper", line: "Free and open source, with Parakeet, meetings, and hex mcp on Mac, Windows, and Linux." },
}

export const COMPARISONS = [WISPR_FLOW, SUPERWHISPER]

export const cardPath = (comparison: Comparison) => `${comparison.path}.png`

export function renderComparison(comparison: Comparison, origin: string): string {
  const name = escape(comparison.name)
  const url = `${origin}${comparison.path.slice("/hex".length)}`
  const rows = comparison.rows
    .map(
      ([label, hex, other]) =>
        `<tr><th scope="row">${escape(label)}</th><td class="hex" data-label="Hex"><span>${hex}</span></td><td data-label="${name}"><span>${other}</span></td></tr>`,
    )
    .join("\n")
  const fits = comparison.fits.map((fit) => `<li>${fit}</li>`).join("\n")
  const sources = comparison.sources.map(([label, href]) => `<a href="${href}">${escape(label)}</a>`).join(", ")
  const also = comparison.also ? `\n<h2>Also in Hex</h2>\n<p class="text">${comparison.also}</p>` : ""

  const art = comparison.art
    ? `<figure class="vs__art"><img src="${escape(comparison.art.src)}" alt="${escape(comparison.art.alt)}" width="${comparison.art.width}" height="${comparison.art.height}" fetchpriority="high"></figure>\n`
    : ""

  const body = `<div class="intent">
<div class="vs">
${art}<p class="eyebrow">Compare</p>
<h1 class="vs__title">Hex <span class="vs__vs">vs</span> ${name}</h1>
<p class="lede">${comparison.lede}</p>
${GET_HEX}
</div>

<h2>Side by side</h2>
<table class="compare">
<colgroup><col class="compare__label"><col><col></colgroup>
<thead><tr><th scope="col"><span hidden>Feature</span></th><th scope="col" class="hex"><span class="compare__brand">${MARK}Hex</span></th><th scope="col">${name}</th></tr></thead>
<tbody>
${rows}
</tbody>
</table>
<p class="note">${name}'s column comes from its own pages, read on ${CHECKED}: ${sources}. Check them for current prices and features.</p>
${also}
<h2>When ${name} may fit better</h2>
<ul class="points">
${fits}
</ul>

${questions(comparison.questions)}

${about(comparison.path)}
</div>`

  return shell(
    origin,
    {
      title: comparison.title,
      description: comparison.description,
      url: comparison.path,
      image: cardPath(comparison),
      imageAlt: `${comparison.card.title}. ${comparison.card.line}`,
      jsonLd: {
        "@context": "https://schema.org",
        "@graph": [
          { "@type": "WebPage", "@id": url, url, name: comparison.title, description: comparison.description, about: { "@id": `${origin}/#app` }, dateModified: CHECKED_ISO },
          hexApp(origin),
          faqPage(url, comparison.questions),
          breadcrumbs(origin, url, `Hex vs ${comparison.name}`),
        ],
      },
    },
    body,
    { crumb: { href: comparison.path, label: `vs ${comparison.name}` }, style: STYLE },
  )
}
