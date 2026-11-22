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
model reader, Swipe typing's decoder and the tap decoder (Spatial autocorrect,
`core/src/decoder`). Tune those here, not in Swift.

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

`tests/tap_parity.rs` replays `tests/fixtures/tap-parity.jsonl`, cases the Swift oracle
(`ios/HEXMobileTests/TapParity`) wrote, and must match them bit for bit; it says how to
write the fixture again, or a larger set for `HEX_TAP_PARITY=/path`.

`examples/tap_replay.rs` replays a typing set through the decoder alone and prints its word
error rate and per-word latency as the Android benchmark (`android/benchmark/typing`) does.
It reads that benchmark's synthetic JSON Lines (`export_synthetic.py`; the sets aren't in the
repository) or iOS's tap JSON (`ios/README.md`, "Replaying real typing"). It leaves out
everything around the decoder (the engine's contraction and typo rules, the dictionary,
capitals, the strip), so compare its numbers only with its own.

```sh
cargo run --release -p hex-keyboard-core --example tap_replay -- \
  /path/en-test.jsonl ../../ios/HEXKeyboard/TypingModel/en.hextm
# a small set of its own, for a smoke test
cargo run --release -p hex-keyboard-core --example tap_replay -- \
  --make-sample 200 /tmp/sample.jsonl ../../ios/HEXKeyboard/TypingModel/en.hextm
```
