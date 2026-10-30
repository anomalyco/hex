import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react"
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

type Os = "mac" | "windows" | "linux"
type Arch = "x64" | "arm64"

interface Row {
  label: string
  note?: string
  file?: string
  command?: string
}

const OSES: { id: Os; label: string; mark: ReactNode }[] = [
  { id: "mac", label: "macOS", mark: <AppleMark /> },
  { id: "windows", label: "Windows", mark: <WindowsMark /> },
  { id: "linux", label: "Linux", mark: <LinuxMark /> },
]

function rows(os: Os, arch: Arch): Row[] {
  const linux = RELEASE.linux
  const all: (Row | false | undefined)[] =
    os === "mac"
      ? [
          { label: "Download", note: "Apple silicon", file: RELEASE.mac.url },
          { label: "Homebrew", command: RELEASE.homebrew },
        ]
      : os === "windows"
        ? [
            { label: "Installer", note: "x64", file: RELEASE.windowsX64.url },
            { label: "Installer", note: "ARM64", file: RELEASE.windowsArm64.url },
            { label: "PowerShell", command: RELEASE.windows.command },
          ]
        : [
            { label: "Script", command: linux.command },
            linux.aur !== undefined && { label: "AUR", command: linux.aur },
            { label: "Debian", command: linux.deb[arch] },
            { label: "Fedora", command: linux.rpm[arch] },
            { label: "Nix", command: RELEASE.nix },
            { label: "Binary", note: arch === "arm64" ? "ARM64" : "x64", file: linux.binary[arch] },
          ]
  return all.filter((row): row is Row => Boolean(row))
}

function osOf(platform: Platform): Os {
  if (platform.startsWith("windows")) return "windows"
  if (platform.startsWith("linux")) return "linux"
  return "mac"
}

const archOf = (platform: Platform): Arch => (platform.endsWith("arm64") ? "arm64" : "x64")

const ICON = { viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round", strokeLinejoin: "round" } as const

function CopyIcon() {
  return (
    <svg aria-hidden="true" className="stack__icon" {...ICON}>
      <rect x="8.5" y="8.5" width="11" height="11" rx="2" />
      <path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2" />
    </svg>
  )
}

function CheckIcon() {
  return (
    <svg aria-hidden="true" className="stack__icon" {...ICON}>
      <path d="m5 12.5 4.5 4.5L19 7.5" />
    </svg>
  )
}

function DownloadIcon() {
  return (
    <svg aria-hidden="true" className="stack__icon" {...ICON}>
      <path d="M12 4.5v11M7 11l5 5 5-5M5 19.5h14" />
    </svg>
  )
}

function StackRow({ row }: { row: Row }) {
  const [copied, copy] = useCopy()
  return (
    <div className="stack__row">
      <span className="stack__label">{row.label}</span>
      <span className="stack__value">
        {row.command !== undefined ? <code title={row.command}>{row.command}</code> : <span>{row.note}</span>}
      </span>
      {row.command !== undefined ? (
        <button
          type="button"
          className="stack__action"
          onClick={() => copy(row.command!)}
          aria-label={copied ? "Copied" : `Copy ${row.label} command`}
          title={copied ? "Copied" : "Copy"}
        >
          {copied ? <CheckIcon /> : <CopyIcon />}
        </button>
      ) : (
        <a
          className="stack__action stack__action--primary"
          href={row.file}
          aria-label={`Download ${row.label}${row.note ? ` (${row.note})` : ""}`}
          title="Download"
        >
          <DownloadIcon />
        </a>
      )}
    </div>
  )
}

// Places the selected-tab pill (`.install__os::before`) under the chosen tab.
// Until the first measurement the selected tab paints its own background, so
// the prerendered page and a page without scripts still show the selection.
// The pill slides only after a click (`data-animate`), so the OS picked from
// the user agent on load doesn't animate in from macOS.
function useTabIndicator(list: RefObject<HTMLDivElement | null>, selected: Os) {
  useLayoutEffect(() => {
    const element = list.current
    if (!element) return
    const place = () => {
      const tab = element.querySelector<HTMLElement>('[aria-selected="true"]')
      if (!tab) return
      element.style.setProperty("--tab-x", `${tab.offsetLeft}px`)
      element.style.setProperty("--tab-width", `${tab.offsetWidth}px`)
      element.dataset.measured = ""
    }
    place()
    // Tab widths change when Inter finishes loading or the viewport resizes.
    const observer = new ResizeObserver(place)
    observer.observe(element)
    return () => observer.disconnect()
  }, [list, selected])
}

export default function Download({ platform }: { platform: Platform }) {
  const [chosenOs, setOs] = useState<Os>()
  const os = chosenOs ?? osOf(platform)
  const arch = archOf(platform)
  const tabs = useRef<HTMLDivElement>(null)
  useTabIndicator(tabs, os)

  return (
    <div className="hero__action install" data-avoid>
      <div className="install__os" role="tablist" aria-label="Operating system" ref={tabs}>
        {OSES.map((entry) => (
          <button
            key={entry.id}
            id={`install-tab-${entry.id}`}
            type="button"
            role="tab"
            aria-selected={entry.id === os}
            aria-controls={`install-panel-${entry.id}`}
            onClick={() => {
              if (tabs.current) tabs.current.dataset.animate = ""
              setOs(entry.id)
            }}
          >
            {entry.mark}
            <span>{entry.label}</span>
          </button>
        ))}
      </div>
      {/* Every OS's stack shares one grid cell, so the tallest reserves the
          height and switching tabs never moves the page. */}
      <div className="install__panels">
        {OSES.map((entry) => (
          <div
            key={entry.id}
            id={`install-panel-${entry.id}`}
            className="stack"
            role="tabpanel"
            aria-labelledby={`install-tab-${entry.id}`}
            data-active={entry.id === os ? "" : undefined}
          >
            {rows(entry.id, arch).map((row) => (
              <StackRow key={`${row.label}-${row.note ?? ""}`} row={row} />
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}
