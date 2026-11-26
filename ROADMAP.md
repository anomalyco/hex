# Roadmap

Hex ships native local dictation, modes and OpenCode rewriting, user TypeScript
transformations, History, Annotate (`.hexannotation` screen/window/area capture and session
routing), Meetings (beta), the Rune desktop app, and release-service self-updates across
Apple Silicon macOS, Windows (x64 and ARM64), and Linux (x64 and ARM64, X11 and compatible
Wayland compositors), plus Hex Mobile in TestFlight and Android development.

This document tracks genuinely unshipped work and physical validation gates. Shipped
behavior and per-capability gaps live in
[`docs/features/README.md`](docs/features/README.md).

## Hardware & Clean-Install Validation Gates

Automated tests and offscreen `--snap` goldens do not replace physical OS and device checks
before stable releases:

- **macOS clean-account & legacy upgrade:** Validate the notarized `anomaly.hex` DMG from a
  clean macOS account (TCC prompts for Microphone, Accessibility, and Screen/System Audio
  Recording, model download, `SMAppService` Launch at Login via `sfltool dumpbtm`, and
  source-free launch), plus a real Sparkle `Install Update` upgrade from 2.3.1 / Swift Hex
  on hardware. Verify macOS 27.3 split Accessibility / event-posting grants on a real
  device.
- **Linux X11, Wayland & signed updates:** Validate a genuine signed cross-version
  self-update on the supported Arch host (artifact signature verification, atomic
  activation, restart, and rollback), and run physical Wayland smoke checks across wlroots
  compositors, KDE, and GNOME (GlobalShortcuts portal triggers, `hex` keybinding commands,
  `wtype` and RemoteDesktop paste, clipboard restore, and closing the window without
  stopping capture or accepted work; see [`docs/linux.md`](docs/linux.md)).
- **Windows x64 & ARM64 desktop parity:** Run physical smoke checks of the tray host, WASAPI
  capture/loopback, Authenticode self-updates, and modifier-only shortcuts on native x64 and
  ARM64 machines. Report the effective output-suppression timestamp from the Linux and
  Windows audio suppressors into the live stream head (matching macOS; see
  [`docs/features/dictation.md`](docs/features/dictation.md)).

## Embedded Transcription SDK Helper Packaging

`@kitlangton/hex` publishes Promise and Effect v4 clients against the authenticated local
transcription service
([`docs/specs/local-transcription-service.md`](docs/specs/local-transcription-service.md),
[`sdk/typescript/README.md`](sdk/typescript/README.md)), and local Electron integration
works with an explicit helper command. Remaining work before turnkey third-party embedding:

- Package a signed, notarized standalone helper binary (decoupled from desktop-only
  UI/resources) for `sdk/service-darwin-arm64` instead of wrapping the full desktop
  executable.
- Validate host crash/EOF cleanup, startup failure reporting, and clean-machine quarantined
  execution inside a signed consumer app.

## Shared Engine Service

Later, not started. Move local model ownership and inference into an on-demand per-user
daemon (`hex engine`) coordinated by `engine.lock` and `local-api.json`, serving the
existing [`/streams`](docs/streams.md) and `/transcriptions` routes while `Hex.app` and
external clients share one warm model and one dictation-first queue. On macOS the daemon
must run with interactive task policy (`ProcessType = Interactive` / `PRIO_DARWIN_PROCESS`),
because `TASK_BACKGROUND_APPLICATION` clamps Metal and CPU decode threads to E-cores even
when thread QoS is interactive.

## Shortcuts & Diagnostics

- **Shortcuts:** Physical chord-discard and cold-microphone checks for Toggle mode
  ([#77](https://github.com/anomalyco/hex/issues/77)); explicit key-consumption,
  cancellation, and rebinding semantics for standalone keypad/character bindings
  ([#67](https://github.com/anomalyco/hex/issues/67)); physical hardware verification of
  Caps Lock remapped to Control ([#37](https://github.com/anomalyco/hex/issues/37)); per-app
  exclusions for the `pause-menu` Lab ([#82](https://github.com/anomalyco/hex/issues/82)).
- **Diagnostics & developer control:** Add process-liveness/heartbeat evidence to
  `live.ndjson` so a crash never leaves a stale `Listening` state looking live. Extend debug
  `hex dev` over `/control` with deterministic capture/job/UI state assertions; publish an
  `npx` MCP wrapper only once it exposes a supported user capability rather than creating a
  second control server.

## Meetings & Model Recommendations

- **Meetings:** Validate end-to-end call recordings on Windows and Linux; record capture
  arrival offsets so offline `meeting_echo_replay` runs need no manual offset; add EventKit
  attendees to `list_meetings`; evaluate on-device semantic search only if BM25 misses real
  paraphrased queries; add a one-shot `hex meeting import-granola <csv>` CLI importer for
  exported Granola CSV transcripts (`microphone` -> `You`, `speaker` -> `Them`, keeping
  Granola summaries separate from Hex notes).
- **Model picker recommendations:** Add a gentle, dismissible update hint in Settings and
  the model picker when a user's saved model is superseded by a better recommendation for
  their language, without switching or downloading automatically.

## Grow Labs › Screenshots

The ⌘⇧4 lab ([`docs/features/labs.md`](docs/features/labs.md)) shoots an area, window, or
display to the clipboard, a folder, and a corner thumbnail. Next, once exercised on
hardware:

- Markup editor from the thumbnail (arrows, text, boxes, blur, crop).
- Scrolling capture of a long window.
- Copy text in a shot (OCR via Apple Vision).
- Self-timer before the shot and pinning a shot as a floating window.

## Grow Hex Mobile (iOS)

Hex Mobile is in TestFlight with an on-device dictation keyboard. Larger tester requests
kept on device and commercially safe:

- **Adaptive touch offsets:** per-user key offsets learned on device from confirmed
  corrections, bounded, decaying, and resettable (ship as an Experiment first).
- **Chinese input:** Pinyin and Shuangpin layouts with candidate selection.
- **Themes:** matched light/dark keyboard color themes.
- **Offline-by-default GIFs:** opt-in user-initiated search only.
- **Local model choice:** switch between local models (Apple Speech, Parakeet); no cloud
  models.
- **Phrase-level grammar autocorrect**, **on-device clipboard history**, **detected-language
  translation**, **additional interface translations**, and real-iPhone verification of
  `ios-dictation-bar-autostart`.

## Deliberate Deferrals

- Keep `~/.config/hex/hex.config.ts` focused on deterministic user transformations; do not
  turn it into a general plugin framework.
- Introduce platform or browser-context seams only when a second concrete implementation
  requires one.
- Splitting user-facing views (History, Meetings) from Settings into two windows remains
  deferred in favor of the single shared Rune window.
