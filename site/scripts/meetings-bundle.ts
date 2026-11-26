// The Meetings showcase (/hex/meetings/) is a standalone page built outside
// the site: plain HTML, ES modules, CSS, images, and a voice bank. It lives in
// public/meetings/ exactly as the page loads it, plus its social card.
// runtimeFiles walks the page's references from index.html (attributes, CSS
// url(), module imports, path-like string literals, and the voice bank's
// manifest), so only what the page loads ships.
import { existsSync, readFileSync } from "node:fs"
import { dirname, join, normalize } from "node:path"

// The folder in the built site, served at MEETINGS_PATH + "/" (src/base.ts).
export const MEETINGS_DIR = "meetings"
// Drawn by scripts/generate-og-meetings.ts, kept across imports.
export const MEETINGS_CARD = "og-meetings.png"

const FOLLOW = /\.(html|css|js|json)$/
const LITERAL = /["'`]((?:\.{1,2}\/)?[A-Za-z0-9_-][A-Za-z0-9_./-]*\.(?:css|js|mjs|json|mp3|wav|ogg|webp|png|jpe?g|gif|svg|avif|woff2?))["'`]/g

type Ref = { ref: string; strict: boolean; fromPage: boolean }

function isRelative(ref: string): boolean {
  return Boolean(ref) && !/^(?:[a-z][a-z0-9+.-]*:|\/|#)/i.test(ref)
}

function referencesIn(file: string, text: string): Ref[] {
  const refs: Ref[] = []
  const add = (ref: string, strict: boolean, fromPage: boolean) => {
    const path = ref.split(/[?#]/)[0]
    if (isRelative(path)) refs.push({ ref: path, strict, fromPage })
  }
  if (file.endsWith(".html")) for (const m of text.matchAll(/\s(?:href|src|poster)="([^"]*)"/g)) add(m[1], true, false)
  if (file.endsWith(".html") || file.endsWith(".css")) for (const m of text.matchAll(/url\(\s*["']?([^"')]+)["']?\s*\)/g)) add(m[1], true, false)
  if (file.endsWith(".js")) {
    // Imports resolve against the module; fetches and other strings against the page.
    const imports = new Set([...text.matchAll(/\b(?:import|from)\s*\(?\s*["'](\.{1,2}\/[^"'\s]+)["']/g)].map((m) => m[1]))
    for (const ref of imports) add(ref, true, false)
    for (const m of text.matchAll(LITERAL)) if (!imports.has(m[1])) add(m[1], false, !m[1].startsWith("."))
  }
  // The manifest names its clips' files; they are fetched against the page.
  if (file.endsWith(".json")) for (const m of text.matchAll(LITERAL)) add(m[1], true, true)
  return refs
}

// Session ids and the bundle's own social and canonical tags, which the build
// writes instead (scripts/meetings.ts).
export function stripHead(html: string): string {
  return html.replace(/^[ \t]*<(?:meta (?:name="opencode-session"|property="og:|name="twitter:)|link rel="canonical")[^>]*>[ \t]*\r?\n?/gm, "")
}

const TITLE = "Hex Meetings"
const LINE = "Local meeting transcripts &amp; notes on your Mac."
const CARD_ALT = "The Hex mark above the words Hex Meetings, with a pill reading Q4 sync detected and a Record button."

// The page's canonical address and social tags, for the origin the build targets.
export function meetingsHead(origin: string): string {
  const url = `${origin}/${MEETINGS_DIR}/`
  const card = `${url}${MEETINGS_CARD}`
  return [
    `<link rel="canonical" href="${url}">`,
    `<meta property="og:type" content="website">`,
    `<meta property="og:site_name" content="Hex">`,
    `<meta property="og:url" content="${url}">`,
    `<meta property="og:title" content="${TITLE}">`,
    `<meta property="og:description" content="${LINE}">`,
    `<meta property="og:image" content="${card}">`,
    `<meta property="og:image:type" content="image/png">`,
    `<meta property="og:image:width" content="1200">`,
    `<meta property="og:image:height" content="630">`,
    `<meta property="og:image:alt" content="${CARD_ALT}">`,
    `<meta name="twitter:card" content="summary_large_image">`,
    `<meta name="twitter:title" content="${TITLE}">`,
    `<meta name="twitter:description" content="${LINE}">`,
    `<meta name="twitter:image" content="${card}">`,
    `<meta name="twitter:image:alt" content="${CARD_ALT}">`,
  ].map((tag) => `${tag}\n`).join("")
}

export type Bundle = { files: string[]; missing: { from: string; ref: string }[] }

// The files the page loads, relative to dir, starting from index.html. An
// HTML or CSS reference, a module import, or a manifest entry that names no
// file is missing; a script's string that names no file is only a string.
export function runtimeFiles(dir: string): Bundle {
  const seen = new Set<string>()
  const missing: { from: string; ref: string }[] = []
  const queue = ["index.html"]
  while (queue.length) {
    const file = queue.shift()!
    if (seen.has(file)) continue
    seen.add(file)
    if (!FOLLOW.test(file)) continue
    for (const { ref, strict, fromPage } of referencesIn(file, readFileSync(join(dir, file), "utf8"))) {
      const path = normalize(fromPage ? ref : join(dirname(file), ref))
      if (!path.startsWith("..") && existsSync(join(dir, path))) queue.push(path)
      else if (strict) missing.push({ from: file, ref })
    }
  }
  return { files: [...seen].sort(), missing }
}
