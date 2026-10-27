#!/bin/sh
# Renders and publishes the Homebrew cask anomalyco/tap/hex
# (anomalyco/homebrew-tap, Casks/hex.rb) from packaging/homebrew/hex.rb.
#
#   release-homebrew.sh render <version> <dmg-url> <sha256> <out>
#   release-homebrew.sh publish <cask> <version>  # needs HOMEBREW_TAP_DEPLOY_KEY
#
# The DMG URL is the release service's macos-arm64-dmg file. `publish` pushes
# with the tap deploy key github-actions-hex-homebrew under its own SSH host
# alias, so it never stands in for github.com; a cask that already matches is
# left alone, so reruns are no-ops.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
usage() {
  echo "Usage: $0 render <version> <dmg-url> <sha256> <out> | publish <cask> <version>" >&2
  exit 1
}

case "${1:-}" in
  render)
    [ $# -eq 5 ] || usage
    version=$2 url=$3 sha256=$4 out=$5
    printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-(beta|dev)\.[0-9]+)?$'
    printf '%s\n' "$sha256" | grep -Eq '^[0-9a-f]{64}$'
    printf '%s\n' "$url" | grep -Eq '^https://[A-Za-z0-9./_-]+\.dmg$'
    case "$url" in
      *"$version"*) ;;
      *)
        echo "The DMG URL must name version $version." >&2
        exit 1
        ;;
    esac
    # Homebrew wants the URL to interpolate the version.
    # shellcheck disable=SC2016
    versioned=$(printf '%s' "$url" | sed "s|$(printf '%s' "$version" | sed 's/[.]/\\./g')|#{version}|g")
    sed -e "s|@VERSION@|$version|" -e "s|@SHA256@|$sha256|" -e "s|@URL@|$versioned|" \
      "$root/packaging/homebrew/hex.rb" > "$out"
    if grep -q '@[A-Z0-9_]*@' "$out"; then
      echo "The cask has an unfilled placeholder." >&2
      exit 1
    fi
    echo "$out"
    ;;
  publish)
    [ $# -eq 3 ] || usage
    cask=$2 version=$3
    : "${HOMEBREW_TAP_DEPLOY_KEY:?Set HOMEBREW_TAP_DEPLOY_KEY to the tap deploy key}"
    temporary=$(mktemp -d)
    trap 'rm -rf "$temporary"' EXIT HUP INT TERM
    (umask 077 && printf '%s\n' "$HOMEBREW_TAP_DEPLOY_KEY" > "$temporary/key")
    # The published ED25519 host key of github.com.
    echo 'github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl' > "$temporary/known_hosts"
    export GIT_SSH_COMMAND="ssh -i $temporary/key -o IdentitiesOnly=yes -o UserKnownHostsFile=$temporary/known_hosts -o StrictHostKeyChecking=yes"
    git clone -q --depth 1 git@github.com:anomalyco/homebrew-tap.git "$temporary/tap"
    cp "$cask" "$temporary/tap/Casks/hex.rb"
    cd "$temporary/tap"
    git add Casks/hex.rb
    if git diff --cached --quiet; then
      echo "anomalyco/tap/hex already serves $version." >&2
      exit 0
    fi
    git -c user.name='Hex release' -c user.email='hex@anoma.ly' commit -qm "hex $version"
    git push -q origin HEAD
    echo "https://github.com/anomalyco/homebrew-tap/blob/$(git rev-parse HEAD)/Casks/hex.rb"
    ;;
  *) usage ;;
esac
