// The published release every download link names. After a release, bump the
// versions here; `bun run check:downloads` (also run by CI) fails until every
// URL below serves a file.
export const MAC_VERSION = "2.3.0"
export const DESKTOP_VERSION = "2.3.0"

const DOWNLOADS = "https://downloads.hex.kitlangton.dev"

export const RELEASE = {
  mac: {
    version: MAC_VERSION,
    url: `${DOWNLOADS}/releases/HEX-${MAC_VERSION}-arm64.dmg`,
  },
  windowsX64: {
    version: DESKTOP_VERSION,
    url: `${DOWNLOADS}/releases/HEX-Setup-${DESKTOP_VERSION}-x64.exe`,
  },
  windowsArm64: {
    version: DESKTOP_VERSION,
    url: `${DOWNLOADS}/releases/HEX-Setup-${DESKTOP_VERSION}-arm64.exe`,
  },
  linux: {
    version: DESKTOP_VERSION,
    installer: `${DOWNLOADS}/install-linux.sh`,
    command: `curl -fsSL ${DOWNLOADS}/install-linux.sh | sh`,
  },
  github: `https://github.com/anomalyco/hex/releases/tag/app-v${DESKTOP_VERSION}`,
  homebrew: "brew install --cask anomalyco/tap/hex",
} as const

export const DOWNLOAD_URLS = [
  RELEASE.mac.url,
  RELEASE.windowsX64.url,
  RELEASE.windowsArm64.url,
  RELEASE.linux.installer,
  `${DOWNLOADS}/linux-update.json`,
  RELEASE.github,
]
