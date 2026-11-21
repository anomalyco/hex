// Writes the Labs pages into the built site: labs.html (/hex/labs),
// labs/<id>.html (/hex/labs/<id>), and a 1200 × 630 social card beside each
// (scripts/card.ts), and adds the pages to the sitemap. src/labs.json comes
// from the app's Labs registry. A lab the stable release's source names is in that release; the
// rest are Dev-build only, and so is every lab when the source can't be read.
//
//   bun scripts/labs.ts --mode dev
import { getWorkerAssetsDir } from "@cloudflare/build-output-utils"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { parseArgs } from "node:util"
import { originForMode } from "../src/base"
import { type Channel, type Lab, cardLine, INDEX_PATH, inSource, osList, pagePath, renderIndex, renderLab } from "../src/labs"
import catalog from "../src/labs.json"
import data from "../src/release-data.json"
import { card } from "./card"

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

const site = (path: string) => join(assets, path.slice("/hex/".length))
mkdirSync(site("/hex/labs/"), { recursive: true })
for (const lab of labs) {
  writeFileSync(site(`${pagePath(lab)}.html`), renderLab(lab, channel(lab), origin))
  writeFileSync(site(`${pagePath(lab)}.png`), await card("Hex Labs", lab.title, cardLine(lab.summary), `${osList(lab.oses)} · ${address}`))
}
writeFileSync(site(`${INDEX_PATH}.html`), renderIndex(labs, origin))
writeFileSync(site(`${INDEX_PATH}.png`), await card("Hex Labs", "Labs", "Features Hex is trying out, each off until you turn it on.", address))

const sitemap = join(assets, "sitemap.xml")
const urls = [INDEX_PATH, ...labs.map(pagePath)].map((path) => `  <url><loc>${origin}${path.slice("/hex".length)}</loc></url>`)
writeFileSync(sitemap, readFileSync(sitemap, "utf8").replace("</urlset>", `${urls.join("\n")}\n</urlset>`))
const stable = labs.filter((lab) => "stable" in channel(lab)).length
console.log(`Labs: ${labs.length} pages (${stable} in Hex ${data.version}, ${labs.length - stable} Dev build only).`)
