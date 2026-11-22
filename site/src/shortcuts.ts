import type { Platform } from "./platform"

export type Os = "mac" | "windows" | "linux"

/**
 * One shortcut per OS: the words the hero's paragraph names and the lowercase
 * legend its keycap wears. Both read this table, so the copy can never describe
 * one key while the key graphic shows another (#2330).
 */
export interface Shortcut {
  // How the hero's "Hold …" line names it, e.g. "Alt+Space".
  copy: string
  // The keycap's legend; a chord's later keys are drawn smaller beside the first.
  legend: string
}

export const OS_SHORTCUTS: Record<Os, Shortcut> = {
  mac: { copy: "Option", legend: "option" },
  windows: { copy: "Alt", legend: "alt" },
  // Alt+Space is the default; GNOME's window menu and KDE Plasma's KRunner keep
  // it, so new setups there dictate with Ctrl+Alt+Space (docs/linux.md). The
  // keycap wears the default's chord: it has one key to put a legend on.
  linux: { copy: "Alt+Space or Ctrl+Alt+Space on GNOME and KDE", legend: "alt + space" },
}

export const OS_NAMES: Record<Os, string> = {
  mac: "Mac",
  windows: "Windows",
  linux: "Linux",
}

// The same names and shortcuts keyed by the platform the visit looks like. Only
// a phone reads differently: it has no dictation shortcut of its own.
export const PLATFORM_NAMES: Record<Platform, string> = {
  mac: OS_NAMES.mac,
  "mac-intel": OS_NAMES.mac,
  "windows-x64": OS_NAMES.windows,
  "windows-arm64": OS_NAMES.windows,
  linux: OS_NAMES.linux,
  "linux-arm64": OS_NAMES.linux,
  mobile: "Mac, Windows, and Linux",
}

export const SHORTCUTS: Record<Platform, string> = {
  mac: OS_SHORTCUTS.mac.copy,
  "mac-intel": OS_SHORTCUTS.mac.copy,
  "windows-x64": OS_SHORTCUTS.windows.copy,
  "windows-arm64": OS_SHORTCUTS.windows.copy,
  linux: OS_SHORTCUTS.linux.copy,
  "linux-arm64": OS_SHORTCUTS.linux.copy,
  mobile: "a shortcut",
}

export function osOf(platform: Platform): Os {
  if (platform.startsWith("windows")) return "windows"
  if (platform.startsWith("linux")) return "linux"
  return "mac"
}
