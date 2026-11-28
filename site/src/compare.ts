// The comparison pages: /hex/vs-wispr-flow and /hex/vs-superwhisper. Hex's
// side is checked against docs/ and llms-full.txt; the other app's side
// against its own public pages, which each page links under "Sources", read on
// CHECKED. Keep to what those pages say, and update the date with the facts.
// scripts/pages.ts writes the pages and their cards at build.
import { withBase } from "./base"
import { about, breadcrumbs, faqPage, GET_HEX, hexApp, questions, STYLE, type Question } from "./intent"
import { escape, shell } from "./labs"

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
}

const COMPUTERS_HEX = "Mac (Apple silicon, macOS 15 or later), Windows 10 and 11 (beta), Linux (beta)"
const PHONES_HEX = "iPhone keyboard, and Android through Hex Preview"
const MEETINGS_HEX = "On the Mac, and on Windows and Linux in beta. Transcribed on your computer and saved in a local folder"
const MCP_HEX = "<code>hex mcp</code>, a read-only server on your computer, with no account, API key, or network"

export const WISPR_FLOW: Comparison = {
  id: "wispr-flow",
  name: "Wispr Flow",
  path: withBase("/vs-wispr-flow"),
  title: "Hex vs Wispr Flow: a free, local, open-source alternative",
  description:
    "Hex and Wispr Flow both type what you say into any app. Hex transcribes on your computer, works offline, is free and open source, and runs on Linux. Wispr Flow transcribes in the cloud.",
  lede: "Both type what you say into the app you're in. Hex runs the speech model on your computer, so it works offline and your audio stays there. Wispr Flow sends your audio to its servers to transcribe it.",
  rows: [
    ["Speech to text", "On your computer, with a local model", "On Wispr's servers"],
    ["Offline", "Yes, after a one-time model download", "No. Transcription needs an internet connection"],
    ["Price", "Free, with no word limit", "Free plan with limited dictation. Pro is $15 a month, or $144 a year"],
    ["Account", "None", "A Wispr account"],
    ["Source code", "Open source, MIT license", "Closed source"],
    ["Computers", COMPUTERS_HEX, "Mac (Apple silicon or Intel, macOS 12 or later), Windows 10 and 11 on x64"],
    ["Phones", PHONES_HEX, "iPhone and Android"],
    ["Linux", "X11 and Wayland, on x86_64 and ARM64", "Not supported"],
    ["Meetings, no bot", MEETINGS_HEX, "Notetaker, on Mac, Windows, and iPhone. Recordings upload to Wispr"],
    ["MCP for agents", MCP_HEX, "A remote, read-only server for Notetaker meetings, through your Wispr account"],
    ["Speech models", "Parakeet, Whisper, Apple Speech on the Mac, or your own GGUF file", "Wispr's cloud models"],
    ["Your own rules", "Word replacements, and JavaScript or TypeScript transforms in <code>hex.config.ts</code>", "Dictionary and snippets"],
  ],
  fits: [
    "You use an Intel Mac, or a macOS older than 15. Hex needs Apple silicon and macOS 15.",
    "You want meeting notes on your iPhone. Hex Meetings runs on desktop computers only.",
    "You'd rather not download a speech model or spend your computer's memory and battery on one. Wispr Flow does that work on its servers.",
    "Your team wants a shared dictionary, central billing, and admin controls, which Wispr Flow's team plans have.",
  ],
  questions: [
    {
      question: "Is Hex a free alternative to Wispr Flow?",
      answer:
        "Yes. Hex is free and open source under the MIT license, with no account, subscription, or word limit. It does the same core job: hold a shortcut, speak, and the text appears where your cursor is.",
    },
    {
      question: "Does Hex work offline?",
      answer:
        "Yes. After a one-time model download, dictation and meeting transcription run on your computer with no network. Wispr Flow's help center says its transcription needs an internet connection.",
    },
    {
      question: "Does Hex run on Linux?",
      answer: `Yes, in beta, on x86_64 and ARM64, with X11 and Wayland desktops such as GNOME, KDE Plasma, Hyprland, and Sway. See <a href="${withBase("/linux")}">Hex for Linux</a>. Wispr Flow lists Linux as unsupported.`,
    },
    {
      question: "Does Hex record meetings without a bot?",
      answer: `Yes. <a href="${withBase("/meetings/")}">Hex Meetings</a> records your microphone and system audio itself and transcribes them on your computer, so nothing joins the call. It's off until you turn it on in the Meetings tab.`,
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
  lede: "Both dictate into any app with a speech model on your computer. Hex is free and open source, with every feature included. Superwhisper is free with local Whisper models, and its Pro plan adds Parakeet, cloud models, and AI modes.",
  rows: [
    ["Source code", "Open source, MIT license", "Closed source"],
    ["Price", "Free, with every feature", "Free with local Whisper models. Pro is $8.49 a month, $84.99 a year, or $249.99 once"],
    ["Speech to text", "Always on your computer", "On your computer with local models. Pro adds cloud models"],
    ["Parakeet", "Free", "Pro"],
    ["Computers", COMPUTERS_HEX, "Mac (Apple silicon or Intel, macOS 14 or later), Windows 10 and 11, Linux"],
    ["Linux desktops", "X11 and Wayland: GNOME, KDE Plasma, Hyprland, Sway, and other wlroots compositors", "Wayland only: Hyprland, KDE Plasma, or GNOME, with systemd"],
    ["Phones", PHONES_HEX, "iPhone, and Android with cloud transcription"],
    ["Meetings, no bot", MEETINGS_HEX, "On the Mac"],
    ["CLI and MCP", "On Mac, Windows, and Linux. <code>hex mcp</code> reads your meetings", "On the Mac, for dictations and meetings"],
  ],
  also: `Hex can also run a GGUF speech model file you add yourself, rewrite dictated text with your own JavaScript in <a href="${withBase("/docs/transforms")}"><code>hex.config.ts</code></a>, and send a screenshot or window to OpenCode with Annotate.`,
  fits: [
    "You use an Intel Mac, or macOS 14. Hex needs Apple silicon and macOS 15.",
    "You want cloud models for the lowest latency, or AI modes that rewrite and translate, in one paid app with priority support.",
    "You want a label for each person in a meeting. Hex labels your microphone You and everyone on the call Them.",
    "Your Windows or Linux computer needs a finished release. Hex on Windows and Linux is in beta.",
  ],
  questions: [
    {
      question: "Is Hex a free alternative to Superwhisper?",
      answer:
        "Yes. Hex is free and open source under the MIT license. Parakeet and Whisper models, meetings, the command line, and hex mcp are all included, and there is no paid plan.",
    },
    {
      question: "Does Hex run on Linux and Windows?",
      answer: `Yes, both in beta. On Linux, Hex works on X11 and Wayland, on x86_64 and ARM64 (see <a href="${withBase("/linux")}">Hex for Linux</a>). On Windows, it runs on Windows 10 and 11, x64 and ARM64.`,
    },
    {
      question: "Does Hex record meetings without a bot?",
      answer: `Yes. <a href="${withBase("/meetings/")}">Hex Meetings</a> records your microphone and system audio itself and transcribes them on your computer. It runs on the Mac, and on Windows and Linux in beta.`,
    },
    {
      question: "Can I use Parakeet for free?",
      answer:
        "Yes. Parakeet models are in Hex's catalog at no cost, along with Whisper models, Apple Speech on the Mac, and any compatible GGUF file you add.",
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

  const body = `<div class="intent">
<p class="eyebrow">Hex · Compare</p>
<h1>Hex vs ${name}</h1>
<p class="lede">${comparison.lede}</p>
${GET_HEX}

<h2>Side by side</h2>
<table class="compare">
<colgroup><col class="compare__label"><col><col></colgroup>
<thead><tr><th scope="col"><span hidden>Feature</span></th><th scope="col">Hex</th><th scope="col">${name}</th></tr></thead>
<tbody>
${rows}
</tbody>
</table>
<p class="note">${name}'s side comes from its own pages, read on ${CHECKED}: ${sources}. Prices and features change, so check there for the latest.</p>
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
