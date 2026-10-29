// Writes src/release-data.json, the release the download buttons describe,
// and copies the installers into public/. The buttons link the site's own
// /hex/download/<target> routes (worker.ts), which redirect at request time
// to the stable release once anoma.ly/release has one and to the newest dev
// build until then, so links never go stale and need no switch-over. This
// only records which channel and version were current at build time; the
// page refreshes the label from the live manifest (src/release.ts).
//
//   bun scripts/release-data.ts [--mode dev]
import { copyFileSync, writeFileSync } from "node:fs"
import { parseArgs } from "node:util"
import { originForMode, RELEASE_SERVICE } from "../src/base"

const { values } = parseArgs({ options: { mode: { type: "string", default: "production" } } })
const site = originForMode(values.mode)
const root = new URL("..", import.meta.url)

type Manifest = { app: string; artifact: string; channel: string; release: { version: string } }

async function manifest(channel: "stable" | "dev"): Promise<Manifest | undefined> {
  const url = `${RELEASE_SERVICE}/hex/desktop/${channel}.json`
  const response = await fetch(url, { headers: { "User-Agent": "hex-site" } })
  if (response.status === 404) return undefined
  if (!response.ok) throw new Error(`${url} answered ${response.status}`)
  const value = (await response.json()) as Manifest
  if (value.app !== "hex" || value.artifact !== "desktop" || value.channel !== channel) throw new Error(`not the Hex desktop ${channel} manifest`)
  return value
}

const stable = await manifest("stable")
const current = stable ?? (await manifest("dev"))
if (!current) throw new Error("anoma.ly/release has no Hex release on stable or dev")
const channel = stable ? "stable" : "dev"
const download = (target: string) => `${site}/download/${target}`
const data = {
  channel,
  version: current.release.version,
  mac: download("macos-arm64-dmg"),
  windowsX64: download("windows-x64-setup"),
  windowsArm64: download("windows-arm64-setup"),
  linuxInstaller: `${site}/install-linux.sh`,
  linuxCommand: `curl -fsSL ${site}/install-linux.sh | sh`,
  linuxX64: download("linux-x64"),
  linuxArm64: download("linux-arm64"),
  debX64: download("linux-x64-deb"),
  debArm64: download("linux-arm64-deb"),
  rpmX64: download("linux-x64-rpm"),
  rpmArm64: download("linux-arm64-rpm"),
  windowsInstaller: `${site}/install-windows.ps1`,
  github: stable ? `https://github.com/anomalyco/hex/releases/tag/v${current.release.version}` : "https://github.com/anomalyco/hex/releases",
}
writeFileSync(new URL("src/release-data.json", root), `${JSON.stringify(data, null, 2)}\n`)
copyFileSync(new URL("../.github/release/install-linux.sh", root), new URL("public/install-linux.sh", root))
copyFileSync(new URL("../.github/release/install-windows.ps1", root), new URL("public/install-windows.ps1", root))
console.log(`Download buttons describe Hex ${data.version} (${channel}).`)
