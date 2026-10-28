# Experiments

Experiments are unfinished features whose code sits on `dev` but stays
dormant until someone turns one on for their own Hex. They are **unsupported**:
an experiment may misbehave, change, or disappear without notice, and the
window offers no switch for them. There is a flag, not a setting. Panes an
experiment adds list under **Experiments** at the bottom of the sidebar.

```ts
Turn an experiment on                    // experiments.sources
├── HEX_EXPERIMENTS=live-chapters,vocabulary  // commas or spaces; this process and its children
├── ~/.config/hex/experiments            // one name per line, # comments; every launch
│   ├── Hex Test.app -> ~/.config/hex-test/experiments
│   └── Windows / Linux -> %USERPROFILE%\.config\hex, ~/.config/hex
├── On = named in either source (union)
├── Unknown name -> Warning in process.log, ignored    // never fatal
└── Missing file -> Nothing on

Launch                                   // experiments.snapshot
├── Read both sources once -> Fixed for the life of the process
├── process.log -> "experiments enabled" or "no experiments enabled"
└── Edit a source -> Applies at the next launch

hex experiments                          // experiments.cli; macOS, Windows, Linux
├── list [--json] -> Each experiment, on/off, source (env, file, env+file)
├── enable <name> -> Add it to the file atomically; print the restart hint
├── disable <name> -> Remove every line naming it; keep comments and others
└── Unknown name -> Error listing the known experiments
```

The known experiments are compiled into
[experiments.rs](../../src/experiments.rs):

- `capture`: Scrawl's capture engine hosted in Hex (macOS; see
  [Captures](capture.md)). It is a build option, off in every public build:
  only a build with `--features capture` (`HEX_CAPTURE=1
  scripts/build-app.sh` for an app) knows the name. Any other build's `list`
  omits it, `enable` rejects it, and a source naming it is ignored. The
  gate is where the work starts: the listener builds no capture host, so there
  is no sink, thread, or bundle folder while it is off. On, it adds
  Settings › Experiments › Scrawl, whose own opt-in (`capture.enabled`,
  default off) is the second gate. Check:
  `capture_exists_only_with_its_build_option` in
  [experiments.rs](../../src/experiments.rs).
- `live-chapters`: chapter a meeting's transcript live while it records
  (macOS, Apple Intelligence; see
  [Live Chapters](meetings.md#live-chapters-experiment-live-chapters)). The
  gate is where the work starts: the recorder's chapter worker, recording
  chapters in the manifest, and the boundary pass after Stop.
- `playground`: Rune's playground in the window (every OS). ⇧⌘D (View ›
  Playground on macOS) or Ctrl+Shift+D swaps the window for every fixture
  scene, grouped by pane (the tour, the model picker, and the shortcut
  explainer each get their own group), and, in builds with captures, the
  capture capsule's states as stories with its flow, tuning, and log. Each
  story starts from a fresh fixture; the real window keeps running
  underneath. While it is on, the window resizes everywhere (Linux and the
  macOS tour otherwise keep one fixed size), so the stage beside the
  playground's columns has room. Off, nothing is attached: no shortcut, menu item, or scene list.
  Headless renders take `--playground [story]` while it is on, e.g.
  `HEX_EXPERIMENTS=playground hex app -- --snap out.png --playground
  "General/mac-settings"`. Check: `every_scene_but_live_lists_under_its_pane`
  in [playground.rs](../../src/rune_ui/playground.rs).
- `smart-turn`: finish hands-free dictation when Pipecat's Smart Turn model
  hears the end of a turn (macOS; see
  [Smart Turn](dictation.md#smart-turn-auto-stop-experiment-smart-turn)). The
  gate is where the work starts: the recognition loop builds no tracker and
  reads no settings, so nothing downloads, loads, or runs while it is off.
  Until the model is ready, and if it fails, dictation behaves as if it were
  off.
- `vocabulary`: a shared list of names and their likely mis-transcriptions,
  for rewriting and search ([Vocabulary](vocabulary.md)). It adds a Settings
  card and the `hex vocabulary` commands, which appear only while it is on.
- `voice-filter`: the Voice Filter pane (macOS), the only home of the Hex
  Microphone driver, its filter modes, and Try it
  ([Hex Microphone](README.md#experimental-hex-microphone-and-voice-filter)).
  It lists under Experiments while the experiment is on, and also whenever
  `experimental.hex_microphone` is true or the driver is installed, so nobody
  loses the controls for a driver they already use. Check:
  `voice_filter_needs_its_experiment_or_hex_microphone` in
  [panes.rs](../../src/rune_ui/tests/panes.rs).

`hex experiments` reads and edits only the sources; it runs before process
logging, the instance lock, the GUI, or permissions, and never talks to a
running Hex. `list` shows what the next launch would read from this shell's
environment and the file. What the running Hex actually read is in
process.log and in `hex status`: on macOS an `Experiments` line (only when
one is on) and `experiments` in `--json` (always, possibly empty); on Linux
`experiments` in its JSON. Windows `hex status` prints the sources' current
names first.

```ts
HEX_EXPERIMENTS reaches Hex              // experiments.env
├── Hex started from that shell (hex app, cargo run) -> Yes
├── macOS hex start / hex restart with Hex not running -> Yes // caller's HEX_* via open --env
├── macOS hex restart / setup relaunch -> The running Hex's own HEX_* // not the caller's
├── Windows update restart -> Inherited
└── Login Items, Finder, Dock, the Linux systemd unit -> No; only the file
```

Use the file for a lasting change; the environment suits a one-off launch.
Debug builds read the same sources: `DEVELOPER_FEATURES_ENABLED` turns no
experiment on.

Gate an experiment where its work starts (the worker, thread, or capture),
with `experiments::enabled(Experiment::X)`, not only where its UI appears.

Checks: the tests in [experiments.rs](../../src/experiments.rs) cover parsing
both sources (comments, unknown names, duplicates, the union), file edits
(creation, idempotence, removal, a symlinked file, no leftover temporary
file), and the list output; `experiments_live_beside_hex_config_ts_in_each_builds_own_folder`
in [app_paths.rs](../../src/app_paths.rs) covers the test build's folder;
`experiments_parse_as_a_read_command` in [main.rs](../../src/main.rs) covers
the macOS CLI; `status_reads_for_a_person` and
`status_reports_experiments_and_reads_older_replies_without_them` in
[control/cli.rs](../../src/control/cli.rs) cover the macOS status text and
JSON. Passed on macOS on 2026-10-23. The Linux and Windows dispatch and status
additions are not built or run by these checks, and no running Hex was asked
for its status.
