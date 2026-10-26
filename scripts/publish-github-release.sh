#!/bin/sh
# Mirrors a release's downloads to its GitHub release, byte for byte. Stable
# pushes tag app-v<version> at the release commit with git (the Releases API
# cannot create it with GITHUB_TOKEN when dev's workflows have moved on) and
# publishes the release off "Latest", which the TypeScript SDK uses; staging
# keeps a draft prerelease.
#
#   publish-github-release.sh <dist>
#
# Environment: HEX_VERSION, HEX_RELEASE_COMMIT, HEX_RELEASE_CHANNEL (stable or
# staging), HEX_RELEASE_TAG, HEX_RELEASE_SOURCE (a checkout of the commit with
# push credentials), HEX_RELEASE_NOTES_URL, GH_TOKEN.
set -eu

repo=${HEX_RELEASE_REPO:-anomalyco/hex}
[ $# -eq 1 ] || {
  echo "Usage: $0 <dist>" >&2
  exit 1
}
dist=$1
version=${HEX_VERSION:?Set HEX_VERSION}
commit=${HEX_RELEASE_COMMIT:?Set HEX_RELEASE_COMMIT}
channel=${HEX_RELEASE_CHANNEL:?Set HEX_RELEASE_CHANNEL}
tag=${HEX_RELEASE_TAG:?Set HEX_RELEASE_TAG}
source=${HEX_RELEASE_SOURCE:?Set HEX_RELEASE_SOURCE}

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

assets=$(cd "$dist" && { ls Hex-*.zip Hex-*.dmg Hex-*.exe hex-*-linux-x64 2>/dev/null || true; })
[ -n "$assets" ] || {
  echo "No release files in $dist." >&2
  exit 1
}

if [ "$channel" = stable ]; then
  notes="$source/docs/releases/$version.md"
  test -f "$notes" || {
    echo "Add release notes at docs/releases/$version.md." >&2
    exit 1
  }
  # An annotated tag's peeled commit is listed last.
  tagged=$(git -C "$source" ls-remote origin "refs/tags/$tag" "refs/tags/$tag^{}" | awk 'END { print $1 }')
  if [ -z "$tagged" ]; then
    git -C "$source" tag "$tag" "$commit"
    git -C "$source" push origin "refs/tags/$tag"
  elif [ "$tagged" != "$commit" ]; then
    echo "Tag $tag already names $tagged, not $commit." >&2
    exit 1
  fi
fi
if ! gh release view "$tag" -R "$repo" --json name >/dev/null 2>&1; then
  if [ "$channel" = stable ]; then
    gh release create "$tag" -R "$repo" --verify-tag --title "Hex $version" \
      --notes-file "$notes" --latest=false
  else
    printf 'Staging build of Hex %s from %s, published to %s.\n' \
      "$version" "$commit" "${HEX_RELEASE_NOTES_URL:-the dev release service}" > "$temporary/notes.md"
    gh release create "$tag" -R "$repo" --target dev --title "Hex $version (staging)" \
      --notes-file "$temporary/notes.md" --draft --prerelease
  fi
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
