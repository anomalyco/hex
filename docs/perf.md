# Performance benchmarks

Hex is benchmarked with [strobe](https://github.com/Hona/strobe) as a native
app. strobe builds two variants (for example `origin/dev` and your checkout),
launches each in a fresh private home, and compares them in alternating runs.

```sh
strobe bench perf/idle.bench.ts --base origin/dev --head .
strobe bench perf/launch.bench.ts --base origin/dev --head .
strobe bench perf/idle.bench.ts --check   # one short, untimed pass
strobe bench perf/idle.bench.ts --filter "window-closed*" --base origin/dev --head .
strobe report experiment <exp_…> --format pr --update-pr <n>   # the results block in PR n
```

`--filter` and `--target` take names, comma lists, and globs.

## In a pull request

Write the PR's own text yourself: what strobe showed that led to the change
(the command and recording id), and what the change does. Then let strobe
place its evidence:

```sh
strobe report experiment <exp_…> --format pr --update-pr <n>
```

It adds one generated block to PR `n`'s body, or replaces that experiment's
block after a rerun, and leaves the rest of the body alone. Don't edit inside
the block (strobe refuses to update a hand-edited one); put your reading of
the numbers above or below it. Without `--update-pr`, the block goes to
stdout for pasting.

Window cases name the screen they measure with `expect` (the window title
and the pane's heading). A block on another screen, such as a first-run
dialog over the pane, is refused `WrongScreen` instead of timed, and the
block's `On screen:` line says what showed.

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
- On Windows, `build.windows` sets `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`,
  so a machine whose default Rust toolchain is GNU still builds with MSVC
  (GNU fails with "dlltool not found"). Windows builds also need the Vulkan
  SDK (`docs/windows.md`).
- Each variant keeps the GPU driver's compiled shaders across its own
  launches (strobe's default, `gpuCache: "warm"`), as a machine that ran Hex
  before has them. Base and head never share them, and discarded warm-up
  launches fill them before the timed pairs. Without them, every launch
  compiled Hex's shaders again: a dictation spent 4.6 to 5.3 s of each 6 s
  run in the driver's compiler. So `cold-launch` is a returning user's launch
  (Hex not running, shaders cached). A first launch after an install or a
  driver update would need a target with `gpuCache: "cold"`.

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
  minute without keyboard or mouse input. Every profile records the
  `anomaly-hex-identity` migration: without it, a copy whose setup isn't
  ready (the copy has no Microphone grant) reopens the first-run tour over
  the window, and every click lands on the tour.

The copy loads no model, except in the listening and dictation cases below.

## Cases

| File | Case | Primary | Measures | Locked session |
| --- | --- | --- | --- | --- |
| `perf/idle.bench.ts` | `window-open` | `cpu.work` | 8 s at rest with the window on General | waits for an unlock |
| `perf/idle.bench.ts` | `window-closed` | `cpu.work` | 8 s at rest, tray only (`app --hidden`) | runs |
| `perf/idle.bench.ts` | `window-closed-memory` | `memory.app.private` | The same tray's private memory and dedicated GPU memory (`gpu.vram`) | runs |
| `perf/launch.bench.ts` | `cold-launch` | `launch.ready` | Process start to the first painted window | waits for an unlock |
| `perf/launch.bench.ts` | `hidden-launch` | `launch.ready` | Process start to `local API started`, tray only | runs |
| `perf/listening.bench.ts` | `listening-at-rest` | `cpu.work` | 8 s listening at rest, microphone released and open | runs |
| `perf/dictation.bench.ts` | `recording` | `cpu.wakes` | 6 s of one dictation recording a fixture, microphone open and released | runs |
| `perf/interaction.bench.ts` | `switch-panes` | `ui.cpu` | General → History → Output → General, each click until the next pane's content is in the accessibility tree | waits for an unlock |
| `perf/interaction.bench.ts` | `history-scroll` | `ui.cpu` | History's list scrolled down 6,000 points and back at 3,000 points a second | waits for an unlock |
| `perf/interaction.bench.ts` | `history-search` | `ui.cpu` | "cache key" typed into History's search, a key every 120 ms; each key searches 10,000 entries | waits for an unlock |
| `perf/interaction.bench.ts` | `model-picker` | `ui.cpu` | The speech model picker opened, its list of every model, back to the comparison, and closed | waits for an unlock |

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

## Dictation cases

`perf/dictation.bench.ts` runs whole dictations with no microphone and no
paste. The `hex-dictation` profiles set `HEX_AUDIO_FIXTURE` to `speech.wav`,
which `perf/seeds/dictation.ts` makes with this machine's own
text-to-speech voice (one sentence at 48 kHz, then 2 s of silence) beside
the model the listening seed links. Every microphone open in the copy plays
that file instead, from its start, in 10 ms chunks at real-time pace, and
no device is opened, so nothing is recorded.

The bench drives the copy through its local API as `hex dictation` does.
Each dictation starts as the sentence begins (with the microphone open,
when the looping file comes round) and ends with `dictation stop --print`,
which returns the processed text instead of pasting it. The profiles move
the copy's shortcuts to F19 and F18 and unbind Paste Last, so a key pressed
while a bench runs on a machine in use reaches nothing.

Only bench builds that have `HEX_AUDIO_FIXTURE`. An older build ignores it
and opens the real microphone; setup then refuses the block, but only
after the microphone was open.

## Interaction cases

`perf/interaction.bench.ts` uses the Settings window the way a person does.
strobe drives it from outside, through what Windows gives assistive
technology: it finds each control in Hex's accessibility tree (UI
Automation, which Rune builds with AccessKit) and posts mouse and key input to
the window. Hex needs no build feature, flag, or bridge for this; a release
build answers the same way for a screen reader.

- Selectors are element keys, which the tree exposes as UI Automation
  `AutomationId`s: `nav-history`, `history-search`, `transcription-control`.
  A key path (`model0/choose-model`) names a key inside a keyed ancestor.
  `role=<role>` and `text=<name>` also work. Keep the keys these cases use
  stable; `docs/features/accessibility.md` lists them.
- Each click waits (`until`) for a key that only the next view has, so a run
  ends when the change is in the tree, not after a fixed wait.
- `interaction.*` counts an input only when the accessibility tree changed
  within a second of it, so the change shows to strobe as well as on screen.
- The home holds a full History: 10,000 dictations, Hex's cap, from
  `perf/seeds/history.ts` (written once into `perf/seeds/.generated/history`).
  The list shows the newest 200, as search results do.
- Choosing a model is not a case: it downloads or loads a model, which costs
  the network and the model, not the window. `model-picker` measures the
  picker's own views.
- They need an unlocked session: Windows hit-tests every point to the lock
  screen while it is locked, so strobe refuses pointer input then.

```sh
strobe bench perf/interaction.bench.ts --base origin/dev --head .
strobe bench perf/interaction.bench.ts --filter "history-*" --check   # untimed smoke run
```
