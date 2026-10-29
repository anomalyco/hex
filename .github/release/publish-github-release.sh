#!/bin/sh
# Mirrors a stable release's downloads to its GitHub release, byte for byte,
# off "Latest" (which the TypeScript SDK uses). Release runs it last, after
# tag-release.sh tagged v<version> at the release commit; it never creates the
# tag itself.
#
#   publish-github-release.sh <dist>
#
# Environment: HEX_VERSION, HEX_RELEASE_COMMIT, HEX_RELEASE_TAG,
# HEX_RELEASE_SOURCE (a checkout of the commit with docs/releases/), GH_TOKEN.
set -eu

repo=${HEX_RELEASE_REPO:-anomalyco/hex}
[ $# -eq 1 ] || {
  echo "Usage: $0 <dist>" >&2
  exit 1
}
dist=$1
version=${HEX_VERSION:?Set HEX_VERSION}
commit=${HEX_RELEASE_COMMIT:?Set HEX_RELEASE_COMMIT}
tag=${HEX_RELEASE_TAG:?Set HEX_RELEASE_TAG}
source=${HEX_RELEASE_SOURCE:?Set HEX_RELEASE_SOURCE}

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

assets=$(cd "$dist" && { ls Hex-*.zip Hex-*.dmg Hex-*.exe hex-*-linux-* 2>/dev/null || true; })
[ -n "$assets" ] || {
  echo "No release files in $dist." >&2
  exit 1
}

notes="$source/docs/releases/$version.md"
test -f "$notes" || {
  echo "Add release notes at docs/releases/$version.md." >&2
  exit 1
}
tagged=$(gh api "repos/$repo/git/ref/tags/$tag" --jq .object.sha)
if [ "$tagged" != "$commit" ]; then
  echo "Tag $tag names $tagged, not $commit." >&2
  exit 1
fi
if ! gh release view "$tag" -R "$repo" --json name >/dev/null 2>&1; then
  gh release create "$tag" -R "$repo" --verify-tag --title "Hex $version" \
    --notes-file "$notes" --latest=false
fi
gh release view "$tag" -R "$repo" --json assets > "$temporary/release.json"
mkdir -p "$temporary/mirror"
for name in $assets; do
  if ! jq -e --arg name "$name" '.assets | any(.name == $name)' "$temporary/release.json" >/dev/null; then
    gh release upload "$tag" "$dist/$name" -R "$repo"
  fi
  gh release download "$tag" -R "$repo" --pattern "$name" --dir "$temporary/mirror" --clobber
  cmp -s "$dist/$name" "$temporary/mirror/$name" || {
    echo "GitHub's $name differs from the release bytes." >&2
    exit 1
  }
  rm -f "$temporary/mirror/$name"
done
echo "https://github.com/$repo/releases/tag/$tag"
