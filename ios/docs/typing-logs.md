# Typing logs (Record typing logs)

Part of [Hex for iPhone](../README.md).

Settings › Privacy › Advanced › **Record typing logs** (off by default, asks for
consent in a dialog before it turns on, App Group key
`experiments.typingLogs`) makes Hex's keyboard write raw logs of everything it
sees: every touch with its position and timing, the key it pressed, what that
did to the text, the text around the cursor, and dictations. They are for
testing and training keyboard prediction and per-user touch models on a Mac.
Nothing is sent anywhere; Privacy › Advanced › Typing logs shares a day's file (or
all of them) through the share sheet, for example by AirDrop.

Once per App Group, the app and the keyboard turn it off at launch for anyone who
had it on (`migrations.typingLogsOff.v1`, `TypingLogSettings.turnOffOnce`). The
logs already recorded stay, and turning it back on afterwards keeps it on.

It is separate from Learn how I type, which keeps only word pairs and per-key
averages and is unchanged.

## Files

- `TypingLogs/yyyy-MM-dd.ndjson` in the App Group container
  (`group.anomaly.hex.mobile`), one per local day (Gregorian date in the
  iPhone's time zone). Each event goes to the file of the day its `t` falls in.
- NDJSON: one JSON object per line, UTF-8, `\n` after every line.
- Kept 14 days (today and the 13 before). Older files are deleted when the
  keyboard starts a logger and when the Typing logs screen opens.
- At most 64 MB a day. The first event that would cross the cap is replaced by
  one `cap_reached` line and nothing more is written that day.
- The keyboard writes nothing without Full Access (it can't reach the App Group)
  and nothing while the experiment is off. The app's keyboard preview and
  simulator fixtures never write logs.

Writing: events are appended to an in-memory buffer on the main thread and
handed to one serial utility queue every 2 s (and when the keyboard goes away,
on a memory warning, and at 1,500 buffered events), which encodes them and
appends them with `FileHandle`. Lines from one session are in order; a session
ends with `session_end` unless the keyboard process was killed first.

## Common fields

Every line starts with these, in this order:

| key | type | meaning |
| --- | --- | --- |
| `v` | int | Schema version, `1`. |
| `t` | int | Wall clock, ms since 1970 (UTC). |
| `mt` | number | Monotonic ms (`CACurrentMediaTime`, time since boot without sleep). Use it for intervals; `UITouch.timestamp` (`ts`) is on the same clock. |
| `type` | string | Event type, below. |
| `session` | string | 12 hex digits, new each time the keyboard appears or moves to another field. |
| `seq` | int | 1, 2, 3… within a session. A gap means lines were lost (only the cap does that). |

Units and spaces:

- Positions and sizes are points in the keyboard view's coordinates: origin at
  the top-left of Hex's keyboard view (the suggestion strip's top edge), x to
  the right, y down. The strip is `y` 0–44 in portrait, the keys below it.
- A rect is `[x, y, width, height]`. Insets are `[top, left, bottom, right]`.
  Points have 2 decimals, ms 3.
- Character counts are Swift `Character`s (grapheme clusters), what one
  backspace removes.
- A key id is the character itself for character keys (always one character:
  `"a"`, `"7"`, `"@"`; letters lowercase) and a word for the others: `shift`,
  `delete`, `space`, `return`, `hex`, `globe`, `layer:numbers`,
  `layer:symbols`, `layer:letters`.

## Events

### `session_start`

The keyboard appeared (`trigger`: `appear`), the field changed while it showed
(`field`), or the experiment was turned on while it showed (`enabled`).

- `surface`: `full` (Hex's keys) or `bar` (the dictation bar; no touches).
- `document`: `UITextDocumentProxy.documentIdentifier`, which changes with the
  field. The host app is not knowable from a keyboard and is never guessed.
- `traits`: `keyboardType`, `returnKeyType`, `autocapitalization`,
  `autocorrection`, `spellChecking`, `smartQuotes`, `smartDashes`,
  `smartInsertDelete`, `inlinePrediction`, `keyboardAppearance` (UIKit case
  names, e.g. `"emailAddress"`, `"send"`, `"no"`), `textContentType` (its raw
  value or null), `secure`, `enablesReturnKeyAutomatically`.
- `bounds` `[w, h]` and `safeArea` of the keyboard's root view when the session
  started (may be before layout; `layout` lines carry the real size),
  `orientation` (`portrait`/`landscape`), `appearance` (`light`/`dark`).
- `device` (`iPhone17,1`; `Simulator:…` in the simulator), `system`
  (`iOS 26.1`), `version` and `build` of Hex, `locale`, `languages` (first
  four preferred), `keyboardLanguage` (the keyboard's `primaryLanguage`),
  `inputMode` (the field's input mode language), `fullAccess`.
- `settings`: `autocorrect` (Hex's), `spatialAutocorrect`, `key_popups`, `haptics`, `learnHowIType`, `cleanupTagger`,
  `hexKeyPlacement`, `dictationLanguage` (BCP 47, or `automatic`), `keyboardLayout`
  (`en`/`nb`/`es`/`de`, empty for the dictation bar), `style` (dictation style).

A `context` line follows at once with the text already in the field.

### `skipped`

Instead of `session_start` in a field that must not be logged; nothing else is
written for that session, not even `session_end`. `reason` is
`secureTextEntry` or `textContentType:<raw value>` for `password`,
`newPassword`, `oneTimeCode` and every `creditCard…` content type. Also
`trigger`, `surface`, `keyboardType`, `textContentType`, `secure`. Every other
field is logged, including ones with autocorrection off.

### `session_end`

`reason`: `disappear`, `field_changed`, `disabled`, or `replaced`.
`durationMS`, and counts of `touches` (began), `edits`, `dictations`. A
`context` line (when the text changed since the last one) and any unwritten
`candidates` come just before it.

### `layout`

Written when the key layout or shift changes, and when a session starts.

- `hash`: 16 hex digits over every key's id, frame and hit frame (and zone rectangles, when a
  typing config moved them) and the strip.
  Touches name it in `layout`.
- `cause`: `session`, `field`, `layout` (width, orientation, globe), `geometry`
  (view size or safe area only), `layer_key` (123/#+=/ABC pressed),
  `layer_auto` (space or return went back to letters), `layer_slide` (hold 123,
  slide, let go), `shift_key`, `typed` (a one-shot shift turned off), `auto`
  (auto-capitalization), `cancel`, `language` (the keyboard layout changed),
  `letters` (its letters' arrangement changed), `typing_config` (Settings ›
  Experiments › Typing configs: the active config changed).
- `layer` (`letters`/`numbers`/`symbols`), `shift` (`off`/`on`/`locked`),
  `shiftHeld` (only when true), `field` (`text`/`url`/`email`), `orientation`,
  `width`, `height` (the keys' layout), `view` `[w, h]`, `safeArea`,
  `horizontalInset` (landscape), `globe`, `hexPlacement`, `language`
  (`en`/`nb`/`es`/`de`), `letters` (`own`/`qwerty`/`qwertz`/`azerty`/`dvorak`,
  Settings › Keyboard › Languages › Layout).
- The first time a hash appears in a day's file the line also has:
  - `letterWidth` (a 10-key row's; 11-key Norwegian and German letters are
    narrower, see `keys`), `metrics` (`side`, `gap`, `rowGap`, `keyHeight`,
    `stripHeight`, `keysBottom`, `cornerRadius`),
  - `strip`: the suggestion strip's rect, and `slots`: its three suggestion
    slots (left, centre, right),
  - `keys`: `[{id, label, row, frame, hitFrame, gapHitFrame?, zoneRects?}]`.
    - `frame` is the drawn keycap and `hitFrame` the touch target.
    - `gapHitFrame` is the space bar's band of the gap above it, under the bottom-row letters
      over it.
    - `zoneRects`: a typing config's zones replaced this key's touch area with these rectangles.
    - Together they tile the key area (row 0 to 3) with no gaps. The Hex key in the strip has
      row `-1`.

  Later lines with the same hash in that file say `"repeat": true` instead, so
  look the keys up by hash. Every touch's hash appears in full earlier in the
  same file.

### `touch`

One finger sample on the keys (not the strip, whose buttons log `ui`/`edit`).

- `phase`: `began`, `moved`, `ended`, `cancelled`. `id`: the finger, stable
  from began to ended.
- `x`, `y`: the raw touch location. `r`: `majorRadius`, `rt`:
  `majorRadiusTolerance` (pt). `force` and `maxForce` only on devices with
  force. `ts`: `UITouch.timestamp` in ms (monotonic, same clock as `mt`).
- `layout`: the layout hash at the time.
- `key`: the key the engine resolves. For `began`, the key the engine presses:
  the key under the point, or a neighbouring letter the next-letter odds gave it
  (`FullKeyboardEngine.letterOdds`). For `moved` and `ended`, the key the finger is held
  on: sliding across character keys moves it; any other key keeps the key it
  went down on. `null` when the finger went down outside every key.
- `under`: the key under the raw point when it differs from `key` (`null`:
  outside every key, on `began`/`ended`/`cancelled`). On `began` it appears
  only when the next-letter odds moved the press to a neighbouring letter, or
  (`null`) when a finger on the strip's lower half while typing went to the row-1
  key below it (`FullKeyboardEngine.takesStripTouch`; `y` is then above the
  keys); otherwise the engine uses the hit frames as logged, so `key` on
  `began` is the key under the point.
- `moved` is throttled: a sample is written once the finger has moved at least
  2 pt and 8 ms since the last line for it, or when it slides onto another key.

Order: `began` and `ended` lines come before the edits they cause (a character
types on release; delete and shift act on press; a press commits the
characters of fingers still down, which is rollover). A `moved` line comes
after what the move did (trackpad cursor steps, accent selection).

### `edit`

What the keys did to the text, with `op`. To rebuild the text before the
cursor, for every `edit` (and `dictation`) line remove `deleted` (a string;
`deletedCount` characters when the text wasn't known) from the end, then append
`inserted`. `cursor` moves need the `context` lines to resync.

| `op` | fields |
| --- | --- |
| `insert` | `inserted`, `src`: `key` (a character or space), `accent` (long-press pick), `slide` (hold 123, slide, let go), `flick` (a docked iPad key pulled down: the character over its letter), `bar` (the dictation bar's space) |
| `delete` | `deleted` (the character), `src`: `key` (on press), `repeat` (held, 75 ms), `repeat_fast` (after 0.85 s, two at a time), `bar`, `selection` (Two-thumb selection's Delete), `selection_cut`, `selection_dictate` (its Dictate, when the dictation's words arrive and replace the range; `deletedCount` when the range is longer than 80 characters) |
| `delete_word` | `deleted`, `src`: `repeat_word` (held past 1.4 s) |
| `autocorrect` | `from`, `to`, `deleted` (= `from`), `inserted` (`to` + space). Applied on space. |
| `autocorrect_undo` | `deleted` (the correction, without its space or mark), `inserted` (what was typed), `src`: `strip`. The strip's revert, offered once delete took the correction's space or mark (or the cursor came back to its end), put back what was typed. |
| `suggestion` | `slot` (0 left, 1 centre, 2 right), `text` (as shown), `deleted` (the word being typed), `inserted` (the replacement + space), `candidates` (as in `candidates`) |
| `double_space_period` | `deleted`: `" "`, `inserted`: `". "` |
| `space_swallowed` | `after`: `suggestion`. A space right after a suggestion typed its own changed nothing. |
| `return` | `inserted`: `"\n"`, `returnKeyType`, `send` (the key said Send); `ignored: true` and no `inserted` when the key was disabled (empty field) |
| `paste` | `inserted`: the strip's Paste chip |
| `undo_dictation` | `deleted` (the dictation), or `deletedCount` when the host showed only its end |
| `cursor` | `offset`: UTF-16 units the cursor moved (negative is left); `src`: absent for the spacebar trackpad (including its settle when a selection starts), `selection` (Two-thumb selection's Copy, or its Delete moving to the range's end), `selection_cut` and `selection_dictate` (the same move before their delete) |

### `candidates`

The suggestions the strip offers: `items`: `[{slot, text, replacement?, kind,
auto?}]`, `kind` `literal` (left, what you typed), `primary` (centre; `auto`:
space will apply it), `secondary`. Written only when they changed, lazily: just
before the next touch goes down and at session end, so each line is what was on
screen when a finger next went down. The strip hides them while it shows Undo
dictation, a status, or the clipboard chips.

### `context`

The host's text around the cursor, as `documentContextBeforeInput` /
`AfterInput` give it. `why`: `session_start`, `space`, `return`,
`suggestion`, `delete_word`, `autocorrect_undo`, `revert`, `paste`, `trackpad`,
`selection`, `undo_dictation`, `dictation`, `session_end`, or `text_did_change` (the host
changed the text or moved the cursor: written once things settle for 0.3 s, at
once when the field emptied). `before` (the last 200 characters), `after` (the
first 200), `beforeLength` and `afterLength` (before capping; hosts often cut
the context short themselves), `hasText`. `cleared: true` when the field had
text and now has none (most apps clear it when they send). An unchanged context
is not written again.

### `ui`

Taps that aren't text: `action`:

- `accent_open` (`id`, `key`, `options`), `accent_pick` (`id`, `index`, `chosen`,
  `rollover` when another key committed it), `accent_cancel`;
- `flick` (`key`, the pulled key's label; `chosen`, what went in): a docked iPad key let go
  past half its pull (Settings › Keyboard › Key flicks), followed by an `insert` with `src: flick`;
- `trackpad_start` (`id`; `hold: true`, as only holding the space bar starts it;
  older logs without it had a 10 pt slide start it), `trackpad_end` (`id`, `offset`: total
  UTF-16 units; with Two-thumb selection on, the space bar's own lift writes it even while the
  other thumb keeps selecting), followed by a `context` (`trackpad`);
- `selection_start` (`id`, the second finger; `thumb: second`, `anchor`: the cursor's
  UTF-16 offset into the trackpad's known text, which is what the selection lays out),
  `selection_end` (`id`, `empty: true` when both
  thumbs lifted with no range and the selection ended like a trackpad lift; when `false`,
  `start` and `end`: the range's UTF-16 offsets from that anchor), `selection_action`
  (`action`: `delete`, `cut`, `copy`, `dictate`, `cancel`, `dismiss` (another key closed the
  actions, or the text no longer matched: `reason` is `key`, `host_edit`, `text` or
  `context`; no `start` or `end`); `start` and `end` as `selection_end` for the others.
  A Dictate writes a second `action: dictate` line when its words arrive: `result` is
  `replaced` (the range was deleted and the words go in there) or `inserted` (the field
  changed, with a `reason`: `field`, `text` or `context`, and the words go in at the
  cursor);
- `selection_cancel` (`id`: the system took a finger away, nothing changed);
- `language_swipe` (`id`, `step`: 1 the next language, -1 the one before) and
  `keyboard_language` (`language`: the layout it switched to), with two or more
  keyboard languages;
- `hex_key` (`dictation`: `warm`, `keyboard`, `open_hex`), `hex_menu`, `style`
  (`style`, `from`: `hex_menu` or `recording_panel`),
  `dictation_language` (`language`: BCP 47 or `automatic`, from the Hex menu),
  `open_hex` (`from: hex_menu`, the menu's Open Hex row),
  `dictation_stop` (`from: hex_key_release` when lifting a push-to-talk hold
  ends it), `dictation_cancel`;
- `next_keyboard` (strip chip), `globe`, `undo_dictation` (`applied: false` when
  the text had changed), `touches_cancelled` (`count`).

### `dictation`

Hex inserted a dictation (from the full keyboard or the dictation bar).

- `source`: `keyboard` (in-keyboard dictation) or `app` (a warm session or a
  handoff to Hex). `resultID`, `style`.
- Each step, when it ran (a step that didn't run is missing): `engine`, `locale`
  (in-keyboard) or `language` (the app), `recordedMS`, `transcribeMS`, `raw`
  (as heard), `fixed` (after Learn how I type's fixes), `spelled` (after custom
  vocabulary, app), `tagged` (after Clean up what I say), `cleaned` (after
  Apple Intelligence, app), `translated`, `formatted` (after the style, app).
- `text`: the transcript Hex delivered; `inserted`: what went into the field
  (with the spacing `TranscriptFormatter.insertionText` added).

A `context` line follows.

### `dictation_outcome`

What became of a dictation, from `DictationEditTracker` (the same inference
Learn how I type uses, run separately). `dictationSession` and `dictation` (its
`seq`) name the `dictation` line it is about. `outcome`: `kept`, `edited`,
`rewritten`, `undone`, `deleted`, `redictated`. `words`, `changed`, `pairs`:
`[{kind, heard, inserted, meant, before, after}]`. It settles within 60 s, when
the keyboard goes away or moves to another field, or on a Send return key, so it
can be in a later session than its dictation.

### `cap_reached`

The day's file hit 64 MB. `bytes` (file size before this line), `cap`; `seq` and
`session` are those of the first event not written.

## Size and cost

Measured in the simulator (Debug build, `TypingLogTests.testLoggingOverheadAndSize`,
3,000 taps of ordinary words): about 1 KB per tap, about 5.2 lines (a `began`,
one `moved`, an `ended`, an `edit`, a `candidates`, plus a `context` per
word and a `layout` when shift changes). Steady typing at 200 characters a
minute is about 12 MB an hour, so the 64 MB cap is about 5 hours of nonstop
typing in a day. On the main thread logging added about 12 µs per tap (35 µs
without it, 48 µs with); encoding and appending run on the utility queue.
Real fingers may produce more `moved` lines than the test's one per tap.

## Reading a day on a Mac

```python
import json
layouts, text = {}, ""
for line in open("2026-11-19.ndjson"):
    e = json.loads(line)
    if e["type"] == "layout" and "keys" in e:
        layouts[e["hash"]] = e["keys"]
    elif e["type"] in ("edit", "dictation"):
        if "deleted" in e: text = text[: len(text) - len(e["deleted"])]
        elif "deletedCount" in e: text = text[: len(text) - e["deletedCount"]]
        text += e.get("inserted", "")
    elif e["type"] == "touch" and e["phase"] == "ended":
        keys = layouts[e["layout"]]   # where the finger lifted, against every key's frame
```

(Python counts code points, not grapheme clusters; use a grapheme library for
emoji and combining marks.)
