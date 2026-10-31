# Labs

Labs is one switch for every unfinished feature: `labs.enabled` in
`settings.json`, **on by default in every build**, stable included, so people
can try the features and send feedback. There are no per-feature switches:
Labs on turns every feature below on, Labs off turns all of them off.

```ts
Settings › Labs                          // labs.page
├── Labs switch -> labs.enabled; "Try unfinished features. They may change or break."
│   └── Send feedback -> https://github.com/anomalyco/hex/issues/new
├── Opt-in features -> Meetings (beta) on macOS, Annotate in dev builds; their own switches
└── Labs on -> Experiments: what Labs turns on (Open jumps to a pane) + Run setup again

hex labs                                 // labs.cli; edits settings.json, no running Hex
├── on | off -> Save labs.enabled; print the restart hint
├── status -> Labs is on/off
└── hex experiments … -> "Experiments are now one switch, Labs: hex labs on|off|status"

hex settings set labs.enabled false      // macOS: through the running Hex
```

Panes a Labs feature adds list indented under **Labs** in the sidebar, which
shows on every OS in every build. `hex status` reports `Labs on|off` (macOS
text), `labs` in its JSON (macOS and Linux), and Windows prints `labs\ton|off`.
`HEX_EXPERIMENTS` and `~/.config/hex/experiments` are no longer read: settings
saved before Labs have no `labs` key, so Labs comes out on.

Some features read Labs once at launch (Live text, Playground, Live chapters'
recorder) and apply after a restart; the Voice Filter pane and the new
onboarding follow the switch at once.

Gate a Labs feature where its work starts (the worker, thread, or capture),
with `labs::enabled()`, not only where its UI appears. In the window, read
`ui.settings.labs.enabled`. When a feature ships, remove its gate.

Shipped features that used to be experiments are no longer gated: Annotate
(dev builds, its own `capture.enabled` switch), Output and Modes, Vocabulary,
and [Dot scope](dictation.md#dot-scope). The smart-turn predictor was removed.

## Features

- Live chapters: chapter a meeting's transcript live while it records
  (macOS, Apple Intelligence; see
  [Live Chapters](meetings.md#live-chapters-labs)). The
  gate is where the work starts: the recorder's chapter worker, recording
  chapters in the manifest, and the boundary pass after Stop.
- New onboarding: the onboarding story as the real first run on every OS
  (`rune_ui/onboarding_kit/live.rs`): the real model download, Microphone and
  Accessibility rows that follow macOS (ready at once on Linux and Windows,
  which ask for no permissions), and a practice dictation with the real
  shortcut and the HUD on the live microphone level. The window's tour stays
  the state machine underneath. Labs' **Run setup again** and
  `hex onboarding reset` (macOS control socket, the Linux service socket,
  and a named event on Windows) forget that onboarding finished
  (`onboarding::reset_at`) and replay the first run without touching
  settings, history, models, or permissions; ready steps pass at once. With
  Labs off they refuse with "Turn on Labs first". Checks:
  `labs_run_setup_again_opens_the_first_run` and
  `run_setup_again_opens_the_first_run_on_linux_and_windows` in
  [panes.rs](../../src/rune_ui/tests/panes.rs). Not yet: words flying into
  the field; the Linux and Windows practice page cannot see the dictation
  start, so it shows no listening state.
- Live text: a streaming dictation shows what you have said so far while
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
- Playground: Rune's playground in the window (every OS). ⇧⌘D (View ›
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
  `hex app -- --snap out.png --playground "Window/General/Settings"`. Checks: `every_scene_but_live_has_a_path_under_a_group`,
  `the_playgrounds_keys_work_with_a_story_open`, and
  `a_scene_opened_from_the_list_hears_command_digits` in
  [playground.rs](../../src/rune_ui/playground.rs).
  On macOS a story opens from a link or the CLI:

  ```ts
  open 'hex://story/prototypes/hud/notch?variant=pulse-fail&play=1'   // hex-test:// in Hex Test.app
  open 'hex://story/prototypes/capture-capsule/suggestions?variant=toast'
  hex window story Prototypes/HUD/Processing [--variant 2] [--play]
  ├── the window opens on the story: its path, name, or slug, `/` or `-` between groups and name
  ├── a link from before the groups nested (`hud/processing`, `General/mac-settings`, `mac-settings`) → that story now
  ├── variant=<n|name> → that variant
  ├── play=1 → its `r` trigger runs, as pressing R does
  ├── Labs off → only the window opens; process.log says why, the CLI answers feature-off
  └── another link, or longer than 2 KiB → ignored and logged
  ```

  An unknown story opens the list and an unknown variant the story; Rune
  logs which to stderr. Checks: `reads_story_links`, `links_from_before_the_list_nested_still_open_their_story`, `ignores_other_links`
  in [story_link.rs](../../src/rune_ui/story_link.rs);
  `every_hud_story_and_variant_opens_from_its_link` and
  `every_capsule_story_and_flow_opens_from_its_link` in
  playground.rs open every HUD and capsule link headlessly. The URL
  handler and the replay have no automated end-to-end check.
- Voice Filter: the Voice Filter pane (macOS), the only home of the Hex
  Microphone driver, its filter modes, and Try it
  ([Hex Microphone](README.md#experimental-hex-microphone-and-voice-filter)).
  It lists under Labs while Labs is on, and also whenever
  `experimental.hex_microphone` is true or the driver is installed, so nobody
  loses the controls for a driver they already use. Turning Labs off also
  turns `experimental.hex_microphone` off. Check:
  `voice_filter_needs_labs_or_hex_microphone` in
  [panes.rs](../../src/rune_ui/tests/panes.rs).

Checks: `the_labs_switch_turns_every_feature_on_and_off`,
`production_builds_offer_meetings_and_labs`, and
`voice_filter_needs_labs_or_hex_microphone` in
[panes.rs](../../src/rune_ui/tests/panes.rs); `labs_parse_as_a_read_command`
in [main.rs](../../src/main.rs); `status_reads_for_a_person` and
`status_reports_labs_and_reads_older_replies_without_it` in
[control/cli.rs](../../src/control/cli.rs);
`every_persisted_setting_has_a_cli_key` in
[control/settings.rs](../../src/control/settings.rs). Passed on Linux on
2026-10-30; the macOS build was not checked, and the Windows build was
type-checked only.
