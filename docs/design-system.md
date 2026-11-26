# Hex Design System

Hex's window, meeting capsules, transcription picker, and onboarding are
drawn with [Rune](https://github.com/anomalyco/rune) in OpenCode's design
language:

- **`rune-kit`** provides the generic components, named after
  `@opencode/ui`'s: `Button`, `IconButton`, `Switch`, `SegmentedControl`,
  `Select`, `Menu`, `Popover`, `TextInput`, `TextArea`, `Badge`, `Progress`,
  and the rest.
- **`rune-theme-opencode`** provides every colour, elevation, radius, type
  style, and motion token (`rune_theme_opencode::kit(dark)`), which is
  `@opencode/ui`'s tokens as data. Hex follows the system appearance, so every
  view works in light and dark; Labs › Appearance can force the main window
  light or dark. The dictation HUD (including its previews) and the meeting
  recording pill (including its expanded controls and finished summary) always draw dark. Other
  overlays follow the OS.

The window follows OpenCode Desktop's layout: a deep-background sidebar and a
raised, rounded content panel. Hex keeps its own information, sections, and
copy; the look comes from the kit.

The dictation HUD ([`rune_hud.rs`](../src/rune_hud.rs)) is outside the
system: Dot scope is a shader (`rune_hud.wgsl`) drawn
by a Rune overlay's compositor on every OS, hand-tuned in HUD Lab.

## Rules

1. **Kit tokens only.** Views name kit tokens through `Paint`
   ([`rune_ui/controls.rs`](../src/rune_ui/controls.rs)): `p.k.c.*` colours,
   `p.k.ty.*` type, `p.k.radius.*`, `p.k.elevation.*`, `p.k.motion.*`, or
   the `Paint` shorthands (`text`, `muted`, `faint`, `accent`, `danger`,
   `success`). The raw numbers left in views are layout metrics: row heights,
   column widths, and gaps. Never write a colour literal outside the theme
   crate.
2. **Kit components first.** Use a `rune-kit` component when one fits. When
   Hex needs more, extend the Hex piece below that does the job, or add one
   beside it; never keep two helpers for the same job.
3. **One accent per screen.** The kit's contrast button
   (`ButtonKind::Primary`) marks at most one action per screen: the thing the
   person came to do. A meeting capsule is its own screen. States are not
   actions: an ongoing state (recording, an available update) is a Secondary
   button with a `status_dot`. `ButtonKind::Danger` is for destructive and
   armed confirmations only; success colours mark ready and live states only.
4. **Few visible actions.** A detail header shows its one to three most common
   actions; the rest go in an overflow menu, with the destructive choice last.
   A destructive choice ends in "…" and keeps its two-step confirmation.
   Space presses the focused button; Return presses only a `.default_button()`.
   Make one the confirming action of a dialog, a tour step, or a recoverable
   confirmation once it shows, never a pane's standing action, and never an
   irreversible one (Clear all history, Confirm delete of a mode).
5. **Hover is instant, and only on one target.** A settings row never paints
   a hover or press fill; only the controls inside it react (a row that
   toggles is a `pressable_row`). Hover fills belong to things that are one
   clickable target: list rows, picker rows, sidebar items. Every `.hover(…)`
   needs nothing extra: the window's root sets
   `.hover_motion_within(Anim::INSTANT, Anim::INSTANT)` (and each overlay
   window's root does too), so every hover below it, kit controls and popups
   included, switches with no fade either way. Never give an element its own
   `hover_motion`. Press and selection motion stay as the
   kit draws them, except in lists: a `list_row` or list + detail `item`
   selects on the pointer's press with no press overlay, and the selected
   fill and the detail swap in that frame (`select_at_once`).
6. **Verify by looking.** Render every view you touched with a `--snap` scene
   in light and dark (see [Verification](#verification)).
7. **Sizes come from `METRICS`, `MODAL`, and `CAPSULE`; a new number is a
   new token with a name.** Row heights are tokens (`min_h` + `center_y`),
   never hand-typed vertical padding. Off-scale spacing literals, non-token
   radii (`>= 3`), ad-hoc `TextStyle::new`, `.mono()`, and private
   `PADDING`/`MODAL_WIDTH`/`STATS_WIDTH` constants in `src/rune_ui/` are
   ratcheted by `scripts/code-shape.py` (`scripts/design-token-exceptions.txt`).

## Spatial scale (4 pt base)

Base 4 pt. Allowed steps: **2** (title to its description, row to row), **4, 8,
12, 16, 24, 32, 48**. 6, 10, and 14 exist only as derived concentric capsule
values and inside kit controls. Two rails per side govern the window: card edge
(16) and text/control rail (32). Modals reuse the same rails.

| Token | Value | Use |
|---|---:|---|
| `METRICS.gutter` | 32 | Text rail: pane title, section labels, row text, modal titles and footers |
| `METRICS.row_px` | 16 | Row inset inside cards; card edge = `gutter - row_px` (16) |
| `METRICS.section_gap` | 24 | Between sections on a page |
| `METRICS.label_gap` | 8 | Section label to its card |
| `METRICS.header_h` | 52 | Pane header height |
| `METRICS.row_h_compact` | 28 | Sidebar item, menu row |
| `METRICS.row_h` | 48 | One-line setting row |
| `METRICS.row_h_described` | 60 | Title + description setting row |
| `METRICS.item_h` | 52 | Two-line list item in split panes |
| `METRICS.icon` / `icon_lead` | 16 / 28 | Leading icon and icon column in settings rows and list rows |
| `METRICS.list_w` / `split_gap` | 320 / 12 | List + detail fixed list width and split gap |
| `METRICS.detail_pad` | 24 | Detail card padding on all sides |
| `METRICS.fact_label_w` | 120 | Fact label column width |
| `METRICS.copy_w` | 280 | Centred empty-state and option copy width |
| `ModalWidth::{Narrow, Medium, Wide}` | 460 / 560 / 640 | Sounds, Stats / Calls, Auto-send rules / Shortcut mode, Vocabulary |
| `MODAL.{top, bottom, gap, margin}` | 20 / 16 / 16 / 24 | Modal vertical rhythm and window clearance |
| `CAPSULE.radius` / `window_inset` | 14 / 4 | Capsule shell radius and transparent window margin |
| `CAPSULE.{chip, row, button}_inset` | 4 / 6 / 8 | Concentric inner insets (radii 10 / 8 / 6) |
| `CAPSULE.row_px` / `text_rail()` | 10 / 16 | Text rail matches `METRICS.row_px` (icon spine 24, label rail 40) |
| `CAPSULE.{header_h, row_h, row_gap}` | 36 / 44 / 2 | Recording and ready panel vertical rhythm |
| `CAPSULE.{island_h, banner_h, close, panel_w}` | 40 / 44 / 28 / 252 | Island, banner, ✕ hit box, and panel width |
| `overlay_dock::GAP` | 8 | Between floating surfaces sharing a screen edge (meeting capsules, Annotate, the HUD; [capture.md](features/capture.md#overlay-dock)) |

## Concentric radius system

`R_inner = R_outer - inset` whenever the inner shape sits closer to the corner
than `R_outer`; equal insets on both axes at a corner. When `inset >= R_outer`
the inner shape is free and takes its component's own token.

| Shape | Radius | Token | Proof |
|---|---:|---|---|
| Badge, tone tag | 2 | `radius.xs` | |
| Sidebar item, small button, value button | 4 | `radius.sm` | |
| Push button, select, text field, list row fill, error panel | 6 | `radius.md` | |
| Card, detail card, window content panel, empty-state badge | 8 | `radius.lg` | Sits 16 inside the panel: free |
| Modal, picker | 10 | `radius.xl` | Cards sit 16 inside: free, stay 8 |
| Capsule shell (island, banner, panel, list, popover, toolbar, the recording pill at rest and open) | 14 | `CAPSULE.radius` | One silhouette: a capsule that opens changes its size, never its corners |
| Capsule chip, inset 4 | 10 | `radius.xl` | `14 - 4` (compile-time assert) |
| Capsule row, inset 6 | 8 | `radius.lg` | `14 - 6` (compile-time assert) |
| Capsule push button, inset 8 | 6 | `radius.md` | `14 - 8` (compile-time assert) |
| Dot, knob, switch | `h / 2` | pill | |

## Typography by role

Five sizes: 18 (display), 14, 13, 12, 11. Never place two styles of the same
size beside each other to make hierarchy; step size (13 over 12) or colour.
Use `p.tabular(...)` (`'tnum' 1`) on any number that changes in place or
stacks in a column; reserve `ty.mono` for code, paths, and commands only.

| Role | Helper / Token | Colour |
|---|---|---|
| Pane title, modal title, kind detail title | `p.title()` (`ty.body_medium`, 14/500/21) | `text_base` |
| User-content detail title (meeting name), stat number | `p.display()` (`DISPLAY`, 18/600/24; `p.tabular` for stats) | `text_base` |
| Document body: transcript, summary | `p.k.ty.body` (14/400/21) | `text_base` |
| Section label above a card, row value | `p.label()` (`ty.ui_medium` lh 16) | `text_muted` / `text_base` |
| Heading inside a detail, list title, capsule title/primary row, button | `p.strong()` (`ty.ui_medium`, 13/530/20) | `text_base` |
| Row title, sidebar item, capsule secondary row | `p.row_title()` (`ty.ui`, 13/440/20) | `text_base` / `text_muted` |
| Description, note, detail meta, capsule detail/summary, empty hint | `p.description()` (`DESCRIPTION`, 12/440/16) | `text_muted` |
| List item meta, sidebar footer, "Copied" hint | `p.meta()` (`ty.small`, 11/440/12) | `text_muted` / `text_faint` / `success` |
| List time, transcript timestamp | `p.tabular(p.meta())` | `text_faint` |
| Group label, speaker label, tag | `p.k.ty.small_medium` (11/530/12) | `text_faint` / `text_muted` / tone |
| Timer, elapsed time, countdown | `p.clock()` (`ty.ui_medium` + tnum) | `text_muted` |
| Code, paths, commands | `p.k.ty.mono` (12/400/18) | `text_muted` / `text_faint` |

## Capsule grammar

| Part | Rule |
|---|---|
| Shell | `CAPSULE.radius` (14), `bg_base`, hair `border_base`, `elevation.overlay`, 4 pt transparent window inset |
| Island (one line) | 40 high; 32 pt chips at inset 4, radius 10; `p.strong()` labels |
| Banner (offer, reminder, call ended) | 44 high (56 for two lines); 28 pt push buttons at inset 8, gap 8; Primary last |
| Panel | 252 wide; header 36; 44 pt rows, radius 8, at inset 6, 2 apart; text rail 16 |
| Row | 16 pt icon, 8 gap; lead action = `p.strong()` in `text_base`, no rest fill; `p.row_title()` in `text_muted` for the rest; every row fills with `hover` only under the pointer |
| Title band | `p.strong()` title, `p.tabular(p.meta())` meta 2 below; a 14 pt ↗ trails the title at 0.6, whole on hover; link hover keeps 8 pt clear below |
| Foot link (open recording) | Under the rows, 2 apart, radius 8: "Open in Hex" in `p.row_title()` `text_muted` on the 16 pt rail with the same ↗, 36 high; once the call has words, their last 3 lines in `p.description()` `text_muted`, anchored to the newest, the oldest fading out (`panel/peek.rs`) |
| Icon slot (call ended, recording automatically) | 36 pt slot at inset 10 holding the 26 pt app icon (radius 6); the slot carries the state: an 8 pt `danger` dot ringed 2 pt in `bg_base` on the icon's bottom-right corner = recording live, a ring round the icon = ending |
| Timer | `p.clock()`, 8 pt `status_dot` 6 before it |
| ✕ | 28 pt box, `CLOSE_SMALL` in a `GhostMuted` icon button, centred on the icon spine (24) in panels and banners alike; destroys only after confirmation |
| Confirmation | `capsule_confirm`: no icon, title + description on the 16 pt rail, equal-width Keep (neutral) and Danger buttons at inset 8, hugs content |
| Countdown | Ring round the control that will act (✕, Undo, app icon); no drain bars |
| Hint | `p.meta()` in `success` with a 12 pt check ("Copied") |

## Hex pieces

These live in [`rune_ui/controls.rs`](../src/rune_ui/controls.rs) and
[`rune_ui/lists.rs`](../src/rune_ui/lists.rs) and compose kit components:

| Job | Piece |
|---|---|
| Settings page (the gutter and the rhythm between sections) | `page` (cards hang `row_px` past the gutter so row text sits on the title's edge) or `bare_page` (free-standing content on the gutter); their numbers are `METRICS`, the one set of layout tokens |
| Settings card, titled card, a card whose label line carries a control | `card`, `section`, `section_with` (Advanced, a status, an add button) |
| Settings row (title, description, control) | `setting_row`, `described_row`, `link_row` (a one-line link under the title, at the plain row height), `toggle_row`; a row that toggles shows `row_switch`; any other row pressed as a whole: `pressable_row` |
| A few exclusive choices / a dropdown | `segmented`, `select` |
| A value whose choices open in a sheet, not a menu | `sheet_button` (the value and a trailing chevron) |
| Main, other, and destructive actions | `button(…, ButtonKind::{Primary, Secondary, Danger}, …)`; inside a row: `small_button`; icon only (with an accessible label): `icon_button` |
| Modal shell, scrim, and transition | `modal_panel` (`ModalWidth`, title on the text rail, footer on the control rail) inside `modal_sheet` (`modal_transition` scale 0.96 + fade, `modal_scrim_transition`) |
| Capsule destructive confirmation | `capsule_confirm` (no icon, title + detail on the 16 pt rail, Keep / Danger buttons at inset 8, hugs content) |
| Shortcut keycaps | `keycaps` (Small keys in a recessed tray as tall as a recorder); one live key that sinks when pressed: `keycap` inside `key_tray`; the recorder is `settings::hotkey_control` |
| Status (never clickable) | `tone_tag`; a live or recorded state: `status_dot` |
| Short message under a control | `note` |
| Text entry saved when typing pauses | `text_field` |
| Keyboard highlight in a search popover | `Highlight` |
| List + detail (Commands, Meetings, History) | `list_detail`: `list_column` (search over the rows, on the title's edge, no divider) and `item` rows (no hover fill, time on the right, a hover action in its place via `row_action`), grouped by `group`; the selected item in `body`'s raised card with `card_header`, `card_title`, `meta_text`, `ghost_icon` actions, `card_scroll`, and `facts`; `empty_state` for nothing to show |
| List + detail (Activity) | `split` with `list_row`, `group_label`, `detail_header`, `detail_title`, `section_label`, `fact` |
| Keyboard-selectable list (sidebar, list + detail, picker languages) | `source_list` around rows made with `list_row` or `source_item` |
| Nothing to show, errors | `message`, `placeholder`, `error_panel` |
| A permission's Allow / Allowed control | `permission_control`, `allowed` |
| Slider | `slider` |

Pane-specific pieces stay in their pane module: the listener card and model
cards in [`rune_ui/settings.rs`](../src/rune_ui/settings.rs) and
[`rune_ui/picker.rs`](../src/rune_ui/picker.rs), the onboarding pieces in
[`rune_ui/onboarding_kit`](../src/rune_ui/onboarding_kit.rs), and the capsules in
[`rune_ui/meeting_capsules.rs`](../src/rune_ui/meeting_capsules.rs).
`platform::Capabilities` decides which panes and rows exist on each OS; a
missing capability omits the row instead of imitating it.

## Copy

- Sentence case for every label, button, title, and section label:
  "Open settings", "Stop & transcribe", "Permissions needed". Proper nouns keep
  their capitals: Hex, OpenCode, Apple Intelligence, Finder, Zoom.
- One product name: "Hex", in copy, comments, and docs alike. `HEX_*`
  environment variables are identifiers, not copy.
- Use "…" (one character) for ellipses, for both progress ("Saving…") and for
  buttons that open a further choice ("Choose a file…").
- Row descriptions are one phrase with no closing period ("Hold to dictate,
  release to transcribe"). Two sentences keep their periods.
- Keep one direct line per control. Don't add intro paragraphs above cards,
  repeat field labels per row, put state words next to switches, or warn
  "cannot be undone" beside a two-step confirmation.
- Every translated string (`tr`, `tr_fill`, `tr_named`) needs all five
  languages in [`desktop/i18n.rs`](../src/desktop/i18n.rs);
  `every_translated_string_has_every_language` scans each UI source.
  Developer-only panes (Activity, HUD Lab) stay English.
- Exception: native macOS menus (`macos_rune/status_item.rs` and the app menu in
  `macos_rune.rs`) follow the macOS title-case convention ("Check for
  Updates…", "Paste Last Dictation", "Stop & Transcribe").

## Verification

```sh
# Headless renders of named fixture scenes (`SCENES` in src/rune_ui.rs):
cargo run -- app -- --list-scenes
cargo run -- app -- --snap /tmp/hex-modes.png --scene modes
cargo run -- app -- --snap /tmp/hex-modes-light.png --scene modes --theme light
cargo run -- app -- --snap /tmp/hex-mac-settings.png --scene mac-settings
# Both themes of several scenes into one folder:
OUT=/tmp/hex-snaps scripts/snap-set.sh modes mac-settings
# Goldens (light and dark) and pane behavior:
scripts/check rune_ui
# After an intended visual change, rewrite the goldens (Linux only):
HEX_UPDATE_GOLDENS=1 cargo test --locked --bin hex rune_ui
```

On a machine without a GPU, render on lavapipe with
`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`. Goldens live in
`tests/golden/` and are compared within a small per-pixel tolerance. The golden
tests run only on Linux; from a Mac, dispatch `check-linux.yml` on the branch
and take its `golden-actual` artifact. Look at every changed image and commit
it with the change that caused it. A scene is a fixture, not native end-to-end
proof: the meeting capsules and the HUD have no scene, and the real window,
overlays, and input still need a person on each OS.
