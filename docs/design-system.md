# Hex Design System

The GPUI desktop app (Settings window, meeting capsules, transcription picker,
and the Linux Settings window) is built from two modules:

- [`src/design_tokens.rs`](../src/design_tokens.rs) holds every raw value:
  colours, the type scale, spacing, radii, and control geometry.
- [`src/desktop_ui.rs`](../src/desktop_ui.rs) holds every shared component.

Panes compose components and name tokens. They never write a raw colour, type
size, line height, font family, or radius. The test
`design_tokens::tests::desktop_sources_use_tokens_instead_of_raw_styles` fails on
`rgb(0x…)`, `rgba(0x…)`, `.text_size(`, `.line_height(px(`, `.font_family(`,
`.rounded(px(`, `.rounded_sm/md/lg()`, `.text_xs/sm/base/lg/xl()`, and
`.truncate()` in
`app_window` (with its macOS, Linux, and Windows modules), `desktop_ui`,
`desktop_transcription_picker`, `linux_app`, `meeting_watcher`, and
`text_input`. `windows_ui` owns the Windows UI face and Segoe glyph sizes.

The dictation HUD ([`dictation_indicator.rs`](../src/dictation_indicator.rs)) is
outside the system: its glossy capsule is hand-tuned in HUD Lab.

## Rules

1. **Tokens only.** If a value is missing, add a named token to
   `design_tokens.rs` with a one-line purpose. Don't inline it.
2. **One component per job.** Use the table below. If no component fits,
   extend one in `desktop_ui.rs` and delete whatever it replaces. Never keep two
   helpers for the same job.
3. **Five type steps.** Pick a step by role, not by look. Don't set weights a
   step already owns.
4. **Sentence case, one name.** Labels, buttons, section labels, and titles use
   sentence case. The product is "Hex" everywhere in the UI.
5. **One icon set.** Use `Icon` / `NavigationIcon`: line-weight SF Symbols at
   medium weight on macOS, never `.fill` variants. Never draw an icon with text
   glyphs or letters.
6. **One accent per screen.** The accent fill (`ButtonKind::Primary`) marks at
   most one action per screen: the thing the person came to do (see
   [Colour](#colour)). Everything else is neutral.
7. **Few visible actions.** A detail header shows its one to three most common
   actions; the rest go in an `Icon::Ellipsis` overflow menu, with the
   destructive choice last after a separator.
8. **Verify by looking.** Capture every pane you touched with
   `scripts/capture-preview.sh` (see [Verification](#verification)).

## Tokens

### Surfaces

The surface hierarchy, most recessed to most elevated; values read Dark / Light.

```ts
SIDEBAR           #111111 / #f0f0f0  // navigation column, picker language list
SIDEBAR_HOVER     #1b1b1b / #e6e6e6  // hover on SIDEBAR rows
CANVAS            #161616 / #f7f7f7  // window base, recessed controls inside cards
SURFACE           #1d1d1d / #ffffff  // cards (panel), menus
SURFACE_HOVER     #252525 / #f2f2f2  // hover on SURFACE rows and secondary buttons
SURFACE_SELECTED  #2e2e2e / #e6e6e6  // selected row, segment, nav item
LINE              #2a2a2a / #e2e2e2  // card borders, dividers, control borders
LINE_STRONG       #3d3d3d / #c8c8c8  // menu borders, keycaps, toggle track, capsule edge
SCRIM             #000000cc / #00000066 // every modal backdrop
```

### Text

```ts
TEXT_PRIMARY    #f2f2f2 / #161616  // titles, values, selected items
TEXT_SECONDARY  #d0d0d0 / #3a3a3a  // body copy, unselected list titles, button labels
TEXT_TERTIARY   #969696 / #5f5f5f  // descriptions, metadata, section labels, placeholders
TEXT_DISABLED   #6e6e6e / #9a9a9a  // unavailable controls and idle status dots only
ON_ACCENT       #f2f2f2 / #ffffff  // text and glyphs on an ACCENT fill
ACCENT_TEXT     #8390ff / #2f4ad8  // accent text on ACCENT_SURFACE, links
DANGER          #f17471 / #c0322f  // errors, destructive labels
SUCCESS         #69d89f / #167a46  // ready and live states
```

`TEXT_TERTIARY` is the weakest colour allowed for readable text. It measures at
least 4.5:1 on every surface in both appearances
(`readable_text_meets_aa_contrast_on_every_surface`).

Settings › Appearance switches the active palette through
`design_tokens::set_appearance`; every colour token reads the current palette at
draw time, so open windows repaint on the next frame.

### Roles

Each role has a text or fill colour, a tinted surface, and, where needed, a
border.

```ts
ACCENT   #3b5cf6 / #3b5cf6  ACCENT_SURFACE  #1c2030 / #e8ecff  ACCENT_BORDER  #2c3560 / #c3ccf8  SELECTION #3b5cf655 / #3b5cf655
SUCCESS  #69d89f / #167a46  SUCCESS_SURFACE #17231a / #e3f5ea
DANGER   #f17471 / #c0322f  DANGER_SURFACE  #2b1718 / #fdeaea  DANGER_BORDER  #66292c / #f0b8b8
```

`ACCENT` is the only primary-action fill, the toggle "on" colour, and the
focus ring. Text and glyphs drawn on it use `ON_ACCENT`, never `TEXT_PRIMARY`.

### Colour

Saturated colour is spent on meaning, never decoration:

- **Accent fill:** at most one Primary button per screen, for the main action
  (Meetings "Start recording", the onboarding setup screen's next unfinished
  row and then "Continue" once every row is ready, a dialog's confirm). A
  meeting capsule is its own screen: "Record", "Stop & transcribe" after the
  call ends, "Reveal" when ready. Never in persistent chrome like the sidebar.
- **States are not actions:** an ongoing state uses a Secondary button with a
  `status_dot` in the state's colour, matching its `tag`. "Stop & transcribe"
  while recording is Secondary with a red dot and the elapsed time; the
  sidebar's "Update" is Secondary with an accent dot.
- **Danger** marks destructive choices and armed confirmations only; **success**
  marks ready and live states only.

### Type scale

The window frame sets `BODY`, so text with no step inherits 13 px.

| Step | Size / line | Weight | Use |
|---|---|---|---|
| `CAPTION` | 11 / 14 | inherited | metadata, timestamps, tags, section labels, small buttons |
| `FOOTNOTE` | 12 / 16 | inherited | descriptions, control and button labels, menu items |
| `BODY` | 13 / 18 | inherited | window default, values, inputs, list and row titles, reading text |
| `HEADLINE` | 15 / 20 | semibold | detail, card, and dialog titles |
| `TITLE` | 20 / 26 | semibold | pane titles (`pane_header`), onboarding page titles, hero keycaps |

Apply steps with the `Typography` trait: `.text_caption()`,
`.text_footnote()`, `.text_body()`, `.text_headline()`, `.text_title()`, or
`.text_step(STEP)`. Each step sets its own line height. Use `.text_monospace()`
for code, logs, and configuration paths.

Emphasis inside a step uses `FontWeight::MEDIUM` (row titles, labels, buttons)
or `FontWeight::SEMIBOLD` (card titles at `BODY`). The only text below the scale
is `KEYCAP_SIDE_BADGE` (7 / 8), the L/R badge on a sided modifier keycap.

### Spacing, radii, geometry

Use GPUI spacing utilities on the 4 px grid: `_1` 4, `_2` 8, `_3` 12, `_4` 16,
`_5` 20, `_6` 24, `_8` 32. `PANE_GUTTER` (32) is the one horizontal inset for
pane headers and bodies, so cards line up with the header rule.

Smaller text beside a larger line (a timestamp beside a row title, a
`detail_row` label beside its value) shares its baseline: inset it from the top
by `baseline_offset(LARGER, SMALLER)`, never `items_center`.

```ts
RADIUS_KNOB 2 · RADIUS_SMALL 4 · RADIUS_CONTROL 6 · RADIUS_PANEL 10 · RADIUS_MODAL 12
icon_radius(size)            // app icons and mode icons: 22% of the size
rounded_full                 // dots, avatars, meeting capsules and their buttons

CONTROL_HEIGHT 30            // buttons, inputs, disclosures, segmented controls
CONTROL_HEIGHT_SMALL 24      // small buttons, keycaps
TAG_HEIGHT 18 · MENU_ITEM_HEIGHT 28 · NAV_ITEM_HEIGHT 32
MENU_WIDTH 180 · MENU_OFFSET 4  // menu width; gap below its trigger
PANEL_HEADER_HEIGHT 38 · SETTINGS_ROW_MIN_HEIGHT 56 · PANE_HEADER_HEIGHT 70
SIDEBAR_WIDTH 220 · PANE_CONTENT_WIDTH 940
PANE_LIST_WIDTH 320 · PANE_LIST_WIDTH_COMPACT 220 (Modes)
DISCLOSURE_WIDTH 220 · SEARCH_FIELD_WIDTH 220 · MODE_FIELD_WIDTH 300
ICON_SIZE 14 · ICON_SIZE_SMALL 11 · TOGGLE 28×16 · DISABLED_OPACITY 0.4
ONBOARDING_WIDTH 520 · ONBOARDING_BODY_WIDTH 440 · ONBOARDING_ART_HEIGHT 148
HERO_KEYCAP_SIZE 56 · HERO_KEYCAP_DEPTH 3 · ONBOARDING_BADGE_SIZE 28
ONBOARDING_PRIMARY_WIDTH 180 · STEP_DOT_SIZE 6 · STEP_DOT_ACTIVE_WIDTH 16
```

### Motion

Springs live in [`src/motion.rs`](../src/motion.rs) and are named through
`design_tokens`:

```ts
SPRING_INSTANT      0.10s · bounce 0.00  // pointer-tied meters, live waveform scrub
SPRING_SNAP         0.25s · bounce 0.00  // toggles, sliding pills, menu/picker open
SPRING_SNAP_EXIT    0.20s · bounce 0.00  // menu/picker close, quick exits
SPRING_SETTLE       0.30s · bounce 0.00  // default state change, pane cross-fade + offset
SPRING_SETTLE_EXIT  0.24s · bounce 0.00  // state exit
SPRING_REFLOW       0.40s · bounce 0.00  // layout disclosures, row height/width reflow
SPRING_GLIDE        0.32s · bounce 0.00  // modal sheets, onboarding page transitions
SPRING_MORPH        0.30s · bounce 0.16  // chip-to-panel morphs
SPRING_POP          0.25s · bounce 0.15  // press/release confirmation
SPRING_REDUCED_FADE 0.16s · bounce 0.00  // opacity-only fallback when Reduce Motion is on
PANE_ENTER_OFFSET   4 px · MENU_ENTER_OFFSET 4 px
```

Drive desktop transitions with `window.spring(id, target, spec, cx)` or
`window.spring_fade(id, target, spec, cx)`. Every spring retargets in-flight
with velocity carry-over, requests animation frames only while unsettled, and
honors `NSWorkspace.accessibilityDisplayShouldReduceMotion` (or
`HEX_REDUCE_MOTION=1`).

## Components

| Job | Component |
|---|---|
| Window root (canvas + `BODY`) | `window_frame` |
| Navigation column / item | `sidebar_frame`, `navigation_item(NavigationIcon, selected)` |
| Pane title bar, optional action | `pane_header`, `pane_header_with_action` |
| Fixed pane body / scrolling pane body | `pane_body` + `pane_content`, `pane_scroll_body(id, pane_content())` |
| List + detail | `pane_list(id, empty, error_title, error, rows)` beside `pane_detail(content)` |
| Row in a list | `list_row(selected)` with `list_row_title`, `list_row_meta` |
| Group heading in a list | `list_section_label(label, first)` |
| Top of a detail card | `detail_header(detail_title(…), meta, actions)` over `detail_body(id)` |
| Any text that must fit one line | `truncated_line(STEP, text)` (ends in "…") |
| Card | `panel()`; titled card: `panel_header(title, action)` |
| Settings | `settings_section_label` → `panel` → `settings_row(title, description, control)` |
| Label above a group | `section_label` (caption, semibold, tertiary); above a card, inset `px_4` to the card's row text |
| Label / value in a detail card | `detail_row` |
| Main action of a view (at most one) | `button(ButtonKind::Primary, …)` |
| Any other action | `button(ButtonKind::Secondary, …)` |
| Destructive or armed confirmation | `button(ButtonKind::Danger, …)` |
| Action inside a row or card | `small_button(kind, …)` |
| Icon-only action | `icon_button(ButtonKind::Ghost, Icon)` in cards and rows; `Secondary` when it stands alone |
| Work in progress where a button was | `busy_label` |
| Dropdown trigger | `disclosure_button(label)`; custom content: `disclosure_frame()` + `Icon::ChevronDown` |
| Popover / menu and its choices | `menu()`, `menu_item(selected)`; under a control: `dropdown_menu()` at `MENU_WIDTH` |
| Destructive / unavailable menu choice | `danger_menu_item()` after `menu_separator()`; `disabled_menu_item()` |
| Overflow of less common actions | `icon_button(ButtonKind::Secondary, Icon::Ellipsis)` opening a `dropdown_menu()` |
| On/off | `toggle(position)`; never repeat "On"/"Off" beside it |
| A few exclusive choices | `segmented_control` + `segmented_item`, or the sliding variants |
| Status (never clickable) | `tag(Tone::{Neutral, Accent, Success, Danger}, label)` |
| Live / recorded state | `status_dot(colour)` |
| Shortcut | `hotkey_keycaps`; drawn large in onboarding art: `hero_keycaps(parts, pressed)` |
| Progress | `progress_track(indicator)` with its label beside it |
| Position in a multi-step flow | `step_dots(index, count)` |
| Text entry | `TextInput` ([`text_input.rs`](../src/text_input.rs)): `BODY`, `CONTROL_HEIGHT`, `RADIUS_CONTROL` |
| Nothing to show | `empty_state`; centred in a card: `detail_placeholder` (`pane_list` centres its own) |
| Errors | `error_panel` (card), `error_message` (inside a card), `inline_error` (under a control) |

A destructive menu choice ends in "…" ("Delete…") and keeps its two-step
confirmation: choosing it closes the menu and replaces the action row with the
armed Danger button and Cancel until one is chosen.

Buttons are always filled or bordered. Only icon buttons and menu items are
borderless. Tags never have a border or hover state, so they can't be mistaken
for buttons. A row carries at most two tags, beside its title; an ongoing
state on its trailing edge ("Active") is a `status_dot` and a footnote label,
never a third tag; progress carries its label beside the bar. Put errors at the top of their scroll area,
never below the fold.

Meeting capsules are pill-shaped. They use the shared buttons with
`.rounded_full()` and the same colours, type, and dots as the window.

## Copy

- Sentence case for every label, button, title, and section label:
  "Open settings", "Stop & transcribe", "Permissions needed". Proper nouns keep
  their capitals: Hex, OpenCode, Apple Intelligence, Finder, Zoom.
- One product name: "Hex". Never "HEX" in UI strings, window titles, or menus.
  File and artifact names (`HEX.app`, `HEX-<version>…`) and `HEX_*` environment
  variables are identifiers, not copy.
- Use "…" (one character) for ellipses, for both progress ("Saving…") and for
  buttons that open a further choice ("Choose from Finder…").
- Settings and permission row descriptions are one phrase with no closing
  period ("Hold to dictate, release to transcribe"). Two sentences keep their
  periods.
- Keep one direct line per control. Don't add intro paragraphs above cards,
  repeat field labels per row, put state words next to toggles, or warn "cannot
  be undone" beside a two-step confirmation.
- Exception: native macOS menus (`status_item.rs` and the app menu in
  `meeting_watcher.rs`) follow the macOS title-case convention ("Check for
  Updates…", "Paste Last Dictation", "Stop & Transcribe").

## Icons

- Use `Icon` (`ArrowRight`, `ChevronDown`, `Close`, `DragHandle`, `Ellipsis`,
  `Globe`, `Plus`) via `icon(Icon, colour)` at `ICON_SIZE_SMALL`. `Ellipsis`
  draws at `ICON_SIZE` so its dots match the other glyphs optically. Use
  `NavigationIcon` for the sidebar.
- On macOS these are line-weight SF Symbols (no `.fill` variants) at medium
  weight, monochrome. On Linux they fall back to Heroicons (navigation) and
  matching glyphs (inline). Windows uses the Heroicons for navigation and the
  matching Segoe Fluent Icons glyphs inline.
- Colour icons with a text token: tertiary at rest, primary when selected or
  hovered.
- A web page or other item without an app icon shows `Icon::Globe` inside the
  same rounded tile, never a letter monogram.
- To add an icon, add an `Icon` variant with both its SF Symbol and its Linux
  fallback.

## Verification

```sh
cargo test --bin voice-control design_tokens desktop_ui   # tokens, contrast, layout, truncation
cargo clippy --all-targets --all-features -- -D warnings
./scripts/capture-preview.sh /tmp/hex-settings.png settings
HEX_PREVIEW_SKIP_BUILD=1 ./scripts/capture-preview.sh /tmp/hex-modes.png modes
```

Capture every pane and state you touched. The preview targets and flags are
listed in [AGENTS.md](../AGENTS.md#development). Check at least these:
alignment with the header rule, section labels on their card's text edge,
shared baselines, no clipped text without "…", at most one accent button per
screen, and sentence case.

## Audit: before → after

This is the consolidation from `origin/main` 1c3dacf, measured across the six
desktop sources.

```ts
Text sizes      11 explicit (7 8 9 10 11 12 13 14 15 18 20) + GPUI's inherited 16
                  -> 5 steps (11 12 13 15 20) + the 7 px keycap badge
                  uses before: 11×44 12×41 10×33 13×8 14×4 15×3 18×3 8×2 20×2 9×1 7×1
                  mapping: 7–11 -> CAPTION, 12 -> FOOTNOTE, 13–14 -> BODY,
                           15–18 -> HEADLINE, 20 -> TITLE (pane titles only)
                  inputs 14, unsized text 16 -> BODY 13 from window_frame
Line heights    8 distinct explicit (16 17 18 19 20 24 26) -> owned by each step
Weights         SEMIBOLD×40 MEDIUM×6 NORMAL×3
                  -> MEDIUM for emphasis (15), SEMIBOLD owned by HEADLINE/TITLE
                     and card titles (7), NORMAL for unselected items (2)
Text colours    TEXT TEXT_SOFT MUTED FAINT + 3 raw greys (f1f1f1 8f8f8f 171717)
                  -> TEXT_PRIMARY TEXT_SECONDARY TEXT_TERTIARY (+ TEXT_DISABLED)
                  FAINT #6e6e6e was below 4.5:1 on SURFACE; merged into TEXT_TERTIARY
Raw colours     35 literals, 29 distinct, beside 18 tokens -> 0 literals, 23 tokens
Radii           2 4 5 6 8 10 + rounded_sm/md/lg (60 call sites)
                  -> RADIUS_KNOB 2, SMALL 4, CONTROL 6, PANEL 10, MODAL 12
Buttons         7 looks (header_button, canvas_button, compact_button, blue fill,
                white fill, danger tint, capsule grey)
                  -> 4 kinds (primary, secondary, ghost, danger) × 2 sizes + 1 tag
Control height  24 26 28 30 32 34 36 -> 30 (CONTROL_HEIGHT) / 24 (small)
Section labels  16 ALL CAPS -> sentence case
Product name    "HEX" in window title, menus, copy -> "Hex"
Icons           mixed filled and line SF Symbols, "×" "+" "≡" "→" glyphs, letter
                monograms -> one outline Icon set, globe for web pages
Truncation      .truncate() clipped mid-glyph or kept a stale width -> truncated_line
Pane geometry   header inset 32, list+detail bodies inset 20 (cards overhung the
                header rule by 12 px) -> one PANE_GUTTER owned by pane_body
```
