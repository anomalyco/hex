#!/bin/sh
# Build the Linux release executable for this machine's architecture,
# dist/hex-<version>-linux-<x64|arm64>, with portable CPU kernels. The
# Release workflow publishes it as the linux-<x64|arm64> target of the
# release service (docs/plans/release-service.md), and
# scripts/package-linux.sh wraps it in the .deb and .rpm packages.
set -eu

script_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
root=${HEX_RELEASE_SOURCE:-$script_root}
version=${HEX_VERSION:-$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" | jq -r '.packages[] | select(.name == "hex") | .version')}
dist=${HEX_RELEASE_DIST:-"$root/dist"}
target_dir=${CARGO_TARGET_DIR:-"$root/target"}

for command in cargo file jq python3; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Linux builds require $command." >&2
    exit 1
  fi
done
case "$(uname -s) $(uname -m)" in
  "Linux x86_64") label=x64 elf='x86-64' ;;
  "Linux aarch64") label=arm64 elf='ARM aarch64' ;;
  *)
    echo "Linux releases must be built on x86_64 or aarch64 Linux." >&2
    exit 1
    ;;
esac
if ! printf '%s\n' "$version" | grep -Eq '^([0-9]+\.[0-9]+\.[0-9]+(-beta\.[0-9]+)?|0\.0\.0-dev-[0-9]+)$'; then
  echo "Release builds need a version like 2.4.0, 2.5.0-beta.1, or 0.0.0-dev-3, not $version." >&2
  exit 1
fi
if [ -n "$(git -C "$root" status --porcelain)" ]; then
  echo "Commit or remove all working-tree changes before building a release." >&2
  exit 1
fi

# Only the executable's path goes to stdout.
"$script_root/scripts/test-install-linux-release.sh" >&2
"$script_root/scripts/set-crate-version.sh" "$root" "$version" >&2
mkdir -p "$dist"
# Portable native code: ggml otherwise tunes its CPU kernels to the build
# machine (for example AVX-512 or SVE), and the release would crash on older
# CPUs. No system BLAS either, so the executable and packages need no
# OpenBLAS runtime.
(cd "$root" && TRANSCRIBE_CMAKE_ARGS="-DGGML_NATIVE=OFF -DTRANSCRIBE_USE_SYSTEM_BLAS=OFF${TRANSCRIBE_CMAKE_ARGS:+ $TRANSCRIBE_CMAKE_ARGS}" \
  cargo build --locked --release) >&2
executable="$dist/hex-$version-linux-$label"
install -m755 "$target_dir/release/hex" "$executable"
file "$executable" | grep -E "ELF 64-bit.*$elf" >/dev/null
test "$("$executable" --version | awk 'NF { value=$NF } END { print value }')" = "$version"
echo "$executable"
