// The site's search and agent surface: the below-the-fold page a crawler reads
// without JavaScript, its structured data, the MCP page, the llms files, the
// sitemap, and the header that keeps dev.anoma.ly out of the index.
import { expect, test } from "bun:test"
import { existsSync, readFileSync } from "node:fs"
import worker from "./worker"
import { canonicalOriginForMode, originForMode, PRODUCTION_HOST } from "./src/base"
import { MCP_PATH, renderMcp } from "./src/mcp"
import { render } from "./src/prerender"

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8")
const html = read("./index.html")
const rendered = render()

function textOf(markup: string): string {
  return markup
    .replace(/<[^>]+>/g, " ")
    .replace(/&#x27;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .replace(/\s+([,.])/g, "$1")
    .trim()
}

const details = rendered.slice(rendered.indexOf('<section class="details"'), rendered.indexOf("<footer"))
const detailsText = textOf(details)

test("the landing page says what it is below the fold, in real headings", () => {
  for (const [id, heading] of [
    ["dictation", "Voice dictation that stays on your machine."],
    ["meetings", "Meetings without a bot."],
    ["tools", "Screenshots, transforms, and the command line."],
    ["faq", "Questions people ask."],
  ]) {
    expect(rendered).toContain(`<h2 id="${id}">${heading}</h2>`)
  }
  expect(detailsText).toContain("Voice dictation that stays on your machine.")
  expect(detailsText).toContain("Meetings without a bot.")
  expect(detailsText).toContain("Screenshots, transforms, and the command line.")
  expect(detailsText).toContain("Questions people ask.")
})

test("the below-the-fold copy names the product category and its facts", () => {
  for (const phrase of [
    "open-source voice dictation",
    "Parakeet or Whisper",
    "Audio never leaves the machine",
    "microphone and your Mac's system audio",
    "no participant to invite and no bot in the call",
    "hex.config.ts",
    "hex mcp",
  ]) {
    expect(detailsText).toContain(phrase)
  }
})

test("the below-the-fold copy is a concise 450 to 550 words", () => {
  const words = detailsText.split(" ")
  expect(words.length).toBeGreaterThanOrEqual(450)
  expect(words.length).toBeLessThanOrEqual(550)
})

test("the structured data is one graph with the app, site, video, and FAQ", () => {
  const block = html.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/)?.[1]
  expect(block).toBeDefined()
  const graph = (JSON.parse(block as string) as { "@graph": Array<Record<string, unknown>> })["@graph"]
  const type = (node: Record<string, unknown>) => node["@type"]
  expect(graph.map(type)).toEqual(["SoftwareApplication", "Organization", "WebSite", "VideoObject", "FAQPage"])
  const app = graph[0]
  expect(app["isAccessibleForFree"]).toBe(true)
  expect(app["sameAs"]).toEqual(["https://github.com/anomalyco/hex"])
  expect(Array.isArray(app["featureList"])).toBe(true)
  const faq = graph[4] as { mainEntity: Array<{ name: string; acceptedAnswer: { text: string } }> }
  expect(faq.mainEntity).toHaveLength(6)
  for (const question of faq.mainEntity) {
    expect(detailsText).toContain(question.name)
    // The visible answer and the structured one are the same words.
    expect(detailsText).toContain(question.acceptedAnswer.text.split(".")[0])
  }
})

test("canonicals and the sitemap always name production, even in a dev build", () => {
  expect(PRODUCTION_HOST).toBe("anoma.ly")
  expect(originForMode("dev")).toBe("https://dev.anoma.ly/hex")
  expect(canonicalOriginForMode("dev")).toBe("https://anoma.ly/hex")
  expect(canonicalOriginForMode("production")).toBe("https://anoma.ly/hex")
})

test("the landing page links the MCP page from the footer and the copy", () => {
  const app = read("./src/App.tsx")
  expect(app).toContain("mcp`}>MCP</a>")
  expect(app.match(/BASE_URL}mcp/g)?.length).toBeGreaterThanOrEqual(3)
})

test("the MCP page is prerendered, complete, and truthful about the tools", () => {
  const page = renderMcp("https://anoma.ly/hex")
  expect(page).toContain("<title>Hex MCP: meeting transcripts for your agent</title>")
  expect(page).toContain('<link rel="canonical" href="https://anoma.ly/hex/mcp">')
  expect(page).toContain('<meta name="description" content="hex mcp is Hex')
  expect(page).toContain("Your meetings, readable by your agent.")
  for (const tool of ["list_meetings", "search_transcripts", "get_transcript", "get_meeting_notes", "get_chapters"]) {
    expect(page).toContain(tool)
  }
  for (const client of ["OpenCode", "Claude Code", "Claude Desktop", "Cursor"]) expect(page).toContain(client)
  expect(page).toContain("claude mcp add hex -- hex mcp")
  expect(page).toContain("mcpServers")
  expect(page).toContain("hex-meeting://")
  // Names the server does not have.
  expect(page).not.toContain("get_meeting<")
  expect(page).not.toContain("search_meetings")
  const ld = page.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/)?.[1]
  expect((JSON.parse(ld as string) as { "@type": string })["@type"]).toBe("SoftwareApplication")
})

test("every agent file exists and points at every new page", () => {
  expect(existsSync(new URL("./public/llms.txt", import.meta.url))).toBe(true)
  expect(existsSync(new URL("./public/llms-full.txt", import.meta.url))).toBe(true)
  expect(existsSync(new URL("./public/skill.md", import.meta.url))).toBe(true)
  const index = read("./public/llms.txt")
  const full = read("./public/llms-full.txt")
  const skill = read("./public/skill.md")
  for (const page of ["https://anoma.ly/hex/", "https://anoma.ly/hex/meetings/", "https://anoma.ly/hex/mcp", "https://anoma.ly/hex/changelog", "https://anoma.ly/hex/iphone", "https://anoma.ly/hex/android", "https://anoma.ly/hex/docs/transforms.md", "https://anoma.ly/hex/docs/admin.md", "https://anoma.ly/hex/llms-full.txt", "https://anoma.ly/hex/skill.md"]) {
    expect(index).toContain(page)
  }
  expect(index).toContain("open-source")
  expect(index).toContain("offline")
  expect(full).toContain("hex mcp")
  expect(skill).toStartWith("---\nname: hex\n")
  expect(skill).toContain("list_meetings")
  expect(skill).toContain("claude mcp add")
})

test("the sitemap names the new routes", () => {
  const sitemap = read("./public/sitemap.xml")
  for (const path of ["/meetings/", "/mcp", "/docs/transforms", "/docs/admin", "/llms.txt", "/llms-full.txt", "/skill.md"]) {
    expect(sitemap).toContain(`<loc>__ORIGIN__${path}</loc>`)
  }
  expect(read("./scripts/build.ts")).toContain("scripts/mcp.ts")
})

const environment = {
  ASSETS: { fetch: () => Promise.resolve(new Response("Not found", { status: 404 })) },
  FILES: { get: () => Promise.resolve(null), head: () => Promise.resolve(null) },
  CANONICAL_HOST: "anoma.ly",
} as unknown as Parameters<typeof worker.fetch>[1]

test("dev.anoma.ly answers noindex on every response", async () => {
  const dev = { ...(environment as object), CANONICAL_HOST: "dev.anoma.ly" } as unknown as Parameters<typeof worker.fetch>[1]
  const page = await worker.fetch(new Request("https://dev.anoma.ly/hex/"), dev)
  expect(page.headers.get("X-Robots-Tag")).toBe("noindex, nofollow")
  const redirect = await worker.fetch(new Request("https://dev.anoma.ly/hex/meetings"), dev)
  expect(redirect.status).toBe(301)
  expect(redirect.headers.get("X-Robots-Tag")).toBe("noindex, nofollow")
  const production = await worker.fetch(new Request("https://anoma.ly/hex/"), environment)
  expect(production.headers.get("X-Robots-Tag")).toBeNull()
})

test("markdown and text routes carry their charset", async () => {
  const markdown = await worker.fetch(new Request("https://anoma.ly/hex/skill.md"), environment)
  expect(markdown.headers.get("Content-Type")).toBe("text/markdown; charset=utf-8")
  const text = await worker.fetch(new Request("https://anoma.ly/hex/llms-full.txt"), environment)
  expect(text.headers.get("Content-Type")).toBe("text/plain; charset=utf-8")
  expect(MCP_PATH).toBe("/hex/mcp")
})
