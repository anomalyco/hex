#!/usr/bin/env bun
// Replaces the Meetings showcase in public/meetings/ with a new cut of the
// standalone page, copying only the files it loads (scripts/meetings/bundle.ts)
// and keeping the social card. Build tools, shader sources, reports, and raw
// audio stay behind. The page's social tags come from the build
// (scripts/meetings/build.ts), so the bundle's own are dropped here.
//
//   bun run import:meetings ~/code/home/artifacts/<date>-hex-meetings-v15
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import { MEETINGS_CARD, runtimeFiles, stripHead } from "./bundle"

const source = process.argv[2]
if (!source || !existsSync(join(source, "index.html"))) throw new Error("usage: bun run import:meetings <bundle folder with index.html>")
const dest = resolve(import.meta.dirname, "../../public/meetings")

const bundle = runtimeFiles(source)
if (bundle.missing.length) {
  throw new Error(`The bundle names files it doesn't have:\n${bundle.missing.map((m) => `  ${m.from}: ${m.ref}`).join("\n")}`)
}

mkdirSync(dest, { recursive: true })
for (const name of readdirSync(dest)) if (name !== MEETINGS_CARD) rmSync(join(dest, name), { recursive: true, force: true })
let bytes = 0
for (const file of bundle.files) {
  mkdirSync(dirname(join(dest, file)), { recursive: true })
  copyFileSync(join(source, file), join(dest, file))
  bytes += statSync(join(dest, file)).size
}

// Session ids and the bundle's own social and canonical tags never ship.
const index = join(dest, "index.html")
writeFileSync(index, stripHead(readFileSync(index, "utf8")))
console.log(`Meetings: ${bundle.files.length} files, ${(bytes / 1024 / 1024).toFixed(2)} MiB from ${source}`)
