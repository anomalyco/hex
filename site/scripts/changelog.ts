// Writes the stable changelog into the built site: changelog.html
// (/hex/changelog) from every docs/releases/<X.Y.Z>.md, newest first, its
// social card (scripts/card.ts), and the page in the sitemap. The notes are
// the files Release publishes, so a release's page entry is its notes file.
//
//   bun scripts/changelog.ts --mode dev
import { getWorkerAssetsDir } from "@cloudflare/build-output-utils"
import { readdirSync, readFileSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { parseArgs } from "node:util"
import { canonicalOriginForMode } from "../src/base"
import { CARD, CHANGELOG_PATH, releasesFrom, renderChangelog } from "../src/changelog"
import { card } from "./card"

const { values } = parseArgs({ options: { mode: { type: "string" }, out: { type: "string" } } })
const origin = canonicalOriginForMode(values.mode)
const root = resolve(import.meta.dirname, "..")
const assets = values.out ?? getWorkerAssetsDir(root)
const notes = resolve(root, "../docs/releases")
const site = (path: string) => join(assets, path.slice("/hex/".length))

const releases = releasesFrom(readdirSync(notes).map((name) => ({ name, text: readFileSync(join(notes, name), "utf8") })))
if (releases.length === 0) throw new Error(`${notes} has no <X.Y.Z>.md release notes`)
for (const release of releases) {
  if (release.blocks.length === 0) throw new Error(`docs/releases/${release.version}.md has no notes`)
}

writeFileSync(site(`${CHANGELOG_PATH}.html`), renderChangelog(releases, origin))
writeFileSync(site(`${CHANGELOG_PATH}.png`), await card("Hex", CARD.title, CARD.line, `${origin.replace("https://", "")}/changelog`))

const sitemap = join(assets, "sitemap.xml")
const url = `  <url><loc>${origin}${CHANGELOG_PATH.slice("/hex".length)}</loc></url>`
writeFileSync(sitemap, readFileSync(sitemap, "utf8").replace("</urlset>", `${url}\n</urlset>`))
console.log(`Changelog: ${releases.length} releases, newest ${releases[0].version}.`)
