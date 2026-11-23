# Typing configs (Settings › Experiments › Typing configs)

Part of [Hex for iPhone](../README.md).

A typing config (`.mhexconfig`) is one file a person's own agent writes from their typing
export, using the skill in [`TypingConfig/package/`](../TypingConfig/package/SKILL.md). It holds:

- **words:** words to keep, words never to correct to, typo fixes and dictation fixes;
- **key positions:** where a tap counts. Keys never move on screen.
  - **Aim** moves the centre the decoder reads a letter's taps against.
  - **Zones** move the edges of what a tap presses.

The format, every field and its limits, and how the parts compose with the next-letter odds,
Learn how I type and Adaptive touch are in
[`TypingConfig/package/reference/typing-config.md`](../TypingConfig/package/reference/typing-config.md).

The experiment is off by default (App Group key `experiments.typingConfigs`). Off, or with no
config, every code path below is a no-op, and typing is byte-identical to Hex without it on the
iOS typing gate (`scripts/typing-gate.py`, en es de fr pl, deterministic hashing).

## Limits: one table

`TypingConfig/limits.json` holds every range, clamp and evidence minimum.
`scripts/build-typing-config.py` generates three things from it:

- `HEXShared/FullKeyboard/TypingConfig/TypingConfigLimits.swift`, which the phone reads;
- the package's `scripts/limits.py`, which `check_config.py` reads;
- `typing-config.schema.json`, with the same file in the package. The generator also copies
  [`typing-logs.md`](typing-logs.md) into the package as `reference/log-format.md`.

`--check` fails when any of these is out of date.

## Reading a file: `TypingConfigReview`

`TypingConfigReview(data:keyboards:checksEvidence:)` is the only parser, for the app and the
keyboard alike.

- The app passes the keyboards Hex's keys have run on, which the keyboard records in
  `typingConfig.keyboards` (`TypingConfigKeyboard`: context and `KeyboardZoneGeometry`).
- The keyboard passes its live layout.
- Each key block applies on the first keyboard it matches; a second block for the same keyboard
  is a duplicate.

It returns:

- for every part, an `Item` with its `Outcome` (`applied`, `clamped(from:to:)`, `shortened` or
  `skipped(Reason)`), its path, list, position and block, the file's evidence for it, and its
  cleaned text (`TypingConfigText`, the same string the keyboard loads);
- for each key block, a `ContextCheck`: what it was made for, the keyboard, the fields that
  differ, and whether the hit model matches;
- `Applied`: what the keyboard uses.

`Reason` lists why a part is skipped, in the order a review shows them: `notAConfig`,
`tooLarge`, `newerFormat`, `unknownField`, `invalidValue`, `hiddenCharacters`, `tooMany`, `duplicate`,
`notEnoughEvidence`, `forAnotherKeyboard`, `needsCheckingAgain`, `keyNotOnKeyboard`,
`edgeNotAllowed`, `twoZonesClaimPoint`, `keyTooSmall`.

`compiled(switchedOn:)` makes the file the keyboard reads, keeping every key block that applies on
one of the keyboards. `TypingConfigStore.writeActive` writes
it to `TypingConfigs/active.json` in the App Group atomically and bumps
`typingConfig.generation`. `removeActive` goes back to Hex defaults.

## The keyboard

- **Loading:** `FullKeyboardView.syncSettings` sets `engine.typingConfig` from
  `TypingConfigActive.current(fullAccess:)`. It returns nothing when the experiment is off,
  without Full Access, or when the file is missing or unreadable. The file is read again only
  when the generation changes.
- **Words** (`TypingLearning.adding`) go to autocorrect on top of Learn how I type's learning, in
  memory only, so removing the config removes exactly them. What the person taught Hex themselves
  wins: a typo they fixed another way, a word they kept.
  - Kept words count as their own and are offered.
  - `neverCorrectTo` blocks corrections on space, next-word revisions and merges.
  - Fixes are applied or offered according to their mode.
  - Dictation fixes are added to the keyboard's dictation.
- **Zones** (`KeyboardLayout.zoneMap`, applied in `rebuildLayout`) give the keys of the letters
  page `zoneRects`: disjoint rectangles that `hitTest` and the next-letter odds' distances use.
  - A zone that would leave any keycap less than its centre ±0.3 keycap is skipped, and so are
    two zones claiming one point.
  - Typing logs record `zoneRects` (and the space bar's `gapHitFrame`) in `layout` lines.
- **Aim** replaces the decoder's centre for the letters it names, over Learn how I type's average
  and Adaptive touch's centre. The baseline no correction may go past stays Learn how I type's.
- **Adaptive touch** records no taps while a config has zones.

## The agent package

`TypingConfig/package/` is what an export carries besides the logs. The skill and its reference
are the product: a stranger's agent must be able to build a good config from them alone.

- `taps.py` maps every logged tap, with the key it was meant for, onto today's keys
  (`state/phone.json` `layout`, which `TypingConfigExportState.layout` writes).
- `key_clouds.py` and `zone_edges.py` find aim and edges worth moving.
- `simulate_zones.py` says exactly which key every logged tap would press under proposed edges,
  using the phone's zone compiler ported to `geometry.py`. `simulate_aim.py` is the touch-model
  approximation for aim.
- `check_config.py` re-runs both on the halves the config says it was tuned and checked on, and
  rejects any claim that doesn't match.
