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
scripts/check 'hex_annotation' 'history::annotations::tests' 'capture_host::library::tests' 'slim_zip::tests'
