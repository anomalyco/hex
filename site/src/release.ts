// The release the download buttons describe. scripts/release-data.ts writes
// release-data.json at build time; the buttons link /hex/download/<target>,
// which the Worker redirects to stable once it exists and to the newest dev
// build until then. useRelease() refreshes the channel and version label from
// the live manifests, so the page never shows a stale build.
// `bun run check:downloads` fails unless every URL below serves a file.
import { useEffect, useState } from "react"
import { RELEASE_SERVICE } from "./base"
import data from "./release-data.json"

export type Channel = "stable" | "dev"

export const RELEASE = {
  mac: { url: data.mac },
  windowsX64: { url: data.windowsX64 },
  windowsArm64: { url: data.windowsArm64 },
  linux: { installer: data.linuxInstaller, command: data.linuxCommand },
  github: data.github,
  homebrew: "brew install --cask anomalyco/tap/hex",
} as const

export const DOWNLOAD_URLS = [RELEASE.mac.url, RELEASE.windowsX64.url, RELEASE.windowsArm64.url, RELEASE.linux.installer]

export type Current = { channel: Channel; version: string }

async function live(channel: Channel): Promise<string | undefined> {
  const response = await fetch(`${RELEASE_SERVICE}/hex/desktop/${channel}.json`, { cache: "no-cache" })
  if (!response.ok) return undefined
  const value = (await response.json()) as { channel?: string; release?: { version?: string } }
  return value.channel === channel ? value.release?.version : undefined
}

export function useRelease(): Current {
  const [current, setCurrent] = useState<Current>({ channel: data.channel as Channel, version: data.version })
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      const stable = await live("stable")
      const next: Current | undefined = stable
        ? { channel: "stable", version: stable }
        : await live("dev").then((version) => (version ? { channel: "dev", version } : undefined))
      if (next && !cancelled) setCurrent(next)
    })().catch(() => {})
    return () => {
      cancelled = true
    }
  }, [])
  return current
}

/** "Version 2.4.0", or "Dev build 0.0.0-dev-236" before the first stable release. */
export function versionLabel({ channel, version }: Current): string {
  return channel === "stable" ? `Version ${version}` : `Dev build ${version}`
}
