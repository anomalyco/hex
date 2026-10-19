# Scrawl in Hex (experiment plan)

Status: M1 complete on `experiment/scrawl` (`crates/scrawl-core` +
`crates/scrawl-capture-{macos,windows,linux}` + end-to-end macOS `⌥⇧S` capture).
Cut from `origin/main` at `8efd990`.

Scrawl ("show an agent the problem") records the screen, your voice, clicks,
and red ink, then packs them into a `.scrawl` bundle an agent can read. Hex
users have asked for it "connected with Hex". This plan makes Hex the one
app: Scrawl moves into the Hex repo as an opt-in feature named **Scrawl
(beta)**, and the standalone Scrawl app is frozen once Hex reaches M4 parity
(section 5).

Scrawl is also the first feature built against Hex's Ghostty-style
**core / platform / runtime (`libhex`) / shell** architecture (section 3), so
today's GPUI shell, the native AppKit shell on `feat/native-macos`, and the
Windows and Linux shells consume the same engine without duplicating capture or
bundle logic.

Sources read: Hex `AGENTS.md`, `docs/features/*`, `docs/design-system.md`,
`src/app_settings.rs`, `src/suppression.rs`, `src/onboarding.rs`,
`src/meeting.rs`, `src/local_api.rs`, `origin/feat/native-macos`
(`src/ffi.rs`, `docs/plans/native-macos-app.md`); Ghostty `include/ghostty.h`,
`src/apprt.zig`, `src/apprt/embedded.zig`, `src/apprt/runtime.zig`,
`src/apprt/surface.zig`; Scrawl `README.md`, `AGENTS.md`, `Cargo.toml`,
`LICENSE` (`6d4fa33`), `src/{main,model,keyframes,sketch,render,autocrop,hold,windows,zoom,export,capture,ui,laser_overlay}.rs`.
`/Users/kit/code/open-source/scrawl` was not modified.

## Decisions locked in with Kit

```ts
Locked decisions
├── One app            -> Fold Scrawl into Hex; freeze standalone Scrawl.app at M4 parity
├── Feature name       -> "Scrawl" (Settings section: "Scrawl (beta)" until out of beta)
├── Shortcut           -> Separate ⌥⇧S chord matched in Hex's event tap (rebindable modifier+key)
├── Architecture       -> Ghostty-style split: GPUI/AppKit-free core crates + per-OS capture crates + libhex + thin shells
├── Escape key         -> Passes through to the foreground app during screen recording; capsule gets Discard + Undo
├── Storage            -> ~/Library/Application Support/voice-control/scrawl/ (owner-only, beside meetings/)
├── iPhone capture     -> Kept on the roadmap as M5 (cabled capture + ScrawlKit ingest)
├── Git history        -> Import Scrawl's core files into crates/scrawl-core with git history preserved
└── License            -> MIT License (Copyright (c) 2026 Kit Langton) carried in crates/scrawl-core/LICENSE + THIRD_PARTY_NOTICES.md
```

## Facts that shape the plan

```ts
Same stack, same pins
├── gpui 0.2.2 (crates.io, font-kit)          // Hex and Scrawl, identical
├── screencapturekit =11.0.0, macos_15_0      // identical
├── cpal 0.18.2, metal 0.29, objc2 0.6.4, objc2-app-kit 0.3.2, block2, core-video
└── Errors differ: Scrawl anyhow, Hex color-eyre   // bridged at crate boundary

Scrawl's modules by layer
├── Pure Rust (crates/scrawl-core): model, keyframes, sketch, render, autocrop,
│   export/{mod,crop,camera,ink}, zoom (viewport math), hold (state machine),
│   windows (pure types + filters), pipeline (grouped inputs + stage functions), traits
├── Per-OS capture crates:
│   ├── crates/scrawl-capture-macos: ScreenCaptureKit, mouse/click/scroll telemetry, CGWindowList
│   ├── crates/scrawl-capture-windows: trait wiring (Windows.Graphics.Capture planned)
│   └── crates/scrawl-capture-linux: trait wiring (X11 XShm/XComposite + Wayland ScreenCast portal planned)
├── GPUI shell: ui.rs (5.7k lines: capsule island, picker, review window -> replaced by Hex shell)
└── scrawl-helper (Swift executable): frame decode moves to a compiled Swift bridge in Hex

Hex today + feat/native-macos + Windows/Linux
├── macOS event tap (suppression.rs): modifying key tap + passive tap for flags and mouse-down
├── Windows beta (#141): low-level keyboard hook (windows_input), UI Automation (windows_context), HUD (windows_indicator)
├── Linux beta: X11 (linux_input) and Wayland evdev + layer-shell HUD (linux_wayland_input, linux_desktop)
├── Screen Recording helpers already exist in meeting.rs (permitted / request / open settings)
├── transcribe.cpp already returns word times (t0_ms, t1_ms) for Parakeet and Whisper
└── feat/native-macos already introduces src/ffi.rs (hex_engine_t, wakeup_cb, event_cb, tick, telemetry)
```

## 1. Settings

In M1, Scrawl appears as a `Scrawl` section in Settings (alongside `Meetings`),
with the `Scrawl (beta)` opt-in toggle off by default and its sub-rows hidden
until enabled:

```ts
Hex > Settings > Scrawl > Scrawl (beta)            // scrawl.enabled; off by default
├── Off -> No shortcut reserved, no screen permission asked, no workers
└── On
    ├── Screen & System Audio Recording missing -> Request -> "Open settings" row (shared with Meetings)
    ├── Scrawl shortcut -> ⌥⇧S by default; Hex's shortcut capture; key chords only
    ├── Crop to activity -> On                    // Scrawl auto-crop, Screen captures only
    ├── Keep recordings -> Video 7 days, bundles 30 (M3)
    ├── Send dev-site captures to OpenCode -> Off (M4)
    └── Captures folder -> Reveal in Finder
```

Persisted as one block in `settings.json`, following `MeetingSettings`:

```ts
scrawl: {
  enabled: false,
  hotkey: { modifiers: ⌥⇧, key: S },
  auto_crop: true,
}
```

Turning it on is the only action that requests Screen Recording. Starting a
capture without the grant refuses cleanly and surfaces the settings row,
matching Meetings and Scrawl's rule that capture code never prompts.

## 2. Hotkeys

### How `⌥⇧S` meets the Option dictation hotkey

Tracing `DictationHotkey::process` with the default Option binding:

```ts
Press ⌥, then ⇧ within 300 ms, then S        // normal fast chord
  -> ⌥ alone matches -> Start (pre-roll capture begins)
  -> ⇧ joins before MINIMUM_HOLD_DURATION -> extra_modifiers -> Discard -> Dirty
  -> S key-down matches reserved Scrawl chord -> HotkeyAction::Scrawl
  -> Nothing transcribed or pasted for dictation; mute/pause/no-sleep never applied

Press ⌥, wait > 300 ms, then ⇧ S              // slow chord
  -> Recording is live; S key-down matches reserved Scrawl chord
  -> Dictation -> Dirty (cancels in-flight Option capture) and triggers Scrawl

Press ⇧ first, then ⌥, then S                 // Shift already down
  -> ⌥ never matches alone -> Scrawl chord fires cleanly
```

Paste Last (`⌥⇧V`) and Paste Meeting (`⌥⌃V`) already work this way inside
`DictationHotkey::process`: on key-down the dictation state transitions to
`Dirty`, the reserved action fires, and the modifying tap swallows the `S`
keystroke.

### Gesture state machine (`scrawl_core::hold`)

```ts
Scrawl enabled, shortcut ⌥⇧S                     // scrawl.hotkey
├── Tap -> Record display under pointer, auto-cropped
│   └── Tap again -> Finish -> Word-timed transcribe + scrawl-core pipeline -> Clipboard
├── Hold ⌥⇧ after S -> Switcher on Screen; S steps Screen → Window; drag = Area   // M3
│   └── Release ⌥⇧ -> Record selection; Esc -> close switcher, record nothing
└── Key-down always: Dictation -> Dirty (discard any < 300 ms or slow-chord capture)

While recording
├── Hold ⇧ + move (>= 6 px span) -> Red ink (M2); clicks -> keyframe candidates + click markers
├── Dictation shortcut -> Works normally (shares the authoritative microphone stream)
├── Voice Action shortcut -> Works normally
├── Escape -> Passed through to the foreground app (cancels dictation only if dictating)
└── Discard -> Capsule button or menu bar "Discard recording", with Undo window (M2)
```

## 3. Architecture: Ghostty-style library, runtime, platform crates, and shells

Hex is transitioning from a single monolithic binary crate toward the structure
Ghostty uses across its macOS AppKit app and Linux GTK app, already started on
`origin/feat/native-macos` (`src/ffi.rs` and `docs/plans/native-macos-app.md`).

### What Hex borrows from Ghostty

```ts
Ghostty source                     -> Role in Hex
├── include/ghostty.h              -> Small C ABI (src/ffi.rs): opaque engine handle (hex_engine_t),
│   (ghostty_app_t,                   runtime config with userdata + wakeup_cb + event_cb,
│    ghostty_runtime_config_s,        main-thread hex_engine_tick, and lock-free high-frequency
│    ghostty_surface_t)               telemetry reads (hex_engine_read_hud_telemetry)
├── src/apprt.zig                  -> Runtime boundary (libhex): separates pure domain/core crates
│   (apprt.App, apprt.Surface,        and OS platform adapters from the UI toolkit hosting the app;
│    apprt.action)                    typed actions and notifications cross a single seam
├── src/apprt/embedded.zig         -> Embedded host contract: external shells (Swift/AppKit Xcode app,
│   (apprt.embedded.App / Surface)    GPUI desktop shell, CLI/MCP) drive libhex without libhex owning
│                                     NSApplication or a UI framework event loop
└── src/apprt/surface.zig          -> Interactive surface abstraction: transient overlays/windows
    (CoreSurface + per-surface        (dictation HUD, meeting capsule, Scrawl picker/ink overlay/capsule)
     input & size callbacks)          receive normalized pointer/key/scale inputs and emit actions
                                      targeted at either the app or a specific surface
```

### Four-layer workspace architecture

```ts
hex workspace
├── 1. Core crates (pure Rust; no GPUI, no AppKit/objc2/Win32/GTK; OS via traits only)
│   ├── crates/scrawl-core                 // M1: bundle model, keyframes, sketch, autocrop + corpus,
│   │   ├── model, keyframes, sketch       //   CPU RGBA/PNG renderer, export plan (crop/camera/ink),
│   │   ├── autocrop, render, zoom         //   hold state machine, window filters, CandidateBuffer,
│   │   ├── export/{mod,crop,camera,ink}   //   grouped inputs (Recording, Interactions, OutputOptions),
│   │   ├── hold, windows, pipeline        //   stage pipeline (plan_keyframes → solve_crop →
│   │   └── traits                         //   render_frames → assemble_bundle), and platform traits
│   ├── crates/hex-dictation-core          // subsequent extraction: pause_segmenter, text_replacements,
│   │                                      //   command_grammar, shortcut state machine
│   └── crates/hex-meeting-core            // subsequent extraction: meeting_detection, meeting_echo,
│                                          //   seam deduplication, markdown/notes formatting
│
├── 2. Platform capture crates (implement scrawl-core's FrameSource, InputSource, Clock, WindowProbe)
│   ├── crates/scrawl-capture-macos        // ScreenCaptureKit (MP4 + candidate screenshots), mouse/click
│   │                                      //   telemetry, CGWindowList + NSScreen Dock probe (M1 live)
│   ├── crates/scrawl-capture-windows      // Trait wiring + Windows.Graphics.Capture / UIA contract
│   └── crates/scrawl-capture-linux        // Trait wiring + X11 (xcb/XShm) & Wayland (ScreenCast portal) contract
│
├── 3. libhex runtime (app core / Ghostty `apprt.embedded` equivalent)
│   ├── Owns settings, permissions, model lifecycle, dictation coordinator, meetings, and scrawl
│   ├── Drives active sessions (DictationSession, MeetingSession, ScrawlSession) through platform traits
│   ├── Reuses DictationAudio's authoritative microphone stream (no second cpal stream)
│   ├── Reuses DictationWorker's warm GGUF/AppleSpeech model for word-timed transcription
│   ├── Rust API for in-process Rust shells (GPUI, CLI, `hex mcp`, tests)
│   └── C ABI (`src/ffi.rs` / `include/hex.h`) for out-of-process or Xcode shells
│
└── 4. Thin shells (UI consumers only; never own capture, audio timelines, or bundle math)
    ├── GPUI shell (`src/app_window.rs`, `src/dictation_indicator.rs`)  // current production binary
    ├── Native AppKit shell (`feat/native-macos`: `macos/HexApp`)       // Swift/AppKit over libhex.a
    ├── Linux shell (`hex.service` + GTK layer-shell HUD + GPUI Settings)
    └── Windows shell (`windows_app` + tray + HUD + GPUI Settings)
```

### `scrawl-core` composable API: grouped inputs and swappable stages

`scrawl-core` groups inputs into four domain types (`Recording`, `Interactions`,
`TranscriptData`, `OutputOptions`) and exposes four pure stages that compose
(`plan_keyframes` → `solve_crop` → `render_frames` → `assemble_bundle`), so any
shell, CLI command, or test can run or swap an individual stage:

```rust
use scrawl_core::{
    Interactions, OutputOptions, Recording, TranscriptData,
    assemble_bundle, plan_keyframes, render_frames, solve_crop,
};

let recording = Recording {
    target,
    duration_ms,
    frames: captured.frames,
    video: Some("recording.mp4".into()),
    video_offset_ms: captured.video_offset_ms,
    audio: Some("audio.wav".into()),
    ..Default::default()
};
let interactions = Interactions {
    strokes,
    events,
    cursor: activity.cursor,
    windows: activity.windows,
    flags: Vec::new(),
};
let options = OutputOptions::for_target(&recording.target);

// Compose stages explicitly (or call `scrawl_core::build_bundle(id, recording, interactions, transcript, options)`):
let plan     = plan_keyframes(&recording, &interactions, &transcript, options.full_frames.as_deref());
let cropped  = solve_crop(plan, &recording, &interactions, options.auto_crop.as_ref());
let rendered = render_frames(recording.duration_ms, cropped);
let built    = assemble_bundle(bundle_id, recording, interactions, transcript, options.note, rendered);
let (dir, bundle, summary) = built.write_to(&bundle_dir)?;
```

### Multi-platform capture crates and capability matrix

Each OS has its own capture crate implementing `scrawl_core::{FrameSource, InputSource, Clock, WindowProbe}` while reusing Hex's existing hotkey, audio, and overlay owners:

```ts
OS / Backend        -> Crates & existing Hex owners reused
├── macOS           -> crates/scrawl-capture-macos (implemented in M1)
│   ├── Screen/Window/Area: ScreenCaptureKit (SCRecordingOutput H.264 MP4 + SCScreenshotManager candidates)
│   ├── Hotkey & Input:     Hex's suppression.rs CGEventTap for ⌥⇧S; session-scoped mouse/click/window sampler
│   ├── Audio & Speech:     Hex's DictationAudio cpal stream + DictationWorker warm word-timed model
│   └── Ink overlay (M2):   Click-through Metal NSWindow excluded via SCContentFilter
│
├── Windows         -> crates/scrawl-capture-windows (trait-wired stub in M1; reuses #141 Windows beta)
│   ├── Screen/Window/Area: Windows.Graphics.Capture (Direct3D11CaptureFramePool + Media Foundation MP4)
│   ├── Hotkey & Input:     Reuses windows_input low-level keyboard hook + session mouse hook
│   ├── Windows & Context:  EnumWindows / DwmGetWindowAttribute + windows_context UI Automation
│   ├── Audio & Speech:     Reuses windows_dictation WASAPI stream + transcription_engine GGUF word timings
│   └── Ink overlay:        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST DirectComposition window
│                           excluded from capture via SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)
│
├── Linux X11       -> crates/scrawl-capture-linux (trait-wired stub in M1; reuses linux_input / linux_service)
│   ├── Screen/Window/Area: xcb_shm_get_image / XComposite pixmap sampling (+ PipeWire when available)
│   ├── Hotkey & Input:     Reuses linux_input X11 grabs + XInput2 pointer/button observation
│   ├── Windows & Cursor:   _NET_CLIENT_LIST_STACKING / XGetGeometry + XQueryPointer
│   └── Ink overlay:        Click-through ARGB GTK/XShape window
│
└── Linux Wayland   -> crates/scrawl-capture-linux (trait-wired stub in M1; reuses linux_wayland_input / linux_desktop)
    ├── Screen/Window:      xdg-desktop-portal ScreenCast -> PipeWire DMA-BUF/MemFd stream
    ├── Hotkey & Clicks:    Reuses linux_wayland_input evdev key/button observation
    ├── Ink overlay:        Reuses linux_desktop gtk-layer-shell click-through surface
    └── Wayland limits:     Portal prompts per session unless compositor supports restore tokens;
                            no global pointer coordinates or window stacking list from the protocol,
                            so auto-crop falls back to pixel scene-diff bounding boxes instead of cursor/window logs
```

```ts
Capability matrix by OS
├── Target: Screen          -> macOS: Yes (SCK) | Windows: Yes (WGC) | Linux X11: Yes (XShm) | Wayland: Portal prompt
├── Target: Window          -> macOS: Yes (SCK) | Windows: Yes (WGC) | Linux X11: Yes (XComposite) | Wayland: Portal-selected
├── Target: Area            -> macOS: Yes (SCK crop) | Windows: Yes (D3D crop) | Linux X11: Yes (rect) | Wayland: Post-crop only
├── Clicks & drags          -> macOS: Yes (CGEvent/HID) | Windows: Yes (WH_MOUSE_LL) | Linux X11: Yes (XI2) | Wayland: Button times only (no global X/Y)
├── Cursor path & windows   -> macOS: Yes (CGWindowList) | Windows: Yes (EnumWindows) | Linux X11: Yes (EWMH) | Wayland: No (compositor-isolated)
├── Auto-crop               -> macOS: Window+cursor+diff | Windows: Window+cursor+diff | Linux X11: Window+cursor+diff | Wayland: Visual diff only
├── Live ink / laser        -> macOS: Metal overlay | Windows: Layered DComp | Linux X11: XShape ARGB | Wayland: wlr-layer-shell only
└── Permission prompts      -> macOS: Screen Recording once | Windows: None (border on Win10) | Linux X11: None | Wayland: Portal picker per session/token
```

### Milestones

```ts
M1a Core crate with history (done)
    Import Scrawl's GPUI-free core into `crates/scrawl-core` with git history preserved
    Grouped input types (`Recording`, `Interactions`, `TranscriptData`, `OutputOptions`) +
    composable stages (`plan_keyframes` -> `solve_crop` -> `render_frames` -> `assemble_bundle`)
    All standalone unit + autocrop-corpus tests pass (`cargo test -p scrawl-core`)

M1b Smallest end-to-end in-Hex capture on macOS + multi-platform capture crates (done)
    `crates/scrawl-capture-macos` (ScreenCaptureKit + mouse/click/window telemetry) and
    `crates/scrawl-capture-{windows,linux}` (cfg-gated trait wiring returning clear unsupported errors)
    Settings > Scrawl (beta) opt-in toggle (off by default; requests Screen Recording only when enabled)
    Tap ⌥⇧S -> record display under pointer + Hex's authoritative microphone stream
    Tap ⌥⇧S -> Hex word-timed transcription + `scrawl-core` stage pipeline ->
               bundle written to ~/Library/Application Support/voice-control/scrawl/<id>.scrawl ->
               clipboard gets summary markdown + contact-sheet & keyframe paths

M2  Ink, click ripples, laser (Metal overlay); capsule with Discard + Undo
    `hex scrawl inspect | frames | zoom`
M3  Hold switcher (Screen / Window / Area); remaining settings rows; retention worker
M4  Send dev-site captures to OpenCode (`opencode2` discovery Hex already has)
    -> Parity for Mac screen capture: freeze standalone Scrawl.app (section 5)
M5  iPhone: ScrawlKit ingest and cabled capture, behind their own opt-in
Later  `hex scrawl export` (video), `hex mcp` read-only access to captures, Windows & Linux capture implementations
```

## 4. Permissions, onboarding, risks

```ts
Hex onboarding (unchanged)                       // gate for dictation only
├── Microphone, Input Monitoring, Accessibility, dictation model
└── Scrawl never blocks or delays this gate

Turn on Scrawl
├── Screen & System Audio Recording -> request once, then "Open settings"
│   └── Granted but this process can't see it -> "Allow Hex, then reopen it."
├── Input Monitoring -> already granted
├── Microphone -> already granted; shares DictationAudio's stream
└── Automation (browser) -> only when "Send dev-site captures" is turned on (M4)
```

Risks:

- **Dictation regressions.** The only change to `suppression.rs` is one more
  reserved chord (`HotkeyAction::Scrawl`) on the `paste_action` path, inactive
  while Scrawl is off. Tests cover fast and slow `⌥⇧S` orders with an Option
  binding, rebinding conflicts, and Escape still cancelling dictation without
  cancelling an active Scrawl screen recording.
- **Privacy and retention.** Bundles live in
  `~/Library/Application Support/voice-control/scrawl/` with `0700` owner-only
  permissions.
- **Running both apps during the transition.** Turning Scrawl on in Hex while
  standalone Scrawl is running would share `⌥⇧S` unless one shortcut is changed.

## 5. Retiring standalone Scrawl.app

The standalone Scrawl app continues unchanged until Hex reaches M4 parity, then
receives one final Sparkle notice ("Scrawl is now part of Hex") without
auto-deleting or migrating settings. Existing bundles in
`~/Library/Application Support/scrawl/captures` stay untouched and readable via
`hex scrawl inspect|frames|zoom`.
