# Accessibility

Hex's window, its dictation HUD, and its capsules expose their elements to the
OS accessibility API: UI Automation on Windows (Narrator, NVDA, Voice
Access), NSAccessibility on macOS (VoiceOver, Voice Control), and AT-SPI on
Linux (Orca). Rune builds the tree with AccessKit from what each element
already is (role, label, text, key, focus) and the builders Hex adds
(`description`, `checked`, `disabled`, `a11y_value`, `live_region`,
`position_in_set`, `a11y_hidden`). The same tree is what strobe drives in
[the interaction benches](../perf.md#interaction-cases).

Nothing is built until an assistive client asks, so Hex pays nothing for it
until then. Names and descriptions go through `desktop::i18n` like the visible
copy.

## Window

```ts
Settings window                               // a11y.window
├── Root -> the window, named by its title
├── Sidebar -> a list; each pane a list item named by its title    // a11y.sidebar
│   ├── The pane that shows -> selected; the others unselected
│   ├── Labs and Playground -> list items too; Labs selected while its page shows
│   │   └── Up and Down reach Labs from the last pane
│   └── Select through accessibility (Invoke) -> the same switch as a click
├── Pane -> main; its title a level 1 heading, each card's label a level 2 heading
├── Setting rows                                                    // a11y.rows
│   ├── A row with a switch -> one switch: named by its title, described by
│   │   its note, checked or unchecked; Toggle flips it as a press does
│   ├── Held by policy or locked on -> the switch is disabled
│   ├── A row with a choice -> the choice is named by the row's title and
│   │   reads its current choice as its value (Speech model, Language,
│   │   Microphone, Vocabulary, Sounds, Shortcut mode, Keep meetings, …)
│   └── Icons, separators, status dots, level bars -> left out
├── History                                                         // a11y.history
│   ├── The list -> named History, its size the entries shown (at most 200)
│   ├── Each entry -> a list item named by its text, described by its kind,
│   │   app, and age; its place in the list; the open one selected
│   ├── Search -> a text field named "Search history"
│   └── Keep -> "History retention" with the retention as its value
├── Meetings -> the same list shape, named Meetings, day headings level 3
├── Speech model picker -> a modal dialog named by its title       // a11y.picker
│   ├── The window behind it leaves the tree while it is open
│   ├── Compare -> one group per model column, named by the model
│   └── Every model -> lists per group, each model a list item with its place
└── Icon-only buttons -> named (Close, Copy, Delete or Delete…, Remove, Back, More, …)
```

Sources: [controls.rs](../../src/rune_ui/controls.rs) (`copy_row_tagged`,
`pressable_row`, `select_sized`, `value_button`),
[sidebar.rs](../../src/rune_ui/sidebar.rs),
[list_detail.rs](../../src/rune_ui/list_detail.rs),
[lists.rs](../../src/rune_ui/lists.rs),
[history.rs](../../src/rune_ui/history.rs), and
[picker.rs](../../src/rune_ui/picker.rs).

Checks: [accessibility_tests.rs](../../src/rune_ui/accessibility_tests.rs)
reads `Runtime::accessibility_json` for every `--snap` scene but `live`:
`every_control_in_every_scene_has_a_name` (every element with a click or
focus action, or a control role, has a non-empty name),
`the_sidebar_says_which_pane_shows_and_switches_through_accessibility`,
`history_lists_its_entries_with_their_place`,
`settings_switches_say_whether_they_are_on`,
`choices_name_what_they_choose_and_read_their_value`,
`the_model_picker_is_a_named_modal_dialog`, and
`the_pane_title_is_its_heading`. They run on every OS in CI. They read the tree
Rune would send; they don't prove what a screen reader says.

## HUD and capsules

```ts
Dictation HUD (click-through overlay)         // a11y.hud
├── A status, polite live region, described "Dictation"
├── Recording -> named "Recording"; Transcribing -> "Transcribing"; a failure -> "Failed"
│   └── The name changing is what screen readers announce
└── Hidden or fading -> no name; the overlay takes no input or focus

Meeting capsules                              // a11y.capsules
├── Each capsule -> a status named by its title; its subtitle readable inside
└── Buttons named by their text; "…" is named More, ✕ Close

Annotate capsule                              // macOS, Linux, Windows
└── Its icons named (Draw, Stop, Discard, Close); Draw checked while drawing
```

Sources: `IndicatorModel::status` in
[dictation/indicator.rs](../../src/dictation/indicator.rs) and `view` in
[rune_hud.rs](../../src/rune_hud.rs);
[meeting_capsules.rs](../../src/rune_ui/meeting_capsules.rs);
[capture_capsule/mod.rs](../../src/rune_ui/capture_capsule/mod.rs). Check:
`the_status_says_recording_then_transcribing` (the model only). The capsules'
and HUD's trees have no test.

## Reading another app (dev builds, macOS)

Hex is also an Accessibility client: it reads other apps' focused window
title, text around the caret, and browser pages
([accessibility.rs](../../src/accessibility.rs)). `hex dev call-window` is a
dev-build probe on that client. It reads every window of one app into a plain
tree and writes it to a file, to answer three questions before Hex names
meeting speakers from a call app's tiles: whether the tiles, names, mute state
and speaking mark are in the tree (also when the call window is behind another
or is a mini window), what one read costs, and how soon the local user's own
tile shows them speaking. The commands and output are in
[cli.md](../cli.md#hex-dev-call-window-dev-builds-macos).

```ts
hex dev call-window dump|watch            // a11y.probe
├── Where -> the running Hex reads (it holds the Accessibility grant); the
│   command asks over POST /dev/accessibility-probe and writes the file
├── Dev builds only -> a stable build's help leaves it out, its hex refuses
│   it as unknown, and its Hex answers the route 404
├── macOS only -> Windows and Linux Hex have no client for UI Automation or AT-SPI
├── Each element -> role, subrole, title, description, value, DOM id
│   (AXDOMIdentifier), DOM classes (AXDOMClassList), children
│   ├── Two messages an element: attributes and children together, then the value
│   ├── A password field -> role only; its value and subtree are never asked for
│   └── A value over 4,096 characters -> not asked for; other strings keep 512 units
├── Budgets -> elements (breadth first across windows), depth, a deadline
│   checked between elements, 0.25 s a message; the reply says which cut it short
├── Reads only -> never clicks, focuses, raises or types; never Hex's own window
├── Web tree -> asked for (AXManualAccessibility) only with
│   --enable-web-accessibility; otherwise an empty tree says so and names the flag
└── Privacy -> the tree goes only to --out, mode 0600; logs get counts and timings
```

Sources: [tree.rs](../../src/accessibility/tree.rs) (the snapshot type, the
bounded walk, the filter), [live.rs](../../src/accessibility/live.rs) (the
live elements), [probe.rs](../../src/accessibility/probe.rs) (the request and
reply, the refusals), [probe/cli.rs](../../src/accessibility/probe/cli.rs)
(the command and its file), and
[local_api/developer.rs](../../src/local_api/developer.rs) (the route).

Checks: [tree/tests.rs](../../src/accessibility/tree/tests.rs) walks
hand-written trees, among them
[a huddle window](../../src/accessibility/tree/fixtures/huddle.json) shaped as
other tools describe Slack's with invented people, and covers each budget, the
password rule, an element that does not answer, the filter, and the JSON
shape. [probe/cli/tests.rs](../../src/accessibility/probe/cli/tests.rs) covers
the arguments, the stable-build refusal, the 0600 file and the refused link,
the watch's lines, summary and tick skipping, and the note naming the flag.
`the_accessibility_probe_route_answers_on_macos_only` posts to the route, and
`the_command_is_listed_in_dev_builds_only_and_its_help_warns` reads the help. None of them asks
another app anything.

## Keys automation uses

strobe's selectors are element keys, which the tree exposes as UI Automation
`AutomationId`s. Keep these stable; `perf/interaction.bench.ts` uses them:
`nav-general`, `nav-history`, `nav-output`, `basics`, `output-content`,
`history-list`, `history-scroll`, `history-search`, `transcription-control`,
`transcription-picker`, `more-models`, `all-view`, `picker-back`,
`compare-view`, `close-picker`.

## Not verified

- No live screen reader session: Narrator, NVDA, VoiceOver, or Orca reading
  the window has not been observed.
- No live UI Automation read of a running Hex: the interaction benches' first
  runs need an unlocked Windows session.
- The meeting and Annotate capsules and the HUD have no tree test.
- The onboarding story and practice game are covered only by the
  every-control-has-a-name check, not by a reading-order review.
- `hex dev call-window` has not read a real call: no live read of Slack, Zoom,
  Google Meet, or Teams has been made, so the huddle fixture's shape (grid cell
  ids, the profile button, the audio label, the speaking class) is a claim
  from other tools, not an observation. Its live reader
  ([live.rs](../../src/accessibility/live.rs)) is checked against fixtures
  only through the walk it feeds; no automated test reads a live tree.
  Whether an app lists a window on another Space, a hidden window, or a mini
  window, and whether Chromium builds a web tree for one, is unmeasured.
