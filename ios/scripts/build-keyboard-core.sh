#!/bin/sh
# Builds the keyboard's Rust core (crates/keyboard, library hex_keyboard) for the platform
# Xcode is building, as build/keyboard-core/$PLATFORM_NAME/libhex_keyboard.a, which HEXKeyboard
# and HEXMobile link. Run by HEXKeyboard's pre-build script phase; Cargo skips the work when
# nothing changed. Always optimised: the decoders are too slow unoptimised, and tests run in
# Debug.
set -eu

case "${PLATFORM_NAME:-}" in
  iphoneos) target=aarch64-apple-ios ;;
  iphonesimulator) target=aarch64-apple-ios-sim ;;
  *) echo "error: build-keyboard-core.sh: no Rust target for platform '${PLATFORM_NAME:-}'" >&2; exit 1 ;;
esac
for arch in ${ARCHS:-arm64}; do
  if [ "$arch" != arm64 ]; then
    echo "error: build-keyboard-core.sh: the keyboard core is built for arm64 only, not $arch" >&2
    exit 1
  fi
done

cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
if [ ! -x "$cargo_bin/cargo" ] || [ ! -x "$cargo_bin/rustup" ]; then
  echo "error: build-keyboard-core.sh: rustup's cargo isn't in $cargo_bin; install rustup (https://rustup.rs)" >&2
  exit 1
fi

crate_dir="$PROJECT_DIR/../crates/keyboard"
out_dir="$PROJECT_DIR/build/keyboard-core"
mkdir -p "$out_dir/$PLATFORM_NAME"

# A clean environment: Xcode's (SDKROOT for iOS, its build settings) would point the host
# build of dependencies' build scripts at the iOS SDK. rustc and cc find the iOS SDK for the
# target themselves, through the same Xcode (DEVELOPER_DIR). Run from the crate so rustup
# picks its pinned toolchain (rust-toolchain.toml).
cd "$crate_dir"
env -i \
  HOME="$HOME" \
  PATH="$cargo_bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  ${DEVELOPER_DIR:+DEVELOPER_DIR="$DEVELOPER_DIR"} \
  ${CARGO_HOME:+CARGO_HOME="$CARGO_HOME"} \
  ${RUSTUP_HOME:+RUSTUP_HOME="$RUSTUP_HOME"} \
  ${CARGO_BUILD_JOBS:+CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS"} \
  IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-18.0}" \
  CARGO_TARGET_DIR="$out_dir/target" \
  cargo build --release --locked --target "$target" \
  --manifest-path "$crate_dir/Cargo.toml" -p hex-keyboard-ffi

built="$out_dir/target/$target/release/libhex_keyboard.a"
library="$out_dir/$PLATFORM_NAME/libhex_keyboard.a"
# Replaced only when it changed, so an unchanged core doesn't relink the targets, and whole,
# so a link never reads half of it.
if ! cmp -s "$built" "$library"; then
  cp "$built" "$library.$$"
  mv -f "$library.$$" "$library"
fi
