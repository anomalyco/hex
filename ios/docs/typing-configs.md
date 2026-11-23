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
  without Full Access, or when the file is missing or unreadable. The file is read and compiled
  once per generation and kept for the life of the process: the words, and each key block's
  context, aim and zones as plain values. A layout rebuild only picks the block for its keyboard
  and reuses the compiled zone map while the keys' geometry is the same.
- **Words** (`TypingLearning.adding`) go to autocorrect on top of Learn how I type's learning, in
  memory only, so removing the config removes exactly them. What the person taught Hex themselves
  wins: a typo they fixed another way, a word they kept.
  - Kept words count as their own and are offered.
  - `neverCorrectTo` blocks corrections on space, next-word revisions and merges.
  - Fixes are applied or offered according to their mode.
  - Dictation fixes are added to the keyboard's dictation.
- **Zones** (`TypingConfigActive.zoneMap`, applied in `rebuildLayout`) give the keys of the letters
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
  using the phone's zone compiler ported to `geometry.py`, and scores the zones on words: the
  words their changed taps are expected to make right and wrong, from what the log says Hex did
  with each word and how often Hex repaired each kind of slip (the package reference, "How a
  zone is scored"). A zone is kept on words, not taps. `simulate_aim.py` is the touch-model
  approximation for aim.
- `check_config.py` re-runs both on the halves the config says it was tuned and checked on, and
  rejects any claim that doesn't match.

## The app: a file arriving, its review, the history

Everything Hex shows for a config, in `HEXMobile/TypingConfigs/`. A file opened while the
experiment is off applies nothing and keeps nothing.

### The file arriving (`HEXMobile/TypingConfigs/TypingConfigInbox.swift`)

- **Type:** `anomaly.hex.typing-config`, exported by Hex (`project.yml`): extension
  `mhexconfig`, conforming to `public.data` (not `public.json`, which AirDrop previews as text
  and never hands to the app), Owner rank, opened in place
  (`LSSupportsOpeningDocumentsInPlace`). Files shows it with Hex's icon.
- **Routes:** a file tapped in Files or iCloud Drive opens Hex in one tap (in place,
  security-scoped); a share sheet's Open in Hex, AirDrop and Mail hand Hex a copy in
  `Documents/Inbox`, deleted once read. `.onOpenURL` sends every `.mhexconfig` to the inbox
  before anything else.
- **Reading:** off the main thread, through `NSFileCoordinator`. A file whose size is over
  `TypingConfigLimits.maxBytes` (64 KB) is never read; one that reports no size is read one
  byte past the limit and refused there. An empty or unreadable file says so. Nothing is
  fetched or run, and the file's name and note are shown as plain text without control or
  direction-changing characters (the note cut at 280 characters).
- **The sheet** (`TypingConfigSheetPresenter`) is presented over whatever is on screen
  (Home, a Settings page, the setup sheet, Typing logs), sized to its content, and can't be
  swiped away: it stays until Apply, Not now or its close button. A second file arriving while
  it is up waits in line ("1 more file waiting"); the same bytes twice are one. When one is
  done the sheet shows the next.
- **Experiment off:** "Typing configs are off" with the file's name, where the switch is and
  that nothing changed; **Open Experiments** sets the sheet aside and opens Settings ›
  Experiments at the switch, and turning it on brings the waiting files' sheet back. Close
  drops them. Nothing is kept while the experiment is off.

### The review (`TypingConfigReviewView.swift`, `TypingConfigWording.swift`)

A rendering of `TypingConfigReview(data:keyboard:)` (`HEXShared/FullKeyboard/TypingConfig`),
which decides every outcome; the app has no parser or clamps of its own. The keyboard it
reviews for is Hex's letters keyboard on this iPhone held upright, in the language it opens
in, with Settings › Keyboard's choices (`TypingConfigKeyboard`).

- **Header:** the file's name, "Typing config · made from N days of typing", and "the one you
  use now" or "opened before" when its bytes are already in the history.
- **Needs Full Access**, before anything else when the keyboard hasn't reported Full Access:
  where to turn it on, Open Settings, and Apply stays off.
- **What it changes**, grouped (Words to keep, Never correct to, Typo fixes, Dictation fixes,
  Where you aim, Touch areas), each group with one line on what it does and each part with its
  own switch, on by default. A group of more than six has an All switch and Show all. A clamped
  value shows the value used and what the file asked for. The file's evidence is shown under
  each part as "The file says: …": the agent's claim, not checked here.
- **For another keyboard:** each key block whose context doesn't match, field by field (the
  file's value and this iPhone's) and which parts it holds, kept in the file, not applied.
- **Skipped:** one row per reason, in the review's order, with a plain sentence and the parts
  it covers (an entry Hex couldn't read by its list and number, an unknown field by its name).
- **From the file:** the note.
- "Hex can't check this config on your typing yet…": there is no on-phone replay in this
  version; the regression watch comes later.
- **Apply** (`TypingConfigActivation.apply`): keeps the file in the history, compiles the parts
  left on (`compiled(switchedOn:)`) and writes `active.json` (`TypingConfigStore.writeActive`,
  which bumps the generation), then "Typing config applied: takes effect the next time the
  keyboard opens". **Not now** and the close button keep it in the history, unapplied.
- **Refused files** (`notAConfig`, `tooLarge`): "This isn't a typing config", the reason, and
  that nothing changed. Nothing is kept.

### The history (`TypingConfigHistory.swift`)

`TypingConfigs/history.json` and `TypingConfigs/history/<sha256>.mhexconfig` in the App Group:
every config kept unchanged, keyed on the SHA-256 of its bytes (the same file twice is one
entry; the file's own `id` isn't checked), with its name, size, source, when it arrived and was
last applied, and the parts switched off (by their place in the file). One is active, or none
(Hex defaults). At most 20: past that the oldest never applied goes first, and an applied one
only when the person deletes it; with 20 applied, the review says to delete one first. Going
back to a config compiles its kept file again on today's keyboard with the same parts off.

### Checks

- `TypingLogTests`: `testATypingConfigArrivesReadOnceAndNeverPastTheLimit`,
  `testTypingConfigHistoryKeysOnTheBytesAndKeepsAppliedOnes`,
  `testApplyingAConfigWritesOnlyItsSwitchedOnPartsAndGoingBackDeletesNothing`.
- Simulator (iOS 27.2, iPhone 17 Pro and iPhone SE, English and 简体中文): each file opened from
  Files › On My iPhone, cold and with Hex running, through every state above. AirDrop,
  Messages, Mail, another app's Open in Hex, and Full Access off on a real iPhone are
  unverified.

### The regression watch (`HEXShared/TypingLog/TypingConfigHealth.swift`, `TypingConfigWatch.swift`)

Hex only ever suggests going back. The one exception is a keyboard that failed to start.

- **Counts.** The keyboard keeps a set of counts for each config, and one for Hex defaults, in the App Group (`typingConfig.health`). It stores no words.
  - What it counts: words finished, autocorrections, autocorrections undone, kept as typed, fixed by hand, and suggestions picked.
  - Where they come from: the hooks in Learn how I type's journal (`TypingJournal.didFinish` and `record`). So the watch needs Learn how I type and Full Access, and it counts nothing while the experiment is off.
  - Trouble is undone + kept as typed + fixed by hand. The rate is trouble per 100 finished words.
- **Comparison.** Apply records the config and the one in use before it (`typingConfig.comparePair`).
  - The new config "seems worse" only when both have at least 400 words, its rate is at least 2× the old one's, and it has at least 8 troubles.
  - Best so far marks the lowest rate over 400 words, once two configs have that many.
- **Telling the person.** This happens at most once per config, and never again after Go back, Keep this one or Turn typing configs off.
  1. **The keyboard's bar notice**, the next time it appears: "Typing may be worse · with this config · Compare with your last one", with Open Hex and an ✕ for good. It needs no permission.
  2. **A notification** a day later, if the bar notice wasn't acted on. The keyboard posts it quietly. "Typing may be worse with this config" / "Tap to compare it with your last one and go back if you want."
  3. **A card on Home**, with Compare and ✕.
  - Settings › Keyboard › Typing configs › Alerts turns off the first two, not the card.
- **The comparison sheet** (`hex-dictation://typing-configs/compare`, from any of the three) shows each side's rate, words and counts, then Go back, Keep this one and Turn typing configs off.
- **Notification permission** is asked on the applied sheet, the first time a config is applied and never before. Hex's own buttons ("Allow notifications" / "Not now") come before the system's prompt.
- **Start-up fallback** (`TypingConfigStartupGuard`).
  - The keyboard marks "starting with config X" in `viewDidLoad` and clears the mark in `viewDidAppear`.
  - A start that finds its mark still there counts a failed start.
  - After two failed starts it starts without the config and leaves word for the app. The app goes back to the config before it and says so on Home: "The keyboard couldn't start with your new config, so Hex went back to the one before."
- **Deleting.** Delete what Hex learned and Delete all typing configs clear the counts and alerts too.

Checks:
- `TypingLogTests`: `testTypingConfigHealthComparesOnlyWithEnoughWordsAndTwiceTheTrouble` and `testTheKeyboardFallsBackOnlyAfterFailingToStartTwiceWithAConfig`.
- Simulator: every state was shown with seeded counts, not real typing. 420 words with 30 troubles against 460 with 12 under Hex defaults, a bar notice backdated 25 hours, and a start mark with one failed start, all written into the App Group's defaults.
