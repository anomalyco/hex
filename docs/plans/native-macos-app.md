# Native macOS App Architecture (`libhex` + AppKit / SwiftUI)

> "This desktop app for Mac looks like shit. Can we make a native AppKit one? It should use the same Rust internals for the shared code. Think about it like how Ghostty works for the most part, even though some of the Rust code does go back to the Swift and AppKit code. Use both AppKit and SwiftUI depending on what is most performant. We could use the Swift Composable Architecture for some of this. The core needs to be architected perfectly."

## Status & Scope

- **Status:** Approved & In Implementation (`feat/native-macos`, M0–M3 implemented, M4 cutover pending)
- **Baseline:** `origin/main` (`1c3dacf`, including PR #134 Meetings + `hex mcp`) and `origin/fix/meeting-echo` (`f2f273d`, PR #135 WAV sample-timeline echo suppression, double-talk preservation, rolling live transcript commits, and single-threaded Moonshine ONNX Runtime sessions) merged into `feat/native-macos`.
- **Relationship to prior plans:** Supersedes the macOS half of [`docs/plans/shared-desktop-ui.md`](shared-desktop-ui.md) while preserving the Linux GPUI desktop shell (`src/linux_app.rs`), `hex` CLI, `local_api`, and TypeScript SDK contracts (`docs/plans/linux.md`, `docs/plans/typescript-sdk.md`). Continues to honor every non-migration rule in [`docs/plans/swift-app-handoff.md`](swift-app-handoff.md): bundle ID `com.kitlangton.hex2`, app name `Hex.app`, Sparkle ZIP root `HEX.app`, and `~/Library/Application Support/voice-control/` as the authoritative data root.
- **Tunable Constants Policy:** `KnobsKit` integration is deferred; all tunable motion, spring, and sizing constants live in a single centralized `HexTuning` definition so they can be wired to live knobs later without touching call sites.

### 2025–2026 macOS Best-Practices Check & AppKit-First Refinement

Before M0 implementation, we reviewed current 2025–2026 macOS engineering guidance and empirical benchmarks:
- **Menu Bar Apps, Activation Policy & Settings Windows** (Peter Steinberger, [*Showing Settings from macOS Menu Bar Items*](https://steipete.me/posts/2025/showing-settings-from-macos-menu-bar-items), 2025; Apple Developer Forums macOS 26/27 `NSStatusItem` & `openSettings` threads): SwiftUI's `MenuBarExtra`, `Settings` scene, `SettingsLink`, and `@Environment(\.openSettings)` fail in `.accessory` (dockless) menu-bar apps on macOS 15–26 without fragile hidden-window hacks, scene-declaration order workarounds, and `Task.sleep` activation delays. **Decision:** Never use SwiftUI `@main App`, `Settings` scene, or `MenuBarExtra`. Own the app lifecycle in `HexAppDelegate` (`NSApplicationDelegate`), the status bar in `StatusBarController` (`NSStatusItem` + `NSMenu` / `NSMenuDelegate`), and the main window in an explicit `NSWindowController` that synchronously promotes `NSApp.setActivationPolicy(.regular)` before `makeKeyAndOrderFront` and restores `.accessory` on `windowWillClose` when `show_dock_icon` is off (preserving `#103`).
- **Non-Activating Floating Panels & Overlays** (Matthew Diakonov / Fazm, [*SwiftUI Floating Panel: NSPanel Patterns for macOS Apps*](https://fazm.ai/blog/swiftui-floating-panel), Apr 2026): SwiftUI has no native floating-panel primitive. The canonical pattern is an `NSPanel` subclass with `styleMask: [.borderless, .nonactivatingPanel]`, `isFloatingPanel = true`, `becomesKeyOnlyIfNeeded = true`, `hidesOnDeactivate = false`, and `collectionBehavior: [.canJoinAllSpaces, .canJoinAllApplications, .fullScreenAuxiliary]`. Furthermore, `NSHostingView` recalculates layout on every frame during live panel resizes. **Decision:** Use pure AppKit `NSPanel` + `CAMetalLayer` + `NSView.displayLink(target:selector:)` (reusing `src/dictation_indicator.metal`) for the 60–120 Hz Dictation/Voice Action HUD pill, and non-activating `NSPanel` overlays with fixed-size container frames for the Meeting offer and active recording capsules.
- **AppKit vs. SwiftUI Layout, Lists & Focus on macOS** (Blake Cerecero, [*AppKit vs SwiftUI on macOS: Layout, Performance, and Trade-offs*](https://digitalblake.com/2026/04/28/swiftui-vs-appkit-macos-ui-performance/), Apr/Jul 2026; Michael Tsai, [*NSTableView With SwiftUI*](https://mjtsai.com/blog/2024/04/15/nstableview-with-swiftui/)): SwiftUI `List` / `LazyVStack` does not recycle a fixed pool of `NSView` instances the way `NSTableView.makeView(withIdentifier:owner:)` / `NSCollectionView` does, causing allocations and scroll stutter at scale; SwiftUI `NavigationSplitView` and `@FocusState` also fight macOS column resize constraints, first-responder tab loops, and custom key capture. **Decision (Updated Split — More AppKit):**
  1. **Window Chrome & Split Container:** Use AppKit `NSWindowController` + `NSSplitViewController` (`NSSplitViewItem(sidebarWithViewController:)` with native `NSVisualEffectView` sidebar vibrancy and `NSOutlineView` / `NSTableView` source list) instead of SwiftUI `NavigationSplitView`.
  2. **Lists, Tables, Text Editors & Key Capture:** Use AppKit `NSTableView` (with cell reuse) for `History`, `Commands` catalog, `Activity` events, `Modes` list, and `Meetings` turns; `NSTextView` for multi-line prompts and meeting notes; and a custom AppKit `NSView` first-responder control for Shortcut Recorders.
  3. **Bounded Inspector Cards & Settings Forms:** Use leaf `NSHostingView` / `NSHostingController` views (or `NSStackView`s) for bounded settings sections and modal sheets, keeping `View.body` strictly free of synchronous parsing or I/O.
- **TCA (`pointfreeco/swift-composable-architecture` `1.26.2`, 2026) + AppKit & Swift 6 Strict Concurrency** ([Point-Free TCA Releases](https://github.com/pointfreeco/swift-composable-architecture)): TCA `1.26.2` pairs `@Reducer` and `@ObservableState` with `@MainActor`-isolated `Store` and `observe { [weak self] in ... }` (via `SwiftNavigation`), letting AppKit `NSViewController` / `NSWindowController` controllers observe state transitions directly without `Combine` `ViewStore` overhead or forcing the outer container into SwiftUI. High-frequency telemetry (60–120 Hz HUD levels/phase) bypasses TCA completely via lock-free C struct polling (`hex_engine_read_hud_telemetry`).

---

## Research & Prior Art

### 1. How Ghostty Splits Core (`libghostty`) and macOS (`AppKit` + `SwiftUI`)

Inspection of `ghostty-org/ghostty` (`include/ghostty.h` and `macos/Sources/Ghostty/Ghostty.App.swift`) shows five architectural patterns that map directly to Hex:

```ts
Ghostty.app (Swift / AppKit @main)
├── AppDelegate.swift (NSApplicationDelegate, Sparkle SPUStandardUpdaterController, NSMenu, Dock policy)
├── Ghostty.App.swift (owns opaque ghostty_app_t + ghostty_config_t; registers ghostty_runtime_config_s)
│   ├── userdata: Unmanaged.passUnretained(self).toOpaque()
│   ├── wakeup_cb(userdata) ──► DispatchQueue.main.async { state.appTick() } ──► ghostty_app_tick(app)
│   ├── action_cb(app, ghostty_target_s, ghostty_action_s) ──► tagged-union switch on main thread
│   └── read/write_clipboard_cb(userdata, ...) ──► NSPasteboard bridge with async completion handle
└── GhosttyKit.xcframework (static C archive + module.modulemap exposing include/ghostty.h)
    └── Core Engine (terminal state, config parser/diagnostics, keybinding tables, Metal surface renderer)
```

1. **Opaque Handle + Runtime Callback Table (`ghostty.h`):** The C header exposes opaque pointers (`typedef void* ghostty_app_t; typedef void* ghostty_config_t;`) initialized via `ghostty_app_new(const ghostty_runtime_config_s*, ghostty_config_t)`. The embedder passes an unretained `userdata` pointer alongside C function pointers (`wakeup_cb`, `action_cb`, `read_clipboard_cb`, `write_clipboard_cb`).
2. **Wakeup + Main-Thread Tick Drainage:** Core background threads never touch AppKit or Swift state directly. When background work produces UI-visible state or platform requests, the core invokes `wakeup_cb(userdata)`, which schedules `DispatchQueue.main.async { state.appTick() }`. `ghostty_app_tick(app)` drains the core's internal mailbox on the main thread and invokes `action_cb(app, target, action)` synchronously on `@MainActor`. This eliminates cross-thread lock contention and Swift concurrency data races.
3. **Tagged Action Union (`ghostty_action_s`):** Core-to-host notifications and platform imperatives travel through a single C tagged union (`ghostty_action_tag_e` + `ghostty_action_u`). Swift switches exhaustively over `action.tag` in `Ghostty.App.action(_:target:action:)`.
4. **Authoritative Core Config:** Configuration parsing, validation, defaults, and diagnostics stay inside the core (`ghostty_config_new`, `ghostty_config_load_default_files`, `ghostty_config_finalize`). Swift reads typed projections and reloads via `ghostty_app_update_config`.
5. **AppKit Lifecycle + Selective SwiftUI:** `AppDelegate` (`NSApplicationDelegate`), `NSWindowController`, `NSMenu`, Sparkle, and `CAMetalLayer` views are pure AppKit. SwiftUI (`NSHostingController` / `NSHostingView`) is used inside window chrome for settings sheets, command palettes, inspector panels, and confirmation dialogs.

### 2. Rust ↔ Swift FFI Evaluation

We evaluated four FFI mechanisms against Hex's constraints (real-time audio/hotkey threads, streaming transcript/audio-level updates, main-actor callbacks, build speed, binary size, and LLDB debuggability):

```ts
FFI Option Comparison
├── 1. Hand-written C ABI + cbindgen (Ghostty style)  [RECOMMENDED]
│   ├── Main-actor callbacks: Native C fn pointers + wakeup_cb/hex_engine_tick() on @MainActor
│   ├── Streaming events: Zero-copy ring/mailbox drained during tick, plus direct 60 Hz HUD atomic poll
│   ├── Ownership/lifetimes: Explicit opaque handles (hex_engine_t) + arena/slice views valid for tick duration
│   ├── Build integration: cargo build --lib (staticlib) + cbindgen -> HexEngineFFI.xcframework
│   ├── Binary size & overhead: Zero runtime/codegen bloat; zero serialization for hot-path audio/HUD frames
│   └── Debuggability: Single LLDB stack frame across Swift -> C -> Rust with exact DWARF symbols
├── 2. UniFFI (Mozilla)
│   ├── Pros: Ergonomic proc-macro IDL for CRUD/records, async Future bridging
│   └── Cons: Marshals every struct/enum through a RustBuffer byte-serialization pass; callbacks require Arc<dyn Trait> vtables that hop threads awkwardly; high macro/codegen churn
├── 3. swift-bridge
│   ├── Pros: Generates C headers + Swift classes directly from Rust modules
│   └── Cons: Fragile maintenance on Swift 6 strict concurrency; weak enum/tagged-union ergonomics; opaque generated glue complicates LLDB debugging
└── 4. BoltFFI
    ├── Pros: Newer high-performance Rust->Swift generator with lower buffer overhead than UniFFI
    └── Cons: Young ecosystem and tooling risk compared to a stable C header; unnecessary when the UI-facing surface is a coarse-grained engine boundary
```

**Recommendation: Hand-written C ABI + `cbindgen` (`HexEngineFFI`), paired with typed JSON snapshots only for cold/complex editor trees (`Modes`, `Commands` catalog).**
- **Why:** Hex already uses `@_cdecl` C bridges in `src/apple_speech.swift` (`hex_apple_speech_*`) and `src/apple_intelligence.swift` (`hex_apple_intelligence_*`). A Ghostty-style C ABI (`include/hex.h` generated/verified by `cbindgen`) keeps hot-path callbacks, lifecycle, audio levels, and HUD state zero-copy and trivially inspectable in LLDB, while cold nested configuration trees (e.g., Mode correction rules and rewrite profiles) can cross the C boundary either as C structs or versioned UTF-8 JSON slices backed by Rust's existing `serde` types without maintaining 50parallel C structs by hand.

### 3. TCA (`pointfreeco/swift-composable-architecture`) & Lessons from Legacy `kitlangton/Hex`

In Kit's archived Swift repository (`github.com/kitlangton/Hex`), the app split into `Hex` (Xcode app target) and `HexCore` (SPM package using `swift-dependencies` and `swift-log`), with TCA reducers in `Hex/Features/{App,Settings,History,Remappings,Transcription}` and `@DependencyClient` wrappers in `Hex/Clients/`.

- **What worked well in `kitlangton/Hex` (Reuse):**
  - `@Reducer` + `StoreOf<Feature>` for multi-step, branched state machines: Onboarding permission flows, Model download/preparation confirmation alerts, Mode/Remapping CRUD editors, and History search/retention confirmation.
  - `@DependencyClient` (`HexEngineClient`) for deterministic `TestStore` tests and instant Xcode `#Preview` fixtures without touching hardware microphones, `CGEventTap`, or `Application Support`.
  - Unified `os.Logger` categories (`HexLog`) with `.private` transcript redaction.
- **What hurt in `kitlangton/Hex` (Do NOT repeat):**
  - Running high-frequency audio metering, hotkey state machines, or streaming transcript tokens through TCA `Action` dispatch caused main-thread reducer churn and equality-diffing overhead.
  - Persisting settings in Swift `@Shared(.fileStorage)` / `UserDefaults` created a split brain once Rust became the engine.
- **Architectural Rule for TCA in `Hex.app`:**
  - **Rust (`libhex`) is the single source of truth** for all domain state, persistence (`settings.json`, `history.json`, `live.ndjson`, meetings), and real-time state machines.
  - **Use TCA (`@ObservableState`, `@Reducer`)** for **windowed user workflows and complex CRUD navigation**: `SettingsFeature`, `ModesFeature`, `VoiceActionFeature`, `CommandsFeature`, `HistoryFeature`, `OnboardingFeature`, and the `MeetingsFeature` browser/detail workspace.
  - **Do NOT use TCA** for **60–120 Hz / low-latency surfaces**: the Dictation/Voice Action HUD pill, the focus-free Meeting live transcript/prompt capsule, the `NSStatusItem` menu bar controller, and the Activity live event tail. Those bind directly to lightweight `@MainActor @Observable` models or AppKit/Metal layers fed by `libhex`.

---

## 1. Process & Ownership Boundary

### Single-Process Architecture (`Hex.app` as `@main`, `libhex` linked statically)

On macOS, `Hex.app` is a native Swift/AppKit executable (`Contents/MacOS/hex`) that links `libhex.a` (`crate-type = ["staticlib", "rlib"]`). There is no daemon/XPC split on macOS because macOS TCC grants (Microphone, Accessibility, Input Monitoring, Screen Recording, Automation) bind to the host app bundle and process tree.

```ts
Hex.app Process Boundary (macOS arm64)
├── Swift Host (@MainActor: AppKit + SwiftUI + TCA)
│   ├── HexAppDelegate (NSApplicationDelegate)
│   │   ├── Startup gate: calls hex_keyboard_initialize_layout_on_main_thread() first
│   │   ├── Boots HexEngine (hex_engine_new) + schedules hex_engine_tick() on wakeup_cb
│   │   ├── Owns StatusBarController (NSStatusItem + NSMenu)
│   │   ├── Owns SparkleController (SPUStandardUpdaterController)
│   │   ├── Owns LoginItemController (SMAppService.mainAppService)
│   │   ├── Owns DictationHUDPanelController (click-through NSPanel + CAMetalLayer)
│   │   ├── Owns MeetingCapsulePanelController (focus-free NSPanel for live meeting transcript/notes)
│   │   └── Owns MainWindowController (NSWindow + NSToolbar + NSHostingController<RootSplitView>)
│   ├── Native Swift Platform Bridges (registered with libhex via vtable at init)
│   │   ├── AppleSpeechBridge (SpeechAnalyzer / SpeechTranscriber, macOS 26+)
│   │   ├── AppleIntelligenceBridge (FoundationModels SystemLanguageModel, macOS 26+)
│   │   └── ApplicationCatalogBridge (NSWorkspace app discovery + icon rendering)
│   └── Windowed Feature Reducers (TCA StoreOf<AppFeature> fed by HexEngineClient)
│       ├── OnboardingFeature (permissions + initial model readiness)
│       ├── SettingsFeature, ModesFeature, VoiceActionFeature, CommandsFeature
│       └── HistoryFeature, MeetingsFeature, ActivityFeature, HUDLabFeature
└── Rust Core Engine (`libhex`)
    ├── Authoritative Audio & Capture: audio (cpal), dictation_audio, dictation, recording_environment
    ├── Authoritative Input & Hotkeys: suppression (CGEventTap), keyboard (TIS snapshot), paste
    ├── Authoritative Recognition & ASR: recognition, parakeet, gguf_session, moonshine, transcription*
    ├── Authoritative Post-Processing: dictation_processor (OpenCode HTTP), text_replacements, personal_commands (Bun)
    ├── Authoritative Context & Actions: context, accessibility (AXUIElement), commands, command_grammar
    ├── Authoritative Meetings Engine: meeting (ScreenCaptureKit), meeting_live, meeting_transcript, meeting_echo, meeting_notes, pause_segmenter, microphone_activity, meeting_detection
    ├── Authoritative Storage & Logs: app_settings (settings.json), history (history.json), events (live.ndjson), app_paths, instance lock
    └── Local API & Dev Control: local_api (loopback HTTP for TS SDK), developer_control (hex dev automation)
```

### Subsystem Ownership Matrix

- **Stays 100% in Rust (`libhex`):**
  - **Audio & Timeline (`audio`, `dictation_audio`, `dictation`, `recording_environment`, `feedback`):** `cpal` CoreAudio streams, timestamped mono PCM ring buffer, 450 ms / 1 s pre-roll, warm vs release-while-idle lifecycle, bounded backoff recovery, sleep assertion, output mute, media pause, and feedback tones.
  - **Hotkey & Event Tap (`suppression`, `keyboard`, `paste`, `accessibility`):** Annotated-session `CGEventTap` on its dedicated high-priority Rust thread, boot-time nanosecond timestamp reconciliation, 300 ms hold/double-tap state machine, 100 ms neutral stale-shortcut recovery, Escape cancellation priority, AX selection/window reads, and generation-safe clipboard paste/restoration.
  - **ASR, Models & Post-Processing (`parakeet`, `gguf_session`, `moonshine`, `transcription*`, `dictation_processor`, `personal_commands`, `text_replacements`):** Strict-Metal `transcribe.cpp` inference, checksum-verified model downloads, single preparation worker with latest-choice admission, OpenCode `opencode2 api` rewrite pipeline, and Bun `@hex/commands` host.
  - **Meetings Capture & Segmentation (`meeting*`, `pause_segmenter`, `microphone_activity`):** `ScreenCaptureKit` system-audio + mic capture, echo cancellation (`meeting_echo`), pause segmentation, atomic final transcript publication, and note generation orchestration (`meeting_notes`).
  - **Persistence & Observability (`app_settings`, `history`, `events`, `desktop_activity`, `local_api`, `developer_control`, `instance`):** `settings.json`, `history.json`, `live.ndjson`, `process.log`, loopback HTTP server, and single-instance lock.
- **Moves to Native Swift (`Hex.app`):**
  - **Application Lifecycle & Dock Policy:** `NSApplicationDelegate` replaces `gpui::Application` and raw `objc2` `setActivationPolicy` calls.
  - **Menu Bar (`StatusBarController`):** Native Swift `NSStatusItem` + `NSMenu` (`NSMenuDelegate`) replaces `src/status_item.rs`, calling `libhex` for installed-only model switching, Paste Last Dictation, and meeting recording controls.
  - **Main Window & All Panes:** Native `NSWindow` + `NavigationSplitView` / `NSSplitViewController` replaces the 10,275-line `src/app_window.rs`, `src/desktop_ui.rs`, `src/desktop_transcription_picker.rs`, and `src/text_input.rs` on macOS.
  - **Focus-Free Overlays (`DictationHUDPanelController`, `MeetingCapsulePanelController`):** AppKit `NSPanel` (`nonactivatingPanel`, `canJoinAllSpaces`, `fullScreenAuxiliary`, `ignoresMouseEvents` where applicable) replaces `src/dictation_indicator.rs` window setup and GPUI `MeetingPanel` windows in `src/meeting_watcher.rs`.
  - **System Integrations (`Sparkle`, `SMAppService`, Permission Guides, App Icons):** Swift owns `SPUStandardUpdaterController` directly (eliminating `src/sparkle.rs` Objective-C runtime glue), `SMAppService.mainAppService` (replacing `src/login_item.rs`), native permission animations (replacing `native/permission_guide.m`), and `NSWorkspace` icon/bundle lookup (replacing `src/application_catalog.rs` Objective-C bridges).
  - **In-Process Apple Frameworks (`SpeechAnalyzer`, `FoundationModels`):** `src/apple_speech.swift` and `src/apple_intelligence.swift` move into the Xcode Swift build and register their C function pointers with `libhex` at startup, eliminating the fragile `swiftc` invocation inside `build.rs`.

### Deep-Dive: Event Tap, Hotkey Capture, and TCC

- **Why the `CGEventTap` MUST remain in Rust (`src/suppression.rs`):**
  1. **Nanosecond Clock Alignment:** `suppression.rs` reconciles annotated-session `CGEventGetTimestamp` (macOS boot-time nanoseconds) directly against CoreAudio input buffer timestamps in `dictation_audio.rs` so delayed press handling reconstructs speech onset from the pre-roll ring and delayed release handling trims post-release audio. Routing key events through Swift would introduce cross-language marshaling on every system keystroke.
  2. **Callback Latency & Watchdog Safety:** macOS disables any `CGEventTap` that blocks the event server. Rust's lock-free/bounded `PendingInputEvents` and `DictationHotkey` state machine already enforce the 100 ms keyboard neutrality check, `SecondaryFn` timestamped modifier-change verification, and shortcut suppression without allocating.
  3. **Keyboard Layout Initialization Invariant:** `HexAppDelegate.applicationWillFinishLaunching` calls `hex_keyboard_initialize_layout()` on the main thread **before** starting `libhex` workers or opening any UI, preserving the exact TIS/TSM snapshot invariant from `AGENTS.md`.
  4. **Shortcut Recorder Handshake:** When the user clicks a shortcut recorder control in Swift Settings/Modes/Voice Action, Swift calls `hex_engine_begin_shortcut_capture(engine, target)`. Rust suspends global hotkey matching in `suppression.rs`, captures the raw physical/modifier chord (including standalone `Fn`/Globe and modifier-only taps that standard `NSEvent` responders miss or swallow), and streams candidate/committed `HexShortcutCaptureEvent` updates back to Swift via `action_cb`.

### Deep-Dive: HUD Rendering (Metal vs Core Animation vs SwiftUI)

Hex has two distinct overlay classes on macOS:

1. **Dictation & Voice Action HUD Pill (`DictationHUDPanelController`):**
   - **Recommendation:** **AppKit `NSPanel` (`ignoresMouseEvents = true`, `styleMask = [.borderless, .nonactivatingPanel]`, `level = .statusBar`) + `CAMetalLayer` driven by `CADisplayLink` (macOS 14+ `NSView.displayLink(target:selector:)`), reusing the existing shader `src/dictation_indicator.metal`.**
   - **Why not SwiftUI `TimelineView` / `Canvas`?** During active capture and transcription, the HUD animates continuous spring physics (width/height expansion, audio-reactive waveform bars, shimmer sweep, and cancel shake) at 60–120 Hz ProMotion while the user is typing or scrolling in another app. `src/dictation_indicator.rs` + `src/dictation_indicator.metal` already computes the entire rounded-pill SDF, border glow, waveform bars, and processing dots in a single fragment shader draw call (`160 bytes` of `HudUniforms`) at `<0.2%` GPU and near-zero CPU with zero heap allocations per frame. Hosting SwiftUI in the HUD would incur AttributeGraph evaluation on the main thread every frame.
   - **Where Swift improves it:** Moving the `NSPanel`, `CAMetalLayer`, and spring animator into ~250 lines of clean Swift (`DictationHUDMetalView`) deletes 1,400 lines of verbose `objc2` message-send boilerplate in `src/dictation_indicator.rs`, keeps the exact `src/dictation_indicator.metal` shader compiled natively by Xcode's Metal toolchain into `default.metallib`, and reads a lock-free `hex_hud_telemetry_s` struct (`phase`, `rms_level`, `mode_accent_rgba`, `is_voice_action`, `queue_depth`) from `libhex` on each `CADisplayLink` vsync.
2. **Meeting Live Capsule (`MeetingCapsulePanelController` from `feat/meetings-ux`):**
   - **Recommendation:** **AppKit `NSPanel` (`nonactivatingPanel`) hosting a lightweight SwiftUI view (`NSHostingView`) backed by `NSVisualEffectView` (`.hudWindow`, `.withinWindow` vibrancy) and Core Animation transitions.**
   - **Why:** Unlike the 120 Hz audio waveform pill, the Meeting capsule updates at speech-segment cadence (1–4 Hz draft/committed transcript turns, speaker labels, and hover-expanded action buttons for Stop / Copy / Notes). SwiftUI inside a non-activating `NSPanel` gives crisp Dynamic Type text layout, native vibrancy, smooth spring height resizing, and interactive hover/click buttons without stealing focus from Zoom/Meet/Slack.

---

## 2. `libhex` Core FFI Surface

### C ABI Architecture (`include/hex.h`)

Following `ghostty.h`, `libhex` exposes a single opaque engine handle (`hex_engine_t`), an embedder callback table (`hex_runtime_config_s`), a wakeup-driven main-thread tick (`hex_engine_tick`), a lock-free HUD telemetry poll (`hex_engine_read_hud_telemetry`), and synchronous/asynchronous command entry points.

```ts
libhex FFI Data Flow
├── Swift @MainActor ──► hex_engine_send_command(engine, cmd_tag, payload_json)
│   └── Validated by Rust, mutates authoritative state / spawns worker, persists settings.json
├── Rust Worker Thread (audio / ASR / meeting / preparation / events)
│   ├── Updates atomic HUD telemetry (rms, phase, queue depth)
│   ├── Pushes typed HexCoreEvent into bounded lock-free main-thread mailbox
│   └── Calls runtime.wakeup_cb(userdata)  [coalesced AtomicBool gate]
└── Swift DispatchQueue.main ──► hex_engine_tick(engine)
    └── Drains mailbox on @MainActor ──► invokes runtime.event_cb(userdata, &event)
```

### Sketch: `include/hex.h` (Generated & Verified via `cbindgen`)

```c
#ifndef HEX_H
#define HEX_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct hex_engine_s* hex_engine_t;

typedef struct {
    const char* ptr;
    size_t len;
} hex_slice_s;

typedef enum {
    HEX_HUD_PHASE_HIDDEN = 0,
    HEX_HUD_PHASE_LISTENING = 1,
    HEX_HUD_PHASE_LOCKED = 2,
    HEX_HUD_PHASE_TRANSCRIBING = 3,
    HEX_HUD_PHASE_VOICE_ACTION = 4,
    HEX_HUD_PHASE_CANCELLED = 5,
    HEX_HUD_PHASE_ERROR = 6,
} hex_hud_phase_e;

// Lock-free 64-byte POD struct polled by CADisplayLink at 60-120 Hz.
typedef struct {
    hex_hud_phase_e phase;
    float rms_level;
    float peak_level;
    uint32_t active_queue_count;
    uint32_t mode_color_rgba;
    uint64_t phase_started_ns;
    bool is_command_mode_awake;
    bool is_voice_action;
    bool is_muted_or_silent;
} hex_hud_telemetry_s;

typedef enum {
    HEX_EVENT_READINESS_CHANGED = 1,
    HEX_EVENT_SETTINGS_CHANGED = 2,
    HEX_EVENT_MODEL_PREP_UPDATED = 3,
    HEX_EVENT_SHORTCUT_CAPTURE_UPDATED = 4,
    HEX_EVENT_HISTORY_UPDATED = 5,
    HEX_EVENT_ACTIVITY_APPENDED = 6,
    HEX_EVENT_MEETING_STATE_CHANGED = 7,
    HEX_EVENT_MEETING_TRANSCRIPT_DELTA = 8,
    HEX_EVENT_OPENCODE_CATALOG_UPDATED = 9,
    HEX_EVENT_PERSONAL_COMMANDS_UPDATED = 10,
    HEX_EVENT_DEV_COMMAND_REQUESTED = 11,
    HEX_EVENT_REQUEST_CHECK_FOR_UPDATES = 12,
} hex_event_tag_e;

typedef struct {
    hex_event_tag_e tag;
    uint64_t generation;
    // Borrowed UTF-8 JSON payload valid for the duration of event_cb.
    hex_slice_s json_payload;
} hex_event_s;

typedef void (*hex_wakeup_cb)(void* userdata);
typedef void (*hex_event_cb)(void* userdata, const hex_event_s* event);

typedef struct {
    void* userdata;
    hex_wakeup_cb wakeup_cb;
    hex_event_cb event_cb;
    bool developer_features_enabled;
} hex_runtime_config_s;

// Must be called on the OS main thread before hex_engine_new.
bool hex_keyboard_initialize_layout_on_main_thread(void);

hex_engine_t hex_engine_new(const hex_runtime_config_s* runtime_cfg);
void hex_engine_free(hex_engine_t engine);

// Drains pending events on the main thread and invokes event_cb synchronously.
void hex_engine_tick(hex_engine_t engine);

// Lock-free read for the 60-120 Hz Metal HUD. Safe from main/display-link thread.
hex_hud_telemetry_s hex_engine_read_hud_telemetry(const hex_engine_t engine);

// Synchronous snapshot queries (returns allocated UTF-8 JSON; free with hex_string_free).
hex_slice_s hex_engine_copy_bootstrap_snapshot_json(const hex_engine_t engine);
hex_slice_s hex_engine_copy_history_page_json(const hex_engine_t engine, hex_slice_s query_utf8, uint32_t limit);
hex_slice_s hex_engine_copy_meeting_detail_json(const hex_engine_t engine, hex_slice_s meeting_id_utf8);
void hex_string_free(hex_slice_s str);

// Command dispatch (settings mutations, model prep, shortcut capture, meeting actions).
bool hex_engine_dispatch_json(hex_engine_t engine, hex_slice_s action_json, hex_slice_s* out_error_json);

#ifdef __cplusplus
}
#endif
#endif /* HEX_H */
```

### Sketch: Swift `HexEngineBridge` & TCA `HexEngineClient`

```swift
import Foundation
import HexEngineFFI
import ComposableArchitecture

@MainActor
public final class HexEngineRuntime {
    public static let shared = HexEngineRuntime()

    private var engine: hex_engine_t?
    private var continuations: [UUID: AsyncStream<HexEngineEvent>.Continuation] = [:]

    public func start(developerFeaturesEnabled: Bool) {
        precondition(Thread.isMainThread, "Must start on OS main thread")
        let layoutReady = hex_keyboard_initialize_layout_on_main_thread()
        precondition(layoutReady, "Failed to initialize TIS keyboard layout snapshot")

        var config = hex_runtime_config_s(
            userdata: Unmanaged.passUnretained(self).toOpaque(),
            wakeup_cb: { userdata in
                guard let userdata else { return }
                // Hop to @MainActor and drain the Rust mailbox
                DispatchQueue.main.async {
                    let runtime = Unmanaged<HexEngineRuntime>.fromOpaque(userdata).takeUnretainedValue()
                    runtime.tick()
                }
            },
            event_cb: { userdata, eventPtr in
                guard let userdata, let eventPtr else { return }
                let runtime = Unmanaged<HexEngineRuntime>.fromOpaque(userdata).takeUnretainedValue()
                runtime.handleCABIEvent(eventPtr.pointee)
            },
            developer_features_enabled: developerFeaturesEnabled
        )
        self.engine = hex_engine_new(&config)
    }

    public func tick() {
        guard let engine else { return }
        hex_engine_tick(engine)
    }

    public nonisolated func readHUDTelemetry() -> hex_hud_telemetry_s {
        // Lock-free atomic snapshot read for CADisplayLink
        guard let engine = MainActor.assumeIsolated({ self.engine }) else {
            return hex_hud_telemetry_s()
        }
        return hex_engine_read_hud_telemetry(engine)
    }

    public func events() -> AsyncStream<HexEngineEvent> {
        let id = UUID()
        return AsyncStream { continuation in
            self.continuations[id] = continuation
            continuation.onTermination = { _ in
                Task { @MainActor in self.continuations.removeValue(forKey: id) }
            }
        }
    }

    public func dispatch(_ command: HexEngineCommand) throws {
        guard let engine else { return }
        let data = try JSONEncoder().encode(command)
        var errSlice = hex_slice_s(ptr: nil, len: 0)
        let ok = data.withUnsafeBytes { rawBuf -> Bool in
            let slice = hex_slice_s(
                ptr: rawBuf.bindMemory(to: CChar.self).baseAddress,
                len: rawBuf.count
            )
            return hex_engine_dispatch_json(engine, slice, &errSlice)
        }
        if !ok {
            defer { hex_string_free(errSlice) }
            let message = String(bytesNoCopy: UnsafeMutableRawPointer(mutating: errSlice.ptr!),
                                 length: errSlice.len, encoding: .utf8, freeWhenDone: false) ?? "Unknown engine error"
            throw HexEngineError.rejected(message)
        }
    }

    private func handleCABIEvent(_ raw: hex_event_s) {
        let data = Data(bytesNoCopy: UnsafeMutableRawPointer(mutating: raw.json_payload.ptr!),
                        count: raw.json_payload.len,
                        deallocator: .none)
        guard let decoded = try? JSONDecoder().decode(HexEngineEvent.self, from: data) else { return }
        for continuation in continuations.values {
            continuation.yield(decoded)
        }
    }
}

@DependencyClient
public struct HexEngineClient: Sendable {
    public var bootstrapSnapshot: @MainActor @Sendable () throws -> HexBootstrapSnapshot
    public var events: @MainActor @Sendable () -> AsyncStream<HexEngineEvent> = { .finished }
    public var dispatch: @MainActor @Sendable (HexEngineCommand) throws -> Void
    public var queryHistory: @MainActor @Sendable (_ query: String, _ limit: UInt32) throws -> [HexHistoryEntry]
    public var loadMeetingDetail: @MainActor @Sendable (_ id: String) throws -> HexMeetingDetail
}
```

---

## 3. Swift Application Architecture (`Hex.app`)

### Module & Package Layout

We organize the macOS front end into a single local Swift Package (`macos/HexMacPackage/Package.swift`) consumed by a thin Xcode app project (`macos/Hex.xcodeproj`), mirroring the `Hex` + `HexCore` ergonomics of `kitlangton/Hex` while keeping nearly all Swift files in SPM so folder structure is git-merge-friendly and testable via `swift test`:

```ts
macos/
├── Hex.xcodeproj (thin wrapper: signing, entitlements, Assets.xcassets, AppIcon.icon, Sparkle.framework)
├── HexApp/ (Xcode @main executable target)
│   ├── main.swift (installs HexAppDelegate, runs NSApplication)
│   ├── Info.plist (com.kitlangton.hex2, LSUIElement/Dock policy, Sparkle keys, usage descriptions)
│   └── Hex.entitlements
└── HexMacPackage/ (local Swift 6 SPM package)
    ├── Package.swift
    ├── Binaries/
    │   └── HexEngineFFI.xcframework (libhex.a + include/hex.h + module.modulemap)
    ├── Sources/
    │   ├── HexEngineClient/ (Typed Swift models, HexEngineRuntime, @DependencyClient, preview fixtures)
    │   ├── HexPlatformBridges/ (AppleSpeechBridge, AppleIntelligenceBridge, AppCatalogBridge, Sparkle, LoginItem)
    │   ├── HexDesignSystem/ (Native AppKit/SwiftUI tokens, shortcut recorder chip, audio level meter, pane headers)
    │   ├── HexOverlays/ (DictationHUDPanelController + Metal shader view, MeetingCapsulePanelController)
    │   ├── HexMenuBar/ (StatusBarController: NSStatusItem + NSMenu with installed-only model picker)
    │   └── HexFeatures/ (TCA Reducers + SwiftUI/AppKit Views)
    │       ├── AppFeature/ (Root split view coordinator, sidebar navigation, modal/sheet routing)
    │       ├── OnboardingFeature/ (Microphone, Input Monitoring, Accessibility, ASR model gate)
    │       ├── SettingsFeature/ (Hotkeys, Microphone, Language/Model picker, Recording & Environment, Updates)
    │       ├── ModesFeature/ (App/Browser context rules, replacements, OpenCode rewrite profile, transformations)
    │       ├── VoiceActionFeature/ (Opt-in toggle, shortcut, OpenCode model & deadline, selection status)
    │       ├── CommandsFeature/ (Opt-in toggle, Moonshine prep status, release-mic conflict sheet, catalog & TS workspace)
    │       ├── HistoryFeature/ (Searchable retained dictations, retention policy, copy/re-paste, two-step clear)
    │       ├── MeetingsFeature/ (Opt-in beta + debug Meetings workspace: live/past turns, echo-cancelled tracks, AI notes)
    │       ├── ActivityFeature/ (Debug-only live NDJSON event tail & diagnostics)
    │       └── HUDLabFeature/ (Debug-only interactive HUD physics & state previewer)
    └── Tests/
        ├── HexEngineClientTests/ (FFI round-trip & snapshot decoding tests)
        └── HexFeaturesTests/ (TCA TestStore unit tests & view snapshot tests)
```

### Component-by-Component UI Mapping (AppKit vs SwiftUI vs TCA)

```ts
Surface & Pane Architecture
├── Main Window Shell (NSWindowController + NSToolbar + NSSplitViewController)
│   ├── Sidebar: AppKit NSSplitViewItem(sidebarWithViewController:) + NSOutlineView/NSTableView source list
│   ├── Pane Scaffold: AppKit split controllers + leaf NSHostingController/NSStackView cards
│   ├── Settings Pane (Leaf SwiftUI/AppKit cards + TCA SettingsFeature observed via observe { })
│   │   ├── Shortcut Recorder: Custom AppKit NSView first-responder control + libhex capture hook
│   │   ├── Transcription Model Picker: Modal sheet/popover with search, language filter, progress ring
│   │   └── Microphone & Volume: Native pop-up button & slider wired to immediate Rust tone preview
│   ├── Modes Pane (AppKit NSSplitViewController + NSTableView mode list + TCA ModesFeature)
│   │   ├── Left column: NSTableView mode list with NSWorkspace app/browser icons
│   │   └── Right column: Context matchers, Replacements NSTableView, OpenCode prompt NSTextView, Transformations
│   ├── Voice Action Pane (Leaf SwiftUI/AppKit form + TCA VoiceActionFeature)
│   ├── Commands Pane (AppKit NSSplitViewController + NSTableView catalog + TCA CommandsFeature)
│   ├── History Pane (AppKit NSTableView with cell reuse + detail inspector + TCA HistoryFeature)
│   │   └── Uses NSTableView makeView(withIdentifier:owner:) for 10,000-entry zero-jank scrolling
│   ├── Meetings Pane (AppKit NSSplitViewController + NSTableView turns + NSTextView notes + TCA MeetingsFeature)
│   │   ├── Transcript Stream: Virtualized NSTableView speaker turns ("You" / "Computer"), live draft tail
│   │   └── Notes & Summary: Native NSTextView markdown editor + Apple Intelligence / OpenCode summary controls
│   └── Activity & HUD Lab Panes (Debug-only; @Observable ring buffer + NSTableView for 1,000+ NDJSON events)
├── Menu Bar Extra (AppKit NSStatusItem + NSMenuDelegate — NO SwiftUI MenuBarExtra)
│   └── Why: SwiftUI MenuBarExtra has broken submenu state updates and window focus quirks; NSMenu is instant
├── Dictation / Voice Action HUD (AppKit NSPanel + CAMetalLayer + NSView.displayLink)
│   └── Zero focus steal, click-through, 60-120 Hz spring animation + dictation_indicator.metal
└── Meeting Offer & Active Capsule (AppKit NSPanel nonactivatingPanel + fixed-size capsule view)
    └── Focus-free floating pills during detected-meeting offers and active meeting recordings
```

### Design System, Accessibility, and Performance Budget

- **Native macOS Look & Feel (Why it stops "looking like shit"):**
  - **System Materials & Colors:** Uses real `NSVisualEffectView` sidebar vibrancy, semantic `NSColor` (`windowBackgroundColor`, `controlBackgroundColor`, `labelColor`, `secondaryLabelColor`, `separatorColor`), and native grouped inset forms (`.formStyle(.grouped)`) instead of flat custom-painted GPUI rectangles.
  - **Automatic Light & Dark Mode:** Supports both macOS Dark and Light appearances natively (with an optional override in Settings), while keeping the floating Dictation HUD pill on its signature high-contrast translucent dark glass material.
  - **Typography & Icons:** San Francisco (`NSFont.systemFont` / `.monospacedDigitSystemFont` for timers and sizes) and hierarchical SF Symbols with smooth symbol effects (`.symbolEffect(.pulse)` for recording states).
- **Accessibility & Keyboard Navigation:**
  - Full VoiceOver support (`accessibilityLabel`, `accessibilityValue`, `accessibilityHint`) on every control, model row, and transcript turn—something GPUI currently lacks on macOS.
  - Full keyboard tab-loop (`NSWindow.initialFirstResponder`, `⌘F` search focus in History/Commands/Meetings, `⌘,` Settings, `⌘1`–`⌘6` pane switching, standard `NSTextView` IME and Undo/Redo stack).
- **Explicit Performance Budgets:**
  - **Idle memory (window closed, menu bar + warm Parakeet/Cohere model):** Swift UI overhead `≤ 18 MB` resident above `libhex` model weights; Main Window `NSWindowController` and its SwiftUI view hierarchy are deallocated when the window closes (recreated on reopen from `HexEngineClient.bootstrapSnapshot()`).
  - **Idle CPU (window closed):** `0.0%` main-thread CPU (`CADisplayLink` paused when HUD phase is `HEX_HUD_PHASE_HIDDEN`).
  - **Hotkey press-to-HUD-visible latency:** `≤ 8 ms` (one vsync at 120 Hz).
  - **Main Window cold open time:** `≤ 35 ms` from menu-bar click to first rendered frame.

---

## 4. Build, Packaging, Signing, and Release Continuity

### Project Generator & Build Graph

We use **Keel (`macos/Project.swift`)** (`keel ensure` materializes gitignored `macos/Hex.xcodeproj`) so that adding Swift files under `macos/HexMacPackage/Sources/` or `macos/HexMacApp/` never touches `.pbxproj` merge conflicts.

```ts
Build Pipeline (scripts/build-app.sh)
├── 1. bun run --cwd sdk/commands build
│   └── Produces managed @hex/commands JS bundle embedded by Rust
├── 2. cargo build --release --lib --target aarch64-apple-darwin
│   ├── Compiles libhex.a (with transcribe.cpp Metal, Moonshine, Audio, Suppression, Meetings)
│   └── cbindgen verifies/emits include/hex.h ──► packaged into HexEngineFFI.xcframework
├── 3. cargo build --release --bin hex --no-default-features --features cli-macos
│   └── Produces lightweight headless CLI binary (or single universal entrypoint; see below)
├── 4. xcodebuild -project macos/Hex.xcodeproj -scheme Hex -configuration Release
│   ├── Compiles Swift 6 targets (HexFeatures, HexOverlays, HexPlatformBridges, AppleSpeech, AppleIntelligence)
│   ├── Compiles src/dictation_indicator.metal ──► Contents/Resources/default.metallib
│   ├── Compiles assets/AppIcon.icon via Xcode 26 Icon Composer (actool)
│   └── Links libhex.a + Sparkle.framework + system frameworks into Hex.app/Contents/MacOS/hex
└── 5. scripts/validate-app.sh & scripts/test-app-identity.sh
    └── Verifies com.kitlangton.hex2, Team ID QC99C9JE59, hardened runtime, notarization, and Sparkle keys
```

### Preserving Single-Binary CLI + GUI Entrypoint (`Contents/MacOS/hex`)

Today, `Hex.app/Contents/MacOS/hex` serves both as the GUI bundle executable (when launched with no args or `--app` / `preview`) and as the CLI (`hex listen`, `hex status`, `hex dev`, `hex internal-local-api`, `hex meeting ...`).
We preserve this exact contract with zero overhead:
- `macos/HexApp/main.swift` calls `hex_cli_maybe_run_from_process_args()` as its very first line.
- If the process was invoked with CLI subcommands (`status`, `listen`, `internal-local-api`, `dev`, `meeting`, `--help`, `--version`), Rust executes the CLI synchronously and exits (`std::process::exit(code)`) before `NSApplication.shared` is ever initialized!
- If launched as the macOS app (no CLI subcommand, or `--app` / `preview`), `hex_cli_maybe_run_from_process_args()` returns `false` and `main.swift` boots `NSApplication.shared` with `HexAppDelegate`.
- This means the TypeScript SDK (`@kitlangton/hex`), `local_api` direct-child spawning, and `hex dev` continue to invoke `Hex.app/Contents/MacOS/hex` without bundling a second binary or duplicating `libhex.a`.

### Release & Sparkle Continuity Checklist

Every invariant in `AGENTS.md`, `scripts/validate-app.sh`, and `scripts/release-app.sh` remains unbroken:
- **Bundle Identity:** `CFBundleName = Hex`, `CFBundleDisplayName = Hex`, `CFBundleIdentifier = com.kitlangton.hex2`, `CFBundleExecutable = hex`, packaged as `Hex.app`.
- **Signing & Notarization:** Signed with Developer ID Application under team `QC99C9JE59` (`VOICE_CONTROL_TEAM_ID=QC99C9JE59`) and notarized with `HEX_NOTARY_PROFILE`, hardened runtime (`--options runtime`), and existing entitlements (`com.apple.security.device.audio-input`, `com.apple.security.automation.apple-events`).
- **Sparkle Archive Root Quirk:** `scripts/release-app.sh` continues to package `Hex.app` inside the manual DMG and stage a copy as `HEX.app` at the root of the Sparkle `.zip` archive so any legacy 2.0.x / 2.1.x installs update cleanly.
- **Monotonic `CFBundleVersion`:** Advances monotonically above current production builds (`compute_bundle_version` in `scripts/build-app.sh`).
- **Dynamic Libraries:** `libmoonshine.dylib` and `libonnxruntime.*.dylib` remain bundled in `Contents/Frameworks/` with `@executable_path/../Frameworks` rpath and inner-out Developer ID code signing.

---

## 5. Linux, CLI, Local API, and SDK Impact

```ts
Cross-Platform Target Split (Cargo.toml)
├── Shared Engine (`libhex` core modules — 100% shared across macOS & Linux)
│   └── audio, transcription_models, transcription_preparation,transcription_service,
│       gguf_session, text_replacements, history, events, desktop_activity, local_api
├── macOS Library + CLI (`cfg(target_os = "macos")`)
│   ├── Exposes `ffi_macos` (include/hex.h) for Swift Hex.app
│   ├── Retains macOS domain modules (suppression, dictation_audio, parakeet, moonshine,
│   │   recognition, commands, personal_commands, dictation_processor, meeting*, app_settings)
│   └── Drops `gpui` dependency on macOS at M4 (saving ~120 crates and ~45s clean compile time on macOS)
└── Linux Binary (`cfg(target_os = "linux")`)
    └── Keeps `gpui` in `[target.'cfg(target_os = "linux")'.dependencies]` for `linux_app.rs`,
        `desktop_ui.rs`, `desktop_transcription_picker.rs`, and `text_input.rs`
```

- **Linux GPUI (`src/linux_app.rs`):** Untouched. Moving `gpui` under `[target.'cfg(target_os = "linux")'.dependencies]` in `Cargo.toml` (at Milestone M4) keeps the Linux X11/Wayland Settings app (`src/linux_app.rs`, `src/desktop_host.rs`, `src/desktop_ui.rs`) working exactly as documented in `docs/plans/linux.md`, while removing `gpui` from macOS builds.
- **`local_api` & TypeScript SDK (`@kitlangton/hex`):** Untouched. `src/local_api.rs` and `src/transcription_service.rs` live inside `libhex` and run in both the GUI process and `hex internal-local-api`.

---

## 6. Strangler Migration Plan & Milestones

We migrate incrementally using a **dual-shell Cargo/Xcode strangler pattern** so `main` stays shippable at every commit and `feat/meetings-ux` lands cleanly without a multi-week freeze.

```ts
Strangler Migration Timeline
├── M0: FFI Spine & Dual-Shell Harness (3–4 days)
│   └── Extract libhex C ABI (hex.h), move apple_speech.swift & apple_intelligence.swift into Xcode,
│       boot empty Hex.app shell alongside existing GPUI shell (--legacy-gpui flag)
├── M1: Menu Bar, 120 Hz Metal HUD, Meeting Capsule & Onboarding Gate (4–5 days)
│   └── Native NSStatusItem, DictationHUDPanelController (reusing dictation_indicator.metal),
│       MeetingCapsulePanelController, SMAppService, Sparkle, and Onboarding window
├── M2: Core Settings, Modes, Voice Action & Commands Panes (5–6 days)
│   └── Native NavigationSplitView window + TCA reducers for Settings, Modes, Voice Action, Commands;
│       parity with scripts/capture-preview.sh
├── M3: History, Meetings Workspace (feat/meetings-ux), Activity & HUD Lab (4–5 days)
│   └── Virtualized History table, full Meetings transcript/echo/notes workspace, debug panes
└── M4: Cutover & GPUI Removal on macOS (2 days)
    └── Delete src/app_window.rs, src/status_item.rs, src/dictation_indicator.rs, src/sparkle.rs,
        gate gpui to target_os = "linux", validate signed release & Sparkle upgrade
```

### Milestone Details

#### M0 — `libhex` FFI Spine & Xcode Workspace (~3–4 days)
- **Scope:**
  - Add `[lib] name = "hex" crate-type = ["staticlib", "rlib"]` to `Cargo.toml`.
  - Create `src/ffi.rs` implementing `hex_engine_new`, `hex_engine_tick`, `hex_engine_read_hud_telemetry`, `hex_engine_copy_bootstrap_snapshot_json`, and `hex_engine_dispatch_json`, plus `cbindgen.toml` generating `include/hex.h`.
  - Create `macos/Hex.xcodeproj` + `macos/HexMacPackage/Package.swift` linking `libhex.a`.
  - Move `src/apple_speech.swift` and `src/apple_intelligence.swift` into `HexPlatformBridges` so Xcode compiles them directly with the macOS 26 SDK (`FoundationModels` weak-linked for `MACOSX_DEPLOYMENT_TARGET=15.0`).
  - Wire `main.swift` to delegate CLI subcommands to Rust (`hex_cli_maybe_run_from_process_args()`).
- **Risks:** Symbol collisions or missing framework link flags when moving from `cargo rustc` binary linking to `xcodebuild` static library linking.
- **Exit Criteria:** `scripts/build-app.sh` produces a valid `Hex.app` via `xcodebuild`; `cargo test` and `swift test` pass; `hex status`, `hex listen`, and `sdk/typescript` tests pass against the new binary.

#### M1 — Menu Bar, Metal Dictation HUD, Meeting Capsule & Onboarding (~4–5 days)
- **Scope:**
  - Implement `StatusBarController` (`NSStatusItem` + `NSMenu`) with installed-only model switching, Paste Last Dictation, Meeting controls, and Check for Updates.
  - Implement `DictationHUDPanelController` (`NSPanel` + `CAMetalLayer` + `CADisplayLink`) using `src/dictation_indicator.metal` and polling `hex_engine_read_hud_telemetry`.
  - Implement `MeetingCapsulePanelController` (`NSPanel` + `NSHostingView`) for the focus-free live meeting bar and detected-meeting prompt from `feat/meetings-ux`.
  - Implement `OnboardingFeature` (TCA + SwiftUI) enforcing the release startup gate (Microphone, Input Monitoring, Accessibility, and selected ASR model).
  - Replace `src/sparkle.rs` and `src/login_item.rs` with native Swift `SPUStandardUpdaterController` and `SMAppService.mainAppService`.
- **Risks:** Window level, collection behavior (`canJoinAllSpaces | fullScreenAuxiliary`), or mouse-event passthrough regressions on the HUD or Meeting capsule.
- **Exit Criteria:** Hotkey dictation, double-tap lock, Escape cancel, Voice Action promotion, and Meeting live capsule work end-to-end with zero focus stealing; `sfltool dumpbtm` verifies Launch at Login registration.

#### M2 — Settings, Modes, Voice Action & Commands Panes (~5–6 days)
- **Scope:**
  - Build the main `NSWindowController` (`NavigationSplitView` with native vibrant sidebar) and TCA features:
    - `SettingsFeature`: Shortcut recorders (Dictation, Paste Last Dictation, Double-tap lock), Microphone picker, Language & Model picker sheet with live download progress and cancellation fence, Release Microphone while Idle confirmation alert, Feedback volume slider with tone preview, History retention picker, Launch at Login, and Dock visibility.
    - `ModesFeature`: Master-detail mode list, app bundle / browser domain activation rules (using native `NSWorkspace` icons), text replacement table, OpenCode rewrite model/variant/prompt editor, and ordered `@hex/commands` transformation chain picker.
    - `VoiceActionFeature`: Persisted opt-in toggle, shortcut recorder, OpenCode model/variant/timeout controls, and live OpenCode availability banner with one-click retry.
    - `CommandsFeature`: Opt-in toggle, Moonshine preparation status/retry, mutual-exclusion confirmation with Release Microphone while Idle, command catalog browser, and `@hex/commands` workspace opener.
- **Risks:** Maintaining exact parity on mutual-exclusion alerts (Commands vs Release Microphone while Idle) and uncommitted shortcut capture cancellation on window close.
- **Exit Criteria:** Every state in `docs/features/{settings,modes,voice-action,voice-commands,model-management,onboarding}.md` is implemented and verified via deterministic `scripts/capture-preview.sh` targets and TCA `TestStore` suites.

#### M3 — History, Meetings Workspace (`feat/meetings-ux`), Activity & HUD Lab (~4–5 days)
- **Scope:**
  - `HistoryFeature`: High-performance searchable transcript table (`NSTableView` / virtualized split detail view), copy/re-paste actions, retention policy selector, and two-step Clear History confirmation.
  - `MeetingsFeature`: Full native workspace for the `feat/meetings-ux` capability set: Meetings beta opt-in toggle in Settings (plus debug-enabled access), live & recorded meeting list, speaker-segmented transcript view (`You` / `Computer`) with echo-cancellation status, inline markdown notes editor, and Apple Intelligence / OpenCode summary generation.
  - `ActivityFeature` & `HUDLabFeature` (Debug builds only): Live NDJSON event stream table over `desktop_activity` and interactive HUD spring/state tuner.
- **Risks:** Smooth scrolling during live meeting transcript streaming and markdown note editing.
- **Exit Criteria:** 10,000-row History synthetic benchmark scrolls at 120 fps; Meetings live recording, draft streaming, atomic final publication, and AI note generation match `docs/features/meetings.md`.

#### M4 — Cutover, Cleanup & Release Validation (~2 days)
- **Scope:**
  - Delete macOS-only GPUI & `objc2` UI bridges: `src/app_window.rs`, `src/status_item.rs`, `src/dictation_indicator.rs`, `src/sparkle.rs`, `src/login_item.rs`, `src/permission_guide.rs`, `native/permission_guide.m`, and `src/application_catalog.rs`.
  - Move `gpui` and `raw-window-handle` in `Cargo.toml` under `[target.'cfg(target_os = "linux")'.dependencies]`.
  - Update `docs/features/*.md` check references to point at the new Swift feature modules and tests.
- **Risks:** Ensuring Linux `cargo check --target x86_64-unknown-linux-gnu` / CI remains green after `Cargo.toml` dependency gating.
- **Exit Criteria:** `scripts/validate-app.sh`, `scripts/test-app-identity.sh`, `cargo clippy --all-targets --all-features -- -D warnings`, `swift test`, and Sparkle upgrade smoke test from current production `Hex.app` all pass.

### Deterministic Preview & `developer_control` Support

`scripts/capture-preview.sh` and `hex dev` remain first-class in the native Swift app:
- Launching `Hex.app/Contents/MacOS/hex preview <pane> [--update-available] [--model-missing] ...` passes the parsed `PreviewConfig` from Rust into `HexAppDelegate`, which initializes `StoreOf<AppFeature>` with deterministic `HexEngineClient.preview(config)` fixtures, sizes the `NSWindow` to the canonical frame, disables window fade animations, and renders immediately for `screencapture -l <windowID>`.
- `developer_control.rs` (`hex dev open-pane`, `hex dev snapshot`, etc.) emits `HEX_EVENT_DEV_COMMAND_REQUESTED` through `event_cb`, allowing `hex dev` CLI automation to drive the native Swift UI identically to the old GPUI window.

---

## 7. Testing Strategy & Invariant Verification

```ts
Verification Pyramid
├── Layer 1: Rust Engine Unit & Integration Tests (`cargo test`)
│   └── Audio timeline, hotkey state machine, model prep worker, history caps, meetings, local_api
├── Layer 2: C ABI & FFI Contract Tests (`cargo test` + `HexEngineClientTests`)
│   └── cbindgen header diff check, JSON snapshot round-trips, wakeup/tick mailbox ordering, leak checks
├── Layer 3: TCA Reducer & Effect Tests (`swift test` via TCA TestStore)
│   └── Exhaustive state/effect assertions for Onboarding, Settings, Modes, Voice Action, Commands, History, Meetings
├── Layer 4: Deterministic Visual Preview Captures (`./scripts/capture-preview.sh`)
│   └── Pixel-accurate window captures for every pane and error/confirmation state in Light and Dark mode
└── Layer 5: Bundle Identity & Live Smoke Verification (`./scripts/test-app-identity.sh` + `hex dev`)
    └── Bundle ID com.kitlangton.hex2, Team QC99C9JE59, SMAppService sfltool check, Sparkle feed upgrade
```

### Preserving Critical `AGENTS.md` Invariants Across the FFI Seam

1. **`keyboard::initialize_layout` Main-Thread Startup Gate:** Enforced by `hex_keyboard_initialize_layout_on_main_thread()` in `HexAppDelegate.applicationWillFinishLaunching`, which asserts `pthread_main_np() != 0` in Rust and aborts `hex_engine_new` if called before layout initialization.
2. **Release Startup Gate vs Non-Blocking Command Model Prep:** `hex_engine_new` starts the hotkey listener only when `onboarding::all_required_ready()` holds; enabling Commands triggers background Moonshine preparation in Rust and streams `HEX_EVENT_MODEL_PREP_UPDATED` to `CommandsFeature` without blocking hotkey dictation.
3. **Shortcut Capture Suspension & Disconnect Cleanup:** Starting shortcut capture in `SettingsFeature` / `ModesFeature` / `VoiceActionFeature` registers a scoped cancellation token in `SettingsFeature.Action.onDisappear` and `NSWindowWillCloseNotification` that invokes `hex_engine_cancel_shortcut_capture(engine)`, restoring the previous binding if uncommitted.
4. **Mutual Exclusion of `Release microphone while idle` and `Commands`:** Enforced both in the TCA reducer (presenting the native confirmation `AlertState`) and authoritatively inside `app_settings.rs` when handling `HexEngineCommand.setReleaseMicrophoneWhileIdle` / `setCommandsEnabled`.
5. **Single Model-Preparation Owner:** Both the `NSMenu` model submenu (`StatusBarController`) and the SwiftUI `TranscriptionPickerSheet` (`SettingsFeature`) dispatch `HexEngineCommand.selectTranscriptionModel` to Rust's single `TranscriptionPreparationController` (`src/transcription_preparation.rs`), with `allow_download = false` from the menu bar and `allow_download = true` from Settings.
6. **HUD Click-Through & `Transcribing` Visibility:** `DictationHUDPanelController` sets `panel.ignoresMouseEvents = true` and `panel.becomesKeyOnlyIfNeeded = false`; Rust's `hex_hud_telemetry_s.phase` stays `HEX_HUD_PHASE_TRANSCRIBING` until the ordered output/paste worker finishes inserting text.

---

## 8. Key Risks, Tradeoffs, and Recommendations

1. **Risk: Dual-State Drift Between TCA Stores and Rust `app_settings` / `history`**
   - *Mitigation & Recommendation:* TCA stores never persist their own state or optimistically diverge from Rust on committed settings. Every setting mutation dispatches a `HexEngineCommand` to Rust; Rust validates, writes `settings.json`, updates atomic projections, and emits `HEX_EVENT_SETTINGS_CHANGED` on the next `hex_engine_tick`, which updates the TCA store's `@ObservableState`.
2. **Decision: C Structs vs JSON Slices for Complex Payloads Over `hex.h`**
   - *Recommendation:* Use flat C structs (`#[repr(C)]`) for hot, high-frequency telemetry (`hex_hud_telemetry_s`) and lifecycle handles, and use borrowed UTF-8 JSON slices (`hex_slice_s`) for cold/structured snapshots and CRUD commands (`Modes`, `History`, `Meetings`, `Settings`). At human UI interaction rates (`< 20 Hz`), `serde_json` + `JSONDecoder` over an in-memory buffer takes `< 40 µs` while eliminating thousands of lines of manual C struct array allocation/deallocation code.
3. **Decision: `NSMenu` vs SwiftUI `MenuBarExtra` for the Status Item**
   - *Recommendation:* Use AppKit `NSStatusItem` + `NSMenu` (`NSMenuDelegate`). SwiftUI `MenuBarExtra` (both `.menu` and `.window` styles) has well-known macOS bugs with dynamic submenu updates, keyboard shortcuts, and stealing focus from the active application when invoking "Paste Last Dictation".
4. **Decision: Xcode Project Management (`Hex.xcodeproj` vs `XcodeGen`)**
   - *Recommendation:* Keep all Swift feature, bridge, and UI code in `macos/HexMacPackage/Package.swift` (where source files are discovered automatically by directory without project file edits) and use a tiny, stable `macos/Hex.xcodeproj` (generated from `macos/Project.swift` via `keel ensure`) solely for the `@main` app bundle wrapper, `Assets.xcassets`, `AppIcon.icon`, entitlements, and `Sparkle.framework` embedding.
5. **Decision: Merging `origin/feat/meetings-ux` Before vs During M0**
   - *Recommendation:* Merge `feat/meetings-ux` into `main` first (or rebase `M0` directly on `feat/meetings-ux`), because `feat/meetings-ux` already cleanly separates the Rust meeting backend (`meeting_transcript.rs`, `meeting_echo.rs`, `meeting_notes.rs`, `pause_segmenter.rs`, `apple_intelligence.rs`) from the UI presentation.

---

## 9. Behavior-ID Parity Checklist (`docs/features/*.md`)

Every user-facing behavior ID across `docs/features/README.md`, `dictation.md`, `commands.md`, `voice-action.md`, `meetings.md`, and `recovery.md` is tracked below against the native macOS implementation (`macos/HexMacPackage` + `src/ffi.rs`).

### Setup, Models, Output, Settings & Maintenance (`docs/features/README.md`)

- `setup.download-recovery` — **Done**: Model transfer host guidance & retry surfaced via `HexTranscriptionModelPickerSheet` and `HexCommandsPane` (`src/ffi.rs`, `HexGeneralSettingsPane.swift`).
- `setup` — **Done**: Startup gate (`HexOnboardingView` + `HexGeneralSettingsPane` top banners) checks Microphone, Input Monitoring, Accessibility, and local model installation before starting dictation (`src/ffi.rs`, `HexAppDelegate.swift`).
- `setup.language-conditioning` — **Done**: Language selector + Parakeet v3 auto-detect notice and Whisper language-hint support in `HexTranscriptionModelPickerSheet` (`HexGeneralSettingsPane.swift`).
- `setup.quick-model-switch` — **Done**: Installed-only `Transcription Model` submenu in `StatusBarController` with remembered language/hints, `Cancel Model Switch`, and `Manage Models…` (`StatusBarController.swift`, `src/ffi.rs`).
- `setup.delete-model` — **Done**: Model deletion in `HexTranscriptionModelPickerSheet` spares active/preparing models and acquires the download lock (`src/ffi.rs`, `HexGeneralSettingsPane.swift`).
- `dictation` — **Done**: Hold-to-dictate, double-tap lock, `double_tap_only`, and Escape cancellation via `libhex` + `HexGeneralSettingsPane` (`src/ffi.rs`, `HexGeneralSettingsPane.swift`).
- `output` — **Done**: Submission-order paste, spoken Send, `Paste Last Dictation` (`⌥⇧V` + menu item), and full `HexHistoryPane` (search, copy text/raw transcript, retention picker, entry delete, two-step Clear History) (`HexHistoryCommandsMeetingsPanes.swift`).
- `settings` — **Done**: Native `HexGeneralSettingsPane` with `HexShortcutRecorderControl` (plus Left/Either/Right side picker), Microphone picker + refresh, Keep Ready vs Release When Idle confirmation alert, While Recording behavior, Transcribe During Pauses, and Feedback Volume slider (`HexGeneralSettingsPane.swift`, `HexSettingsStore.swift`).
- `recording.environment` — **Done**: Rust `recording_environment` RAII sleep assertion, mute, and media pause owned by `libhex` worker.
- `modes` — **Done**: `HexModesPane` master-detail editor for Global and custom modes: app bundle & Brave domain activation rules, phrase-boundary corrections, OpenCode AI rewrite (model, thinking variant, prompt), and ordered `@hex/commands` transformations (`HexModesAndVoiceActionPanes.swift`).
- `recovery` / `maintenance` — **Done**: Menu bar reachable when dockless; orderly shutdown in `HexAppDelegate.applicationWillTerminate`.
- `maintain.startup` / `maintain.dock-policy` — **Done**: `HexAppDelegate.applyDockPolicy` promotes `.regular` while Settings is visible and restores `.accessory` on close when `show_dock_icon` is off (`HexAppDelegate.swift`).
- `maintain.login-item` — **Done**: `LaunchAtLoginController` (`SMAppService.mainApp`) handles `.enabled`, `.notRegistered`/`.notFound`, and `.requiresApproval` with deep link to System Settings > Login Items (`HexSettingsStore.swift`, `HexGeneralSettingsPane.swift`).
- `maintain.updates` / `maintain.managed-updates` — **Done**: `SparkleController` + `DisableUpdates` managed-organization policy in `StatusBarController` and `HexGeneralSettingsPane` (`SparkleController.swift`, `StatusBarController.swift`).
- `maintain.linux-service` / `dictate.feedback.linux` — **Done (N/A on macOS)**: Preserved untouched in Linux targets.

### Dictation (`docs/features/dictation.md`)

- `dictate.hold` / `dictate.short-tap` — **Done**: 300 ms hold threshold and short-tap discard in `libhex` (`suppression.rs`, `recognition.rs`).
- `dictate.lock` / `dictate.double-tap-only` — **Done**: Toggles and key-binding gate in `HexGeneralSettingsPane` (`HexGeneralSettingsPane.swift`, `src/ffi.rs`).
- `dictate.boundaries` / `dictate.release-tail` / `dictate.cold-microphone` — **Done**: Authoritative timeline and warm/cold microphone policy in `libhex` (`dictation_audio.rs`).
- `dictate.model-windows` / `dictate.incremental` / `dictate.model-memory` / `dictate.inference-threads` — **Done**: `Transcribe during pauses` toggle in `HexGeneralSettingsPane` backed by `libhex` (`parakeet.rs`).
- `dictate.silence-gate` / `dictate.overlap` / `dictate.cancel` — **Done**: Silence gate, queued overlap, and Escape cancellation in `libhex`.
- `dictate.feedback` — **Done**: Click-through 120 Hz Metal `DictationHUDPanelController` (`src/dictation_indicator.metal`) + immediate volume preview tones (`DictationHUDPanelController.swift`, `src/ffi.rs`).

### Voice Commands (`docs/features/commands.md`)

- `commands` / `commands.opt-in` — **Done**: `HexCommandsPane` opt-in toggle, mutual-exclusion confirmation with `Release microphone while idle`, background Moonshine preparation status, Retry, and Turn Off actions (`HexHistoryCommandsMeetingsPanes.swift`, `HexSettingsStore.swift`).
- `commands.awake-sleep` / `dictate.voice-controls` — **Done**: Handled by `libhex` recognition worker and reflected in `DictationHUDPanelController` telemetry (`is_command_mode_awake`).
- `commands.catalog` / `commands.context` — **Done**: Searchable command catalog and detail inspector with live foreground context label in `HexCommandsPane` (`HexHistoryCommandsMeetingsPanes.swift`).
- `commands.personal-config` — **Done**: `Create Custom Config`, `Open hex.config.ts`, `Copy Agent Prompt`, and reload error banner in `HexCommandsPane` (`src/ffi.rs`, `HexHistoryCommandsMeetingsPanes.swift`).

### Voice Action (`docs/features/voice-action.md`)

- `voice-action` / `voice-action.opt-in` — **Done**: `HexVoiceActionPane` independent opt-in toggle (reachable when OpenCode is unavailable), conflict validation, and saved state preservation (`HexModesAndVoiceActionPanes.swift`, `src/ffi.rs`).
- `voice-action.shortcut` / `voice-action.selection` — **Done**: `HexShortcutRecorderControl` for the `edit` target + Option-to-Option+Command promotion and Accessibility selection capture in `libhex`.
- `voice-action.processing` / `voice-action.output` / `voice-action.cancel` — **Done**: OpenCode model & thinking-variant pickers, catalog refresh, setup docs link, and HUD `voiceAction` phase in `HexVoiceActionPane` and `DictationHUDPanelController`.

### Meetings (`docs/features/meetings.md`)

- `meetings.opt-in` — **Done**: `Meetings (Beta)` section in `HexGeneralSettingsPane` (enable toggle, Screen & System Audio permission check, transcript folder picker/reveal/reset, Apple Intelligence summary toggle) (`HexGeneralSettingsPane.swift`).
- `meetings.detect` / `meetings.offer` / `meetings.record` / `meetings.call-ended` — **Done**: Focus-free `MeetingCapsulePanelController` for detected-call offers, active recording status, call-ended prompt, and ready summary (`MeetingCapsulePanelController.swift`, `StatusBarController.swift`).
- `meetings.rolling` / `meetings.echo` / `meetings.output` — **Done**: Dual-stream capture, WAV sample-timeline echo suppression with double-talk preservation (`f2f273d`), rolling `transcript.rolling.ndjson` delta tracking (`HEX_EVENT_MEETING_TRANSCRIPT_DELTA`), coalesced speaker turns (`You` / `Them`), single-threaded Moonshine ONNX Runtime initialization (`moonshine::run_sessions_single_threaded()` in both CLI and `libhex` FFI startup), and Markdown export in `libhex` (`src/ffi.rs`, `src/meeting_echo.rs`, `src/meeting_live.rs`, `src/meeting_transcript.rs`).
- `meetings.rename` / `meetings.delete` / `meetings.title.provisional` / `meetings.notes` — **Done**: Full `HexMeetingsPane` workspace with search, start/stop recording, inline rename, two-step delete (disabled while busy), Apple Intelligence notes card (Summarize / Regenerate / Copy Notes), Copy Transcript, and Reveal Markdown (`HexHistoryCommandsMeetingsPanes.swift`).
- `meetings.mcp` / `meetings.mcp.mirror` — **Done**: Preserved via CLI passthrough (`hex mcp`) in `macos/HexApp/main.swift` (`src/lib.rs`).

### Recovery (`docs/features/recovery.md`)

- `recover.cancel` / `recover.shortcut` / `recover.tap-interruption` / `recover.microphone` / `recover.cold-open` / `recover.late-notification` / `recover.command-pressure` / `recover.optional-processing` — **Done**: Enforced by `libhex` workers and surfaced in `HexGeneralSettingsPane`, `HexCommandsPane`, `HexVoiceActionPane`, and `HexActivityPane`.
- `recover.keyboard-layout` — **Done**: `HexAppDelegate.applicationDidFinishLaunching` calls `HexEngineRuntime.initializeKeyboardLayoutOnMainThread()` before `hex_engine_new` (`HexAppDelegate.swift`, `HexEngineClientTests.swift`).

### Remaining Cutover Work (Milestone M4)

- **Pending M4 — Release Build Script Cutover (`scripts/build-app.sh`):** `scripts/build-app.sh` still packages the Rust binary (`target/release/hex`) rather than running `cargo build --release --lib` + `xcodebuild -project macos/Hex.xcodeproj -scheme Hex -configuration Release`. Until M4 switches `scripts/build-app.sh`, native `Hex.app` builds are produced via `xcodebuild` into `~/Library/Caches/hex-native-build/Hex.app` and tested locally with `scripts/swap-in-native-app.sh`.
- **Partial — Legacy GPUI Preview Flag Parity (`scripts/capture-preview.sh`):** The native app supports `--preview-dictation`, `HexHudLabPane` (live-wired to `HexTuning.HUD`), and `hex dev` (`HEX_EVENT_DEV_COMMAND_REQUESTED`), while the legacy CLI `preview <target>` flags still route to `src/app_window.rs` until M4 removes GPUI on macOS.
- **Architectural Simplification — Single `HexMacFeature` Target (`@Observable` Store):** In `macos/HexMacPackage`, M0–M3 were implemented in a single `HexMacFeature` module using `@MainActor @Observable` (`HexSettingsStore` + `HexEngineRuntime`) rather than splitting into multiple SPM targets or pulling in external `swift-composable-architecture`.
- **Pending M4 — Delete Legacy macOS GPUI Modules:** Removing `src/app_window.rs`, `src/status_item.rs`, `src/dictation_indicator.rs`, `src/sparkle.rs`, `src/meeting_watcher.rs`, and gating `gpui` to `cfg(target_os = "linux")` in `Cargo.toml` is scheduled for Milestone M4 after local testing.
