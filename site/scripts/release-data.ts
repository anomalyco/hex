// Writes src/release-data.json, the release every download link names, from
// the Anomaly release service's stable manifest (anoma.ly/release, for both
// site environments), and copies the
// installers into public/. Until the service has a stable Hex release, the
// links name the frozen 2.3.1 downloads.
//
//   bun scripts/release-data.ts [--mode dev]
import { copyFileSync, writeFileSync } from "node:fs"
import { parseArgs } from "node:util"
import { originForMode, RELEASE_SERVICE } from "../src/base"

const { values } = parseArgs({ options: { mode: { type: "string", default: "production" } } })
const service = RELEASE_SERVICE
const site = originForMode(values.mode)
const root = new URL("..", import.meta.url)
const manifestUrl = `${service}/hex/desktop/stable.json`

type Target = { url: string; size: number; sha256: string }
type Manifest = { app: string; artifact: string; channel: string; release: { version: string; targets: Record<string, Target> } }

async function manifest(): Promise<Manifest | undefined> {
  const response = await fetch(manifestUrl, { headers: { "User-Agent": "hex-site" } })
  if (response.status === 404) return undefined
  if (!response.ok) throw new Error(`${manifestUrl} answered ${response.status}`)
  const value = (await response.json()) as Manifest
  if (value.app !== "hex" || value.artifact !== "desktop" || value.channel !== "stable") throw new Error("not the Hex desktop stable manifest")
  return value
}

// Target URLs resolve against the manifest's URL (anomalyco/release docs/protocol.md).
// A dev release without a platform's build (macOS before the Apple secrets
// exist) keeps that platform's frozen 2.3.1 link; production needs them all.
function fileUrl(value: Manifest, target: string, fallback: string) {
  const entry = value.release.targets[target]
  if (entry) return new URL(entry.url, manifestUrl).toString()
  if (values.mode !== "dev") throw new Error(`Hex ${value.release.version} has no ${target} build`)
  console.warn(`Hex ${value.release.version} on ${service} has no ${target} build; linking ${fallback}.`)
  return fallback
}

const legacy = "https://anoma.ly/hex/files"
const current = await manifest()
const data = current
  ? {
      version: current.release.version,
      mac: fileUrl(current, "macos-arm64-dmg", `${legacy}/releases/HEX-2.3.1-arm64.dmg`),
      windowsX64: fileUrl(current, "windows-x64-setup", `${legacy}/releases/HEX-Setup-2.3.1-x64.exe`),
      windowsArm64: fileUrl(current, "windows-arm64-setup", `${legacy}/releases/HEX-Setup-2.3.1-arm64.exe`),
      linuxInstaller: `${site}/install-linux.sh`,
      linuxCommand: `curl -fsSL ${site}/install-linux.sh | sh`,
      github: `https://github.com/anomalyco/hex/releases/tag/v${current.release.version}`,
      manifest: manifestUrl,
    }
  : {
      version: "2.3.1",
      mac: `${legacy}/releases/HEX-2.3.1-arm64.dmg`,
      windowsX64: `${legacy}/releases/HEX-Setup-2.3.1-x64.exe`,
      windowsArm64: `${legacy}/releases/HEX-Setup-2.3.1-arm64.exe`,
      linuxInstaller: `${legacy}/install-linux.sh`,
      linuxCommand: `curl -fsSL ${legacy}/install-linux.sh | sh`,
      github: "https://github.com/anomalyco/hex/releases/tag/app-v2.3.1",
      manifest: `${legacy}/linux-update.json`,
    }
writeFileSync(new URL("src/release-data.json", root), `${JSON.stringify(data, null, 2)}\n`)
copyFileSync(new URL("../scripts/install-linux-release.sh", root), new URL("public/install-linux.sh", root))
copyFileSync(new URL("../scripts/install-windows.ps1", root), new URL("public/install-windows.ps1", root))
console.log(`Download links name Hex ${data.version}${current ? ` from ${service}` : " (no release on the service yet)"}.`)
