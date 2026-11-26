// Finishes the Meetings showcase in the built site (public/meetings/ copied by
// Vite): writes its canonical address and social tags for this mode's origin
// into index.html, and refuses a file Workers static assets can't serve.
//
//   bun scripts/meetings.ts --mode dev
import { getWorkerAssetsDir } from "@cloudflare/build-output-utils"
import { readFileSync, statSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { parseArgs } from "node:util"
import { originForMode } from "../src/base"
import { MEETINGS_CARD, MEETINGS_DIR, meetingsHead, runtimeFiles, stripHead } from "./meetings-bundle"

const { values } = parseArgs({ options: { mode: { type: "string" } } })
const origin = originForMode(values.mode)
const dir = join(getWorkerAssetsDir(resolve(import.meta.dirname, "..")), MEETINGS_DIR)

const bundle = runtimeFiles(dir)
if (bundle.missing.length) throw new Error(`Meetings names files it doesn't have: ${bundle.missing.map((m) => `${m.from} → ${m.ref}`).join(", ")}`)
// Workers static assets serve files up to 25 MiB.
const LIMIT = 25 * 1024 * 1024
let bytes = 0
for (const file of [...bundle.files, MEETINGS_CARD]) {
  const size = statSync(join(dir, file)).size
  if (size > LIMIT) throw new Error(`meetings/${file} is ${size} bytes, over the 25 MiB asset limit`)
  bytes += size
}

const index = join(dir, "index.html")
const html = stripHead(readFileSync(index, "utf8"))
if (!html.includes("</head>")) throw new Error("meetings/index.html has no </head>")
writeFileSync(index, html.replace("</head>", `${meetingsHead(origin)}</head>`))
console.log(`Meetings: ${bundle.files.length + 1} files, ${(bytes / 1024 / 1024).toFixed(2)} MiB.`)
