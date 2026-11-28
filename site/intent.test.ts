// The comparison pages, Hex for Linux, and the crawlable half of Meetings:
// prerendered and complete, their questions word for word in their FAQPage,
// honest about platforms and limits, and linked from the landing page.
import { expect, test } from "bun:test"
import { readFileSync } from "node:fs"
import { COMPARISONS, renderComparison, SUPERWHISPER, WISPR_FLOW } from "./src/compare"
import { LINUX_PATH, renderLinux } from "./src/linux"
import { finishMeetings, MEETINGS_DESCRIPTION, MEETINGS_QUESTIONS, MEETINGS_TITLE } from "./src/meetings-page"
import { render } from "./src/prerender"
import worker from "./worker"

const ORIGIN = "https://anoma.ly/hex"
const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8")
const INSTALL = {
  command: "curl -fsSL https://anoma.ly/hex/install-linux.sh | sh",
  deb: "curl -fLo /tmp/hex.deb https://anoma.ly/hex/download/linux-x64-deb && sudo apt install /tmp/hex.deb",
  rpm: "curl -fLo /tmp/hex.rpm https://anoma.ly/hex/download/linux-x64-rpm && sudo dnf install /tmp/hex.rpm",
  nix: "nix profile add github:anomalyco/hex",
}

const text = (markup: string) =>
  markup
    .replace(/<[^>]+>/g, " ")
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .replace(/\s+([,.)])/g, "$1")
    .trim()

type Graph = { "@graph": Array<Record<string, unknown>> }
function graphOf(page: string): Graph["@graph"] {
  const block = page.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/)?.[1]
  return (JSON.parse(block as string) as Graph)["@graph"]
}

// Each structured question and answer is on the page, word for word.
function expectFaqVisible(page: string) {
  const faq = graphOf(page).find((node) => node["@type"] === "FAQPage") as { mainEntity: Array<{ name: string; acceptedAnswer: { text: string } }> }
  expect(faq.mainEntity.length).toBeGreaterThanOrEqual(4)
  const visible = text(page)
  for (const entry of faq.mainEntity) expect(visible).toContain(`${entry.name} ${entry.acceptedAnswer.text}`)
}

test("each comparison is prerendered with its canonical, social tags, and a card", () => {
  for (const comparison of COMPARISONS) {
    const page = renderComparison(comparison, ORIGIN)
    const url = `https://anoma.ly${comparison.path}`
    expect(page).toContain(`<link rel="canonical" href="${url}">`)
    expect(page).toContain(`<meta property="og:url" content="${url}">`)
    expect(page).toContain(`<meta property="og:image" content="${url}.png">`)
    expect(page).toContain('<meta name="twitter:card" content="summary_large_image">')
    expect(page).toContain(`<h1 class="vs__title">Hex <span class="vs__vs">vs</span> ${comparison.name}</h1>`)
    // Room for header art: none until a comparison sets it.
    expect(page).not.toContain("vs__art\"")
    expect(renderComparison({ ...comparison, art: { src: "/hex/vs.webp", alt: "Hex versus", width: 1440, height: 640 } }, ORIGIN)).toContain('<figure class="vs__art"><img src="/hex/vs.webp"')
    expect(page.match(/<h1[ >]/g)).toHaveLength(1)
    expect(comparison.description.length).toBeLessThanOrEqual(200)
    expect(comparison.title.length).toBeLessThanOrEqual(65)
    expect(page).toContain(`When ${comparison.name} may fit better`)
    // The other side is sourced and dated.
    for (const [, href] of comparison.sources) expect(page).toContain(`href="${href}"`)
    expect(page).toContain("read on November 28, 2026")
    expectFaqVisible(page)
    expect(graphOf(page).map((node) => node["@type"])).toEqual(["WebPage", "SoftwareApplication", "FAQPage", "BreadcrumbList"])
  }
})

test("the comparisons state the facts each side's pages state", () => {
  const wispr = text(renderComparison(WISPR_FLOW, ORIGIN))
  for (const fact of ["Transcription needs an internet connection", "$15 a month", "Notetaker", "Remote server for Notetaker meetings", "Not supported", "Intel Mac"]) {
    expect(wispr).toContain(fact)
  }
  const superwhisper = text(renderComparison(SUPERWHISPER, ORIGIN))
  for (const fact of ["$8.49 a month", "$249.99 once", "Wayland only", "macOS 14", "Parakeet Free Pro", "x86_64 and ARM64"]) {
    expect(superwhisper).toContain(fact)
  }
  // Superwhisper ships on Linux and has a CLI and MCP on the Mac; the page must not say otherwise.
  expect(superwhisper).not.toMatch(/Superwhisper[^.]*(no Linux|not on Linux|Mac and iOS only)/)
  for (const page of [wispr, superwhisper]) {
    expect(page).toContain("Mac, Windows, and Linux")
    expect(page).not.toContain("—")
  }
})

test("the Linux page offers this release's commands and every desktop's path", () => {
  const page = renderLinux(ORIGIN, INSTALL)
  expect(page).toContain('<link rel="canonical" href="https://anoma.ly/hex/linux">')
  expect(page).toContain("<title>Hex for Linux: offline voice dictation for X11 and Wayland</title>")
  for (const command of Object.values(INSTALL)) expect(page).toContain(command.replaceAll("&", "&#38;"))
  expect(page).not.toContain("yay -S hex-bin")
  expect(renderLinux(ORIGIN, { ...INSTALL, aur: "yay -S hex-bin" })).toContain("yay -S hex-bin")
  for (const phrase of ["anomaly.hex:dictation", "dbus-update-activation-environment", "wtype", "RemoteDesktop portal", "GlobalShortcuts portal", "Ctrl+Alt+Space", "AVX2", "glibc 2.36", "PipeWire", "Vulkan"]) {
    expect(page).toContain(phrase)
  }
  // Copy buttons stay hidden until the page's one script shows them.
  const buttons = page.match(/<button [^>]*>/g) ?? []
  expect(buttons).toHaveLength(4)
  for (const button of buttons) expect(button).toMatch(/ hidden>$/)
  expect(page.match(/<script>/g)).toHaveLength(1)
  expectFaqVisible(page)
})

test("the Meetings build adds the crawlable section, title, and FAQ to the showcase", () => {
  const html = finishMeetings(read("./public/meetings/index.html"), ORIGIN)
  expect(html).toContain(`<title>${MEETINGS_TITLE}</title>`)
  expect(html).toContain(`<meta name="description" content="${MEETINGS_DESCRIPTION}">`)
  expect(html.match(/<title>/g)).toHaveLength(1)
  expect(html.match(/<meta name="description"/g)).toHaveLength(1)
  expect(html.indexOf('<section class="mx"')).toBeLessThan(html.indexOf('<footer class="site-footer">'))
  for (const phrase of ["How Hex records a meeting", "WASAPI loopback", "PipeWire", "Core Audio process tap", "hex meetings show latest --md", "hex mcp"]) {
    expect(html).toContain(phrase)
  }
  expect(MEETINGS_QUESTIONS.length).toBe(5)
  expectFaqVisible(html)
  expect(() => finishMeetings("<html><head></head><body></body></html>", ORIGIN)).toThrow()
})

test("the landing page's footer and the agent files link every new page", () => {
  const footer = render().slice(render().indexOf("<footer"))
  for (const href of ["/hex/linux", "/hex/vs-wispr-flow", "/hex/vs-superwhisper"]) expect(footer).toContain(`href="${href}"`)
  const sitemap = read("./public/sitemap.xml")
  for (const path of ["/linux", "/vs-wispr-flow", "/vs-superwhisper", "/meetings/"]) expect(sitemap).toContain(`<loc>__ORIGIN__${path}</loc>`)
  for (const file of ["./public/llms.txt", "./public/llms-full.txt"]) {
    const body = read(file)
    for (const page of ["https://anoma.ly/hex/linux", "https://anoma.ly/hex/vs-wispr-flow", "https://anoma.ly/hex/vs-superwhisper"]) expect(body).toContain(page)
  }
  expect(read("./scripts/build.ts")).toContain("scripts/pages.ts")
  expect(LINUX_PATH).toBe("/hex/linux")
})

test("every page draws Hex's real mark: two rings and a solid core", () => {
  const core = "m32 24 7.5 4.25v8.5L32 41l-7.5-4.25v-8.5Z"
  expect(read("./src/App.tsx")).toContain(core)
  expect(read("./public/favicon.svg")).toContain(core)
  for (const page of [...COMPARISONS.map((comparison) => renderComparison(comparison, ORIGIN)), renderLinux(ORIGIN, INSTALL)]) {
    expect(page).toContain(`<path class="core" d="${core}"/>`)
  }
})

test("the new pages are noindex on dev.anoma.ly", async () => {
  const env = {
    ASSETS: { fetch: () => Promise.resolve(new Response("<!doctype html>", { status: 200 })) },
    FILES: { get: () => Promise.resolve(null), head: () => Promise.resolve(null) },
    CANONICAL_HOST: "dev.anoma.ly",
  } as unknown as Parameters<typeof worker.fetch>[1]
  for (const path of ["/hex/linux", "/hex/vs-wispr-flow", "/hex/vs-superwhisper", "/hex/meetings/"]) {
    const response = await worker.fetch(new Request(`https://dev.anoma.ly${path}`), env)
    expect(response.status).toBe(200)
    expect(response.headers.get("X-Robots-Tag")).toBe("noindex, nofollow")
  }
})
