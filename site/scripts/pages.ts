// Writes the comparison pages (/hex/vs-wispr-flow, /hex/vs-superwhisper) and
// Hex for Linux (/hex/linux) into the built site, a social card beside each,
// and each page in the sitemap when its committed copy doesn't list it.
//
//   bun scripts/pages.ts --mode dev
import { getWorkerAssetsDir } from "@cloudflare/build-output-utils"
import { readFileSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { parseArgs } from "node:util"
import { canonicalOriginForMode } from "../src/base"
import { cardPath, COMPARISONS, renderComparison } from "../src/compare"
import { LINUX_CARD, LINUX_CARD_PATH, LINUX_PATH, renderLinux } from "../src/linux"
import data from "../src/release-data.json"
import { RELEASE } from "../src/release"
import { card } from "./card"

const { values } = parseArgs({ options: { mode: { type: "string" }, out: { type: "string" } } })
const origin = canonicalOriginForMode(values.mode)
const assets = values.out ?? getWorkerAssetsDir(resolve(import.meta.dirname, ".."))
const address = (path: string) => `${origin.replace("https://", "")}${path.slice("/hex".length)}`
const site = (path: string) => join(assets, path.slice("/hex/".length))
const versioned = (html: string) => html.replaceAll("__HEX_VERSION__", data.version)

for (const comparison of COMPARISONS) {
  writeFileSync(site(`${comparison.path}.html`), versioned(renderComparison(comparison, origin)))
  writeFileSync(site(cardPath(comparison)), await card("Hex", comparison.card.title, comparison.card.line, address(comparison.path)))
}

const linux = RELEASE.linux
writeFileSync(site(`${LINUX_PATH}.html`), versioned(renderLinux(origin, { command: linux.command, deb: linux.deb.x64, rpm: linux.rpm.x64, nix: RELEASE.nix, aur: linux.aur })))
writeFileSync(site(LINUX_CARD_PATH), await card("Hex", LINUX_CARD.title, LINUX_CARD.line, address(LINUX_PATH)))

const sitemap = join(assets, "sitemap.xml")
let text = readFileSync(sitemap, "utf8")
for (const path of [...COMPARISONS.map((comparison) => comparison.path), LINUX_PATH]) {
  const loc = `<loc>${origin}${path.slice("/hex".length)}</loc>`
  if (!text.includes(loc)) text = text.replace("</urlset>", `  <url>${loc}</url>\n</urlset>`)
}
writeFileSync(sitemap, text)
console.log(`Pages: ${COMPARISONS.map((comparison) => comparison.path).join(", ")}, ${LINUX_PATH}, and their cards.`)
