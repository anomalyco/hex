# Performance benchmarks

Hex is benchmarked with [strobe](https://github.com/Hona/strobe) as a native
app. strobe builds two variants (for example `origin/dev` and your checkout),
launches each in a fresh private home, and compares them in alternating runs.

```sh
strobe bench perf/idle.bench.ts --base origin/dev --head .
strobe bench perf/launch.bench.ts --base origin/dev --head .
strobe bench perf/idle.bench.ts --check   # one short, untimed pass
```

Run timed benches on a quiet machine. Close other apps and keep the session
unlocked.

## What runs

- `strobe.json` builds `hex` with `--profile strobe --features strobe` into
  `{cache}/cargo-target`, a target folder strobe shares between variants.
  Today the build output is the Windows executable.
- The `strobe` profile is release's code with line tables kept in `hex.pdb`,
  so CPU profiles name Hex's functions. It is never shipped.
- The `strobe` feature is the hook for the UI toolkit's benchmark bridge. It
  changes nothing in Hex yet.
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
- `HF_HUB_CACHE={home}/.cache/huggingface/hub`: the copy never finds models
  in your Hugging Face cache (on Windows that scan otherwise reads your real
  profile).
- The seeded `settings.json` keeps the listener off (`enable_on_launch: false`):
  no keyboard hook, no microphone, and no model load. History is kept
  forever, so a seeded `history.json` never expires mid-run. The seeded
  `onboarding-complete` marker skips first-run setup. The two `scrawl-*`
  migrations skip importing Scrawl's settings from your home.
- `guard` stops a run if your own `settings.json` or `local-api.json`
  changes, or your Hex exits.

The copy loads no model. A future case that dictates must seed one into
`hex-data/models`.

## Cases

| File | Case | Primary | Measures |
| --- | --- | --- | --- |
| `perf/idle.bench.ts` | `window-open` | `cpu.work` | 8 s at rest with the window on General |
| `perf/idle.bench.ts` | `window-closed` | `cpu.work` | 8 s at rest, tray only (`app --hidden`) |
| `perf/launch.bench.ts` | `cold-launch` | `launch.ready` | Process start to the first painted window |

Secondary metrics are thread wakeups (`cpu.wakes`), GPU use, and private
memory. `strobe query <rec> threads` names the threads; Hex names every
long-lived thread, and the window's thread is `hex-ui` on Windows.

## Listening cases

`perf/listening.bench.ts` runs Hex with its listener on: the
`hex-listening` profile (microphone released while idle) and
`hex-listening-warm` (microphone open, the default). The copy installs its
keyboard hook and loads the default English model, which
`perf/seeds/listening.ts` copies from this machine's own Hex install into
`perf/seeds/.generated` (read only; strobe copies the seed into each private
home). The warm profile opens the machine's real microphone; neither records.
Leave the dictation shortcut alone while it runs.