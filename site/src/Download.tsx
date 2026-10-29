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

function ShareMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="mark mark--stroke">
      <path d="M12 15V4M8 8l4-4 4 4M6 12v7h12v-7" />
    </svg>
  )
}

const SEP = "\u2002\u00b7\u2002"

export const PLATFORM_NAMES: Record<Platform, string> = {
  mac: "Mac",
  "mac-intel": "Mac",
  "windows-x64": "Windows",
  "windows-arm64": "Windows",
  linux: "Linux",
  mobile: "Mac, Windows, and Linux",
}

export const SHORTCUTS: Record<Platform, string> = {
  mac: "Option",
  "mac-intel": "Option",
  "windows-x64": "Ctrl+Space",
  "windows-arm64": "Ctrl+Space",
  linux: "Alt+Space",
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

function SendLink() {
  const [copied, copy] = useCopy()
  const send = () => {
    const url = location.origin + import.meta.env.BASE_URL
    if (navigator.share) {
      void navigator.share({ title: "Hex", text: "Voice dictation for Mac, Windows, and Linux", url }).catch(() => {})
    } else {
      copy(url)
    }
  }
  return (
    <m.button type="button" className="download" onClick={send} whileTap={{ scale: 0.985 }}>
      <ShareMark />
      <span>{copied ? "Link copied" : "Send yourself the link"}</span>
      <span aria-hidden="true" />
    </m.button>
  )
}

const WINDOWS_NOTE =
  "The installer isn\u2019t code-signed yet, so Windows may say it protected your PC. Choose More info, then Run anyway."

function Primary({ platform }: { platform: Platform }) {
  switch (platform) {
    case "mac":
      return (
        <>
          <Button href={RELEASE.mac.url} mark={<AppleMark />}>
            Download for Mac
          </Button>
          <p className="requirements">
            Version {RELEASE.mac.version}
            {SEP}Apple silicon{SEP}macOS 15 or later
          </p>
        </>
      )
    case "mac-intel":
      return (
        <>
          <span className="download download--disabled" aria-disabled="true">
            <AppleMark />
            <span>Download for Mac</span>
            <span aria-hidden="true" />
          </span>
          <p className="requirements">Hex needs a Mac with Apple silicon and macOS 15 or later.</p>
        </>
      )
    case "windows-x64":
    case "windows-arm64": {
      const arm = platform === "windows-arm64"
      const build = arm ? RELEASE.windowsArm64 : RELEASE.windowsX64
      return (
        <>
          <Button href={build.url} mark={<WindowsMark />}>
            Download for Windows
          </Button>
          <p className="requirements">
            Version {build.version}
            {SEP}Windows 10 or 11{SEP}
            {arm ? "ARM64" : "x64"}
            {SEP}beta
          </p>
          <p className="download-note">{WINDOWS_NOTE}</p>
        </>
      )
    }
    case "linux":
      return (
        <>
          <Command text={RELEASE.linux.command} label="Install command for Linux" />
          <p className="requirements">
            Version {RELEASE.linux.version}
            {SEP}x86_64 Arch Linux{SEP}glibc 2.44 or later{SEP}beta
          </p>
          <p className="download-note">
            Paste it into a terminal. The installer checks the signed release, needs no root, and adds Hex to your app
            launcher.
          </p>
        </>
      )
    case "mobile":
      return (
        <>
          <SendLink />
          <p className="requirements">Hex runs on Mac, Windows, and Linux computers.</p>
        </>
      )
  }
}

interface Row {
  key: string
  mark: ReactNode
  name: string
  detail: string
  href?: string
  command?: string
}

function rows(platform: Platform): Row[] {
  const all: (Row & { platforms: Platform[] })[] = [
    {
      key: "mac",
      platforms: ["mac", "mac-intel"],
      mark: <AppleMark />,
      name: "Mac",
      detail: "Apple silicon, macOS 15 or later",
      href: RELEASE.mac.url,
    },
    {
      key: "homebrew",
      platforms: [],
      mark: <AppleMark />,
      name: "Homebrew",
      detail: "Mac",
      command: RELEASE.homebrew,
    },
    {
      key: "windows-x64",
      platforms: ["windows-x64"],
      mark: <WindowsMark />,
      name: "Windows x64",
      detail: "Needs a GPU driver with Vulkan",
      href: RELEASE.windowsX64.url,
    },
    {
      key: "windows-arm64",
      platforms: ["windows-arm64"],
      mark: <WindowsMark />,
      name: "Windows ARM64",
      detail: "Snapdragon PCs",
      href: RELEASE.windowsArm64.url,
    },
    {
      key: "linux",
      platforms: ["linux"],
      mark: <LinuxMark />,
      name: "Linux",
      detail: "x86_64 Arch, glibc 2.44 or later",
      command: RELEASE.linux.command,
    },
  ]
  return all.filter((row) => !row.platforms.includes(platform))
}

export default function Download({ platform }: { platform: Platform }) {
  return (
    <div className="hero__action" data-avoid>
      <Primary platform={platform} />
      <details className="platforms">
        <summary>Other platforms</summary>
        <ul>
          {rows(platform).map((row) => (
            <li key={row.key} className="platform">
              {row.mark}
              <span className="platform__name">{row.name}</span>
              <span className="platform__detail">{row.detail}</span>
              {row.href && (
                <a className="platform__link" href={row.href}>
                  Download
                </a>
              )}
              {row.command && <Command text={row.command} label={`Install command for ${row.name}`} />}
            </li>
          ))}
          <li className="platform platform--github">
            <a href={RELEASE.github}>Every {RELEASE.mac.version} download on GitHub</a>
          </li>
        </ul>
      </details>
    </div>
  )
}
