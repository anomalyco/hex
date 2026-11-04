# Performance benchmarks

Hex is benchmarked with [strobe](https://github.com/Hona/strobe) as a native
app. strobe builds two variants (for example `origin/dev` and your checkout),
launches each in a fresh private home, and compares them in alternating runs.

```sh
strobe bench perf/idle.bench.ts --base origin/dev --head .
strobe bench perf/launch.bench.ts --base origin/dev --head .
strobe bench perf/idle.bench.ts --check   # one short, untimed pass
strobe report experiment <exp_…> --format pr   # tables for the PR body
```

Run timed benches on a quiet machine. Close other apps and keep the session
unlocked.

## What runs

- `strobe.json` builds `hex` with `cargo build --release` into
  `{cache}/cargo-target`, a target folder strobe shares between variants.
  The build output is `hex.exe` on Windows and `hex` on macOS and Linux.
- The build's env keeps release's code and adds line tables in `hex.pdb`, so
  CPU profiles name Hex's functions: `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`,
  `CARGO_PROFILE_RELEASE_STRIP=false`, and `CARGO_PROFILE_RELEASE_INCREMENTAL=false`.
  It needs no profile or feature in `Cargo.toml`, so every commit builds the
  same way: a `--base` from before the benches existed, and `--bisect` across
  history. Such a build is never shipped.
- On a machine whose default Rust toolchain is not MSVC, set
  `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc` before you run strobe.
  Windows builds also need the Vulkan SDK (`docs/windows.md`).

## Isolation

The `hex` app profile in `strobe.json` keeps a launched copy away from your
own Hex:

- `HEX_APPLICATION_SUPPORT_DIR={home}/hex-data`: settings, history, logs,
  models, locks, and `local-api.json` live in the private home.
- A data root other than the default claims its own Windows single-instance
  event, so the copy never signals or yields to your running Hex, and on
  Linux never hands the launch to your systemd unit. Inside a redirected
  home the default (`AppData/Roaming/hex`, `.local/share/hex`) follows the
  redirection, so name another folder, as `hex-data` does.
- `HEX_DISABLE_UPDATES=1`: no update checks and no network.
- `USERPROFILE` (Windows) and `HOME` locate `~/.config/hex`, so your
  `hex.config.ts` stays out of the copy.
- Every folder Hex and Annotate derive from yours follows the copy's
  environment on Windows too (`USERPROFILE`, `APPDATA`, `LOCALAPPDATA`;
  `app_paths` and `scrawl_router::user_dirs`), where `dirs` would ask the
  shell for your own: Annotate's captures folder and its retention, Scrawl's
  router folders and the Scrawl settings import, the meetings folder, and the
  Hugging Face cache. Annotate stays on in the benches, as it is for users,
  and works only inside the private home.
- `HF_HUB_CACHE={home}/.cache/huggingface/hub`: the copy never finds models
  in your Hugging Face cache (on Windows that scan otherwise reads your real
  profile).
- The seeded `settings.json` keeps the listener off (`enable_on_launch: false`):
  no keyboard hook, no microphone, and no model load. History is kept
  forever, so a seeded `history.json` never expires mid-run. The seeded
  `onboarding-complete` marker skips first-run setup. The two `scrawl-*`
  migrations skip importing Scrawl's settings from your home.
- `guard` stops a run if your own `settings.json` or `local-api.json`
  changes, or your Hex exits. Each guard names its OS.
- macOS: a variant runs as a bare `hex` executable, not `Hex.app`, so it has
  no bundle id and shares no Preferences, saved state, or permissions with
  your installed Hex. `moved` tells strobe that the copy keeps its data in
  the private home (`HEX_APPLICATION_SUPPORT_DIR`), not in your
  `~/Library/Application Support/hex`, so your own Hex there does not stop
  the bench. strobe brings the copy's window to the front only after a
  minute without keyboard or mouse input.

The copy loads no model, except in the listening cases below.

## Cases

| File | Case | Primary | Measures | Locked session |
| --- | --- | --- | --- | --- |
| `perf/idle.bench.ts` | `window-open` | `cpu.work` | 8 s at rest with the window on General | waits for an unlock |
| `perf/idle.bench.ts` | `window-closed` | `cpu.work` | 8 s at rest, tray only (`app --hidden`) | runs |
| `perf/idle.bench.ts` | `window-closed-memory` | `memory.app.private` | The same tray's private memory and dedicated GPU memory (`gpu.vram`) | runs |
| `perf/launch.bench.ts` | `cold-launch` | `launch.ready` | Process start to the first painted window | waits for an unlock |
| `perf/launch.bench.ts` | `hidden-launch` | `launch.ready` | Process start to `local API started`, tray only | runs |
| `perf/listening.bench.ts` | `listening-at-rest` | `cpu.work` | 8 s listening at rest, microphone released and open | runs |

Secondary metrics are thread wakeups (`cpu.wakes`), GPU use, and private
memory. `strobe query <rec> threads` names the threads; Hex names every
long-lived thread, and the window's thread is `hex-ui` on Windows.

Tray cases use `window: false`: strobe neither waits for nor checks a window,
so they also run while the session is locked. Cases with a window, or with
presented frames, wait for an unlock.

To see where one case spends its time, record it once:

```sh
strobe record --bench perf/idle.bench.ts#window-closed --head .
strobe query <rec> threads
```

## Listening cases

`perf/listening.bench.ts` runs Hex with its listener on: the
`hex-listening` profile (microphone released while idle) and
`hex-listening-warm` (microphone open, the default). The copy installs its
keyboard hook and loads the default English model, which
`perf/seeds/listening.ts` copies from this machine's own Hex install into
`perf/seeds/.generated`. strobe shares a seed file this large (64 MB and up)
read only between launches instead of copying it into each private home. The warm profile opens the machine's real microphone; neither records.
Leave the dictation shortcut alone while it runs.