# Captures In Hex

Status: steps 2 and 3, and the input half of step 4 with the Scrawl pane,
are built (step 4's half on branch `scrawl-in-hex`) as an experiment behind a build
option (off in every public build) on Scrawl's real engine, the
`scrawl-capture` crate imported into `crates/capture` (see
[Built so far](#built-so-far)); nothing ships, and Kit decides later whether
the fold happens. This is the Hex side of ROADMAP "Fold Scrawl Into Hex": how
Hex hosts Scrawl's capture engine without weakening dictation.

## Built so far

Step 2: `crates/capture` is Scrawl's `crates/capture` at `kitlangton/scrawl`
`e351ddb` (branch `engine/split`), copied without history, package name
`scrawl-capture` kept, with private test values replaced by synthetic ones,
`cargo fmt`, and clippy fixes that change no behaviour. Hex depends on it by
path. It sits in `[workspace] exclude`, so `cargo build`, `cargo test`, and
`--workspace` never build it; it compiles only as the `capture` feature's
macOS dependency, sharing the root `Cargo.lock` and `target/`. On macOS
`scripts/check` and CI name it (`-p voice-control -p scrawl-capture
--features capture`) to lint and test it. Scrawl's own changes now come over
by hand.

Step 3, `capture_host` on that crate:

```ts
hex capture start|stop|cancel|status     // `--features capture`; POST /control `scrawl-capture`
└── listener thread -> capture_host::CaptureHost (exists only with the experiment on)
    ├── start -> owner-only <captures folder>/<unix ms>.scrawl -> reply at once
    │   ├── DictationAudio::open_capture_sink(bounded 512 chunks, since = t0)
    │   └── capture-audio thread -> Engine::start(Start { t0_boot_ns, ActiveDisplay,
    │       AudioIn::External, bundle_dir }) (blocks: target lookup, ScreenCaptureKit)
    ├── capture-audio thread -> Timeline: 16 kHz, gaps padded, start offset kept
    ├── start while two stopped captures are still writing -> busy
    ├── stop -> close_capture_sink(through) (≤ 100 ms wait) -> reply with the bundle path
    │   ├── capture-audio thread -> Capture::stop: screen, window log, and input end now
    │   └── capture-finish thread, one at a time, ≤ 2 waiting ->
    │       SealedCapture::finish(Some(RecordedAudio { start_offset_ms, gaps }))
    │       ├── HexTranscriber -> parakeet::transcribe_with_words, 30 s chunks,
    │       │   each a foreground job on the dictation engine's loaded model
    │       ├── Scrawl writes recording.mp4, audio.wav, frames/, scrawl.json, summary.md
    │       │   (its own files 0600) -> Finished { words, gaps, missing_transcript }
    │       └── Hex makes every file 0600 and folder 0700 (recording.mp4, folders)
    └── cancel -> Capture::cancel, remove the bundle
```

- Hex's seam is three small traits in `src/capture_host.rs`, `CaptureEngine`,
  `Recording`, and `Sealed`, implemented for
  `scrawl_capture::capture::{Engine, Capture, SealedCapture}` so the host's
  tests run without a screen. Hex implements the crate's `Host` (bounded
  `try_send` events, logs and metrics to tracing), `ConfigSource`, and
  `Transcriber`. `hex capture status` shows the last capture's words, audio
  gaps, and why it has no transcript.
- `ConfigSource` is the engine's only settings source; nothing on Hex's path
  reads `~/.config/scrawl/config.json` (the crate reads Scrawl's config and
  logs only after `enable_scrawl_host_defaults` or `enable_file_log`, which
  Hex never calls). Hex has no capture settings yet, so
  `HexConfig` sets Scrawl's defaults deliberately: auto-crop on (display
  captures) and the `agentContext` summary format off.
- The engine is `Engine::new(..).with_resources(SharedResources::none())`;
  Hex never calls `install_shared_resources`, so no ingest server (47777),
  delivery retries, or global shortcuts. `Engine::new` routes the crate's
  process-wide log to `Host::log` when no sink is set, so each line reaches
  Hex's tracing once and nothing goes to `~/Library/Logs/Scrawl`.
  `Engine::new` and `Engine::start` call no TIS/TSM
  (layout-aware shortcuts are resolved only in `install_shared_resources`).
- One capture records at a time. `stop` seals it and the capture-audio
  thread calls `Capture::stop` at once, ending ScreenCaptureKit, the window
  log, and input; the `SealedCapture` then waits for the one writer, at most
  two at a time. With `SharedResources::none()` each capture has its own
  input telemetry, so a capture recording while an earlier one is written
  shares nothing with it.
- Bundles go to `<captures folder>/<unix ms>.scrawl`, Scrawl's own folder
  unless `capture.folder` names another (step 3 used `<support>/captures`):
  Scrawl's own `scrawl.json` format, words in ms on the capture's clock.
- `hex dev capture-replay <wav>…` runs the same `CaptureHost` headlessly on a
  fresh transcription engine, feeding WAV files instead of the microphone; it
  records the screen too (debug builds only, like every `hex dev` command).
- Views read one `CaptureSnapshot` (`capture_host::snapshot`, woken by
  `capture_host::watch`): the recording capture (bundle, start, whether the
  screen started), stopped captures with the engine's latest stage, and the
  last settled one (done, failed, cancelled, with words and gaps). The host
  publishes it on every change; engine progress moves a capture on but never
  settles it. The capsule, the router, and `hex capture list` subscribe here
  rather than to the host.
- Routing and delivery (step 5), a cabled iPhone, the capsule, the on-screen
  picker, and the ink and laser overlays are built; see those sections and
  `docs/features/capture.md`. Not built yet: iPhone ingest (ScrawlKit
  pairing), moving the ingest token, and a listener key path for the open
  list (⌥⇧V, typing, arrows, Return).

Step 4, first half (the shortcut, input, Escape) with the pane, import, and
coexistence from step 5:

```ts
settings.json capture.* (CaptureSettings, cfg hex_capture; other builds keep the raw key)
├── enabled (off) · hotkey (⌥⇧S) · transcriber (parakeet|apple) · folder (None = Scrawl's)
│   · media_days / bundle_days (Scrawl's Week preset: 7 / 30)
├── every key in `hex settings` (control/settings.rs CAPTURE_KEYS)
└── load -> import once (migration `scrawl-capture-settings`, not in MIGRATIONS):
    ~/.config/scrawl/config.json + Scrawl's shortcut file, read-only

recognition loop, every iteration
├── CaptureHost::reconcile -> start/stop the host as capture_on() changes
│   (experiment && capture.enabled); watch_scrawl polls Scrawl.app every 2 s
├── runtime_hotkeys() drops capture while Scrawl.app runs
├── fresh ⌥⇧S press (CaptureShortcut) -> CaptureHost::toggle: start | stop
│   (a press within SHORTCUT_DEBOUNCE, 400 ms, of the start is ignored)
└── Esc, nothing else recording, a capture recording -> cancel it first

suppression event tap (the one session tap)
├── observation mask + mouse down/up/drag/scroll when capture_available()
├── wants_input() (a capture records) -> report_capture_input -> TAP_INPUT
│   (bounded 1 024, drops counted) -> LiveInput::forward -> HostInput
│   (Moved / ModifiersChanged stand in for Scrawl's 8 ms poller)
└── capture-only events end there; keys, flags, and downs continue to the listener

crates/capture hosted_input.rs: Scrawl's ink and click rules as pure functions
(is_pin_click, drew_before, is_real_ink, on_own_window); Scrawl's own key tap
calls the same key_input / key_name. after_shortcut(): Shift must come up once.

Settings › Experiments › Scrawl (rune_ui/scrawl.rs; PlatformState.scrawl is
Some while capture_available(), so the pane exists only then)
├── Capture with Hex · Shortcut · Screen Recording · Transcriber
├── Captures folder (Reveal | choose) · Keep (retention presets)
├── "Scrawl is running" + Use Hex for captures -> quit_scrawl (NSRunningApplication)
└── "Hex copied Scrawl's settings once…" when the import read a file

capture-retention thread -> retention::run_pass(capture.* policy): 90 s, then hourly;
skipped while a capture is unfinished or Scrawl.app runs
```

- Deviations from Scrawl: the shortcut acts on the press (Scrawl's Carbon
  hotkey also starts on press, but Hex has no "start on key-up" pending
  state), a press within 400 ms of the start is ignored rather than stopping
  a capture that has barely begun, and pointer position comes from tap events
  rather than an 8 ms poller.
- The pane exists only in `cfg(hex_capture)` builds: a public build never
  compiles the module, its strings, or its scenes, and a feature build shows
  it only while the experiment is on. Its scenes (`mac-scrawl*`) render only
  from a feature build, so they have no goldens (goldens run on Linux).
- iPhone pairing is omitted, not shown disabled: it needs the ingest server,
  which Hex doesn't run yet. A cabled iPhone needs no row.

### Features and gating

- Hex's Cargo feature `capture` (off by default) turns on the optional
  macOS-only dependencies `scrawl-capture` (`default-features = false,
  features = ["screen", "helper"]`) and `anyhow` (the crate's traits return
  `anyhow::Result`).
- `build.rs` sets `cfg(hex_capture)` for a macOS build, debug or release,
  with the feature on, and every line of capture code is behind it. Builds
  without it (every public release, Linux, Nix, Windows) never compile the
  crate or its Swift helper and refuse `scrawl-capture` with `unsupported`.
- The Release, test-build, and Nix builds never pass the feature.
  `scripts/release-app.sh` refuses `HEX_CAPTURE`, and
  `scripts/validate-app.sh` (run on every macOS release artifact) refuses a
  bundle with `Contents/MacOS/scrawl-helper` or a `hex` containing the
  capture host's start-up log line.
- `scripts/check` and macOS CI lint and test Hex and the crate with
  `--features capture` (the crate's ~260 tests run in seconds). Linux,
  Windows, and Nix builds never select it: the dependency is macOS-only and
  the crate is outside the workspace.
- Off: `phone` (ingest server, pairing), `mic` (the engine's own cpal stream),
  `input` (rdev taps), `hex-client`, `parakeet`, `cli`. `helper` stays on for
  `scrawl-helper`, which decodes `recording.mp4` into full-quality keyframes
  at finish (without it the bundle keeps the 480 px sampled frames).

### Helper and permissions

- `scrawl-helper` is built by the crate's `build.rs` beside the binary
  (`target/<profile>`, three levels above `OUT_DIR`), and the engine looks
  for it beside its own executable (`helper::path`); `$SCRAWL_HELPER`
  overrides. Missing, finish keeps the 480 px candidate frames as keyframes.
  Hex's capture path uses it for movie decoding, which needs no TCC, for
  `devices` (the USB registry, no TCC), and for `mirror --no-microphone`, a
  cabled iPhone's capture, which needs Camera.
- Camera belongs to Hex, the helper's responsible process, so only
  `HEX_CAPTURE=1` builds add `NSCameraUsageDescription` to Hex.app's
  Info.plist (and the helper's embedded one); `scripts/validate-app.sh`
  refuses a release bundle that has it.
- `HEX_CAPTURE=1 scripts/build-app.sh` builds with `--features capture` and
  `SCRAWL_HELPER_PLIST=packaging/macos/scrawl-helper-Info.plist` (embedded
  bundle ID `anomaly.hex.scrawl-helper`, name Hex), copies the helper to
  `Hex.app/Contents/MacOS/scrawl-helper`, and signs it before the app, with
  the app's identity, hardened runtime, a timestamp, identifier
  `<bundle id>.scrawl-helper`, and no entitlements.
- Screen Recording belongs to the process that runs ScreenCaptureKit: Hex
  itself (or the terminal running a dev build). The helper is Hex's child, so
  anything it touches is attributed to Hex, its responsible process.

## Shape

```ts
Hex.app (one process, macOS only for captures)
├─ hex (voice-control package, root of a new workspace)
│  ├─ capture_host            ← new: the experiment's one owner
│  │  ├─ Host impl            logs → tracing, metrics, own windows, clipboard
│  │  ├─ Transcriber impl     Parakeet words on the transcription engine
│  │  ├─ ConfigSource impl    settings.json `capture.*` + one-time import
│  │  └─ capsule + ink overlay (Rune overlays, storybook scenes)
│  ├─ suppression             + capture shortcut, + pointer events while capturing
│  ├─ dictation_audio         + bounded capture sink on the same timeline
│  └─ transcription_engine    + foreground capture clip (after queued dictation)
└─ crates/capture (package `scrawl-capture`, imported without history)
   ├─ features on in Hex:  screen, helper (phone later, for iPhone ingest)
   └─ features off in Hex: input, mic, hex-client, parakeet, cli
```

The engine is Scrawl's: screen sampling, keyframes, sketch and summary,
rendering, autocrop, bundles, delivery, routing (with Jev), retention,
recovery, and iPhone ingest. Hex supplies the microphone, the input, the
transcription, the interface, and the settings. The engine never opens a
microphone, installs an event tap, asks for Input Monitoring, or pastes on its
own inside Hex.

## Workspace and builds

- The root `Cargo.toml` lists `crates/capture` in `[workspace] exclude`
  (built). The root stays the `voice-control` package; the rename to `hex`
  is a separate decision.
- Hex depends on `scrawl-capture` only under
  `[target.'cfg(target_os = "macos")'.dependencies]`, with
  `default-features = false, features = ["screen", "helper"]` today (`phone`
  joins with iPhone ingest).
  Linux and Windows builds never compile it, so their CI and packages don't
  change.
- `crates/capture/Cargo.toml` lists explicit versions (no `workspace = true`)
  so the Scrawl app can depend on it by Git revision until it retires.
- macOS CI lints and tests `-p scrawl-capture` with `--features capture` (built).
- `crates/capture/build.rs` compiles the engine's Swift helper. Hex builds it
  with a Hex identity (`SCRAWL_HELPER_PLIST`), and `scripts/build-app.sh`
  copies and signs it into `Contents/MacOS/` (built for `HEX_CAPTURE=1`).

## Experiment gate

Experiment `capture`. With it off, nothing below starts: no shortcut is
registered, no capture sink exists, no ingest port is bound, no retention or
delivery-retry worker runs, and the helper is never launched. The gate sits
where the work starts (the `capture_host` worker), not only in the window.

## Input

- The capture shortcut (default `⌥⇧S`) is a third target in `suppression`,
  beside dictation and Voice Action, with the same capture and conflict rules
  as the other bindings. It refuses bindings that overlap dictation.
- While a capture records, and only then, the session tap also reports mouse
  down, up, drag and scroll, and the Shift state. `capture_host` feeds them to
  `Capture::input()`. Scrawl's ink and click rules (Shift ink, pin and ink
  thresholds, ignoring clicks on Hex's own windows) move into the engine as
  pure functions over these events, so Hex runs no second tap.
- Escape cancels an active capture before it cancels dictation work. A
  capture never uses the hold threshold or the post-release tail.
- Built: see step 4 in [Built so far](#built-so-far).

## Microphone

- `dictation_audio` has a bounded capture sink (`open_capture_sink`,
  `close_capture_sink`): the capture receives the same timestamped samples
  dictation sees. There is never a second `cpal` stream.
- Dictation keeps working during a capture. Both are projections of one
  timeline, so holding the dictation shortcut mid-capture still pastes, and
  the capture still records that speech as narration.
- With "Release microphone while idle", a capture holds the stream open for
  its duration, like a recording.
- Sink pressure drops capture audio and marks a gap; it never stalls the
  timeline or dictation.

## Transcription

- `capture_host` implements the engine's `Transcriber` with the transcription
  engine's loaded model and word timestamps (`parakeet::transcribe_with_words`;
  dictation still discards its words).
- A finished capture's clip is foreground work: it runs after dictation that
  was already queued and before meeting chunks. It never loads a second model.
  It goes to the engine in 30 s chunks, one job each, so a dictation sent
  meanwhile waits for one chunk at most. A chunk waits while dictation
  captures or transcribes, and the engine holds one that arrives during a live
  stream until the stream ends, so it never interrupts one.
- Scrawl's spoken cues need measured word times. Compared with Apple's
  SpeechAnalyzer on 36 real captures (Oct 25), Parakeet Unified English's word
  times run late: starts by a median 280 ms (about 520 ms after a pause), ends
  by 180 ms, with no drift and no cost at the 30 s seams; raw, 53% of cue words
  are over 300 ms late. `HexTranscriber` subtracts the lag fitted on half the
  captures (`WORD_LAGS` in `capture_host`: starts 280 ms, or 520 ms when the
  gap since the previous word's end is at least 250 ms; ends 195 ms), which
  puts 96% of the other half's cue words within 300 ms of Apple. The lag is
  keyed to the pinned model file: another model, or a new pin, keeps raw times
  until it is measured again. The Apple helper path stays selectable
  (`hex settings set capture.transcriber apple`).

## Interface

- The capsule (recording, processing, done, route toast, picker) and the ink
  and laser overlay are click-through Rune overlays, like the meeting capsules.
  Their window ids go to the engine through `Host::own_windows` so screen
  capture leaves them out and clicks pass through.
- Each state has a `--snap` scene and a storybook entry.
- Scrawl's switch chips (`docs/capture.md`): a shortcut-started recording opens
  onto Area · Window · Screen for 3.5 s (never folding while hovered) or until
  the first click, scroll, or stroke. A chip restarts rather than retargets:
  Window and Area discard the recording and turn the capsule into the picker.
  The host side is built: `CaptureTarget`, `CaptureCommand::{Start, Discard, Switch}`
  through `RecognitionControl::CaptureCommand`, `RecordingCapture::{target,
  worked}`, `SettledCapture::error`, and `begin_picking` for ⌥⇧S and Esc while
  the picker is open. The shortcut starts on its modifiers' release, as
  Scrawl's does. The iPhone chip's host side is built too:
  `CaptureTarget::Phone`, `TargetKind::Phone`, `CaptureSnapshot.phone` (the
  cabled phone's name from a `capture-phone` look at each start and on
  `CaptureCommand::FindPhone`). The mirror runs with `--no-microphone`; the
  narration is Hex's timeline audio moved onto the movie's clock
  (`RecordedAudio::since`) and transcribed by Hex's transcriber. A mirror
  that ends on its own (`Event::PhoneEnded`) finishes the capture at the
  listener's next reap.
- The done capsule's route chips read `SettledCapture.route` (its answer,
  then only the deliveries the capsule can't know itself) and send
  `CaptureCommand::{Send, Undo, Redirect}`. Built; `offer` isn't drawn.
- A Scrawl pane under Experiments, feature-owned (D-5): opt-in, shortcut,
  Screen Recording, transcriber, captures folder, retention, and "Use Hex for
  captures" while Scrawl runs (built; iPhone joins with ingest).
- Screen Recording is asked for at the first capture or from the pane, not in
  core onboarding.

## Routing and delivery

- Built (`capture_host/routing.rs`): Scrawl's router (`crates/router`) runs
  in Hex's process with `capture.routing` (imported once from Scrawl; never a
  Jev key) and Scrawl's own router folders, so `scrawl router undo`, rule
  offers, and evals see Hex's sends as Scrawl's. A `capture-look` thread
  reads the tab, terminal, and sessions at the stop; the one `capture-route`
  thread (queue 16) decides once the bundle is written, waits out a rule's
  3 s undo, sends, and carries out the capsule's Send, Undo, and Redirect on
  the newest capture in order. Each change lands in `last.route`.
- Delivery uses the engine's delivery path. Queued sends retry every 15 s,
  except in tests, while Scrawl.app runs, or while anything serves the ingest
  port (that process retries). Hex never stores the OpenCode server password
  itself; the engine reads it as Scrawl does.
- Not built: `live.ndjson` events for routing (ids, bands, scores, model,
  stages, and outcomes, never transcript, summary, or window text).

## Coexistence with Scrawl

- One owner at a time. While Scrawl (`dev.kitlangton.scrawl`) runs, Hex does
  not register the capture shortcut, run retention, bind the ingest port
  (47777), or run delivery retries. Settings shows "Use Hex for captures",
  which quits Scrawl only on that click (built, without ingest).
- Configuration: a one-time, read-only import of Scrawl's settings into
  `settings.json` `capture.*` keys, each with a `hex settings` key (built).
  The ingest token moves to an owner-only file in Hex's support folder so
  paired phones keep working (not built).
- Captures stay in Scrawl's captures folder; Hex writes and cleans them there
  (built).

## Invariants to add

- Capture never opens a microphone, installs an event tap, or loads a model
  of its own, and never blocks or drops dictation audio.
- Capture queues are bounded: the capture sink, the engine's accessibility
  and retention queues (unbounded in Scrawl today; bound them before import),
  and the frame candidate buffer (lower than Scrawl's 384 MB).
- One engine per process (the engine keeps process-global state).
- The experiment off means no capture work at all.

## CLI

Through `POST /control`, like everything else, behind the build option
(built; `docs/cli.md`): `hex capture start|stop|cancel|status`,
`hex capture list [--limit N]|show <capture>`, `hex capture send <capture>
[--to <session>]`, and `hex capture route <capture> [--ask-jev]`. Not built:
engine tools (autocrop eval, capture simulation, reprocess) under
`hex dev capture` in debug builds.

## Order on the branch

1. Scrawl prepares the engine on its own exploration branch (its inventory's
   refactors: `lib.rs`, pipeline out of `main.rs`, event, config, permission,
   clipboard and transcriber seams, one `Capture` type, Cargo features).
2. Import `crates/capture` here, with the macOS-only dependency and CI.
   Built: copied without history from `e351ddb`.
3. `capture_host` with the experiment gate, the capture sink, Parakeet words,
   and a headless `hex capture start|stop` producing a bundle. Built.
4. Input and Escape, then the Rune capsule, picker, and ink overlay with
   scenes and motion traces. Built.
5. Router in-process, the Scrawl pane, import, and coexistence. Built.
6. Compare Parakeet and SpeechAnalyzer word times on recorded captures. Done
   (Oct 25): Parakeet's lag is corrected in `capture_host`.

The Oct 18 `experiment/scrawl` branch was a copy that drifted; `feat/capture`
supersedes its capture sink and word transcriber, so it can be deleted.

## Asks for Scrawl's API

Answered by `engine/split`: `Start::t0_boot_ns`, `RecordedAudio` with
`start_offset_ms` and `gaps`, words relative to the first sample, and
`SharedResources::none()` by default.

Then answered at `86d7d6c`: `Capture::stop` returning a `SealedCapture`
(the screen stops at seal, `finish` only writes), per-capture telemetry
without global shortcuts, no TIS/TSM in `Engine::new`, and autocrop and
`agentContext` from `ConfigSource`.

Then answered at `e351ddb` (the imported commit): `Engine::new` bridges the
crate's log to `Host::log` when no sink is set, and a library writes Scrawl's
log file or reads its config only when asked; `Engine::start`'s docs say it
blocks; bundle files Scrawl writes are `0600`; and `Finished` reports `gaps`
and `missing_transcript`.

Left from hosting the real engine:

- No audio-only target: every capture records the screen and needs Screen
  Recording. Hex's narration-only tests use a fake engine.
- `Engine::start` blocks on target discovery and ScreenCaptureKit; Hex runs it
  off the listener thread.
- `finish` launches `scrawl-helper` to decode the movie, so a library host
  ships and signs a second executable (Hex does, with `HEX_CAPTURE=1`) or
  loses full-quality keyframes. Decoding in-process would drop it.
- `recording.mp4`, the window log, and folders still get the process umask;
  Hex tightens them after `finish`.
- `phone`, `routing`, `delivery`, and the ingest HTTP code compile
  regardless of features, and the crate pulls a second `windows` (0.61) into
  `Cargo.lock` for its Windows target.
- `anyhow` in the public traits makes a `color_eyre` host convert every error.
