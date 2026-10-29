export type Platform = "mac" | "mac-intel" | "windows-x64" | "windows-arm64" | "linux" | "linux-arm64" | "mobile"

const PLATFORMS: Platform[] = ["mac", "mac-intel", "windows-x64", "windows-arm64", "linux", "linux-arm64", "mobile"]

interface UserAgentData {
  platform?: string
  mobile?: boolean
  getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string; bitness?: string }>
}

function userAgentData(): UserAgentData | undefined {
  return (navigator as Navigator & { userAgentData?: UserAgentData }).userAgentData
}

function override(): Platform | undefined {
  const value = new URLSearchParams(location.search).get("platform")
  return PLATFORMS.find((platform) => platform === value)
}

/** A synchronous first guess from the user agent; `?platform=` overrides it. */
export function guessPlatform(): Platform {
  const forced = override()
  if (forced) return forced
  const agent = navigator.userAgent
  const data = userAgentData()
  const hint = data?.platform ?? ""
  if (/iPhone|iPod|iPad|Android/.test(agent) || data?.mobile) return "mobile"
  // iPadOS asks for the desktop site and reports itself as a Mac.
  if (/Macintosh/.test(agent) && navigator.maxTouchPoints > 1) return "mobile"
  if (/Windows/.test(agent) || hint === "Windows") {
    return /\bARM64\b|\baarch64\b/i.test(agent) ? "windows-arm64" : "windows-x64"
  }
  if (/Mac OS X|Macintosh/.test(agent) || hint === "macOS") return "mac"
  if (/Linux|X11|CrOS/.test(agent) || hint === "Linux") return /\baarch64\b|\barmv8/i.test(agent) ? "linux-arm64" : "linux"
  return "mac"
}

/**
 * Chromium browsers report the real CPU architecture through UA client hints,
 * which tells ARM64 Windows and Linux and Intel Macs apart. Other browsers keep the guess.
 */
export async function refinePlatform(guess: Platform): Promise<Platform> {
  if (override()) return guess
  const data = userAgentData()
  if (!data?.getHighEntropyValues) return guess
  try {
    const { architecture } = await data.getHighEntropyValues(["architecture", "bitness"])
    if (guess.startsWith("windows") && architecture) {
      return architecture === "arm" ? "windows-arm64" : "windows-x64"
    }
    if (guess.startsWith("linux") && architecture) {
      return architecture === "arm" ? "linux-arm64" : "linux"
    }
    if (guess === "mac" && architecture === "x86") return "mac-intel"
  } catch {
    // Hints are optional; the first guess stands.
  }
  return guess
}
