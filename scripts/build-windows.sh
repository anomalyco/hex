#!/bin/sh
# Build one Windows release executable, dist/Hex-<version>-windows-<x64|arm64>.exe,
# from Git Bash on Windows: HEX_WINDOWS_ARCH=x86_64 (default; Vulkan SDK
# required) or aarch64 (clang-cl and Ninja, best on an ARM64 host). CI then
# Authenticode-signs it and wraps it in the Setup.exe installer; the Release
# workflow publishes both (docs/plans/release-service.md).
set -eu

script_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
root=${HEX_RELEASE_SOURCE:-$script_root}
version=${HEX_VERSION:-$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" | jq -r '.packages[0].version')}
dist=${HEX_RELEASE_DIST:-"$root/dist"}
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
arch=${HEX_WINDOWS_ARCH:-x86_64}
case "$arch" in
  x86_64) label=x64 ;;
  aarch64) label=arm64 ;;
  *)
    echo "HEX_WINDOWS_ARCH must be x86_64 or aarch64." >&2
    exit 1
    ;;
esac
triple="$arch-pc-windows-msvc"

for command in cargo jq; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Windows builds require $command." >&2
    exit 1
  fi
done
case "$(uname -s)" in
  MINGW64* | MSYS*) ;;
  *)
    echo "Windows releases must be built from Git Bash on Windows." >&2
    exit 1
    ;;
esac
if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-(beta|dev)\.[0-9]+)?$'; then
  echo "Release builds need a version like 2.4.0, 2.5.0-beta.1, or 2.5.0-dev.3, not $version." >&2
  exit 1
fi
if [ -n "$(git -C "$root" status --porcelain)" ]; then
  echo "Commit or remove all working-tree changes before building a release." >&2
  exit 1
fi
installer="$root/scripts/install-windows.ps1"
# PowerShell needs a Windows path; Git Bash converts only bare arguments.
installer_windows=$(cygpath -w "$installer" 2>/dev/null || printf '%s' "$installer")
powershell -NoProfile -Command "[void][ScriptBlock]::Create((Get-Content -Raw '$installer_windows'))"

"$script_root/scripts/set-crate-version.sh" "$root" "$version"
mkdir -p "$dist"
# Portable native code: ggml otherwise tunes its CPU kernels to this build
# machine (for example AVX-512), and the release would crash on older CPUs.
cmake_args="-DGGML_NATIVE=OFF"
if [ "$arch" = aarch64 ]; then
  # ggml's ARM CPU backend refuses MSVC; clang-cl uses LLVM codegen, so
  # ggml's own threadpool replaces OpenMP there.
  cmake_args="$cmake_args -DGGML_OPENMP=OFF"
  export CC=clang-cl CXX=clang-cl CMAKE_GENERATOR=Ninja
fi
# Cargo reads .cargo/config.toml (the static Windows runtime) from the
# working directory, not from --manifest-path.
(cd "$root" && TRANSCRIBE_CMAKE_ARGS="$cmake_args" cargo build --locked --release --target "$triple")
executable="$dist/Hex-$version-windows-$label.exe"
cp "$target_dir/$triple/release/voice-control.exe" "$executable"
test "$(head -c 2 "$executable")" = MZ
# An executable for the other architecture cannot run here; the installer
# test checks it on its own runner.
if [ "$(uname -m)" = "$arch" ]; then
  test "$("$executable" --version | awk 'NF { value=$NF } END { print value }')" = "$version"
fi
echo "$executable"
