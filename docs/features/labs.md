# Labs

Labs holds unproven, user-requested, and externally contributed features behind
per-feature switches, **off by default**. There is no master switch. Every build
shows General, Output, History, Meetings (`meetings.enabled`, off by default),
Annotate (`capture.enabled`, on by default), and Labs; a Labs feature with its
own sidebar pane (such as Voice Filter, Playground, or the Modes editor) joins
the sidebar only while enabled.

## Single Source Of Truth

Do not maintain a hand-copied feature table in Markdown. The authoritative list
of Labs features, metadata, and generated web catalog live in code:

- **Registry:** `src/labs/feature.rs` (`Feature::ALL`) defines every feature's
  storage binding, CLI name, group, audience, supported OSes, join date
  (`yyyymmdd`), and companion `extra_keys`.
- **UI copy:** `src/labs/card.rs`, `src/labs/description.rs`, and
  `src/desktop/i18n/` own localized card titles and summaries.
- **Runtime gates:** `src/labs.rs` and `src/labs/gates.rs` expose hot-path
  check helpers.
- **Web catalog:** `site/src/labs.json` is generated from `Feature::ALL` by
  `src/labs/web.rs` (for `anoma.ly/hex/labs` and `/hex/labs/<id>`).
- **Live listing:** `hex labs` prints every feature available on the current OS
  and build with its state and audience.

## Groups

`src/labs/group.rs` (`Group::ALL`) orders collapsible headers in Settings ›
Labs (`hex://labs?category=<id>` and `labs.groups` in `settings.json`):

`dictation`, `output`, `history`, `meetings`, `annotate`, `shortcuts`, `models`.

## Audience & Availability

Each `Feature` declares an `Audience` (`src/labs.rs`):

- **`Everyone`:** ships in both stable and dev builds.
- **`Dev`:** exists only in dev builds (`dev_build::is_dev_build`: debug,
  `HEX_LOCAL_BUILD`, `HEX_TEST_BUILD`, and `HEX_BUILD_CHANNEL=dev`).

In a stable build, `Dev` features are never listed, accepted (`hex labs on`,
`hex settings set`), or executed — `Feature::available()` omits them, runtime
gates read `false` even if `settings.json` says `true`, and the saved value in
`settings.json` stays untouched.

`Feature::on_this_os()` additionally gates OS and session availability (for
example, `KeyboardModelSwitching` checks input-source support and
`AnnotateReplay` requires `cfg(hex_capture)` with replay support).

## Runtime Gating, Storage & Policy

1. **Gate where work starts:** check the lab at the worker, thread, capture, or
   hook entry point — never only in Settings UI visibility.
2. **Storage & switch bank (`src/labs.rs`):**
   - Features bound as `labs(<field>)` live in `AppSettings::labs` and are
     encoded into a lock-free `AtomicU64` bitset (`SwitchBank`), lazy-loaded
     once or updated atomically by `labs::apply(&LabsSettings)`.
   - Companion payloads (for example `labs.unload_idle_minutes`,
     `labs.keep_microphone_open_seconds`, `labs.voice_prompt_phrase`,
     `labs.yolo_mode_apps`) are published before the bank bits so hot-path
     readers never observe a bit enabled ahead of its payload.
   - Features bound as `app(<path>)` (such as
     `experimental.hex_microphone`, `paste_with_shift`,
     `keyboard_model_switching.enabled`, `pause_browser_media`,
     `share_dictation_shortcut`) read directly from `AppSettings` and do not
     use `SwitchBank`.
3. **Managed policy locks (`src/policy.rs`, `Feature::lock`):**
   - `Lock::Labs` locks every Labs feature off.
   - Feature-specific locks also apply: `VoicePrompt` ->
     `Lock::OpenCodeProcessing`, every `Group::Meetings` feature ->
     `Lock::Meetings`, and every `Group::Annotate` feature -> `Lock::Annotate`.
   - Locked features read off and refuse CLI and UI changes.
4. **Toggle side effects & refusals (`src/labs/feature.rs`, `src/labs/cli.rs`):**
   - Turning off shortcut-expanding labs repairs saved shortcuts that rely on
     them (`DoubleTapModifiers`, `StandaloneSpecialKeys`, `SectionKeyAlone`,
     `MicrophoneKeyAlone`); turning on `Screenshots` clears conflicting
     capture shortcuts.
   - Turning off `ParakeetUltra` is refused while it is the active dictation
     model.
   - Turning off `VoiceFilter` is refused while the macOS Hex Microphone
     driver bundle is installed (uninstall from its pane first).

## CLI & Settings Parity

`src/labs/cli.rs` exposes:

- `hex labs` / `hex labs list` — list available Labs features, state, and audience.
- `hex labs on <name>` / `hex labs off <name>` — routes through a running Hex's
  `/control` endpoint when active; otherwise updates `settings.json` via
  three-way merge.
- `hex labs link <name>` (macOS) — prints the `hex://labs/<id>` deep link.

Every persisted toggle (`settings_key`) and companion key (`extra_keys`) must
also be registered in `hex settings` (`every_persisted_setting_has_a_cli_key`,
see [docs/cli.md](../cli.md)).

## Web Catalog (`site/src/labs.json`)

`src/labs/web.rs` exports every `Audience::Everyone` feature except internal
Annotate sub-labs (`Screenshots`, `AnnotateReplay`, `AnnotatePointerScreen`,
`CopyCaptureZip`, `AnnotateAgentTools`, `AnnotateAgentInstall`,
`AnnotateShortcutPastes`).

```sh
HEX_UPDATE_LABS_JSON=1 scripts/check labs::web
```

- `labs::web` tests fail whenever `site/src/labs.json` drifts from
  `Feature::ALL`.
- Pushing `site/**` to `dev` builds `/hex/labs`, `/hex/labs/<id>`, 1200×630
  social cards, and sitemap entries on deploy (not during PR checks).

## Lifecycle

### Adding a lab
1. Add a `#[serde(default)]` off-by-default setting in `AppSettings` /
   `LabsSettings` and register its key (and any companion keys) in
   `src/control/settings/`.
2. Add the `Feature` descriptor in `src/labs/feature.rs`, card/description
   entries in `src/labs/{card,description}.rs`, translations in every language
   under `src/desktop/i18n/`, and gate helpers in `src/labs/gates.rs`.
3. Gate the feature where its work starts.
4. Run `HEX_UPDATE_LABS_JSON=1 scripts/check labs::web` and
   `scripts/check labs control::settings`, and commit `site/src/labs.json`.

### Graduating a lab (shipping always-on)
1. Remove the `Feature` variant and runtime gate checks while keeping existing
   user settings backward-compatible.
2. Add the former CLI name and explanation to `GRADUATED` in `src/labs/cli.rs`
   so `hex labs on|off <name>` prints a helpful note instead of an unknown-lab
   error.
3. Regenerate `site/src/labs.json` and run `scripts/check labs control::settings`.

### Retiring a lab
1. Remove the `Feature` variant, UI card, and runtime code while ensuring
   existing `settings.json` files containing the old key still load cleanly.
2. Regenerate `site/src/labs.json` and run `scripts/check labs control::settings`.

## Verification

- `scripts/check labs control::settings` — registry invariants, `SwitchBank`
  encoding, policy locks, CLI parity, and `site/src/labs.json` freshness.
- `scripts/check every_lab_card_string_has_every_language` — i18n completeness
  for every Labs card.
- `bun test` in `site/` (`site/labs.test.ts`) — web catalog ordering, links,
  and metadata (live pages remain unverified until deployed).
