// Hex Labs on the web: a page per lab at /hex/labs/<id>, the index at
// /hex/labs, and each lab's social card. labs.json comes from the Rust Labs
// registry (src/labs/web.rs in the app), so a lab's title, line, group, and
// OSes are never copied by hand; scripts/labs.ts writes the pages at build.
import { DEV_DOWNLOAD_PATH, withBase } from "./base"

export type Os = "macos" | "windows" | "linux"

export type Lab = {
  id: string
  title: string
  summary: string
  group: string
  groupTitle: string
  oses: Os[]
  added: string
}

// Where a lab ships. `stable` names the stable version that has it; a lab
// it doesn't have (or every lab, when the build couldn't tell) is in the Dev
// build only.
export type Channel = { stable: string } | { dev: true }

const OS_NAMES: Record<Os, string> = { macos: "Mac", windows: "Windows", linux: "Linux" }

// The dev build's download per OS: the Worker's /hex/download/dev/<target>
// redirects, and /hex/download/dev the Mac's.
const DEV_DOWNLOADS: Record<Os, string> = {
  macos: DEV_DOWNLOAD_PATH,
  windows: withBase("/download/dev/windows-x64-setup"),
  linux: withBase("/download/dev/linux-x64-deb"),
}

export function escape(text: string): string {
  return text.replace(/[&<>"']/g, (character) => `&#${character.charCodeAt(0)};`)
}

// A card's line as a sentence: the window's lines mostly leave off the stop.
export function sentence(text: string): string {
  return /[.!?]$/.test(text) ? text : `${text}.`
}

// The social card's line: the first sentence of a long one.
export function cardLine(summary: string): string {
  const line = sentence(summary)
  if (line.length <= 130) return line
  const end = line.search(/[.;](\s|$)/)
  return end > 0 ? `${line.slice(0, end)}.` : line
}

export function osList(oses: Os[]): string {
  const names = oses.map((os) => OS_NAMES[os])
  return names.length < 3 ? names.join(" and ") : `${names.slice(0, -1).join(", ")}, and ${names.at(-1)}`
}

// Newest first; labs added the same day keep the registry's order.
export function newestFirst(labs: Lab[]): Lab[] {
  return labs
    .map((lab, index) => ({ lab, index }))
    .sort((a, b) => b.lab.added.localeCompare(a.lab.added) || a.index - b.index)
    .map(({ lab }) => lab)
}

// The `hex labs …` names a stable version's Rust source knows: its
// `Feature::name` strings, which every lab id is.
export function inSource(lab: Lab, source: string): boolean {
  return source.includes(`"${lab.id}"`)
}

export const pagePath = (lab: Lab) => withBase(`/labs/${lab.id}`)
// The card's ?v= changes with what it shows, so link previews that cached an
// older card fetch the new one.
export function cardVersion(lab: Lab): string {
  let hash = 0x811c9dc5
  for (const character of `${lab.title}\n${lab.summary}\n${lab.oses.join(",")}`) {
    hash = Math.imul(hash ^ character.codePointAt(0)!, 0x01000193) >>> 0
  }
  return hash.toString(36)
}
export const cardPath = (lab: Lab) => withBase(`/labs/${lab.id}.png?v=${cardVersion(lab)}`)
export const INDEX_PATH = withBase("/labs")
export const INDEX_CARD_PATH = withBase("/labs.png")

const STYLE = `
:root { color-scheme: only dark; }
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; background: radial-gradient(circle at 50% -10%, rgba(255,255,255,0.06), transparent 520px), #070707; color: #f2f2ef; font: 16px/1.55 Inter, ui-sans-serif, system-ui, sans-serif; font-feature-settings: "cv02", "cv03", "cv04", "cv11", "ss01"; -webkit-font-smoothing: antialiased; }
a { color: inherit; }
header { max-width: 720px; margin: 0 auto; padding: 28px 24px 0; display: flex; align-items: center; gap: 10px; font-weight: 600; }
header a { text-decoration: none; display: inline-flex; align-items: center; gap: 10px; }
header svg { width: 24px; height: 24px; fill: none; stroke: currentColor; stroke-width: 3.5; stroke-linejoin: round; }
header .crumb { color: #8a8b90; font-weight: 500; }
main { max-width: 720px; margin: 0 auto; padding: 72px 24px 96px; }
.eyebrow { margin: 0 0 14px; color: #8a8b90; font-size: 14px; font-weight: 500; letter-spacing: 0.01em; }
h1 { margin: 0; font-size: clamp(40px, 8vw, 64px); font-weight: 600; letter-spacing: -0.035em; line-height: 1.04; text-wrap: balance; }
.summary { margin: 18px 0 0; color: #b4b5ba; font-size: 20px; line-height: 1.45; max-width: 32em; text-wrap: pretty; }
.actions { display: flex; flex-wrap: wrap; gap: 12px; margin: 40px 0 0; }
.button { display: inline-flex; align-items: center; justify-content: center; min-height: 48px; padding: 0 22px; border-radius: 999px; font-weight: 600; text-decoration: none; transition: background-color 120ms ease, border-color 120ms ease; }
.button--primary { background: #f2f2ef; color: #070707; }
.button--primary:hover { background: #ffffff; }
.button--secondary { border: 1px solid #2c2d31; background: #121214; }
.button--secondary:hover { border-color: #46474d; }
.note + .note { margin-top: 6px; }
.note { margin: 18px 0 0; color: #8a8b90; font-size: 15px; max-width: 36em; }
.note a { text-decoration-color: #4a4b50; text-underline-offset: 3px; }
.note a:hover { text-decoration-color: currentColor; }
code { white-space: nowrap; font: 14px ui-monospace, SFMono-Regular, Menlo, monospace; color: #d6d7db; background: #141416; border: 1px solid #222326; border-radius: 6px; padding: 2px 6px; }
.about { margin: 56px 0 0; padding: 24px 0 0; border-top: 1px solid #1d1e21; color: #8a8b90; font-size: 15px; }
.about p { margin: 0 0 8px; }
ol { list-style: none; margin: 40px 0 0; padding: 0; }
li { border-top: 1px solid #1d1e21; }
li a { display: grid; grid-template-columns: 1fr auto; gap: 4px 24px; padding: 18px 0; text-decoration: none; }
li a:hover .title { text-decoration: underline; text-decoration-color: #4a4b50; text-underline-offset: 4px; }
.title { font-size: 18px; font-weight: 600; letter-spacing: -0.01em; }
.line { grid-column: 1; color: #a3a4a9; font-size: 15px; }
.meta { grid-row: 1 / span 2; grid-column: 2; align-self: center; color: #6f7075; font-size: 13px; text-align: right; white-space: nowrap; }
.index-intro { margin: 18px 0 0; color: #b4b5ba; font-size: 18px; max-width: 34em; }
`

const MARK = `<svg aria-hidden="true" viewBox="0 0 64 64"><path d="M32 4 57 18.5v27L32 60 7 45.5v-27Z"/><path d="M32 15 48 24.25v15.5L32 49l-16-9.25v-15.5Z"/></svg>`

type Meta = { title: string; description: string; url: string; image: string; imageAlt: string }

function head(origin: string, meta: Meta): string {
  const url = `${origin}${meta.url.slice("/hex".length)}`
  const image = `${origin}${meta.image.slice("/hex".length)}`
  const tags = [
    `<meta charset="utf-8">`,
    `<meta name="viewport" content="width=device-width, initial-scale=1">`,
    `<meta name="theme-color" content="#070707">`,
    `<title>${escape(meta.title)}</title>`,
    `<meta name="description" content="${escape(meta.description)}">`,
    `<link rel="canonical" href="${escape(url)}">`,
    `<meta property="og:type" content="website">`,
    `<meta property="og:site_name" content="Hex">`,
    `<meta property="og:locale" content="en_US">`,
    `<meta property="og:title" content="${escape(meta.title)}">`,
    `<meta property="og:description" content="${escape(meta.description)}">`,
    `<meta property="og:url" content="${escape(url)}">`,
    `<meta property="og:image" content="${escape(image)}">`,
    `<meta property="og:image:type" content="image/png">`,
    `<meta property="og:image:width" content="1200">`,
    `<meta property="og:image:height" content="630">`,
    `<meta property="og:image:alt" content="${escape(meta.imageAlt)}">`,
    `<meta name="twitter:card" content="summary_large_image">`,
    `<meta name="twitter:title" content="${escape(meta.title)}">`,
    `<meta name="twitter:description" content="${escape(meta.description)}">`,
    `<meta name="twitter:image" content="${escape(image)}">`,
    `<meta name="twitter:image:alt" content="${escape(meta.imageAlt)}">`,
    `<link rel="icon" href="${withBase("/favicon.svg")}" type="image/svg+xml">`,
    `<style>${STYLE}</style>`,
  ]
  return tags.join("\n")
}

function shell(origin: string, meta: Meta, body: string): string {
  return `<!doctype html>
<html lang="en">
<head>
${head(origin, meta)}
</head>
<body>
<header><a href="${withBase("/")}">${MARK}<span>Hex</span></a><span class="crumb">/</span><a class="crumb" href="${INDEX_PATH}">Labs</a></header>
<main>
${body}
</main>
</body>
</html>
`
}

function getHex(lab: Lab, channel: Channel): { href: string; label: string; where: string } {
  if ("stable" in channel) {
    return { href: withBase("/"), label: "Get Hex", where: `In Hex ${escape(channel.stable)} and later.` }
  }
  const [first, ...rest] = lab.oses
  const others = rest.map((os) => `<a href="${DEV_DOWNLOADS[os]}">${OS_NAMES[os]}</a>`)
  return {
    href: DEV_DOWNLOADS[first],
    label: `Get Hex Dev for ${OS_NAMES[first]}`,
    where: `In the Dev build, not yet in a stable release.${others.length ? ` Dev build for ${others.join(" or ")}.` : ""}`,
  }
}

// The deep link opens Settings › Labs on the lab's card. Only the Mac app
// registers hex://, so labs Mac doesn't have offer the command alone.
export function renderLab(lab: Lab, channel: Channel, origin: string): string {
  const description = `${sentence(lab.summary)} A Hex Labs feature for ${osList(lab.oses)}.`
  const get = getHex(lab, channel)
  const mac = lab.oses.includes("macos")
  const open = mac ? `<a class="button button--primary" href="hex://labs/${escape(lab.id)}">Open in Hex</a>` : ""
  const getClass = mac ? "button--secondary" : "button--primary"
  const first = mac ? "No Hex yet? Get it first, then come back and press Open in Hex." : `Turn it on in Settings › Labs.`
  const body = `<p class="eyebrow">Hex Labs · ${escape(lab.groupTitle)} · ${escape(osList(lab.oses))}</p>
<h1>${escape(lab.title)}</h1>
<p class="summary">${escape(sentence(lab.summary))}</p>
<div class="actions">${open}<a class="button ${getClass}" href="${get.href}">${escape(get.label)}</a></div>
<p class="note">${get.where}</p>
<p class="note">${first} Or run <code>hex labs on ${escape(lab.id)}</code>.</p>
<section class="about">
<p>Labs are features we're trying out, each off until you turn it on in Settings › Labs.</p>
<p><a href="${INDEX_PATH}">Every lab</a></p>
</section>`
  return shell(
    origin,
    {
      title: `${lab.title} · Hex Labs`,
      description,
      url: pagePath(lab),
      image: cardPath(lab),
      imageAlt: `Hex Labs: ${lab.title}. ${cardLine(lab.summary)}`,
    },
    body,
  )
}

export function renderIndex(labs: Lab[], origin: string): string {
  const items = newestFirst(labs)
    .map(
      (lab) =>
        `<li><a href="${pagePath(lab)}"><span class="title">${escape(lab.title)}</span><span class="line">${escape(sentence(lab.summary))}</span><span class="meta">${escape(lab.groupTitle)} · ${escape(osList(lab.oses))}</span></a></li>`,
    )
    .join("\n")
  const body = `<h1>Hex Labs</h1>
<p class="index-intro">Features we're trying out, newest first. Each is off until you turn it on in Settings › Labs.</p>
<ol>
${items}
</ol>`
  return shell(
    origin,
    {
      title: "Hex Labs",
      description: "Features Hex is trying out, each off until you turn it on in Settings › Labs.",
      url: INDEX_PATH,
      image: INDEX_CARD_PATH,
      imageAlt: "Hex Labs: features Hex is trying out.",
    },
    body,
  )
}
