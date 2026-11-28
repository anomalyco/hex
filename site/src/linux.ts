// Hex for Linux at /hex/linux: the install commands this build's release
// offers (src/release.ts, passed in by scripts/pages.ts), how each desktop gets
// its shortcut and paste, the requirements, and Meetings on PipeWire. Every
// claim is checked against docs/linux.md and docs/features/meetings.md.
import { withBase } from "./base"
import { about, breadcrumbs, faqPage, hexApp, questions, STYLE, type Question } from "./intent"
import { escape, shell } from "./labs"
import { copy, COPY_SCRIPT } from "./mcp"

export const LINUX_PATH = withBase("/linux")
export const LINUX_CARD_PATH = withBase("/linux.png")
export const LINUX_CARD = { title: "Hex for Linux", line: "Offline voice dictation for X11 and Wayland. Free, open source, x86_64 and ARM64." }

const TITLE = "Hex for Linux: offline voice dictation for X11 and Wayland"
const DESCRIPTION =
  "Free, open-source voice dictation for Linux. Hold a shortcut, speak, and a local model types into the focused window on X11, GNOME, KDE Plasma, Hyprland, or Sway. x86_64 and ARM64."

// The commands the home page's Linux tab offers for this build's release.
export type LinuxInstall = {
  command: string
  deb: string
  rpm: string
  nix: string
  // Only once the first stable release has pushed hex-bin.
  aur?: string
}

const mono = (text: string) => `<code class="cmd">${escape(text)}</code>`

const DESKTOPS: [desktop: string, shortcut: string, paste: string][] = [
  ["X11, such as i3", "Hex grabs the shortcut you record", "Clipboard and Ctrl+V"],
  ["GNOME, KDE Plasma", "The GlobalShortcuts portal. New setups use Ctrl+Alt+Space", "The RemoteDesktop portal"],
  ["Hyprland, Omarchy", "Bind <code>anomaly.hex:dictation</code> in your config", "<code>wtype</code> and <code>wl-copy</code>"],
  ["Sway and other wlroots", "Bind <code>hex dictation start</code> and <code>stop</code> to a key", "<code>wtype</code> and <code>wl-copy</code>"],
  ["niri", "The GlobalShortcuts portal, with <code>xdg-desktop-portal-gnome</code>", "<code>wtype</code> and <code>wl-copy</code>"],
]

const REQUIREMENTS: [label: string, value: string][] = [
  ["Processor", "x86_64 with AVX2 (2013 or newer), or ARM64"],
  ["System", "glibc 2.36 or later: Debian 12, Ubuntu 23.04, Fedora 37, current Arch, or newer"],
  ["Graphics", "A Vulkan driver for the window. A software driver works. Transcription runs on the GPU or the CPU"],
  ["Audio", "ALSA, or PipeWire with its ALSA support"],
  ["Wayland", "<code>wl-clipboard</code>, plus <code>wtype</code> on wlroots compositors and niri"],
]

const QUESTIONS: Question[] = [
  {
    question: "Does Hex work on Wayland?",
    answer:
      "Yes. Hex pastes with wtype on wlroots compositors and niri, and through the RemoteDesktop portal on GNOME and KDE Plasma. Shortcuts come from the GlobalShortcuts portal, or from your own keybindings that run hex commands.",
  },
  {
    question: "Does Hex work on Hyprland and Omarchy?",
    answer:
      "Yes. Bind anomaly.hex:dictation to a key in your Hyprland config. Omarchy runs Hyprland, so the same bind works there, and Hex's tray icon shows in its bar.",
  },
  {
    question: "Do I need a GPU?",
    answer:
      "No. Hex transcribes on your GPU through Vulkan when it can, and on the CPU when it can't. The window still needs a Vulkan driver, and a software one works.",
  },
  {
    question: "Does Hex work offline on Linux?",
    answer: "Yes. The first run downloads a speech model. After that, dictation and meeting transcription make no network request.",
  },
  {
    question: "Is Hex for Linux free?",
    answer: "Yes. Hex is free and open source under the MIT license, with no account, subscription, or word limit.",
  },
]

export function renderLinux(origin: string, install: LinuxInstall): string {
  const url = `${origin}/linux`
  const installs: [label: string, command: string, note: string][] = [
    ["Install script", install.command, "Installs to your home folder and updates itself. No root."],
    ["Debian, Ubuntu", install.deb, "On ARM64, the same command with <code>linux-arm64-deb</code>."],
    ["Fedora, openSUSE", install.rpm, "On ARM64, the same command with <code>linux-arm64-rpm</code>."],
    ["Nix", install.nix, "Nix handles updates."],
    ...(install.aur ? [["Arch (AUR)", install.aur, "pacman updates it."] as [string, string, string]] : []),
  ]
  const rows = installs
    .map(
      ([label, command, note]) =>
        `<div class="row row--copy"><span class="row__label">${label}</span><span class="row__value">${mono(command)}<small>${note}</small></span>${copy(command, `the ${label} command`)}</div>`,
    )
    .join("\n")
  const desktops = DESKTOPS.map(
    ([desktop, shortcut, paste]) =>
      `<tr><th scope="row">${escape(desktop)}</th><td data-label="Shortcut"><span>${shortcut}</span></td><td data-label="Paste"><span>${paste}</span></td></tr>`,
  ).join("\n")
  const requirements = REQUIREMENTS.map(([label, value]) => `<div class="row"><span class="row__label">${label}</span><span>${value}</span></div>`).join("\n")

  const body = `<div class="intent">
<p class="eyebrow">Hex for Linux</p>
<h1>Voice dictation for Linux</h1>
<p class="lede">Hold a shortcut, speak, and Hex types what you said into the focused window. A speech model runs on your GPU or CPU, and nothing you say leaves your machine.</p>

<h2>Install</h2>
<p class="sub">Builds for x86_64 and ARM64. Then open Hex from your launcher, or run <code>hex</code>. The first run downloads a speech model.</p>
<div class="rows">
${rows}
</div>

<h2>Your desktop</h2>
<p class="sub">Hex uses your desktop's portals where it has them, and your own keybindings where it doesn't.</p>
<table class="compare compare--plain">
<colgroup><col class="compare__label"><col><col></colgroup>
<thead><tr><th scope="col">Desktop</th><th scope="col">Shortcut</th><th scope="col">Paste</th></tr></thead>
<tbody>
${desktops}
</tbody>
</table>
<p class="note">GNOME 46 and older have no shortcuts portal, so Hex Settings can add its shortcuts to GNOME's custom shortcuts. A shortcut needs a key besides modifiers, so Super alone won't work.</p>
<pre class="block"><code><span class="dim"># Hyprland: share your session with Hex's service, then bind a key</span>
exec-once = dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP
bind = , End, global, anomaly.hex:dictation

<span class="dim"># Sway: hold F9 to dictate</span>
bindsym F9 exec hex dictation start
bindsym --release F9 exec hex dictation stop</code></pre>

<h2>Requirements</h2>
<div class="rows">
${requirements}
</div>

<h2>Meetings on Linux</h2>
<p class="text"><a href="${withBase("/meetings/")}">Hex Meetings</a> records your microphone and each output's PipeWire monitor, so no bot joins the call, and transcribes the meeting on your machine. Turn it on in the Meetings tab. Titles and summaries stay off unless you send them to your own OpenCode model.</p>

<h2>From a terminal</h2>
<pre class="block"><code>hex status --json               <span class="dim"># what Hex is doing</span>
hex dictation toggle            <span class="dim"># bind this anywhere</span>
hex transcribe call.m4a         <span class="dim"># a file, whether or not Hex runs</span>
hex meetings show latest --md   <span class="dim"># the last meeting as Markdown</span>
hex mcp                         <span class="dim"># meetings for your coding agent</span>
journalctl --user -u hex.service</code></pre>
<p class="note">Hex runs as one process per user under a systemd user unit, with a tray icon where your desktop has a tray. The <a href="https://github.com/anomalyco/hex/blob/dev/docs/linux.md">Linux guide</a> covers each desktop in detail.</p>

${questions(QUESTIONS)}

${about(LINUX_PATH)}
</div>
${COPY_SCRIPT}`

  return shell(
    origin,
    {
      title: TITLE,
      description: DESCRIPTION,
      url: LINUX_PATH,
      image: LINUX_CARD_PATH,
      imageAlt: `${LINUX_CARD.title}. ${LINUX_CARD.line}`,
      jsonLd: {
        "@context": "https://schema.org",
        "@graph": [
          hexApp(origin, {
            "@id": `${url}#app`,
            name: "Hex for Linux",
            url,
            operatingSystem: "Linux",
            processorRequirements: "x86_64 (AVX2) or aarch64",
            downloadUrl: `${origin}/download/linux-x64`,
            description: DESCRIPTION,
          }),
          faqPage(url, QUESTIONS),
          breadcrumbs(origin, url, "Linux"),
        ],
      },
    },
    body,
    { crumb: { href: LINUX_PATH, label: "Linux" }, style: STYLE },
  )
}
