# state/: the keyboard now, and what Hex had already learned

The logs show what happened. `state/` says two things the logs can't: which keyboard the person
uses now (so a config is built for it), and what Hex had already learned (so you tell a new
finding from something the phone already does). Hex writes it when the person exports.

## state/phone.json

| Key | Meaning |
|---|---|
| `exportId` | This export's id. Put it in `madeFrom.exportId`. |
| `formats` | `typingConfig`: the format version this Hex reads. Write exactly that `version`. `hitModel`: the version of Hex's default touch areas. `logs`: the log format. |
| `device`, `system`, `hexVersion`, `hexBuild`, `locale` | The phone, iOS, Hex. |
| `keyboardLanguages` | Keyboard layouts in use (`en`, `es`, …). |
| `settings` | Hex's keyboard settings (as in `session_start.settings` in the logs). |
| `experiments` | Experiment switches that change typing: `adaptiveTouch`, `learnEachLanguageSeparately`, `typingConfigs`. |
| `layout` | The letters keyboard the person uses now (below). A config's `keys` block is for this one. |
| `learnedWords` | Words Learn how I type keeps (autocorrect leaves them alone): `count`, `known` (the words), `complete` (false: partial). |
| `learnedFixes` | Typo → word fixes it learned: `pairs` (`[[typo, word]]`), `complete`. |
| `neverCorrect` | Words it learned never to correct (kept over a correction twice). |
| `tapOffsets` | Learn how I type's per-key averages the decoder uses: `keycaps` by letter, `[dx, dy]` in keycaps. Averaged by the key each tap **pressed**. |
| `adaptiveTouch` | Whether Adaptive touch's centres are in use, and its offsets by letter if so. |
| `activeTypingConfig` | The typing config in use now (`{"id": …}`), or null. A new config says `basedOn` its id. |

A value of `null` means Hex couldn't say. `"complete": false` means the list is partial: don't count
a word as new just because it is missing.

### layout

Written by `TypingConfigExportState.layout`, from the keyboard the person last used:

| Key | Meaning |
|---|---|
| `context` | The keyboard, as a config's `keys[].context` names it. Copy it unchanged. |
| `letterWidth`, `keyHeight` | The letter keycap's width and height in points: a zone's units. |
| `gap`, `rowGap` | The gaps between keycaps across and between rows. |
| `touchArea` | `[x, y, w, h]`: where touches go to the keys. Its edges are the keyboard's outer edges, which zones can't move. |
| `keys` | Every key: `id` (as the logs name it), `row` (`-1` the Hex key, `-2` the number row), `frame` (the keycap), `hitFrame` (its touch target), `gapHitFrame` (the space bar's band of the gap above it). Together they tile `touchArea`. |

`scripts/taps.py` maps every logged tap onto these keys: it keeps the tap's place on the keycap it
landed on, scaled to that keycap now. `scripts/geometry.py` compiles zones against them as Hex
does.

## state/vocabulary.json

Every word-like token in the logs, lowercased.

- `model` is the cost in Hex's language model for the keyboard's language: nats, `-ln p`; about
  10 is a common word and 14 a rare one; `null` means the model doesn't have it.
- `dictionary` is whether the system spell checker accepts it.

A word with `model: null` and `dictionary: false` is one Hex would try to correct, unless the
person's own list has it.
