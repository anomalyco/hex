# Port Hex's Desktop UI From GPUI To Rune

**Status:** Done and merged into `main`: Rune is Hex's only UI toolkit and GPUI is
deleted. Every surface runs on Rune: the Linux single-process window and HUD,
the Windows tray app and HUD, the macOS menu-bar app with its meeting capsules,
every pane (General, Modes, Voice Action, History, Commands, Meetings, Voice
Filter, Activity, HUD Lab), the onboarding tour, and the transcription picker. The
macOS dictation HUD stays a native Metal `NSPanel` (`dictation_indicator.rs`,
with `rune_hud/spring.rs` for its springs). Rune, `rune-kit`, and
`rune-theme-opencode` are pinned to a commit on Rune's public MIT `main`
(`https://github.com/anomalyco/rune.git`; see
[`rune-dependency.md`](rune-dependency.md) and
[`single-process.md`](single-process.md)). Do not merge to `main` until a
person has checked [the real-window list](#needs-a-person-to-look) on each OS:
Linux ran in a real session, macOS was compiled, tested, and launched headless
over SSH, and Windows was cross-compiled.

## Design Direction

The Rune front end uses **OpenCode's design language through Rune's component
library** ([`../design-system.md`](../design-system.md)):

- Controls come from `rune-kit` (`Button`, `IconButton`, `Switch`,
  `SegmentedControl`, `Select`, `Menu`, `Popover`, `TextInput`, `TextArea`,
  `Badge`, `Progress`, …).
- Every colour, elevation, radius, type style, and motion token comes from
  `rune_theme_opencode::kit(dark)`.
- The window follows OpenCode Desktop's layout: a deep-background sidebar and a
  raised, rounded content panel, following the system light/dark appearance.
- `desktop_i18n::tests::every_translated_string_has_every_language` scans
  `src/rune_ui.rs` and every pane and host module so all user-facing copy ships
  in every supported language.

## Module Structure

```text
src/rune_ui.rs                   app spec, shared state, host bridge, poll,
                                 drafts, shell (sidebar + panel), --snap scenes
src/rune_ui/controls.rs          cards, rows, switches, segmented, selects,
                                 buttons, keycaps, notes, text fields, Highlight
src/rune_ui/lists.rs             split list/detail, rows, group labels, tags,
                                 permission control, small_button, slider
src/rune_ui/platform.rs          Capabilities, PlatformState, PlatformRequest,
                                 NativeHost / Fixture seam
src/rune_ui/platform_settings.rs macOS and Windows General rows; Meetings settings
src/rune_ui/settings.rs          General pane and shortcut recorder
src/rune_ui/modes.rs             Modes pane (rules, corrections, OpenCode, transformations)
src/rune_ui/voice_action.rs      Voice Action pane
src/rune_ui/models.rs            OpenCode catalog, model and thinking pickers
src/rune_ui/history.rs           History pane
src/rune_ui/picker.rs            Transcription language/model picker modal
src/rune_ui/onboarding.rs        First-run setup tour across all platforms
src/rune_ui/commands.rs          Commands pane (macOS)
src/rune_ui/meetings.rs          Meetings pane (macOS)
src/rune_ui/meeting_capsules.rs  Offer, call-ended, recording, and ready capsules (macOS)
src/rune_ui/fun.rs               Voice Filter pane: Hex Microphone (macOS, experiment)
src/rune_ui/hud_lab.rs           Developer HUD Lab pane (macOS debug)
src/rune_ui/activity.rs          Developer Activity pane (debug)
src/rune_ui/linux_host.rs        In-process Linux host bridge
src/rune_ui/windows_host.rs      In-process Windows host bridge and keyboard-poll capture
src/rune_ui/macos_host.rs        In-process macOS host bridge, NativeHost, and chord monitor
src/rune_ui/tests.rs             Headless Linux UI tests and goldens
src/rune_ui/tests/panes.rs       macOS and Windows surface tests and goldens
```

## Surfaces And Goldens

| Surface | Status | Goldens (`tests/golden/rune-*.png`) |
| --- | --- | --- |
| Shell & General (Linux) | Sidebar navigation, status card, desktop shortcuts (X11 / portal / commands), dictation, behavior, application rows, Quit Hex, Update button when ready | `rune-portal-{light,dark}`, `rune-commands-dark`, `rune-disabled-light`, `rune-capturing-dark`, `rune-chinese-dark`, `rune-menus-light` |
| Voice Action | Opt-in with shortcut conflict check, recorder, searchable OpenCode model and thinking pickers, missing/unavailable OpenCode notice | `rune-voice-action-{light,dark}`, `rune-voice-action-off-light`, `rune-voice-action-unavailable-{light,dark}`, `rune-voice-action-models-dark` |
| History | Search, retention select, two-step Clear all, detail with Copy and Delete, latency and processing facts | `rune-history-{light,dark}`, `rune-history-empty-light` |
| Modes | Global + custom modes, searchable application picker, native file dialog ("Choose a file…": FileChooser portal, `NSOpenPanel`, `IFileOpenDialog`), website rules, corrections, OpenCode rewriting, drag-reordered transformations, two-step delete | `rune-modes-{light,dark}`, `rune-modes-global-light`, `rune-modes-picker-{light,dark}` |
| Transcription picker | Modal over scrim: languages, model cards with recommendation and `Streams` tags, Use/Download/Cancel/Delete (two-step), live progress | `rune-picker-{light,dark}`, `rune-picker-downloading-{light,dark}` |
| Onboarding tour | `desktop::onboarding::Flow`: Linux/Windows Welcome → Model setup → Done; macOS adds Microphone/Accessibility rows, System Settings guides, Give it a try practice field, and relaunch recovery | `rune-onboarding-dark`, `rune-onboarding-setup-{light,dark}`, `rune-mac-onboarding*` (10 states) |
| General (macOS & Windows) | Dictation, Output, and Application cards with a collapsed Advanced disclosure (fits 980×760 on macOS), macOS permissions, Launch at login, Show Dock icon, shortcut sides; Windows listener card, Launch at login, Enable on launch; Check for updates under the sidebar version | `rune-mac-settings-{light,dark}`, `rune-mac-settings-advanced-light`, `rune-mac-settings-permissions-dark`, `rune-mac-settings-sides-dark`, `rune-windows-settings-light` |
| Commands (macOS) | Task-grouped catalog, search, detail, Create/Edit config, Copy agent prompt, enable card with microphone-policy confirmation, command model retry/disable notice | `rune-mac-commands-{light,dark}`, `rune-mac-commands-confirm-light`, `rune-mac-commands-unavailable-dark` |
| Meetings (macOS) | Day-grouped list, search, detail with summary and `You`/`Them` turns, Copy transcript, inline Rename, overflow menu (Copy summary, Reveal, Summarize/Regenerate, Delete…), two-step trash, Start/Stop recording; Recordings | Settings switch (Call recording, Calendar, Detected calls); opt-in card while off | `rune-meetings-{light,dark}`, `rune-meetings-settings-{light,dark}`, `rune-meetings-off-{light,dark}`, `rune-meetings-calendar-{light,dark}`, `rune-meetings-calendar-denied-dark`, `rune-meetings-recording-{light,dark}`, `rune-meetings-actions-dark`, `rune-meetings-delete-light`, `rune-meetings-rename-dark`, `rune-meetings-failed-dark`, `rune-meetings-empty-light` |
| Voice Filter (macOS, Experiments) | Driver status and install/update/uninstall, nine voice-filter modes, live sliders, Try it 5 s recorder and dual `rune::Shape` bar waveforms | `rune-mac-voice-filter-{light,dark}`, `rune-mac-voice-filter-muted-dark`, `rune-mac-voice-filter-driver-missing-light` |
| HUD Lab & Activity (debug) | HUD Lab state/queue/style/slider controls and preset copy; Activity live event list and raw JSON inspector | `rune-mac-hud-lab-{light,dark}`, `rune-mac-activity-light` |

Run headlessly on any OS:

```sh
cargo run -- app -- --list-scenes
cargo run -- app -- --snap /tmp/hex-modes.png --scene modes --theme light
cargo test rune_ui
```

`HEX_UPDATE_GOLDENS=1` rewrites the 53 goldens in `tests/golden/`. On a host
without a GPU, render on lavapipe with
`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`.

## Observed Verification

- **Linux (apollo, niri Wayland, 2026-10-22):**
  - Real window in Dax's session (`app_id: hex`, no CSD under niri's
    `prefer-no-csd`, crisp at scale 2 and fractional scale 1.5
    ([1.5](rune-ui/hex-rune-real-niri-fractional.png)), responsive row wrapping
    down to 640 px ([after](rune-ui/hex-rune-real-niri-narrow.png))).
  - Single-process lifecycle: launching `hex` opens the window in the running
    instance, closing the window leaves the same PID answering, relaunching
    reopens the window, and `hex quit` exits cleanly.
  - Headless Sway (`scripts/test-wayland-paste.sh` and `linux_rune_hud`):
    `hex dictation start` shows the top-edge recording pill and `stop` hides it
    after transcription; virtual-pointer clicks pass through the overlay.
  - CJK fallback (`Noto Sans SC`/`JP`) renders Chinese UI without tofu
    ([Chinese](rune-ui/hex-rune-shell-chinese-dark.png)).
- **macOS (hailmary, Command Line Tools stub, 2026-10-22):**
  - Compiles with `cargo clippy --all-targets -- -D warnings`, passes the macOS
    unit tests and Metal golden renders, and a debug `hex app` with an isolated
    data root started, created its 980×760 window, answered `hex dev
    status|show|hud`, and exited cleanly on Ctrl-C.
- **Windows (apollo cross-check, 2026-10-22):**
  - `cargo xwin clippy --all-targets -- -D warnings` passes for both
    `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`.

## Needs A Person To Look

Before cutting the first release with the Rune front end:

- **Linux:** pointer hover/press states and I-beam cursor over text fields;
  dragging a transformation with a real pointer; opening the FileChooser portal
  dialog ("Choose a file…"); Wayland `wayland-data-control` copy/paste; IME and
  search-popover auto-focus; recording a physical F-key or Super shortcut; X11
  floating window geometry and override-redirect HUD.
- **macOS:** visual inspection of the window, traffic-light inset, and
  `El::window_drag` on a real display; menu-bar status item, Dock reopen, and
  the update swap and relaunch; AppKit modifier-only/Fn shortcut capture; native Metal
  dictation HUD (`dictation_indicator.rs`) and interactive Rune meeting
  capsules (`meeting_capsules.rs`); choosing a file or folder in `NSOpenPanel`
  from Modes and Meetings inside the real app (a Pro harness checked the
  panel's configuration, cancel, and that the loop keeps running; no capture).
- **Windows (x64 and ARM64):** first real launch of `windows_rune.rs`; tray
  icon and menu; relaunch focus handoff; keyboard-polling shortcut capture;
  `windows_rune_hud.rs` layered click-through overlay above the taskbar; Launch
  at login and self-update restart; Modes' "Choose a file…" `IFileOpenDialog`
  (type-checked only).

## Remaining Rune And Parity Gaps

- **Rune rough edges still worked around in Hex:** `Pixels::set` doesn't wake
  its overlay (the Windows HUD sends an empty update), no rounded child clip
  (the picker hand-rounds its left column), no modifier-only key events (the
  macOS host keeps an NSEvent monitor), and the event loop panics without a
  display (Linux catches that unwind). Queued in Rune behind the current core
  work.
- **Keyboard navigation (landed Oct 23):** Rune's focus model now covers Tab
  in visual order with keyboard-only rings, macOS Full Keyboard Access,
  menu and select arrows with type-select, modal focus traps that return focus
  to the opener, and focus that scrolls into view. Hex's sidebar and list
  panes are source lists (Up/Down select, typing jumps to a row), Command-1…9
  open the shown panes, toggle rows and shortcut recorders take focus, and the
  transcription picker traps focus. List arrows stop at the ends, and focus
  follows a Command-digit selection. Still open: the model and application
  search popovers (`rune-kit` `Popover`) don't trap focus.
- **Windows custom caption:** uses the native Win32 title bar for now.
