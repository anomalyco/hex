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
└── Icon-only buttons -> named (Close, Copy, Delete, Remove, Back, More, …)
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
