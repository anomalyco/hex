# The typing config, format 0 (`.mhexconfig`)

A typing config is one JSON file an agent writes from a person's Hex typing export. It holds
two things.

- **Words:** words to keep, words never to correct to, typo fixes and dictation fixes.
- **Key positions:** *where a tap counts*.
  - Hex's keys stay where Apple's are. A config never moves a keycap on screen.
  - It changes two things about how taps are read: where the person aims for each letter
    (**aim**), and which key a tap near an edge presses (**zones**).

Hex opens the file, shows each part with its own switch, checks it against the person's own
logged typing, and applies only what they switch on. Earlier configs stay in a history.

The numbers below come from `ios/TypingConfig/limits.json`. The phone's clamps, this package's
`scripts/limits.py` and `typing-config.schema.json` are generated from it and can't disagree.

## Aim and zones

**Aim** is where the person's finger actually lands for each letter.

- It is given per letter key as `[dx, dy]`: the offset from the centre of that key's keycap to
  the mean landing point of the taps *meant* for that letter.
- Units are that keycap's width and height (x to the right, y down). The range on each axis is
  ±0.3; a reader clamps larger values.
- Aim moves only the centre that Spatial autocorrect (the decoder) reads taps against. It never
  changes which key a tap presses.
- It is tied to one context (below).

**Zones** move the edges of what a tap presses.

- A zone moves one edge of one key's touch area by `move` from where Hex puts that edge.
- Units are the letter keycap height for `top` and `bottom` and the letter keycap width for
  `left` and `right`. The range is ±0.25. Positive grows the key: it takes taps from its
  neighbours across that edge. Negative gives a strip of its area to them.
- Every key must keep its keycap's centre ± 0.3 of its keycap in each direction. So no key
  becomes unreachable or smaller than 60% of its keycap each way, and no zone crosses a keycap's
  centre.
- Every point belongs to exactly one key. Two zones that claim the same point are both skipped.
- Zones are tied to one context and to Hex's hit model (below).

## Who wins: how aim and zones compose with what Hex already does

1. **Zones decide the key a tap presses.** Hex's default edges are used for every edge the
   config doesn't move.
2. **The next-letter odds then act as they do today.** They may move the press to a letter whose
   touch area is within `letterReach` (5 pt) of the point, if that letter is likelier to come
   next. The distance is measured to the zoned areas.
3. **For the letters a config's aim names in the live context, the config's aim replaces both
   Learn how I type's per-key average and Adaptive touch's centre.** It is never added to
   either. Letters it doesn't name keep what Hex learned. The baseline that no correction may go
   past stays Learn how I type's averages, as it is for Adaptive touch today.
4. **No double counting.** Learn how I type averages a tap by the key it *pressed*: a tap meant
   for o that landed on l counts for l. A config's aim is measured by the letter *meant*, which
   is the better centre for the decoder. That is why it replaces Hex's averages rather than
   adding to them.
5. **While a config's zones are active, Adaptive touch records no taps.** Its labels assume Hex's
   default edges. Learn how I type keeps recording.

## Context

A `keys` block is for one context. Hex applies the block whose context matches the live keyboard
and lists the others as "for another keyboard" (not applied, not an error).

| Field | Meaning | Matches when |
|---|---|---|
| `width` | the keyboard's width in points | equal after rounding to 10 pt |
| `orientation` | `portrait` or `landscape` | equal |
| `posture` | `phone`, or the iPad's and the Duo's postures | equal |
| `language` | the keyboard layout's language (`en`, `es`, `de`, …) | equal |
| `letters` | the letters' arrangement (`qwerty`, `qwertz`, `azerty`, `dvorak`, `own`) | equal |
| `split` | a split keyboard | equal |
| `hexPlacement` | the Hex key's side (`stripTrailing`, `stripLeading`, …) | equal |
| `numberRow` | Settings › Keyboard › Number row on | equal |
| `keyHeight`, `rowPitch`, `letterWidth` | the letter keycap's height, the row pitch and the letter keycap's width, in points | within 15% |
| `hitModel` | the version of Hex's default edges (`KeyboardZone.hitModel`; `state/phone.json` gives it) | equal. When it differs, the block's **zones** are skipped as "needs checking again" and its aim still applies. |

Copy the context from `state/phone.json` `layout.context`: the keyboard the person uses now. Use
only taps that `scripts/taps.py` mapped into that context (it maps older layouts key by key).

## The file

UTF-8 JSON, at most 65,536 bytes, extension `.mhexconfig`, type `anomaly.hex.typing-config`
(conforms to `public.data`). One file, never a folder or a zip.

```json
{
  "format": "hex.typing-config",
  "version": 0,
  "id": "cfg-0123456789abcdef",
  "basedOn": null,
  "madeFrom": {"exportId": "export-…", "hexVersion": "9.0.0", "hexBuild": "127",
               "logDays": ["2026-11-21"], "heldOut": {"tunedOn": "A", "checkedOn": "B"}},
  "madeFor": {"device": "iPhone19,2", "language": "en"},
  "note": "Plain text, at most 280 characters.",
  "words": {"keep": [], "neverCorrectTo": [], "fixes": [], "dictation": []},
  "keys": [{"context": {…}, "aim": {}, "zones": []}]
}
```

- `id`: `cfg-` plus the first 16 hex digits of the SHA-256 of the file's canonical JSON without
  `id` (keys sorted, no spaces, UTF-8). `scripts/check_config.py --write` fills it in.
  - It is the file's name for itself, which other configs' `basedOn` refers to.
  - Hex doesn't recompute it, because languages print numbers differently. Its history knows
    each file by the SHA-256 of its bytes.
- `basedOn`: the id of the config this one starts from (`state/phone.json` `activeTypingConfig`),
  or null.
- `madeFrom.heldOut`: which half of the sessions the values were tuned on, and which half they
  were checked on (`scripts/split.py`).
- `note`: the agent's own words. It is shown as plain, untrusted text and cut at 280 characters.
  Never quote the person's typing in it.
- `words` and `keys` are both optional. Leave out anything you have no evidence for: absent
  means Hex's default.

## Words

Words are their own layer on the phone. They are never written into what Hex learned, so
removing the config removes exactly their effect. Each entry is 2–32 letters, with an
`evidence` object that `check_config.py` recomputes from the logs.

| Field | What it does for the person | Entry | Max | Evidence needed |
|---|---|---|---|---|
| `keep` | Hex treats the word as theirs: never corrects it to something else, offers it, and reads it as one of their words | `{"word", "evidence": {"times", "sessions"}}` | 500 | left in place ≥ 2 times in ≥ 2 sessions; not in Hex's model (`state/vocabulary.json`); not already in `learnedWords` |
| `neverCorrectTo` | Autocorrect never puts this word in place of what was typed (on space, in a next-word revision or a merge). It can still be typed and offered. For a capitalised word of theirs ("DONT") replacing ordinary typing, or a wrong target that keeps coming back | `{"word", "evidence": {"takenBack"}}` | 500 | corrections *to* it taken back or retyped ≥ 2 times |
| `fixes` | A typo and the word meant. `apply`: space puts the word in, as Learn how I type's learned fixes do; when the typo is itself a word it is only offered. `suggest`: offered in the strip, never put in by space | `{"typo", "word", "mode", "evidence": {"times", "sessions"}}` | 500 | fixed by hand the same way ≥ 2 times in ≥ 2 sessions and never the other way round. A typo that is a word must be `suggest`. Learn how I type already learns a fix retyped twice: add only what it can't see |
| `dictation` | A word or phrase dictation mishears, and what was meant (1–3 words each), applied after Hex's own fixes | `{"heard", "meant", "evidence": {"times"}}` | 200 | fixed the same way ≥ 2 times after dictating |

## Key positions

`keys` holds up to 8 blocks, one per context:

```json
{"context": {"width": 402, "orientation": "portrait", "posture": "phone", "language": "en",
             "letters": "qwerty", "split": false, "hexPlacement": "stripTrailing", "numberRow": false,
             "keyHeight": 42, "rowPitch": 53, "letterWidth": 33.5, "hitModel": 1},
 "aim": {"o": {"offset": [0.0, 0.18], "evidence": {…}}},
 "zones": [{"key": "space", "edge": "top", "move": 0.15, "evidence": {…}}]}
```

### Aim

| | |
|---|---|
| Key | a letter, lowercase (`"o"`) |
| Value | `offset`: `[dx, dy]` in that keycap's width and height, each −0.3…0.3 (clamped) |
| Measure it as | the mean landing point of taps **meant** for the letter (`scripts/key_clouds.py`), shrunk toward 0 by n/(n+20), on the tuning half |
| Set it when | ≥ 40 meant taps on the tuning half and ≥ 20 on the check half, and `scripts/simulate_aim.py` on the check half gains at least as many taps as it loses and the check half's own mean is within 0.1 keycap of the value |
| Evidence | `{"tuned": {"taps", "gained", "lost"}, "checked": {"taps", "gained", "lost"}}` from `simulate_aim.py` |
| Leave it out when | the meant-letter mean is within 0.05 keycap of Hex's own (`state/phone.json` `tapOffsets`, which is by key pressed: compare with `key_clouds.py`'s pressed-key column, like with like) |

### Zones

| | |
|---|---|
| Key | `key`: a letter, `space`, `layer:numbers` (123), `return`, `shift` or `delete` |
| Edge | letters: `top`, `bottom`, `left`, `right`; `space`: `top`, `left`, `right`; `layer:numbers`: `top`, `right`; `return`: `top`, `left`; `shift`: `top`, `right`; `delete`: `top`, `left`. Never the keyboard's outer edge or the strip above row 1 |
| Value | `move`, −0.25…0.25, in letter keycap heights (top, bottom) or widths (left, right). Positive grows the key. Use steps of 0.05 |
| Measure it as | `scripts/simulate_zones.py`: for the proposed edges, the key every logged tap would press, against the key it was meant for: taps gained and taps lost, per key |
| Set it when | ≥ 15 taps fall in the moved strip on the tuning half, and on the check half the gain is ≥ 3 taps net with lost ≤ 34% of gained |
| Evidence | `{"tuned": {"taps", "gained", "lost"}, "checked": {"gained", "lost"}}`, exactly as `simulate_zones.py` printed them. `check_config.py` re-runs the simulation and rejects a zone whose numbers don't match |
| Rule | **Never write a zone you haven't simulated.** Simulate all of a block's zones together: they interact |

Which edge is which:

- A key's `top` edge is the top of its touch area across the key's width.
  - For the space bar, that is the top of the gap above it, which is already the space bar's on
    Hex today, so the bottom-row letters above it end at their keycaps.
  - `space` `top` +0.15 moves that edge 0.15 letter keycap heights up, into c, v, b, n, m.
- `o` `bottom` +0.1 makes taps up to 0.1 keycap below o's edge press o, not k or l, over o's
  width.
- `space` `left` −0.1 gives 0.1 letter keycap width of the space bar's left end to 123.

## Held out

1. Tune every value on one half of the sessions (`scripts/split.py`; A by default) and check it on
   the other.
2. Record it in `madeFrom.heldOut`. A value that doesn't hold on the check half doesn't go in.
3. Words are too sparse for halves: they need their counts in 2 different sessions instead.

## Reading (what Hex does with a file)

On the phone, `TypingConfigReview` (in `HEXShared/FullKeyboard/TypingConfig/`) does all of this.
It gives an outcome for every part: applied, clamped (from what to what), or skipped with a
reason from a fixed list.


- **A file over 65,536 bytes, not JSON, or with another `format`:** refused, with nothing applied.
- **A `version` newer than this Hex:** the fields it knows are read, and the rest are listed as
  "needs a newer Hex".
- **Unknown keys, at any level:** ignored and listed as skipped.
- **An entry with a bad value** (a word too long, an unknown edge, a missing key): that entry
  is skipped and listed. The rest is read.
- **Numbers out of range:** clamped to the range and shown clamped.
- **A `keys` block for another context:** kept and listed, not applied.
- **Zones that break the keycap rule or overlap:** those zones are skipped and listed.
- **`evidence` is never trusted:** Hex shows it as the file's claim and checks the config by
  replaying the person's own logs.
- **Text is cleaned:** the note and every word, fix and dictation fix lose control characters,
  bidirectional overrides, isolates and marks, and invisible format characters (zero-width
  space, word joiner, soft hyphen, …).
  - The zero-width joiner and non-joiner are kept where they belong to a spelling: between
    letters of Arabic, Persian, Syriac, the Indic scripts, Sinhala, Myanmar or Khmer, and the
    joiner between emoji.
  - Words are then NFC-normalised and trimmed.
  - A word that had anything removed from inside it is skipped (`hiddenCharacters`), not
    applied: what it would type isn't what it shows.
  - So don't write such characters.

## On the phone (the contract with Hex's import)

- **Switch:** Settings › Experiments › Typing › **Typing configs**, App Group key
  `experiments.typingConfigs`, off by default. Off, the keyboard reads no config and types
  exactly as without one.
- **What the keyboard reads:** only `TypingConfigs/active.json` in the App Group, and only with
  Full Access.
  - The app writes it atomically (write, then rename): the format-0 object with only the parts
    the person switched on, cleaned and clamped, and no `evidence` or `note`.
  - It holds every key block that applies on one of the person's keyboards. The keyboard
    applies the one for the keyboard it is on.
- **Which keyboards:** the keyboard records each letters keyboard it runs on (context and key
  geometry) in `typingConfig.keyboards`, most recent first, at most 8. The app reviews a file
  against those.
  - After writing, the app increments `typingConfig.generation` (an integer in the App Group's
    defaults). The keyboard reloads when it changes.
- **A missing or unreadable `active.json`** means no config, never a keyboard that fails to
  start.
- **Removing the config** (Hex defaults) deletes `active.json` and increments the generation.

## Three worked examples

### 1. Words only

`word_outcomes.py` (all, then each half) shows three things:

- "eepy" left in place 3 times in 3 sessions; Hex corrected it twice and it was taken back both
  times;
- "DONT" (a capitalised word of theirs) put in for typed "dont" twice, both taken back;
- "thus" fixed by hand to "this" twice in 2 sessions.

```json
"words": {
  "keep": [{"word": "eepy", "evidence": {"times": 3, "sessions": 3}}],
  "neverCorrectTo": [{"word": "DONT", "evidence": {"takenBack": 2}}],
  "fixes": [{"typo": "thus", "word": "this", "mode": "suggest", "evidence": {"times": 2, "sessions": 2}}]
}
```

"thus" is itself a word, so the fix is `suggest`.

### 2. A space bar hit high

`space_edges.py --half A` reports the space-bar touches as NOT TYPICAL:

- they land 9.4 pt above the bar's centre on average (sd 8.3);
- 10.5% land above the keycap;
- 12 bottom-row letter taps in the band just above the space bar were meant as spaces.

`--half B` agrees (9.0 pt, 11.0%). Simulating `space` `top` at +0.10, +0.15 and +0.20 on half A:

| move | gained | lost | net |
|---|---|---|---|
| +0.10 | 9 | 1 | +8 |
| +0.15 | 14 | 2 | +12 |
| +0.20 | 15 | 6 | +9 |

+0.15 is the best on half A. On half B, +0.15 gains 11 and loses 1.

```json
"zones": [{"key": "space", "edge": "top", "move": 0.15,
           "evidence": {"tuned": {"taps": 31, "gained": 14, "lost": 2}, "checked": {"gained": 11, "lost": 1}}}]
```

### 3. A right hand that lands low

`key_clouds.py` shows taps meant for y u i o p h j k l n m landing 0.2–0.3 keycap low, on both
halves.

- **Aim** for each of those letters with ≥ 40 / 20 meant taps, for example o: tuned mean
  `[0.01, 0.31]` from 212 taps, shrunk by 212/232 to `[0.0, 0.28]`. `simulate_aim.py`: tuned
  gained 23, lost 4; checked gained 19, lost 3.
- **Zones** for the top row's right side, where the landing clouds cross into the row below:
  `o` `bottom` +0.10 and `p` `bottom` +0.10, simulated together.
  - Tuned: 48 taps in the strips, gained 31, lost 7.
  - Checked: gained 26, lost 5.
  - `i` `bottom` +0.10 lost more than it gained on half B, so it was left out.

```json
"aim": {"o": {"offset": [0.0, 0.28], "evidence": {"tuned": {"taps": 212, "gained": 23, "lost": 4}, "checked": {"taps": 190, "gained": 19, "lost": 3}}}},
"zones": [
  {"key": "o", "edge": "bottom", "move": 0.10, "evidence": {"tuned": {"taps": 30, "gained": 19, "lost": 4}, "checked": {"gained": 16, "lost": 3}}},
  {"key": "p", "edge": "bottom", "move": 0.10, "evidence": {"tuned": {"taps": 18, "gained": 12, "lost": 3}, "checked": {"gained": 10, "lost": 2}}}
]
```
