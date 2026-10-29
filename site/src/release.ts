// The files and commands the install switcher offers. scripts/release-data.ts
// writes release-data.json at build time; file links name /hex/download/<target>,
// which the Worker redirects to stable once it exists and to the newest dev
// build until then.
// `bun run check:downloads` fails unless every URL below serves a file.
import data from "./release-data.json"

export type Channel = "stable" | "dev"

const deb = (url: string) => `curl -fLo /tmp/hex.deb ${url} && sudo apt install /tmp/hex.deb`
const rpm = (url: string) => `curl -fLo /tmp/hex.rpm ${url} && sudo dnf install /tmp/hex.rpm`

export const RELEASE = {
  channel: data.channel as Channel,
  mac: { url: data.mac },
  windowsX64: { url: data.windowsX64 },
  windowsArm64: { url: data.windowsArm64 },
  windows: {
    installer: data.windowsInstaller,
    command: `powershell -ExecutionPolicy Bypass -Command "iwr ${data.windowsInstaller} -OutFile install-windows.ps1; ./install-windows.ps1"`,
  },
  linux: {
    installer: data.linuxInstaller,
    command: data.linuxCommand,
    binary: { x64: data.linuxX64, arm64: data.linuxArm64 },
    deb: { x64: deb(data.debX64), arm64: deb(data.debArm64) },
    rpm: { x64: rpm(data.rpmX64), arm64: rpm(data.rpmArm64) },
    // The AUR package is pushed by the first stable release.
    aur: data.channel === "stable" ? "yay -S hex-bin" : undefined,
  },
  github: data.github,
  homebrew: "brew install --cask anomalyco/tap/hex",
  nix: "nix profile install github:anomalyco/hex",
} as const

export const DOWNLOAD_URLS = [
  RELEASE.mac.url,
  RELEASE.windowsX64.url,
  RELEASE.windowsArm64.url,
  RELEASE.linux.installer,
  RELEASE.windows.installer,
  data.linuxX64,
  data.linuxArm64,
  data.debX64,
  data.debArm64,
  data.rpmX64,
  data.rpmArm64,
]
