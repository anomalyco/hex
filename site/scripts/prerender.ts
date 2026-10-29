// Puts the server-rendered Mac page into the built index.html, so the copy and
// the download button are in the HTML before JavaScript runs; main.tsx hydrates
// it. Stamps the release version into the JSON-LD and this mode's origin into
// every absolute URL.
import { getWorkerAssetsDir } from "@cloudflare/build-output-utils"
import { readFileSync, rmSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { parseArgs } from "node:util"
import { render } from "../dist-server/prerender.js"
import { originForMode } from "../src/base"
import data from "../src/release-data.json"

const { values } = parseArgs({ options: { mode: { type: "string" } } })
const origin = originForMode(values.mode)
const root = resolve(import.meta.dirname, "..")
const assets = getWorkerAssetsDir(root)

const index = join(assets, "index.html")
const html = readFileSync(index, "utf8")
const marker = '<div id="root"></div>'
if (!html.includes(marker)) throw new Error(`index.html has no ${marker}`)
writeFileSync(index, html.replace(marker, `<div id="root">${render()}</div>`).replaceAll("__HEX_VERSION__", data.version))
for (const name of ["index.html", "llms.txt", "robots.txt", "sitemap.xml"]) {
  const path = join(assets, name)
  writeFileSync(path, readFileSync(path, "utf8").replaceAll("__ORIGIN__", origin))
}
rmSync(join(root, "dist-server"), { recursive: true, force: true })
