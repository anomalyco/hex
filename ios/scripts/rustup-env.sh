# Sourced by build-keyboard-core.sh and ci_scripts/ci_post_clone.sh. find_rustup sets
# rustup_bin to the folder of rustup and its own cargo (a proxy that follows
# crates/keyboard/rust-toolchain.toml, never another cargo such as the one Homebrew's `rust`
# puts in /opt/homebrew/bin), and fails when there is none. It looks on PATH, then in
# CARGO_HOME (~/.cargo by default) and ~/.cargo, then in Homebrew's rustup (keg-only in
# opt/rustup/bin, or linked into bin) under HOMEBREW_PREFIX (/opt/homebrew by default).
find_rustup() {
  rustup_bin=
  on_path=$(command -v rustup 2>/dev/null || true)
  brew=${HOMEBREW_PREFIX:-/opt/homebrew}
  for dir in "${on_path%/*}" "${CARGO_HOME:-$HOME/.cargo}/bin" "$HOME/.cargo/bin" \
    "$brew/opt/rustup/bin" "$brew/bin"; do
    if [ -n "$dir" ] && [ -x "$dir/rustup" ] && [ -x "$dir/cargo" ] && [ "$dir/cargo" -ef "$dir/rustup" ]; then
      rustup_bin=$dir
      return 0
    fi
  done
  return 1
}
