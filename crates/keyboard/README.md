# Hex keyboard core

The keyboard's platform-independent logic in Rust, shared by the iOS keyboard extension
(`ios/HEXKeyboard`, through `ios/HEXShared`) and the Android IME (`android/app`). Each OS
keeps its own layer on top: views, touches, the text field, haptics, dictation, storage, and
anything only the OS can answer (Apple's spelling checker, the system lexicon).

- `core/` (`hex-keyboard-core`): safe Rust. It never reads a clock, a file or the OS; the
  host passes in times, key geometry and data.
- `ffi/` (`hex-keyboard-ffi`, library `hex_keyboard`): the C interface in
  `include/hex_keyboard.h` (Swift imports it as `HexKeyboardCore`), and with the `jni`
  feature the Android entry points. All `unsafe` code is here; every entry point catches
  panics.

This is its own Cargo workspace with its own `Cargo.lock` and pinned toolchain
(`rust-toolchain.toml`), so the desktop workspace never builds it.

Pieces move here one at a time from the Swift keyboard (`ios/HEXShared/FullKeyboard`), each
proven identical to the Swift it replaces before iOS switches to it. Today: the typing
model reader, Swipe typing's decoder, the tap decoder (Spatial autocorrect,
`core/src/decoder`, which iOS runs through `TypingDecoder` and `TypingDecoderModel`) and
autocorrect's decisions (`core/src/autocorrect`). Tune those here, not in Swift.

## Setup

Install rustup ([rustup.rs](https://rustup.rs) or `brew install rustup`): in this folder it
uses the toolchain, components and targets `rust-toolchain.toml` pins (`rustup toolchain
install` fetches them ahead). iOS's build (`ios/scripts/build-keyboard-core.sh`, HEXKeyboard's
first build phase), Xcode Cloud's `ci_post_clone.sh` and Android's debug build find it the same
way (`tools/rustup-env.sh`): on PATH, in `CARGO_HOME` (by default `~/.cargo`), or Homebrew's
(`opt/rustup/bin` or `bin` under `HOMEBREW_PREFIX`), taking only a cargo that runs the pinned
toolchain here. Android also needs `cargo install cargo-ndk`.

```sh
cd crates/keyboard
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

`--all-features` includes the `jni` feature, so the Android entry points (`ffi/src/jni.rs`)
are checked too.

Tune Swipe typing here, in `core/src/swipe`: iOS's `SwipeDecoder` is a wrapper of it. The
decoder was ported from the Swift one it replaced, and proven to read every swipe as it did
(the same candidates in the same order, costs equal bit for bit) on 200,000 generated swipes,
in a separate run and in one process, and on recorded FUTO swipes typed through the keyboard.
The committed `core/tests/fixtures/swipe-parity.jsonl` holds 800 of those swipes per language
with the Swift decoder's readings: `core/tests/swipe_parity.rs` checks the core against it
(and, with `HEX_SWIPE_PARITY=<file>`, a larger export), and iOS's `SwipeFixtureTests` checks
the shipped path through the C interface. A change that reads any swipe differently fails
both: if it's intended, refresh the fixture from the app's decoder (the simulator writes to
the Mac's `/tmp`) and commit it with the change:

```sh
cd ios && xcodegen generate
TEST_RUNNER_HEX_SWIPE_PARITY_OUT=/tmp/swipe-parity.jsonl TEST_RUNNER_HEX_SWIPE_PARITY_COUNT=800 \
  xcodebuild test -project HEXMobile.xcodeproj -scheme HEXMobile \
  -destination 'platform=iOS Simulator,name=<simulator>' \
  -only-testing:HEXMobileTests/SwipeParityExportTests
cp /tmp/swipe-parity.jsonl ../crates/keyboard/core/tests/fixtures/swipe-parity.jsonl
cd ../crates/keyboard && cargo test --release --test swipe_parity -- --nocapture
```

`TEST_RUNNER_HEX_SWIPE_PARITY_TIMING=1` adds each read's time; a larger count makes a larger
file for `HEX_SWIPE_PARITY`.

## The tap decoder

`core/src/decoder` is `TypingDecoder` (`ios/HEXShared/FullKeyboard/TypingDecoder.swift`)
step for step, with one difference: where Swift walks a dictionary, readings that cost
exactly the same came out in an order that changed from run to run; here a tie in the search
goes to the commoner word, then the one earlier in the word list, and a tie between
run-together readings to the one with fewer words. About 1% of the parity cases have such a
tie (`tests/tap_parity.rs`).

`tests/tap_parity.rs` replays `tests/fixtures/tap-parity.jsonl` and must match it bit for bit,
and iOS's `TapParityGolden` (`ios/HEXMobileTests/TapParity`) generates the same cases again and
runs them through the shipped Swift wrappers and the C interface. The fixture holds the
answers of the Swift decoder this replaced (dev fc867484, in the fixed tie order), so a test
that fails against it is a change in behaviour.

When a change to the decoder is meant to change its answers, write the fixture again from the
shipped decoder and review the difference (`git diff --stat`, then a few lines) before
committing it with the change. From `ios/`, after `xcodegen generate`:

```sh
TEST_RUNNER_HEX_TAP_PARITY_OUT=$PWD/../crates/keyboard/core/tests/fixtures/tap-parity.jsonl \
xcodebuild test -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>" \
  -only-testing:HEXMobileTests/TapParityExport
```

`TEST_RUNNER_HEX_TAP_PARITY_COUNT` sets the cases per language (400 by default, which
`TapParityGolden` expects); a larger set written to `/tmp` runs in Rust with
`HEX_TAP_PARITY=/tmp/set.jsonl cargo test --release -p hex-keyboard-core --test tap_parity`.
A fixture written this way has no `tie` or `swift` notes: there is no Swift order any more.

### The tap benchmark

`examples/tap_replay` replays typing sets through the tap decoder alone, with the parameters
tuned for each model's language, and prints its word error rate and per-word latency as TSV
lines (in the Android benchmark's style, `android/benchmark/typing/Replay.java`) and as a
markdown table for a PR. It leaves out everything around the decoder (the engine's
contraction and typo rules, the dictionary, capitals, the strip), so compare its numbers only
with its own. It reads:

- `android/benchmark/typing/held-out/<lang>.tsv.gz`, the benchmark's 2,000 test sentences per
  language;
- the public tap datasets, converted by `ios/benchmark/typing-replay/convert.py` to iOS's tap
  JSON (`ios/README.md`, "Replaying real typing"), from wherever you keep them: they aren't in
  the repository;
- the Android benchmark's JSON Lines (`export_synthetic.py`).

All are typed on the 402 pt geometry `Replay.originalGeometry` gives. Each row has a name in
its set (a held-out row's Tatoeba ID, an iOS sentence's source, session and place in it, a
JSON Lines row's `id`), and the name alone picks its split: the test split when the first byte
of SHA-256(`hex-tap-split-v1:` + name) is odd, else the dev split (about half each).

Tune on the dev split, the default. The test split is sealed: it is read only with `--split
test --sealed`, once per setting. The setting is a hash of every language's parameters and
its model's SHA-256; the tool refuses the read when `bench/sealed.tsv` already holds that hash,
and otherwise appends the hash, the date and each set's results to it (with the set's
SHA-256). Commit the ledger with the change it measured.

```sh
cd crates/keyboard
M=../../ios/HEXKeyboard/TypingModel H=../../android/benchmark/typing/held-out
cargo run --release -p hex-keyboard-core --example tap_replay -- \
  $H/en.tsv.gz $M/en.hextm $H/es.tsv.gz $M/es.hextm $H/de.tsv.gz $M/de.hextm \
  /path/noisy-phone-test.json $M/en.hextm
# once the setting is locked: the same sets, sealed
cargo run --release -p hex-keyboard-core --example tap_replay -- --split test --sealed <the same sets>
# a small set of its own, for a smoke test
cargo run --release -p hex-keyboard-core --example tap_replay -- \
  --make-sample 200 /tmp/sample.jsonl $M/en.hextm
```

## Autocorrect

`core/src/autocorrect` is `AutocorrectEngine` (`ios/HEXShared/FullKeyboard/AutocorrectEngine*.swift`)
decision for decision: the strip, what space puts in, the next word's revision of the word
before, bilingual autocorrect's partner language, and the helpers they read (the never-suggest
list, German's capitals, Learn how I type's typos). The spelling dictionary stays the host's,
behind `TextChecker` (iOS: `UITextChecker`), asked in the same order the Swift asked; your
lexicon, vocabulary, kept words (the host keeps `LearnedWordsStore` on disk), settings and the
field come in as data. `autocorrect::text` replicates the Swift and Foundation string semantics
the decisions read by (grapheme clusters, canonical equivalence, `caseInsensitiveCompare`,
diacritic folding).

`tests/autocorrect_parity` replays `tests/fixtures/autocorrect-parity.jsonl`, cases dev
06ac30af's Swift engine answered with a deterministic dictionary, and must give the same
answers, the same engine state and the same dictionary calls in the same order; iOS's
`AutocorrectParityGolden` (`ios/HEXMobileTests/AutocorrectParity`) replays the same cases on the
shipped `AutocorrectEngine`, which runs on this core. The fixture also holds what Swift
answered otherwise (its own-words order, its decoder's ties, Foundation's
`caseInsensitiveCompare`, iOS's old never-suggest list), so those decided changes stay
counted. `tests/autocorrect_parity/main.rs` says how to write it again from the shipped engine.

A language is its BCP 47 tag across the C and JNI interfaces. Its facts are one manifest each
in `languages/<tag>.json`, compiled in by `core/build.rs` (no JSON is parsed while the keyboard
runs): the dictionaries to try, the typing model it reads, its accents, two-letter words,
starters and rules. `words_from` reads another manifest's fields, one level deep (fr-CA and
fr-CH read fr's, all but `model`: a model is read by the language it was built for). The host
hands over the letter grid it shows (`hk_autocorrect_set_keys`).
