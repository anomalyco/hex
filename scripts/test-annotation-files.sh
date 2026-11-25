#!/usr/bin/env bash
# Only file fixtures, never the user's library, clipboard, microphone or router.
set -eu
fixture=$(mktemp -d "${TMPDIR:-/tmp}/hex-annotation-check.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export HOME="$fixture" USERPROFILE="$fixture" HEX_APPLICATION_SUPPORT_DIR="$fixture/hex"
export APPDATA="$fixture/roaming" LOCALAPPDATA="$fixture/local" XDG_CONFIG_HOME="$fixture/config"
export XDG_DATA_HOME="$fixture/data" XDG_STATE_HOME="$fixture/state" XDG_CACHE_HOME="$fixture/cache"
export HEX_DISABLE_UPDATES=1 SCRAWL_NO_OPENCODE=1 SCRAWL_CONFIG="$fixture/no-config.json"
export SCRAWL_HELPER="$fixture/no-helper" SCRAWL_UI_HELPER="$fixture/no-ui-helper"
if [[ "${1:-}" == --editor-media ]]; then
    # Hex's bundled AVFoundation decoder, resolved relative to our test
    # executable. helper::path neither searches PATH nor opens another app.
    unset SCRAWL_HELPER
    pkgid=$(cargo pkgid)
    cargo nextest run --locked -p "hex@${pkgid##*[#@]}" --bin hex --run-ignored only annotation_packages_open_in_the_editor
    exit
fi
filters=(hex_annotation history::annotations::tests capture_host::library::tests capture_host::annotate_read::tests paste::recent::tests screenshots::recording::tests slim_zip::tests rune_ui::history::unified::proof::one_history)
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    scripts/check "${filters[@]}"
else
    # A synced worker has sources but no Git metadata. Run the same scoped
    # nextest stage; run the Git-based code shape and pin checks in the checkout.
    pkgid=$(cargo pkgid)
    cargo nextest run --locked -p "hex@${pkgid##*[#@]}" -p scrawl-router -p scrawl-capture "${filters[@]}"
fi
