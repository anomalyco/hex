# Sourced by ios/scripts/build-keyboard-core.sh, ios/ci_scripts/ci_post_clone.sh and
# android/app/build.gradle.kts. `find_rustup <crate folder>` sets rustup_bin to a folder that
# has rustup and rustup's cargo, and fails when there is none. rustup's cargo is whatever runs
# the toolchain the crate's rust-toolchain.toml pins when started in the crate: rustup's own
# links in ~/.cargo/bin, or the wrapper scripts Homebrew's rustup puts in opt/rustup/bin (it
# installs the pinned toolchain on first use, as a build would). Another cargo, such as the one
# Homebrew's `rust` puts in /opt/homebrew/bin, reports its own version and is passed over. It
# looks on PATH, then in CARGO_HOME (~/.cargo by default) and ~/.cargo, then in Homebrew's
# rustup (keg-only in opt/rustup/bin, or linked into bin) under HOMEBREW_PREFIX
# (/opt/homebrew by default).
find_rustup() {
  rustup_bin=
  channel=$(sed -n 's/^channel *= *"\(.*\)"$/\1/p' "$1/rust-toolchain.toml")
  [ -n "$channel" ] || return 1
  on_path=$(command -v rustup 2>/dev/null || true)
  brew=${HOMEBREW_PREFIX:-/opt/homebrew}
  for dir in "${on_path%/*}" "${CARGO_HOME:-$HOME/.cargo}/bin" "$HOME/.cargo/bin" \
    "$brew/opt/rustup/bin" "$brew/bin"; do
    [ -n "$dir" ] && [ -x "$dir/rustup" ] && [ -x "$dir/cargo" ] || continue
    version=$(cd "$1" && "$dir/cargo" --version 2>/dev/null) || continue
    case "$version" in
      "cargo $channel "*)
        rustup_bin=$dir
        return 0
        ;;
    esac
  done
  return 1
}
