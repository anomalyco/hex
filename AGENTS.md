<!-- tallow -->
# Hex Agent Guide

## Workflow

- `dev` is canonical and ships the Rune desktop UI on macOS, Windows, and Linux (GPUI is
  gone; there is no `main`).
- `dev` and `production` accept changes only through pull requests. `.github/`,
  `CODEOWNERS`, `src/updater*`, `app/update-requirement.txt`, `*.entitlements`, and
  release/signing scripts require Dax (`@thdxr`).
- Branch from `dev`, open a PR to `dev`, and merge once it compiles locally without waiting
  on CI (`compile.yml` runs `cargo clippy --locked --bin hex -- -D warnings` on PRs; full
  test suites run in Release's `check`/`stable` channels and manual `check-<os>.yml`
  dispatches).
- Publish app releases from `dev` (`release.yml`). Fast-forward `production` from `dev` only
  to deploy the production site (`deploy.yml`); never commit directly to `production`.
- Any change to what the window draws must update goldens in the same PR: run
  `HEX_UPDATE_GOLDENS=1 cargo test --locked --bin hex rune_ui` on Linux (from macOS,
  dispatch `check-linux.yml` on the branch and take its `golden-actual` artifact), inspect
  every changed image, and commit them with the change.
- Bump Hex's Rune pin (`rune`, `rune-kit`, `rune-theme-opencode`, `rune-motion` in
  `Cargo.toml` and `Cargo.lock`) only in a dedicated dependency PR or when a task needs a
  Rune fix. When bumping, adapt to Rune's `CHANGELOG.md`, update
  `cargoGitDependencies."rune-0.0.1"` (rev and output hash) in `nix/package.nix`, keep
  `windows` at `0.62.2` for `gpu-allocator` in `Cargo.lock`, and run
  `scripts/check rune_ui`. Report Rune bugs upstream rather than working around them. Never
  run `cargo publish` (neither Hex nor Rune is published to crates.io).

## Product & Surface Rules

Hex is a local, observable native-Rust voice appliance (`anomaly.hex`) for macOS with
explicit Linux (X11 and compatible Wayland) and Windows (x64 and ARM64) beta contracts. Keep
consequential behavior explicit and modules deep. User settings persist in `<Application
Support>/hex/settings.json`; deterministic personal transcript transformations live in
`~/.config/hex/hex.config.ts` (embedded QuickJS; public contract at
`site/public/docs/transforms.md`).

- **Labs gate (`src/labs/feature.rs`, [`docs/features/labs.md`](docs/features/labs.md)):**
  Every build shows General, Output, History, **Meetings** (off by default via
  `meetings.enabled`), **Annotate** (on by default in every build via `capture.enabled`),
  and Labs. **New, unproven, user-requested, and externally contributed
  features land in Labs, off by default** — never add a master switch or ship them on by
  default. Each `labs::Feature` declares its settings key, group, OSes, and audience
  (`Everyone` or `Dev`; stable builds never list, accept, or run `Dev` labs). Gate where the
  feature's work starts (worker, thread, capture), not only its UI. Adding or changing a lab
  requires `HEX_UPDATE_LABS_JSON=1 scripts/check labs::web` (updates `site/src/labs.json`
  for `/hex/labs/<id>`) and `scripts/check labs control::settings`.
- **Feature map ([`docs/features/README.md`](docs/features/README.md)):** Read the index and
  relevant capability maps (`dictation.md`, `capture.md`, `meetings.md`, `labs.md`,
  `own-models.md`, `vocabulary.md`, `control-cli.md`, `transcript-cli.md`,
  `accessibility.md`, `recovery.md`) before changing user-facing behavior, then verify
  against the source. Update an affected map only when user-facing behavior, defaults,
  prerequisites, OS availability, or recovery contracts change — do not narrate internal
  refactors or paste dated test logs. Distinguish implemented behavior, automated checks,
  and physical-device proof limits. Keep future work in [`ROADMAP.md`](ROADMAP.md).
- **Desktop UI ([`docs/design-system.md`](docs/design-system.md)):** Read before touching
  any pane, capsule, or picker. Compose `rune-kit` components and Hex pieces
  (`src/rune_ui/controls.rs`, `src/rune_ui/lists.rs`) using `rune-theme-opencode` tokens —
  never color literals or off-scale spacing. Keep shared UI in pane modules and OS behavior
  in `src/rune_ui/{macos,windows,linux}_host.rs`. `platform::Capabilities` gates per-OS
  panes/rows and every OS difference must name a reason in `platform::DIFFERENCES`
  (`parity_tests`). Use sentence case, "Hex" in copy, `desktop::i18n`
  (`tr`/`tr_fill`/`tr_named` — tests fail if any language is missing), AccessKit names on
  every control (`accessibility_tests`), and at most one accent Primary button per screen.

## Repo Layout

- `src/audio/`, `src/dictation/`, `src/suppression/`, `src/recognition/`,
  `src/keyboard*.rs`, `src/paste/`, `src/continuation.rs`, `src/feedback/`: authoritative
  microphone timeline, bounded stream recovery, pre-roll/release-tail capture, chord state
  machine, foreground context/mode processing, tones, and clipboard-restoring paste
  ([`docs/features/dictation.md`](docs/features/dictation.md),
  [`docs/features/recovery.md`](docs/features/recovery.md)).
- `src/transcription/`, `src/parakeet/`, `src/apple_speech.*`: single-thread
  `TranscriptionEngine` owning the warm model, speculative GPU streaming with whole-clip
  fallback, catalog/custom/HuggingFace-found GGUF models, preparation worker, Silero VAD,
  and `/transcriptions` + `/streams` ([`docs/streams.md`](docs/streams.md),
  [`docs/features/own-models.md`](docs/features/own-models.md)).
- `src/meeting/`, `src/calendar*`, `src/microphone_activity/`, `src/apple_intelligence.*`:
  dual-track meeting capture, rolling pause-cut Parakeet transcription yielding to
  dictation, sample-timeline echo suppression, chapters/notes, retention, and read-only
  `meeting::query` + `hex mcp` ([`docs/features/meetings.md`](docs/features/meetings.md)).
- `src/capture_host/`, `crates/capture/`, `crates/capture-core/`, `crates/router/`: Annotate
  screen/window/area/iPhone capture (`cfg(hex_capture)`, `scrawl-capture` excluded from the
  Cargo workspace), `.hexannotation` packages, dictation-timeline audio sink, and in-process
  OpenCode session routing ([`docs/hex-annotation.md`](docs/hex-annotation.md),
  [`docs/features/capture.md`](docs/features/capture.md)).
- `src/rune_ui/`, `src/rune_hud/`, `src/macos_rune/`, `src/windows/`, `src/linux/`,
  `src/desktop/`: shared Rune window/onboarding/capsules, click-through GPU Dot-scope HUD
  (`src/rune_hud.wgsl`), and single-instance menu-bar/tray/daemon hosts
  ([`docs/linux.md`](docs/linux.md), [`docs/windows.md`](docs/windows.md),
  [`docs/nix.md`](docs/nix.md)).
- `src/app_settings/`, `src/app_paths.rs`, `src/control/`, `src/local_api/`,
  `src/personal_commands/`, `src/net/`, `src/updater/`, `src/policy.rs`: resilient
  `settings.json`, data-root paths, bearer-authenticated loopback API + `hex` CLI
  ([`docs/cli.md`](docs/cli.md), [`docs/transcript-cli.md`](docs/transcript-cli.md)),
  QuickJS user transforms, native TLS/proxy HTTP ([`docs/downloads.md`](docs/downloads.md)),
  release-service updater
  ([`docs/plans/release-service.md`](docs/plans/release-service.md)), and macOS
  managed-preference locks (`site/public/docs/admin.md`).
- `sdk/typescript/` (`@kitlangton/hex`,
  [`docs/specs/local-transcription-service.md`](docs/specs/local-transcription-service.md)),
  `site/` (Cloudflare Worker for `anoma.ly/hex` and `/hex/changelog`), `perf/` (strobe
  benches, [`docs/perf.md`](docs/perf.md)), `ios/` and `android/` (Hex Mobile; shared
  `.hexwords` contract in [`docs/specs/hex-words.md`](docs/specs/hex-words.md)).

## Cross-Cutting Invariants

- **Code shape (`scripts/code-shape.py`):** Keep source files under 800 non-test lines
  (1,500 hard cap; inline `#[cfg(test)]` excluded; test files warn over 1,000). Ceilings in
  `scripts/file-size-exceptions.txt` only ratchet down
  (`python3 scripts/code-shape.py --update-exceptions`).
- **Startup & platform guards:** GUI startup must call `keyboard::initialize_layout` on the
  OS main thread before settings, keyboard-using workers, or Rune start
  (`rune::prewarm_gpu()` runs on macOS app launches only, never CLI commands). macOS `main`
  sets `GGML_METAL_TENSOR_DISABLE` first (avoids M5 Metal tensor-API kernel bugs) and exits
  through `desktop/process_exit` (drains workers under a 5 s deadline, then calls `_exit` so
  C++/Metal static destructors never race live threads). Linux `main` sets `DISABLE_RTKIT`
  before any ALSA/PipeWire stream opens.
- **Real-time capture & single model owner:** Audio capture is lossless against UI, context,
  inference, and paste stalls; queues stay bounded and accepted dictation jobs finish in
  submission order without cancelling or blocking new captures. One `TranscriptionEngine`
  thread owns the loaded dictation model; meeting chunks, Annotate word chunks, and
  `/streams` yield to dictation and never load a second copy of the dictation model. Widen
  `GGML_VULKAN_SHADER_TYPES` in `.cargo/config.toml` before adding a catalog model whose
  weights use tensor types beyond `f32,f16,q8_0,q5_0`. Cohere chunks stay capped at 35 s.
- **Settings resilience & CLI parity (`src/app_settings/persistence.rs`):** Settings errors
  never prevent launch. Invalid fields default in memory and back up the exact original to
  `settings.json.unreadable-<sha256>` (owner-only) before any save; non-object or
  invalid-JSON files refuse every save until restart. Keep saved JSON backward-compatible
  (omit unchosen `meetings.retention`; never omit optional fields like `paste_last_hotkey`
  whose absence restores a nonempty default; preserve missing custom-model selections while
  falling back at runtime). Every persisted setting requires a `hex settings` key
  (`every_persisted_setting_has_a_cli_key`), and breaking `/control` or transcript CLI JSON
  changes must bump `CONTROL_VERSION` / schema versions ([`docs/cli.md`](docs/cli.md),
  [`docs/transcript-cli.md`](docs/transcript-cli.md)). Read-only CLIs (`hex history`, `hex
  meetings list|show|chapters|search`, `hex mcp`) and `/control` callers dispatch before
  process logging, instance locks, GUI, or permissions, and never launch Hex (only `hex
  start` and `hex restart` launch it).
- **Paths, network, OpenCode, and subprocesses:** Derive user/data/document directories
  through `app_paths` (and `scrawl_router::user_dirs` in Annotate crates), never `dirs`
  directly (on Windows `dirs` ignores redirected `USERPROFILE`/`APPDATA`/`LOCALAPPDATA` in
  tests and benchmarks). Make all outbound HTTPS requests through `src/net` (OS TLS, system
  proxy, resumable ranged downloads), never by shelling out to `curl`. Construct child
  processes through `desktop::background_process` (suppresses console flashes on Windows).
  Never start, probe, or search for an OpenCode binary; connect only to an already running
  service registered in `opencode/service*.json` via `desktop/opencode_service`
  ([`docs/opencode-compatibility.md`](docs/opencode-compatibility.md)). User `hex.config.ts`
  scripts reach `fetch` or `node:child_process` only after exact content-hash approval
  (`personal_commands/trust`).
- **Privacy & data liability:** Never persist captured audio by default
  (`HEX_RETAIN_DICTATION_AUDIO` and Labs › Keep recordings are explicit owner-only opt-ins).
  Event logs (`live.ndjson`, `process.log`, owner-only and trimmed in place under 10 MB)
  never record dictated text, full URLs, window titles, or transform payloads — `History`
  (7-day default retention, 10k entries / 16 MiB hard caps) is the only dictation text
  store. Meeting expiry and `hex meetings delete` move folders to the Trash (never `rm`;
  expiry never touches exported Markdown). Hugging Face cache scans (`found_models`) happen
  on demand, header-only and read-only. Keep private hostnames, devices, paths, accounts,
  and internal model names out of commits, PRs, fixtures, and screenshots.
- **Wayland & Linux boundaries ([`docs/linux.md`](docs/linux.md)):** Keep one general
  Wayland path (portals + `wl-copy` + `wtype`/RemoteDesktop; no compositor-specific code,
  raw input devices, or `uinput`, and XWayland only for the input-free HUD or explicit
  `gnome-*` Labs while XWayland already runs). Bind all GlobalShortcuts in one session with
  one `BindShortcuts` call (KDE wipes shortcuts on a second session) and never bind Escape
  on the portal.
- **Release & updater security boundary ([`docs/releasing.md`](docs/releasing.md),
  [`docs/windows-signing.md`](docs/windows-signing.md),
  [`docs/test-builds.md`](docs/test-builds.md)):** Build jobs run repository code with zero
  secrets or OIDC tokens; sign/publish/deploy jobs (`release`, `dev`, `production`
  environments) sparse-checkout only `.github/release/`, pin every action by SHA, verify
  `SHA256SUMS` digests, and treat build artifacts strictly as data. Never overwrite
  published artifacts with different bytes, never escalate privileges in updaters, and
  **never delete `docs/releases/*.md`** (they generate `/hex/changelog` and gate stable
  releases). Onboarding gates every stable release (`scripts/check-onboarding.sh`).

## Development & Verification

```sh
./scripts/setup.sh && ./scripts/setup-parakeet.sh
scripts/check                          # fmt, clippy, code-shape, and cargo-nextest (read full output)
scripts/check meeting::query           # fmt, then matching nextest cases only
scripts/check labs control::settings   # required before merging any lab or persisted setting
HEX_UPDATE_LABS_JSON=1 scripts/check labs::web  # regenerate site/src/labs.json after lab changes
cargo run -- status --json             # query running Hex over /control (docs/cli.md)
cargo run -- app -- --list-scenes
cargo run -- app -- --snap /tmp/hex-settings.png --scene mac-settings --theme dark
./scripts/test-app-identity.sh         # macOS bundle identity check
./scripts/test-install-linux-release.sh  # Linux installer + signature check (x86_64/aarch64)
```

- **Tests & headless UI:** Always run `scripts/check [filter]` rather than raw `cargo test`
  (`.config/nextest.toml` bounds hung tests). `--snap` renders fixture `SCENES` in
  `src/rune_ui.rs` without starting the runtime or touching user data (on GPU-less Linux,
  set `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`). Never play live audio or
  wake displays during automated tests; `HEX_AUDIO_FIXTURE=<wav>` feeds real-time fixture
  audio without opening a microphone. Use `termctrl` to check `hex dev dashboard` TUI
  changes.
- **Benchmarks ([`docs/perf.md`](docs/perf.md)):** `strobe.json` and `perf/*.bench.ts` drive
  release builds through AccessKit element keys (keep benchmarked keys stable;
  [`docs/features/accessibility.md`](docs/features/accessibility.md)) using isolated
  `HEX_APPLICATION_SUPPORT_DIR`, `HEX_DISABLE_UPDATES=1`, and redirected
  `HOME`/`USERPROFILE`. Add a benchmark case alongside performance changes.
- **TypeScript SDK (`sdk/typescript`, `@kitlangton/hex`):** User-facing SDK changes require
  a Changeset under `sdk/.changeset/`. From `sdk/`, run `bun install --frozen-lockfile`,
  then in `sdk/typescript/` run `bun run check && bun run test && bun run build && npm pack
  --dry-run`. Releases publish via Changesets and `.github/workflows/release-typescript.yml`
  (`--latest=false`), never direct `npm publish`.

## Diagnostics

- **macOS:** `~/Library/Application Support/hex/` (`logs/live.ndjson`, `logs/process.log`,
  `meetings/`, `annotations/`)
- **Windows:** `%APPDATA%\hex` for settings/install state; `%LOCALAPPDATA%\hex` for
  machine-local logs, models, and annotations
- **Linux:** `~/.local/share/hex` (or `$XDG_DATA_HOME/hex`)
