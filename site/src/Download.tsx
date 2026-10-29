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

function Button({ href, mark, children }: { href: string; mark: ReactNode; children: ReactNode }) {
  return (
    <m.a className="download" href={href} whileHover={{ y: -1 }} whileTap={{ scale: 0.985 }}>
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
type PerArch = Record<Arch, string>

interface Method {
  id: string
  label: string
  file?: string | PerArch
  command?: string | PerArch
}

const OSES: { id: Os; label: string; mark: ReactNode; button: string }[] = [
  { id: "mac", label: "macOS", mark: <AppleMark />, button: "Download for Mac" },
  { id: "windows", label: "Windows", mark: <WindowsMark />, button: "Download for Windows" },
  { id: "linux", label: "Linux", mark: <LinuxMark />, button: "Download for Linux" },
]

function methods(os: Os): Method[] {
  const linux = RELEASE.linux
  const all: (Method | false | undefined)[] =
    os === "mac"
      ? [
          { id: "dmg", label: "Download", file: RELEASE.mac.url },
          { id: "homebrew", label: "Homebrew", command: RELEASE.homebrew },
        ]
      : os === "windows"
        ? [
            { id: "installer", label: "Installer", file: { x64: RELEASE.windowsX64.url, arm64: RELEASE.windowsArm64.url } },
            { id: "powershell", label: "PowerShell", command: RELEASE.windows.command },
          ]
        : [
            { id: "script", label: "Script", command: linux.command },
            linux.aur !== undefined && { id: "aur", label: "AUR", command: linux.aur },
            { id: "deb", label: "Debian", command: linux.deb },
            { id: "rpm", label: "Fedora", command: linux.rpm },
            { id: "nix", label: "Nix", command: RELEASE.nix },
            { id: "binary", label: "Binary", file: linux.binary },
          ]
  return all.filter((method): method is Method => Boolean(method))
}

const pick = (value: string | PerArch, arch: Arch) => (typeof value === "string" ? value : value[arch])
const perArch = (method: Method) => typeof method.file === "object" || typeof method.command === "object"

function osOf(platform: Platform): Os {
  if (platform.startsWith("windows")) return "windows"
  if (platform.startsWith("linux")) return "linux"
  return "mac"
}

const archOf = (platform: Platform): Arch => (platform.endsWith("arm64") ? "arm64" : "x64")

function Tab({ selected, onSelect, children }: { selected: boolean; onSelect: () => void; children: ReactNode }) {
  return (
    <button type="button" role="tab" aria-selected={selected} onClick={onSelect}>
      {children}
    </button>
  )
}

export default function Download({ platform }: { platform: Platform }) {
  // The visitor's choices; until they pick, the detected platform decides.
  const [chosenOs, setOs] = useState<Os>()
  const [chosenArch, setArch] = useState<Arch>()
  const [chosenMethod, setMethod] = useState<Partial<Record<Os, string>>>({})
  const os = chosenOs ?? osOf(platform)
  const arch = chosenArch ?? archOf(platform)
  const options = methods(os)
  const method = options.find((option) => option.id === chosenMethod[os]) ?? options[0]
  const name = OSES.find((entry) => entry.id === os)!
  const intelMac = platform === "mac-intel" && os === "mac"

  return (
    <div className="hero__action install" data-avoid>
      <div className="install__os" role="tablist" aria-label="Operating system">
        {OSES.map((entry) => (
          <Tab key={entry.id} selected={entry.id === os} onSelect={() => setOs(entry.id)}>
            {entry.mark}
            <span>{entry.label}</span>
          </Tab>
        ))}
      </div>

      <div className="install__methods">
        <div role="tablist" aria-label={`Ways to install on ${name.label}`}>
          {options.map((option) => (
            <Tab
              key={option.id}
              selected={option.id === method.id}
              onSelect={() => setMethod((current) => ({ ...current, [os]: option.id }))}
            >
              {option.label}
            </Tab>
          ))}
        </div>
        {perArch(method) && (
          <div className="install__arch" role="tablist" aria-label="Architecture">
            <Tab selected={arch === "x64"} onSelect={() => setArch("x64")}>
              x64
            </Tab>
            <Tab selected={arch === "arm64"} onSelect={() => setArch("arm64")}>
              ARM64
            </Tab>
          </div>
        )}
      </div>

      {method.file !== undefined &&
        (intelMac ? (
          <span className="download download--disabled" aria-disabled="true">
            {name.mark}
            <span>Requires Apple silicon</span>
            <span aria-hidden="true" />
          </span>
        ) : (
          <Button href={pick(method.file, arch)} mark={name.mark}>
            {name.button}
          </Button>
        ))}
      {method.command !== undefined && (
        <Command text={pick(method.command, arch)} label={`${method.label} install command for ${name.label}`} />
      )}
    </div>
  )
}
