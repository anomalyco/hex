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
  view works in light and dark.

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
| How a modal enters and leaves | `modal_transition` on the surface (scale 0.96 and fade), `modal_scrim_transition` on its scrim |
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
