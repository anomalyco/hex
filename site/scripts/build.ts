#!/usr/bin/env bun
// `bun run build [--mode dev]`: release data from the release service, typecheck, client build
// (Cloudflare Build Output), SSR build, then prerender.
import { parseArgs } from "node:util"
import { $ } from "bun"

const { values } = parseArgs({ options: { mode: { type: "string", default: "production" } } })
const mode = values.mode

await $`bun scripts/release-data.ts --mode ${mode}`
await $`tsc -b`
await $`vite build --mode ${mode}`
await $`vite build --mode ${mode} --ssr src/prerender.tsx --outDir dist-server`
await $`bun scripts/prerender.ts --mode ${mode}`
