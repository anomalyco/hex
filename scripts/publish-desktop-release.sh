#!/bin/sh
# Publish the Linux and Windows release signed by a successful Release desktop
# workflow run, using this Mac's Wrangler login (the personal Cloudflare
# account that owns hex-releases). Artifacts go first and feeds last; nothing
# replaces different bytes or an equal or newer version. The Setup.exe
# installers and the GitHub mirror follow; see publish-desktop-downloads.sh.
#
#   scripts/publish-desktop-release.sh <run-id>
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
repo=${HEX_RELEASE_REPO:-anomalyco/hex}
feeds="linux-update.json windows-update.json windows-aarch64-update.json"
if [ $# -ne 1 ]; then
  echo "Usage: $0 <release-desktop-run-id>" >&2
  exit 1
fi
run=$1
for command in gh git jq wrangler; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Publishing requires $command." >&2
    exit 1
  fi
done

run_info=$(gh run view "$run" -R "$repo" --json workflowName,conclusion,headBranch,event)
if [ "$(printf '%s' "$run_info" | jq -r .workflowName)" != "Release desktop" ] ||
  [ "$(printf '%s' "$run_info" | jq -r .conclusion)" != success ]; then
  echo "Run $run is not a successful Release desktop run." >&2
  exit 1
fi
branch=$(printf '%s' "$run_info" | jq -r .headBranch)
case "$branch" in
  main | app-v*) ;;
  *)
    echo "Run $run used workflow code from $branch; publish only runs from main or a release tag." >&2
    exit 1
    ;;
esac

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
dist="$temporary/dist"
gh run download "$run" -R "$repo" -n signed-release -D "$dist"
commit=$(jq -r .commit "$dist/release.json")
version=$(jq -r .version "$dist/release.json")

# Publish the installers exactly as committed in the release.
if ! git -C "$root" cat-file -e "$commit^{commit}" 2>/dev/null; then
  git -C "$root" fetch --quiet origin "$commit"
fi
source="$temporary/source"
mkdir -p "$source/scripts"
for installer in install-linux-release.sh install-windows.ps1; do
  git -C "$root" show "$commit:scripts/$installer" > "$source/scripts/$installer"
done

echo "Publishing HEX $version from $commit (run $run)." >&2
# shellcheck disable=SC2086
HEX_VERSION="$version" HEX_RELEASE_SOURCE="$source" \
  "$root/scripts/release-feed.sh" publish "$dist" $feeds

# Setup.exe installers to R2, then the GitHub mirror of the downloads.
"$root/scripts/publish-desktop-downloads.sh" "$dist" "$version"
