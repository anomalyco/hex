# Typing replay converters

`TypingReplayTests` can replay real recorded touches through the keyboard. The
recordings are not vendored here. Download a public dataset yourself and convert
it with `convert.py`; the script only reads local files and makes no network
requests.

Both output shapes are described in `ios/HEXMobileTests/TypingReplayTests.swift`.
Swipes have one gesture per word, taps have every committed touch of the
sentence in order. Coordinates are in the unit grid: x 0…1 spans the ten columns
of the top letter row (q's left edge to p's right edge, so their centres are 0.05
and 0.95), y 0…1 runs from the top of the first letter row to the bottom of the
third (row centres at 1/6, 1/2 and 5/6), and rows two and three start half and one
and a half columns in.
Times are milliseconds from the gesture's first point (swipes) or the sentence's first
touch (taps). Each swipe also records `start`: when its finger landed, in milliseconds from
the sentence's first landing, so the replay keeps the person's own gaps between words. A file
without `start` still replays: the test begins each gesture 0.5 s after the previous lift,
longer than the two windows the replay's clock drives (the double-space period and the strip's
touch guard). A start that would overlap the previous touch is moved to its lift, and the
report counts those as `clamped`. A space tap has y = 1.25 and x along 0.25…0.75 of the
dataset's own space bar.

Every subcommand writes the same JSON object and prints sentences, touches and
what it skipped to stderr:

```sh
python3 convert.py <dataset> <input…> -o recording.json [--limit N]
```

`--limit` caps emitted sentences. Run the replay with:

```sh
TEST_RUNNER_HEX_SWIPE_REPLAY=recording.json xcodebuild test \
  -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>" \
  -only-testing:HEXMobileTests/TypingReplayTests
```

Use `TEST_RUNNER_HEX_TAP_REPLAY` for a tap recording.

The report line’s fields are described in
[`ios/README.md`](../../README.md#replaying-real-typing): swipes report `attempts` (the
decoder’s denominator, missed attempts included), `all-words-accuracy` (every in-dictionary
word, taps included and skipped gestures wrong), `taps`/`taps-right` (recordings that are
taps), `not-swipes`, `skipped` and `clamped`.

## FUTO swipe

[Dataset](https://huggingface.co/datasets/futo-org/swipe.futo.org) · MIT ·
[paper](https://huggingface.co/papers/2606.25247).

`dev.jsonl`, `test.jsonl` and `train.jsonl` are in the repository root;
`swipe-2/swipe2.jsonl` through `swipe-5/swipe5.jsonl` are the later runs. For
example:

```sh
curl -L -o dev.jsonl \
  https://huggingface.co/datasets/futo-org/swipe.futo.org/resolve/main/dev.jsonl
python3 convert.py futo dev.jsonl -o futo-dev.json
```

```sh
python3 convert.py futo swipe-5/swipe5.jsonl -o futo-swipe5.json
```

`--valid-only` skips records whose `potentially_invalid_sentence` is true; it is
off by default. `distance` is not used.

Each JSONL line is one word: `session`, `sentence`, `word_idx`, `word`, and
`data` with epoch-millisecond `t` and canvas-normalised `x`/`y`. Records are
grouped by `(session, sentence)` and ordered by `word_idx`; a sentence is only
emitted when every word from `sentence.split()` is present. Words keep their
punctuation, which the tests ignore when comparing letters. Each gesture's `start`
is its first sample's epoch `t` less the sentence's first word's, so the gaps the
epoch stamps record survive the conversion (median about 0.9 s, minimum 85 ms in
`dev`).

The root splits do not carry `layout`, `language` or `dual_finger`; those are
assumed qwerty/English/single-finger. `swipe-5` does carry them, and records
with another layout, another language, or `dual_finger = 1` are skipped (a
dual-finger `data` value is an `L`/`R` object, not a point list). The dataset
has deliberate gaps when a volunteer skipped a word, so incomplete sentences are
common; the counts show how many were dropped. A one-letter word is often recorded as
a single point; it is kept, and the replay reads it as a tap.

`dev.jsonl` is about 304 MB, so the converter streams it line by line.

## How We Swipe

[Dataset](https://osf.io/sj67f) · Leiva et al., MobileHCI 2021 · MIT.

Download `swipelogs.zip` (about 70 MB) from OSF. Pass the zip directly, a
directory of logs, or a single log:

```sh
python3 convert.py howweswipe swipelogs.zip -o howweswipe.json
python3 convert.py howweswipe swipelogs/03ft0n5ol992m393fb9lpc0uqt.log -o one-user.json
```

Rows are space-separated: `sentence timestamp keyb_width keyb_height event
x_pos y_pos x_radius y_radius angle word is_err`. Words in `sentence` are
joined with underscores. A swipe is a run of consecutive `touchmove` rows for
the same word, split on a word change or `touchend`, following the approach of
the study's `stats_create.py`; the last attempt for each word in a sentence is
used. Rows shorter than 12 columns and rows whose `is_err` is not 0/1 are
ignored, and extra columns are ignored.

Each gesture's `start` comes from its first row's `time`, so the replay keeps the
gaps between words. The geometry is inferred from the study's web keyboard, not
exported with the logs: ten columns across `keyb_width`, and three letter rows of
pitch `(keyb_height - 4) / 3.7` starting one pixel down; treat it as approximate.
The converter has been run over the full `swipelogs.zip`: 1,338 logs, 20,118
sentences, 86,831 words, 1,465 sentences skipped for a missing word, no empty
gestures. It never reads `touchstart` rows: a swipe is its `touchmove` rows, so a
touch whose start wasn't logged still begins at its first move. Some logs write a
row twice — the same sentence, millisecond, point, event and word, sometimes with a
13th column — and the converter keeps both, adding the same point and time to the
path; taking the last attempt for a word hides the retries that differ. A word
recorded as a single point is kept, since the replay reads it as a tap.

## Noisy Typing on QWERTY Keyboards

[Dataset](https://zenodo.org/records/24225302) · Vertanen and Kristensson, IUI
2023 · CC-BY-4.0.

Download `vertanen_noisy_typing_json.zip` (about 22.5 MB) from Zenodo:

```sh
python3 convert.py noisy vertanen_noisy_typing_json.zip -o noisy-phone-test.json --set test
```

The zip holds `noisy_typing/json/keyboards.json` and a 422 MB
`noisy_typing/json/noisy_typing.json` array. The array is read one record at a
time, so memory stays bounded; each record is one sentence attempt with
`chunks` of `taps` whose `input` holds the touch samples.

The default device is `phone` (`--device` selects another). `--set dev`,
`--set test` or `--set all` (the default) filters normal sets; `*_word` sets
are not emitted unless `--word-sets` is given, because they are force-aligned
variants of the same input, but their per-word tap counts are still read to
segment the plain records. Deleted chunks are skipped, and so are records
without a reference (the composition tasks in `vt_exp3`). A keyboard's
coordinates are mapped with a line through each letter row's first and last key
centres for x, and a line through q's and z's row-mean y centres for y; the grid
centres are 0.05…0.95 across and 1/6, 1/2 and 5/6 down. A tap whose nearest key
is `<sp>` becomes a space tap; taps nearest to other non-letter keys are skipped.
Samples keep the dataset's relative times, shifted to the sentence's first touch.

The eyes-free studies are left out unless `--eyes-free` is given:
`eyesfree_exp1`'s `blindfolded` condition and `eyesfree_exp2` tapped a keyboard
they only imagined, so their touches do not land where the keys are. The `small`
and `tiny` conditions of `vt_exp3` and `vt_potential` are left out unless
`--small-keyboards` is given: they used smaller-than-full-size keyboards, not the
full-size sighted typing the other datasets measure. `--limit` takes records in the
dataset's own order, not a sample: with both filters off, the first ~266 phone test
records are `eyesfree_exp1`'s sighted condition, so a small cut measures only that
study.

The logs do not always record spaces. `vt_exp1`, `vt_exp2`'s `nospace`
condition and `vt_exp3` typed a run of letters and let their decoder choose the
word breaks; the `swipe` condition and `eyesfree_exp2` drew a space as a right
swipe. A touch that travels more than two q-key widths along x is read as a space
when its record has no spacebar tap. A record with fewer space taps than the reference
has words is segmented by the tap counts of its force-aligned `*_word` twin, which holds
that attempt's letter taps split at the reference's words; a record whose twin is missing
stays one run of letters. The stderr line counts how many sentences were split either way
and how many ran together.

## Tap Typing with Touch Sensing Images

[Dataset](https://github.com/google-research-datasets/tap-typing-with-touch-sensing-images)
· CC-BY-4.0.

Download `touch_data.csv` (about 28 MB), `prompt_data.csv` and
`keyboard_data.json` from the repository, then:

```sh
python3 convert.py tsi touch_data.csv prompt_data.csv keyboard_data.json -o tsi.json
```

Rows are grouped by `(participant_id, task_id, trial_id)` and sorted by
`timestamp_ms`. `was_deleted` taps are dropped. The default `--prompt phrase`
keeps only `prompt_type == "phrase"`; `--prompt all` also keeps the random
letter strings, which are deliberate non-words. The CSV has very large heatmap
columns, so the converter raises Python's CSV field limit and only keeps the
coordinate columns.

Coordinates are mapped with the single Pixel 6 Pro keyboard in
`keyboard_data.json`: the x pitch comes from q and p's centres (with a half-key
margin so q's centre is 0.05), and the y pitch comes from q and z. A tap whose
nearest key is `SPACE` becomes a space tap, and one whose nearest key is `.` is
dropped; the reference's `ref_char` is never used to choose what to keep. Words
stay as `prompt.split()`, including their punctuation.

## Biases

- TSI drops `was_deleted` taps, so a mistake the participant fixed is invisible.
- Noisy records with fewer space taps than reference words and no `_word` twin stay one
  run of letters (the stderr line's "run together"), so their later words cannot come out
  right.
- FUTO ignores `distance`; `--valid-only` skips records marked
  `potentially_invalid_sentence` and is off by default.
- How We Swipe's keyboard geometry is inferred from the study's web keyboard, not exported
  with the logs.
- How We Swipe logs some rows twice, so a swipe's path can hold the same point and time two or
  more times; the repeat adds no new sample. Retries that differ are hidden by taking the last
  attempt for a word.

## Pitfalls

- The data is not vendored and the converters never download anything.
- FUTO root splits and How We Swipe logs are recordings of what people did, not
  clean synthetic gestures; incomplete sentences and retries are normal.
- Noisy Typing's JSON is much larger than its zip; use `--limit` while
  experimenting. `--set`, `--word-sets`, `--eyes-free` and `--small-keyboards`
  change which attempts are emitted.
- TSI's `--prompt phrase` is the useful default; random prompts are not words.
- Output can be large. The tests report per-source accuracy, and `HEX_REPLAY_LIMIT`
  caps sentences inside the test as well.
