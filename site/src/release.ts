// The release every download link names. scripts/release-data.ts writes
// release-data.json from the Anomaly release service's stable manifest at
// build time, so a deploy after a release picks up its version and files;
// `bun run check:downloads` fails unless every URL below serves a file.
import data from "./release-data.json"

export const MAC_VERSION = data.version
export const DESKTOP_VERSION = data.version

export const RELEASE = {
  mac: { version: data.version, url: data.mac },
  windowsX64: { version: data.version, url: data.windowsX64 },
  windowsArm64: { version: data.version, url: data.windowsArm64 },
  linux: { version: data.version, installer: data.linuxInstaller, command: data.linuxCommand },
  github: data.github,
  homebrew: "brew install --cask anomalyco/tap/hex",
} as const

export const DOWNLOAD_URLS = [
  RELEASE.mac.url,
  RELEASE.windowsX64.url,
  RELEASE.windowsArm64.url,
  RELEASE.linux.installer,
  data.manifest,
  RELEASE.github,
]
