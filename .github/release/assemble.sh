#!/usr/bin/env bash
# assemble.sh <in> <dist>: gathers the release files from the verified
# artifacts in <in>/<name>/ into <dist>, refusing any file that is not one of
# the release service targets for HEX_VERSION (docs/plans/release-service.md)
# or that two artifacts both hold.
. "$(dirname "$0")/lib.sh"

in=$(posix "$1")
dist=$(posix "$2")
v=$HEX_VERSION
mkdir -p "$dist"
for file in "$in"/*/*; do
  name=$(basename "$file")
  [ "$name" = SHA256SUMS ] && continue
  case "$name" in
    "hex-$v-linux-x64" | "hex-$v-linux-x64.deb" | "hex-$v-linux-x64.rpm" | \
      "hex-$v-linux-arm64" | "hex-$v-linux-arm64.deb" | "hex-$v-linux-arm64.rpm" | \
      "hex-$v-macos-arm64.zip" | "hex-$v-macos-arm64.dmg" | \
      "hex-$v-windows-x64.exe" | "hex-$v-windows-arm64.exe" | \
      "hex-setup-$v-windows-x64.exe" | "hex-setup-$v-windows-arm64.exe") ;;
    *) die "$name is not a release file of Hex $v." ;;
  esac
  [ ! -e "$dist/$name" ] || die "Two artifacts hold $name."
  cp "$file" "$dist/$name"
done
(cd "$dist" && ls -l && sha256 -- *) | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"
