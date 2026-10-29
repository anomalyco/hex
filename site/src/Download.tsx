import * as m from "motion/react-m"
import { useEffect, useState, type ReactNode } from "react"
import type { Platform } from "./platform"
import { RELEASE } from "./release"

export function AppleMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="mark">
      <path d="M16.68 12.9c.02 2.3 2.02 3.06 2.04 3.07-.02.06-.32 1.1-1.05 2.17-.63.92-1.29 1.84-2.32 1.86-1.01.02-1.34-.6-2.5-.6-1.15 0-1.51.58-2.47.62-.99.04-1.74-.99-2.38-1.91-1.3-1.88-2.3-5.32-.96-7.65a3.7 3.7 0 0 1 3.15-1.91c.98-.02 1.91.66 2.5.66.6 0 1.72-.82 2.9-.7.49.02 1.88.2 2.77 1.5-.07.04-1.65.96-1.68 2.89ZM14.81 7.27c.53-.64.89-1.53.79-2.41-.76.03-1.68.51-2.23 1.15-.49.57-.92 1.47-.8 2.34.85.07 1.71-.43 2.24-1.08Z" />
    </svg>
  )
}

export function WindowsMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="mark">
      <path d="M4 5.6 10.5 4.7v6.6H4zM11.4 4.6 20 3.4v7.9h-8.6zM4 12.2h6.5v6.6L4 17.9zM11.4 12.2H20v7.9l-8.6-1.2z" />
    </svg>
  )
}

export function LinuxMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="mark mark--stroke">
      <path d="M5 5.5h14v13H5zM8 9l2.5 2.5L8 14M12.5 14H16" />
    </svg>
  )
}

export const PLATFORM_NAMES: Record<Platform, string> = {
  mac: "Mac",
  "mac-intel": "Mac",
  "windows-x64": "Windows",
  "windows-arm64": "Windows",
  linux: "Linux",
  "linux-arm64": "Linux",
  mobile: "Mac, Windows, and Linux",
}

export const SHORTCUTS: Record<Platform, string> = {
  mac: "Option",
  "mac-intel": "Option",
  "windows-x64": "Ctrl+Space",
  "windows-arm64": "Ctrl+Space",
  linux: "Alt+Space",
  "linux-arm64": "Alt+Space",
  mobile: "a shortcut",
}

function useCopy(): [boolean, (text: string) => void] {
  const [copied, setCopied] = useState(false)
  useEffect(() => {
    if (!copied) return
    const timer = setTimeout(() => setCopied(false), 1600)
    return () => clearTimeout(timer)
  }, [copied])
  return [
    copied,
    (text) => {
      void navigator.clipboard?.writeText(text).then(() => setCopied(true))
    },
  ]
}

function Command({ text, label }: { text: string; label: string }) {
  const [copied, copy] = useCopy()
  return (
    <div className="command">
      <code aria-label={label}>{text}</code>
      <button type="button" className="command__copy" onClick={() => copy(text)}>
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  )
}

function Button({
  href,
  mark,
  secondary,
  children,
}: {
  href: string
  mark?: ReactNode
  secondary?: boolean
  children: ReactNode
}) {
  return (
    <m.a
      className={secondary ? `download download--secondary${mark ? "" : " download--compact"}` : "download"}
      href={href}
      whileHover={{ y: -1 }}
      whileTap={{ scale: 0.985 }}
    >
      {mark}
      <span>{children}</span>
      <span className="download__arrow" aria-hidden="true">
        {"\u2193"}
      </span>
    </m.a>
  )
}

type Os = "mac" | "windows" | "linux"
type Arch = "x64" | "arm64"

const OSES: { id: Os; label: string; mark: ReactNode }[] = [
  { id: "mac", label: "macOS", mark: <AppleMark /> },
  { id: "windows", label: "Windows", mark: <WindowsMark /> },
  { id: "linux", label: "Linux", mark: <LinuxMark /> },
]

const ARCH_NAMES: Record<Arch, string> = { x64: "x64", arm64: "ARM64" }

interface Install {
  // Download buttons: the first names the visitor's architecture.
  files: { label: string; url: string }[]
  commands: string[]
  // Linux lists its commands first: the binary is the least common way in.
  commandsFirst?: boolean
}

function install(os: Os, arch: Arch): Install {
  const other: Arch = arch === "x64" ? "arm64" : "x64"
  const pair = (name: string, urls: Record<Arch, string>) => [
    { label: `Download for ${name}`, url: urls[arch] },
    { label: ARCH_NAMES[other], url: urls[other] },
  ]
  const both = (urls: Record<Arch, string>) =>
    [arch, other].map((value) => ({ label: `Download ${ARCH_NAMES[value]}`, url: urls[value] }))
  switch (os) {
    case "mac":
      return { files: [{ label: "Download for Mac", url: RELEASE.mac.url }], commands: [RELEASE.homebrew] }
    case "windows":
      return {
        files: pair("Windows", { x64: RELEASE.windowsX64.url, arm64: RELEASE.windowsArm64.url }),
        commands: [RELEASE.windows.command],
      }
    case "linux": {
      const linux = RELEASE.linux
      return {
        files: both(linux.binary),
        commands: [linux.command, ...(linux.aur ? [linux.aur] : []), linux.deb[arch], linux.rpm[arch], RELEASE.nix],
        commandsFirst: true,
      }
    }
  }
}

function osOf(platform: Platform): Os {
  if (platform.startsWith("windows")) return "windows"
  if (platform.startsWith("linux")) return "linux"
  return "mac"
}

const archOf = (platform: Platform): Arch => (platform.endsWith("arm64") ? "arm64" : "x64")

export default function Download({ platform }: { platform: Platform }) {
  // The visitor's choice; until they pick, the detected platform decides.
  const [chosen, setChosen] = useState<Os>()
  const os = chosen ?? osOf(platform)
  const entry = OSES.find((option) => option.id === os)!
  const { files, commands, commandsFirst } = install(os, archOf(platform))
  const intelMac = platform === "mac-intel" && os === "mac"

  const buttons = (
    <div className={commandsFirst ? "install__files install__files--even" : "install__files"}>
      {intelMac ? (
        <span className="download download--disabled" aria-disabled="true">
          {entry.mark}
          <span>Requires Apple silicon</span>
          <span aria-hidden="true" />
        </span>
      ) : (
        files.map((file, index) => (
          <Button key={file.url} href={file.url} mark={index === 0 && !commandsFirst ? entry.mark : undefined} secondary={index > 0 || commandsFirst}>
            {file.label}
          </Button>
        ))
      )}
    </div>
  )
  const rows = (
    <div className="install__commands">
      {commands.map((command) => (
        <Command key={command} text={command} label={`Install command for ${entry.label}`} />
      ))}
    </div>
  )

  return (
    <div className="hero__action install" data-avoid>
      <div className="install__os" role="tablist" aria-label="Operating system">
        {OSES.map((option) => (
          <button
            key={option.id}
            type="button"
            role="tab"
            aria-selected={option.id === os}
            onClick={() => setChosen(option.id)}
          >
            {option.mark}
            <span>{option.label}</span>
          </button>
        ))}
      </div>
      {commandsFirst ? rows : buttons}
      {commandsFirst ? buttons : rows}
    </div>
  )
}
