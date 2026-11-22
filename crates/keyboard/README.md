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
model reader and Swipe typing's decoder. Tune those here, not in Swift.

```sh
cd crates/keyboard
cargo test
cargo clippy --all-targets -- -D warnings
```

Swipe typing's decoder reads every swipe as the Swift decoder it was ported from
(`SwipeDecoderOracle`, kept in `ios/HEXMobileTests/SwipeParity` until iOS switches): the same
candidates in the same order, with costs equal bit for bit. `core/tests/swipe_parity.rs`
checks the committed `core/tests/fixtures/swipe-parity.jsonl` (800 swipes per language) and,
with `HEX_SWIPE_PARITY=<file>`, a larger export. To export one (the simulator writes to the
Mac's `/tmp`):

```sh
cd ios && xcodegen generate
TEST_RUNNER_HEX_SWIPE_PARITY_OUT=/tmp/hex-swipe-parity-100k.jsonl \
TEST_RUNNER_HEX_SWIPE_PARITY_COUNT=100000 TEST_RUNNER_HEX_SWIPE_PARITY_TIMING=1 \
  xcodebuild test -project HEXMobile.xcodeproj -scheme HEXMobile \
  -destination 'platform=iOS Simulator,name=<simulator>' SWIFT_OPTIMIZATION_LEVEL=-O \
  -only-testing:HEXMobileTests/SwipeParityExportTests
cd ../crates/keyboard
HEX_SWIPE_PARITY=/tmp/hex-swipe-parity-100k.jsonl cargo test --release --test swipe_parity -- --nocapture
```
