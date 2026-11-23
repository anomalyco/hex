# Build and test

Part of [Hex for iPhone](../README.md).

Prerequisites: Xcode 26+ and [XcodeGen](https://github.com/yonaskolb/XcodeGen) 2.46.0+.

```sh
cd ios
scripts/fetch-cleanup-model.sh   # the cleanup tagger's Core ML model, kept out of git
xcodegen generate
xcodebuild test -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>"
```

The emoji index files are generated and committed: after changing
`scripts/emoji-aliases-en.txt` or the sources, run `scripts/build-emoji-data.py`
(it downloads the pinned Unicode and CLDR files, or reads them from `--sources`).

The cleanup tagger's model (`HEXKeyboard/CleanupTagger/HexCleanupTagger.mlpackage`,
25 MB) is a GitHub release asset (`ios-model-cleanup-tagger-1`), not a git file.
`fetch-cleanup-model.sh` downloads it and checks its SHA-256; Xcode Cloud runs it
from `ci_scripts/ci_post_clone.sh`. A new model gets a new release tag and hash.

### Replaying real typing

`TypingReplayTests` runs recordings of real fingers through the keyboard as it runs: the
key plane, Swipe typing or taps with Spatial autocorrect, then the engine's spacing. It is
opt-in, one test per mode, and the datasets below are not vendored, so convert a recording
to one of the two JSON shapes first. `xcodebuild` passes environment variables to the test
runner only when they are prefixed with `TEST_RUNNER_`, and strips the prefix, so
`TEST_RUNNER_HEX_SWIPE_REPLAY` becomes `HEX_SWIPE_REPLAY`:

```sh
TEST_RUNNER_HEX_SWIPE_REPLAY=/path/swipes.json xcodebuild test \
  -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>" \
  -only-testing:HEXMobileTests/TypingReplayTests
```

Swipe recordings are `{"sentences":[{"source":String,"session":String,"words":[String],
"gestures":[{"x":[Double],"y":[Double],"t":[Double],"start":Double?}]}]}`: one gesture per
word, x 0…1 across the ten columns of the top letter row, from q's left edge to p's right
edge (their centres at 0.05 and 0.95), y 0…1 from the top of the first letter row to the
bottom of the third (row centres at 1/6, 1/2 and 5/6), t milliseconds since the finger
landed, and the optional `start` milliseconds from the sentence's first landing to that
one, so gestures keep the person's gaps; without it each begins 0.5 s after the previous
lift, longer than the two windows the replay's clock drives that a lift leaves open (the
double-space period and the strip's touch guard). A start that would put a finger down before
the previous one came up is moved to that lift, in both modes, and `clamped` counts those. A
gesture recorded as one point is a tap. Hex's second and third letter rows are offset half a
column and one and a half columns, as the format assumes. Tap recordings replace `gestures`
with `taps`, one `{x, y, t}` per touch, each array holding that touch's samples (a tap is
usually one) on the sentence's clock; y > 1 is the space bar, x 0.25…0.75 across it.
`TEST_RUNNER_HEX_TAP_REPLAY=/path/taps.json` does the same for taps, with Spatial
autocorrect reading each word on its space.

Both report a line per source inside `TYPING-REPLAY … TYPING-REPLAY-END`: total words,
words outside the dictionary and their share (a word is in the dictionary when the decoder's
model has it or English's contractions in `AutocorrectEngine.englishContractions` do; the
model lookup gets only the word's ASCII a–z letters and apostrophes, so accented, hyphenated
and numeric tokens normally count as outside), and accuracy among the in-dictionary words.
Scoring lowercases, straightens curly `’`, trims leading and trailing apostrophes, and keeps
ASCII letters: a tap compares letters, and an apostrophe only when the recorded word has one;
a swipe compares letters only. Taps report that as `accuracy` over every in-dictionary word and
also `all-words-accuracy` over every word, so the gap between the two is what the
out-of-dictionary words contribute. Swipes report `accuracy` and `typed-or-offered` over the
swipe attempts (`attempts`): gestures that became swipes, ones whose path left the key by a
swipe's pitch and weren't promoted, and ones that read nothing. They also report
`in-dictionary-accuracy` over every in-dictionary word — taps included, a skipped gesture
counted wrong — and `all-words-accuracy` over every word, which is the end-to-end number:
what the person reading the field saw, with an out-of-dictionary word now able to count as
right when the field has it. `taps` and `taps-right` are the recordings whose path never
leaves its key by a swipe's pitch: the keyboard types that key's letter, and the field keeps
it, as for a person typing a one-letter word. Tap mode keeps Swipe typing attached as the
keyboard runs it, so a recorded tap whose samples drift can be promoted to a swipe;
`read-as-swipes` counts the recorded taps the plane took that way. `not-swipes` counts swipe
attempts the plane never took as swipes or that read nothing; `skipped` counts gestures that
began off the letter keys, and `clamped` the touches moved forward to the previous lift;
unlike `taps` and `taps-right`, which count in-dictionary words, those three count every
recorded touch, and `read-as-swipes` also counts every delivered tap, whatever its word. A
tap's words compare an apostrophe only when the recorded word has one; a swipe's by letters
only, because a swipe's path runs over letter keys and cannot draw one. The decoder's own
median and p90 milliseconds close the line, though a swipe's preview decode can overlap its
timed final decode, so a `decode-ms` sample may include that overlap; the words are
unaffected. `HEX_REPLAY_LIMIT` caps sentences per source; `HEX_REPLAY_OUT` writes each
sentence's intended and typed words (prefix both with `TEST_RUNNER_` too).

The test with no recording checks Hex's key geometry against the grid and runs clean
gestures through the same mapping to type the right words; it cannot catch a converter that
emits wrong coordinates.

Numbers can move between simulator iOS versions because autocorrect also consults the system
text checker for word validity and names, and its dictionary changes with iOS.

Public datasets of real typing it is meant for:

- [FUTO swipe dataset](https://huggingface.co/datasets/futo-org/swipe.futo.org) (MIT).
- [How We Swipe](https://osf.io/sj67f) (Leiva et al., MobileHCI 2021; MIT).
- [Noisy Typing on QWERTY Keyboards](https://zenodo.org/records/24225302) (Vertanen and
  Kristensson, IUI 2023; CC-BY-4.0).
- [Tap Typing with Touch Sensing Images](https://github.com/google-research-datasets/tap-typing-with-touch-sensing-images)
  (CC-BY-4.0).

See [`benchmark/typing-replay/README.md`](../benchmark/typing-replay/README.md) for each
dataset's download and its `convert.py` command.
