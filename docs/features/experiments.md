# Experiments

Experiments are unfinished features whose code sits on `dev` and can be turned
on in dev, local, and test builds (`experiments::is_dev_build()`). Production
builds (`!experiments::is_dev_build()`) ignore `HEX_EXPERIMENTS` and
`~/.config/hex/experiments` completely, hide Annotate, Voice Filter, and every
experiment, and on macOS show **Labs** with only the opt-in `Meetings (beta)`
switch (`meetings.enabled`, default off; hidden on Linux and Windows where
there is no release Lab).

In dev builds, **Labs** at the bottom of the sidebar adds Annotate and live
toggle switches for every experiment supported on the current OS, writing
`~/.config/hex/experiments` atomically and applying changes live where
supported (such as showing or hiding `Voice Filter` immediately). Panes an
active experiment adds list indented under **Labs**.

```ts
Production vs dev builds                 // experiments.dev-build
├── Production (!is_dev_build()) -> Ignore env & file; macOS Labs has only Meetings (beta); Linux/Windows hide Labs
└── Dev / local / test (is_dev_build())
    ├── Labs › Experiments switches -> Write ~/.config/hex/experiments + apply live
    ├── HEX_EXPERIMENTS=live-chapters,live-text   // commas or spaces; this process
    ├── ~/.config/hex/experiments        // one name per line, # comments
    │   ├── Hex Test.app -> ~/.config/hex-test/experiments
    │   └── Windows / Linux -> %USERPROFILE%\.config\hex, ~/.config/hex
    ├── Unknown name -> Warning in process.log, ignored    // never fatal
    └── Missing file -> Nothing on

hex experiments                          // experiments.cli; dev builds only
├── list [--json] -> Each experiment, on/off, source (env, file, build, or a mix)
├── enable <name> -> Add it to the file atomically; print the restart hint
├── disable <name> -> Remove every line naming it; keep comments and others
└── Unknown name -> Error listing the known experiments
```

The known experiments are compiled into
[experiments.rs](../../src/experiments.rs):

- `capture` is retired: [Annotate](capture.md) is gated by `is_dev_build()` and
  its `capture.enabled` switch in Labs. A file or variable naming it is ignored.
- `modes` is retired: the Output tab (`Pane::Output`) and per-app Modes
  processing are always on in the main sidebar (`General`, `Output`, `History`)
  in both production and dev builds. A file or variable naming it is ignored.
- `smart-turn` is retired and its ONNX Runtime predictor was removed on
  2026-10-29. A file or variable naming it is ignored silently.
- `vocabulary` is retired: [Vocabulary](vocabulary.md) became a General
  card and the `hex vocabulary` commands on 2026-10-29. A file or variable
  naming it is ignored silently.
- `dot-scope` is retired: [Dot scope](dictation.md#dot-scope) became the
  HUD on every OS on 2026-10-29, and the capsule it replaced is gone. A file
  or `HEX_EXPERIMENTS` that still names it is ignored silently. Check:
  `retired_experiments_are_ignored_silently` in
  [experiments.rs](../../src/experiments.rs).
- `live-chapters`: chapter a meeting's transcript live while it records
  (macOS, Apple Intelligence; see
  [Live Chapters](meetings.md#live-chapters-experiment-live-chapters)). The
  gate is where the work starts: the recorder's chapter worker, recording
  chapters in the manifest, and the boundary pass after Stop.
- `new-onboarding`: the onboarding story as the real first run on macOS
  (`rune_ui/onboarding_kit/live.rs`): the real model download, Microphone and
  Accessibility rows that follow macOS, and a practice dictation with the
  real shortcut and the HUD on the live microphone level. The window's tour
  stays the state machine underneath. `Hex Test.app` builds turn it on
  (source `build`). Labs' Restart onboarding and `hex onboarding reset` turn
  it on for the install and replay the first run without touching settings,
  history, models, or permissions; ready steps pass at once. Not yet: words
  flying into the field.
- `live-text`: a streaming dictation shows what you have said so far while
  you dictate, as one click-through line just past the recording HUD (every
  OS; see [Streaming Finish](dictation.md#streaming-finish)). The gate is
  where the work starts: with it off, the engine gives a hotkey capture's
  live stream no partial slot and never reads the model's text between
  feeds, and no host adds the line's overlay. Each dictation's `finished
  live stream` line in process.log counts its text changes (`partials`);
  `RUST_LOG=hex=debug` adds a `live text` line per change with the audio
  fed and the word counts, never the words. Checks:
  `a_dictation_stream_publishes_its_text_after_each_feed_with_live_text`,
  `without_live_text_a_dictation_stream_publishes_nothing`,
  `each_dictation_stream_has_its_own_text`, and
  `a_failed_feed_clears_the_text_and_stops_publishing` in
  [transcription_engine.rs](../../src/transcription_engine.rs);
  `live_text_follows_the_recording_and_never_outlives_it` in
  [dictation.rs](../../src/dictation.rs); the three tests in
  [live_text.rs](../../src/live_text.rs) (a recording's end clears only its
  own text); `a_spot_places_the_plate_just_past_the_hud`,
  `a_short_line_hugs_its_words_in_the_middle`,
  `a_long_line_stays_inside_the_overlay_on_one_line`, and
  `no_words_draw_nothing` in
  [rune_ui/live_text.rs](../../src/rune_ui/live_text.rs), whose opt-in
  `snapshots` renders the line in light and dark; and
  `live_text_hangs_just_past_the_plate_wherever_it_sits` in
  [hud_placement.rs](../../src/hud_placement.rs). The line has no golden
  and no `--snap` scene (overlays have none), and nothing headless checks
  how it appears and hides on screen.
- `playground`: Rune's playground in the window (every OS). ⇧⌘D (View ›
  Playground on macOS) or Ctrl+Shift+D swaps the window for a list of
  folded groups, nested by path:

  ```ts
  Prototypes        stories that aren't the window
  ├── Onboarding    the options and First run
  ├── Meetings      Live notes: a canned call's chapters and notes under a
  │                 draggable capsule, hidable, copied whole, new, or by
  │                 chapter (nothing real runs; live_notes.rs)
  ├── HUD           (macOS) each placement × look × ending, Processing, Your voice + live HUD Lab Dot scope sliders & Save
  └── Capture capsule   (macOS) whole flow and each state × flow, notch, family
  Window            every macOS-window fixture scene by pane, under a readable name
  ├── General       Settings, Microphone missing, Shortcut/…, Updates/…
  ├── Output, Modes … HUD Lab, Activity, Transcription model, Shortcut explainer, Onboarding tour
  Linux & Windows   the portal and Windows fixtures
  ```

  Every group starts folded; opening a story, from the list or a link,
  unfolds the groups above it, and folds hold until Hex quits. The list
  keeps only its own keys (arrows, Return, J/K, a story's trigger letters,
  Tab, Esc, `/` to filter) and ⇧⌘D; every other key, ⌘-chords and digits
  included, reaches the open story. Each
  story starts from a fresh fixture; the real window keeps running
  underneath. While it is on, the window resizes everywhere (Linux and the
  macOS tour otherwise keep one fixed size), so the stage beside the
  playground's columns has room. Off, nothing is attached: no shortcut, menu item, or scene list.
  Headless renders take `--playground [story]` while it is on, e.g.
  `HEX_EXPERIMENTS=playground hex app -- --snap out.png --playground
  "Window/General/Settings"`. Checks: `every_scene_but_live_has_a_path_under_a_group`,
  `the_playgrounds_keys_work_with_a_story_open`, and
  `a_scene_opened_from_the_list_hears_command_digits` in
  [playground.rs](../../src/rune_ui/playground.rs).
  First run's side controls tune rise, leave, travel, stagger and keyboard
  arrival; Quick motion and Gentle motion are presets. The real first run
  uses the default values from
  [landing.rs](../../src/rune_ui/onboarding_kit/landing.rs): zero-bounce
  vertical entrances/exits, with each stagger delaying the whole entrance.
  The demonstration's second send removes its input field in the same layout
  change, so the card contracts once while the old field fades inside it.
  Polling follows the display clock; the scripted HUD samples between 60 Hz
  frames, the download HUD advances one retained simulation, and the microphone
  bars follow the level continuously. A real press keeps the waiting HUD open;
  completion collapses it until another press. The compositor draws a waveform
  per effect slot, retaining an outgoing page's waveform through its fade. The
  `the_hud_advances_between_sixty_hertz_frames` regression checks different
  half-frame poses, and
  `pressing_keeps_the_waiting_hud_open_and_completion_stays_gone` checks the live
  HUD on a virtual clock. A headless story recording does not prove native display
  pacing or a real permission grant.
  On macOS a story opens from a link or the CLI:

  ```ts
  open 'hex://story/prototypes/hud/notch?variant=pulse-fail&play=1'   // hex-test:// in Hex Test.app
  open 'hex://story/prototypes/capture-capsule/suggestions?variant=toast'
  hex window story Prototypes/HUD/Processing [--variant 2] [--play]
  ├── the window opens on the story: its path, name, or slug, `/` or `-` between groups and name
  ├── a link from before the groups nested (`hud/processing`, `General/mac-settings`, `mac-settings`) → that story now
  ├── variant=<n|name> → that variant
  ├── play=1 → its `r` trigger runs, as pressing R does
  ├── experiment off → only the window opens; process.log says why, the CLI answers feature-off
  └── another link, or longer than 2 KiB → ignored and logged
  ```

  An unknown story opens the list and an unknown variant the story; Rune
  logs which to stderr. Checks: `reads_story_links`, `links_from_before_the_list_nested_still_open_their_story`, `ignores_other_links`
  in [story_link.rs](../../src/rune_ui/story_link.rs);
  `every_hud_story_and_variant_opens_from_its_link` and
  `every_capsule_story_and_flow_opens_from_its_link` in
  playground.rs open every HUD and capsule link headlessly. The URL
  handler and the replay have no automated end-to-end check.
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
