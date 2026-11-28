// Writes the MCP page into the built site: mcp.html (/hex/mcp), its social
// card, and the page in the sitemap. The page is static: the copy, the setup,
// the tool list, and its JSON-LD are all in the HTML before JavaScript runs.
//
//   bun scripts/mcp.ts --mode dev
import { getWorkerAssetsDir } from "@cloudflare/build-output-utils"
import { readFileSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { parseArgs } from "node:util"
import { canonicalOriginForMode } from "../src/base"
import { MCP_CARD_PATH, MCP_PATH, renderMcp } from "../src/mcp"
import data from "../src/release-data.json"
import { card } from "./card"

const { values } = parseArgs({ options: { mode: { type: "string" }, out: { type: "string" } } })
const origin = canonicalOriginForMode(values.mode)
const root = resolve(import.meta.dirname, "..")
const assets = values.out ?? getWorkerAssetsDir(root)
const site = (path: string) => join(assets, path.slice("/hex/".length))

writeFileSync(site(`${MCP_PATH}.html`), renderMcp(origin).replaceAll("__HEX_VERSION__", data.version))
writeFileSync(site(MCP_CARD_PATH), await card("Hex MCP", "Your agent can read your meetings.", "Local transcripts, chapters, and notes over stdio. Read-only.", `${origin.replace("https://", "")}/mcp`))

// The sitemap's committed copy names the page; add it only when it does not,
// so a rebuild never lists the same URL twice.
const sitemap = join(assets, "sitemap.xml")
const sitemapText = readFileSync(sitemap, "utf8")
const loc = `<loc>${origin}${MCP_PATH.slice("/hex".length)}</loc>`
if (!sitemapText.includes(loc)) writeFileSync(sitemap, sitemapText.replace("</urlset>", `  <url>${loc}</url>\n</urlset>`))
console.log("MCP: /hex/mcp and its card.")
