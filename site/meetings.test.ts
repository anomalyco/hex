// The Meetings showcase at /hex/meetings/: a standalone page imported with
// `bun run import:meetings`. It must load only files it ships, carry no
// session ids, fit Workers static assets, and live at its trailing-slash
// address so its relative paths resolve.
import { expect, test } from "bun:test"
import { readFileSync, statSync } from "node:fs"
import { join } from "node:path"
import { MEETINGS_CARD, meetingsHead, runtimeFiles, stripHead } from "./scripts/meetings/bundle"
import worker from "./worker"

const dir = new URL("./public/meetings/", import.meta.url).pathname
const html = readFileSync(join(dir, "index.html"), "utf8")

test("the page names only files it ships", () => {
  const bundle = runtimeFiles(dir)
  expect(bundle.missing).toEqual([])
  expect(bundle.files).toContain("audio/manifest.json")
})

test("every shipped file is one the page loads, or its card", () => {
  const loaded = new Set([...runtimeFiles(dir).files, MEETINGS_CARD])
  const shipped = [...new Bun.Glob("**/*").scanSync({ cwd: dir, dot: true })]
  expect(shipped.filter((file) => !loaded.has(file))).toEqual([])
})

test("files fit Workers static assets", () => {
  for (const file of new Bun.Glob("**/*").scanSync({ cwd: dir })) expect(statSync(join(dir, file)).size).toBeLessThan(25 * 1024 * 1024)
})

test("the committed page carries no session id or social tags of its own", () => {
  expect(html).not.toContain("opencode-session")
  expect(stripHead(html)).toBe(html)
})

test("the build writes the canonical address and the card for its origin", () => {
  const head = meetingsHead("https://anoma.ly/hex")
  expect(head).toContain('<link rel="canonical" href="https://anoma.ly/hex/meetings/">')
  expect(head).toContain('<meta property="og:image" content="https://anoma.ly/hex/meetings/og-meetings.png">')
  expect(head).toContain('<meta name="twitter:card" content="summary_large_image">')
  expect(head).toContain('<meta property="og:title" content="Hex Meetings">')
  // A fresh bundle's own tags give way to the build's.
  expect(stripHead(html.replace("</head>", head + "</head>"))).toBe(html)
})

test("/hex/meetings redirects to its trailing-slash address", async () => {
  const env = { CANONICAL_HOST: "anoma.ly" } as unknown as Parameters<typeof worker.fetch>[1]
  const response = await worker.fetch(new Request("https://anoma.ly/hex/meetings?x=1"), env)
  expect(response.status).toBe(301)
  expect(response.headers.get("Location")).toBe("https://anoma.ly/hex/meetings/?x=1")
})

test("the landing page links Meetings in the header and the footer", () => {
  const source = (name: string) => readFileSync(new URL(`./src/${name}`, import.meta.url), "utf8")
  expect(source("App.tsx").match(/meetings\/`\}>\s*Meetings/g)?.length).toBe(1)
  expect(source("Below.tsx")).toContain('<a href={withBase("/meetings/")}>Meetings</a>')
})
