// Hex's stable changelog at /hex/changelog: every release's notes, newest
// first. The notes are docs/releases/<X.Y.Z>.md, the files the Release
// workflow publishes to GitHub, so nothing is copied by hand;
// scripts/changelog.ts reads them and writes the page at build. Dev builds
// have their own notes at /hex/dev (src/dev-notes.ts).
import { CHANGELOG_PATH, DEV_NOTES_PATH } from "./base"
import { escape, shell } from "./labs"

export { CHANGELOG_PATH }
export const CHANGELOG_CARD_PATH = `${CHANGELOG_PATH}.png?v=1`
export const CARD = { title: "Changelog", line: "What changed in each stable release of Hex." }

const REPOSITORY = "https://github.com/anomalyco/hex"
// Where a relative link in the notes points: the file's own folder on GitHub.
const NOTES_FOLDER = `${REPOSITORY}/blob/dev/docs/releases/`

export type Block =
  | { type: "heading"; text: string }
  | { type: "paragraph"; text: string }
  | { type: "list"; items: string[] }
  | { type: "quote"; blocks: Block[] }

export type Release = { version: string; title: string; blocks: Block[] }

const VERSION = /^([0-9]+)\.([0-9]+)\.([0-9]+)$/

function parts(version: string): [number, number, number] | undefined {
  const match = VERSION.exec(version)
  return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : undefined
}

// Orders versions by number, so 2.1.10 comes after 2.1.9. Only for sorting
// the page and choosing a tag's prefix; Hex itself never compares versions.
export function compareVersions(a: string, b: string): number {
  const left = parts(a) ?? [0, 0, 0]
  const right = parts(b) ?? [0, 0, 0]
  return left[0] - right[0] || left[1] - right[1] || left[2] - right[2]
}

// The released versions among a folder's file names, newest first. Only
// <X.Y.Z>.md is a release; other notes (linux-wayland-beta.md) are not.
export function versions(files: string[]): string[] {
  return files
    .filter((file) => file.endsWith(".md") && parts(file.slice(0, -".md".length)))
    .map((file) => file.slice(0, -".md".length))
    .sort((a, b) => compareVersions(b, a))
}

// GitHub has a release for every version since 2.1.16, tagged app-v<version>
// through 2.3.1 and v<version> from 3.0.0. Earlier versions have none.
export function releaseUrl(version: string): string | undefined {
  if (compareVersions(version, "2.1.16") < 0) return undefined
  const tag = compareVersions(version, "3.0.0") < 0 ? `app-v${version}` : `v${version}`
  return `${REPOSITORY}/releases/tag/${tag}`
}

// The Markdown the notes use: one title, section headings, paragraphs,
// bullets, and quoted callouts, each hard-wrapped. Anything else reads as a
// paragraph.
export function parseNotes(markdown: string): { title: string; blocks: Block[] } {
  const blocks: Block[] = []
  let title = ""
  let open: Block | undefined
  let quote: string[] | undefined
  const closeQuote = () => {
    if (quote) blocks.push({ type: "quote", blocks: parseNotes(quote.join("\n")).blocks })
    quote = undefined
  }
  for (const raw of markdown.split(/\r?\n/)) {
    const line = raw.trim()
    if (line.startsWith(">")) {
      open = undefined
      quote = [...(quote ?? []), line.replace(/^>\s?/, "")]
      continue
    }
    closeQuote()
    const heading = /^(#{1,6})\s+(.*)$/.exec(line)
    const item = /^[-*]\s+(.*)$/.exec(line)
    if (line === "") {
      open = undefined
    } else if (heading) {
      open = undefined
      if (heading[1].length <= 2 && !title && blocks.length === 0) title = heading[2]
      else blocks.push({ type: "heading", text: heading[2] })
    } else if (item) {
      if (open?.type !== "list") blocks.push((open = { type: "list", items: [] }))
      open.items.push(item[1])
    } else if (open?.type === "list") {
      open.items[open.items.length - 1] += ` ${line}`
    } else if (open?.type === "paragraph") {
      open.text += ` ${line}`
    } else {
      blocks.push((open = { type: "paragraph", text: line }))
    }
  }
  closeQuote()
  return { title, blocks }
}

// The releases in a folder's files, newest first.
export function releasesFrom(files: { name: string; text: string }[]): Release[] {
  const text = new Map(files.map((file) => [file.name, file.text]))
  return versions(files.map((file) => file.name)).map((version) => ({ version, ...parseNotes(text.get(`${version}.md`)!) }))
}

const INLINE = /`([^`]+)`|\*\*([^*]+)\*\*|\[([^\]]+)\]\(([^)\s]+)\)|(https?:\/\/[^\s<>)\]]+)|(?<![\w&/])#([1-9][0-9]{0,4})\b/g

function link(href: string, html: string): string {
  let url: URL
  try {
    url = new URL(href, NOTES_FOLDER)
  } catch {
    return html
  }
  if (url.protocol !== "https:" && url.protocol !== "http:") return html
  return `<a href="${escape(url.toString())}">${html}</a>`
}

// A line's inline Markdown as HTML: code, bold, links, bare addresses, and
// #123 as the issue or pull request on GitHub. Everything else is escaped.
export function inline(text: string): string {
  let html = ""
  let last = 0
  for (const match of text.matchAll(INLINE)) {
    html += escape(text.slice(last, match.index))
    last = match.index + match[0].length
    const [, code, bold, label, href, address, issue] = match
    if (code !== undefined) html += `<code>${escape(code)}</code>`
    else if (bold !== undefined) html += `<strong>${inline(bold)}</strong>`
    else if (label !== undefined) html += link(href, inline(label))
    else if (address !== undefined) {
      // A sentence's own stop is not part of the address.
      const url = address.replace(/[.,;:!?]+$/, "")
      html += link(url, escape(url)) + escape(address.slice(url.length))
    } else html += link(`${REPOSITORY}/issues/${issue}`, `#${issue}`)
  }
  return html + escape(text.slice(last))
}

// Section headings read "New" whether the notes wrote "new" or "New".
function sentenceCase(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1)
}

function renderBlock(block: Block): string {
  switch (block.type) {
    case "heading":
      return `<h3>${inline(sentenceCase(block.text))}</h3>`
    case "paragraph":
      return `<p>${inline(block.text)}</p>`
    case "list":
      return `<ul>
${block.items.map((item) => `<li>${inline(item)}</li>`).join("\n")}
</ul>`
    case "quote":
      return `<blockquote>
${block.blocks.map(renderBlock).join("\n")}
</blockquote>`
  }
}

export function renderRelease(release: Release): string {
  const version = escape(release.version)
  const url = releaseUrl(release.version)
  const github = url ? `<a class="release__link" href="${escape(url)}">GitHub release</a>` : ""
  return `<section class="release" id="${version}">
<div class="release__head"><h2><a href="#${version}">${version}</a></h2>${github}</div>
${release.blocks.map(renderBlock).join("\n")}
</section>`
}

// The notes' own blocks, below the Labs pages' title and line. The shell's
// list and code styles are for its index rows and commands, so they are reset
// here for prose.
const STYLE = `
.index-intro a { text-decoration-color: #4a4b50; text-underline-offset: 3px; }
.index-intro a:hover { text-decoration-color: currentColor; }
.releases { margin: 56px 0 0; }
.release { padding: 28px 0 0; margin: 28px 0 0; border-top: 1px solid #1d1e21; scroll-margin-top: 20px; }
.release:first-child { margin-top: 0; }
.release__head { display: flex; align-items: baseline; justify-content: space-between; gap: 16px; margin: 0 0 16px; }
.release h2 { margin: 0; font-size: 26px; font-weight: 600; letter-spacing: -0.025em; line-height: 1.2; font-variant-numeric: tabular-nums; }
.release h2 a { text-decoration: none; }
.release h2 a:hover { text-decoration: underline; text-decoration-color: #4a4b50; text-underline-offset: 4px; }
.release__link { color: #8a8b90; font-size: 14px; white-space: nowrap; text-decoration-color: #4a4b50; text-underline-offset: 3px; }
.release__link:hover { color: #f2f2ef; }
.release h3 { margin: 24px 0 8px; font-size: 16px; font-weight: 600; letter-spacing: -0.01em; }
.release p, .release li { color: #b4b5ba; max-width: 38em; text-wrap: pretty; }
.release p { margin: 0 0 12px; }
.release ul { list-style: disc; margin: 0 0 12px; padding: 0 0 0 22px; }
.release li { border: 0; margin: 0 0 8px; padding-left: 4px; }
.release li::marker { color: #6f7075; }
.release strong { color: #f2f2ef; font-weight: 600; }
.release p a, .release li a { display: inline; padding: 0; text-decoration: underline; text-decoration-color: #4a4b50; text-underline-offset: 3px; }
.release p a:hover, .release li a:hover { text-decoration-color: currentColor; }
.release code { white-space: normal; overflow-wrap: anywhere; -webkit-box-decoration-break: clone; box-decoration-break: clone; }
.release blockquote { margin: 0 0 20px; padding: 16px 18px 4px; border: 1px solid #222326; border-radius: 12px; background: #121214; font-size: 15px; }
.release blockquote p { max-width: none; }
`

export function renderChangelog(releases: Release[], origin: string): string {
  const body = `<h1>Changelog</h1>
<p class="index-intro">What changed in each stable release of Hex, newest first. Dev builds have <a href="${DEV_NOTES_PATH}">their own notes</a>.</p>
<div class="releases">
${releases.map(renderRelease).join("\n")}
</div>`
  return shell(
    origin,
    {
      title: "Hex changelog",
      description: "What changed in each stable release of Hex, newest first.",
      url: CHANGELOG_PATH,
      image: CHANGELOG_CARD_PATH,
      imageAlt: `Hex: ${CARD.title}. ${CARD.line}`,
    },
    body,
    { crumb: { href: CHANGELOG_PATH, label: "Changelog" }, style: STYLE },
  )
}
