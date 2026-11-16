// Writes the Labs pages into the built site: labs.html (/hex/labs),
// labs/<id>.html (/hex/labs/<id>), and a 1200 × 630 social card beside each
// (satori lays the card out, resvg draws it, so the build needs no browser),
// and adds the pages to the sitemap. src/labs.json comes from the app's Labs
// registry. A lab the stable release's source names is in that release; the
// rest are Dev-build only, and so is every lab when the source can't be read.
//
//   bun scripts/labs.ts --mode dev
import { getWorkerAssetsDir } from "@cloudflare/build-output-utils"
import { Resvg } from "@resvg/resvg-js"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { parseArgs } from "node:util"
import satori from "satori"
import { originForMode } from "../src/base"
import { type Channel, type Lab, cardLine, INDEX_PATH, inSource, osList, pagePath, renderIndex, renderLab } from "../src/labs"
import catalog from "../src/labs.json"
import data from "../src/release-data.json"

const { values } = parseArgs({ options: { mode: { type: "string" }, out: { type: "string" } } })
const origin = originForMode(values.mode)
const root = resolve(import.meta.dirname, "..")
const assets = values.out ?? getWorkerAssetsDir(root)
const labs = catalog.labs as Lab[]
const address = `${origin.replace("https://", "")}/labs`

async function stableSource(): Promise<string | undefined> {
  if (data.channel !== "stable") return undefined
  const url = `https://raw.githubusercontent.com/anomalyco/hex/v${data.version}/src/labs.rs`
  try {
    const response = await fetch(url, { headers: { "User-Agent": "hex-site" } })
    if (response.ok) return await response.text()
    console.warn(`${url} answered ${response.status}; every lab says Dev build.`)
  } catch (error) {
    console.warn(`${url} failed (${error}); every lab says Dev build.`)
  }
  return undefined
}

const source = await stableSource()
const channel = (lab: Lab): Channel => (source !== undefined && inSource(lab, source) ? { stable: data.version } : { dev: true })

const font = (weight: number) => readFileSync(join(root, `node_modules/@fontsource/inter/files/inter-latin-${weight}-normal.woff`))
const fonts = [
  { name: "Inter", data: font(500), weight: 500 as const, style: "normal" as const },
  { name: "Inter", data: font(600), weight: 600 as const, style: "normal" as const },
]
const icon = `data:image/png;base64,${readFileSync(join(root, "scripts/og/hex-icon.png")).toString("base64")}`

type Node = { type: string; props: Record<string, unknown> }
const el = (type: string, style: Record<string, unknown>, children?: unknown, props: Record<string, unknown> = {}): Node => ({
  type,
  props: { style, children, ...props },
})

function lineSize(line: string): number {
  return line.length <= 70 ? 36 : 30
}

function titleSize(title: string): number {
  if (title.length <= 16) return 108
  if (title.length <= 26) return 88
  return 72
}

// The card: the icon and "Hex Labs" above, the title big, the line, and the
// OSes, on the home page card's black with its soft light.
async function card(title: string, line: string, footer: string): Promise<Buffer> {
  const tree = el(
    "div",
    {
      width: 1200,
      height: 630,
      display: "flex",
      color: "#f3f3f0",
      fontFamily: "Inter",
      backgroundColor: "#070707",
      backgroundImage: "radial-gradient(circle at 160px 90px, rgba(255,255,255,0.075), rgba(255,255,255,0) 520px)",
    },
    el("div", { position: "absolute", top: 72, right: 84, bottom: 68, left: 84, display: "flex", flexDirection: "column", justifyContent: "space-between" }, [
      el("div", { display: "flex", alignItems: "center", gap: 18 }, [
        el("img", { width: 76, height: 76, margin: "-8px" }, undefined, { src: icon, width: 76, height: 76 }),
        el("div", { display: "flex", fontSize: 34, fontWeight: 600, letterSpacing: "-0.02em" }, "Hex Labs"),
      ]),
      el("div", { display: "flex", flexDirection: "column" }, [
        el(
          "div",
          { display: "flex", fontSize: titleSize(title), fontWeight: 600, letterSpacing: "-0.045em", lineHeight: 1.02, maxWidth: 1032 },
          title,
        ),
        el("div", { display: "flex", marginTop: 26, color: "#a3a5ab", fontSize: lineSize(line), fontWeight: 500, letterSpacing: "-0.015em", lineHeight: 1.3, maxWidth: 980 }, line),
      ]),
      el("div", { display: "flex", color: "#6f7177", fontSize: 26, fontWeight: 500 }, footer),
    ]),
  )
  const svg = await satori(tree as Parameters<typeof satori>[0], { width: 1200, height: 630, fonts })
  return new Resvg(svg, { fitTo: { mode: "width", value: 1200 } }).render().asPng()
}

const site = (path: string) => join(assets, path.slice("/hex/".length))
mkdirSync(site("/hex/labs/"), { recursive: true })
for (const lab of labs) {
  writeFileSync(site(`${pagePath(lab)}.html`), renderLab(lab, channel(lab), origin))
  writeFileSync(site(`${pagePath(lab)}.png`), await card(lab.title, cardLine(lab.summary), `${osList(lab.oses)} · ${address}`))
}
writeFileSync(site(`${INDEX_PATH}.html`), renderIndex(labs, origin))
writeFileSync(site(`${INDEX_PATH}.png`), await card("Labs", "Features Hex is trying out, each off until you turn it on.", address))

const sitemap = join(assets, "sitemap.xml")
const urls = [INDEX_PATH, ...labs.map(pagePath)].map((path) => `  <url><loc>${origin}${path.slice("/hex".length)}</loc></url>`)
writeFileSync(sitemap, readFileSync(sitemap, "utf8").replace("</urlset>", `${urls.join("\n")}\n</urlset>`))
const stable = labs.filter((lab) => "stable" in channel(lab)).length
console.log(`Labs: ${labs.length} pages (${stable} in Hex ${data.version}, ${labs.length - stable} Dev build only).`)
