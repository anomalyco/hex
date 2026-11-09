#!/usr/bin/env bash
# publish-dev-notes.sh <dev-notes.json> <version> <commit>: checks the dev
# channel's notes, written without secrets by scripts/release-dev-notes.sh,
# and publishes them as dev-notes.json on the one-commit `dev-notes` branch.
# The site Worker serves that file at
# https://anoma.ly/hex/files/dev-notes.json and renders
# https://anoma.ly/hex/dev from it, so a new dev build shows without a site
# deploy and no job needs a bucket credential.
#
# The file is untrusted data: the site renders it and the app shows it, so it
# must be the expected shape, put this build first, and link only to this
# repository's pull requests and commits. The branch is replaced, not
# appended to, so its history never grows.
. "$(dirname "$0")/lib.sh"

[ $# -eq 3 ] || die "Usage: publish-dev-notes.sh <dev-notes.json> <version> <commit>"
file=$1 version=$2 commit=$3
[ -f "$file" ] || die "$file does not exist."
: "${GH_TOKEN:?Set GH_TOKEN}"
: "${GITHUB_REPOSITORY:?Set GITHUB_REPOSITORY}"
repository=$GITHUB_REPOSITORY
branch=dev-notes
path=dev-notes.json
# One JSON document, re-emitted by jq: what is published is exactly what was
# checked, valid UTF-8 without duplicate keys.
canonical=$(mktemp)
trap 'rm -f "$canonical"' EXIT
jq -se 'length == 1' "$file" > /dev/null || die "The dev notes are not one JSON document."
jq -c '.' "$file" > "$canonical" || die "The dev notes are not JSON."
[ "$(wc -c < "$canonical")" -le 200000 ] || die "The dev notes are over 200 KB."
jq -e --arg version "$version" --arg commit "$commit" '
  def text($max): type == "string" and length > 0 and length <= $max and all(explode[]; . >= 32 and (. < 127 or . > 159));
  (keys == ["builds", "schema"]) and .schema == 1
  and (.builds | type == "array" and length >= 1 and length <= 40)
  and .builds[0].version == $version and .builds[0].commit == $commit
  and ([.builds[].version] | length == (unique | length))
  and all(.builds[];
    (keys == ["commit", "items", "published_at", "version"])
    and (.version | type == "string" and test("^0\\.0\\.0-dev-[0-9]+$"))
    and (.commit | type == "string" and test("^[0-9a-f]{40}$"))
    and (.published_at | type == "string" and test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"))
    and (.items | type == "array" and length <= 40)
    and all(.items[];
      (keys == ["title", "url"])
      and (.title | text(200))
      and (.url | type == "string"
        and test("^https://github\\.com/anomalyco/hex/(pull/[0-9]{1,7}|commit/[0-9a-f]{40})$"))))
' "$canonical" > /dev/null || die "The dev notes are not the expected shape for $version ($commit)."

# Replace the branch with one commit that has no parent, so the repository
# keeps the current file and no history.
# gh prints the error body to stdout when the branch doesn't exist yet, so only a SHA counts.
existing=$(gh api "repos/$repository/git/ref/heads/$branch" --jq .object.sha 2> /dev/null || true)
[[ $existing =~ ^[0-9a-f]{40}$ ]] || existing=
parents='[]'
blob=$(jq -cn --arg content "$(base64 -w0 < "$canonical")" '{content: $content, encoding: "base64"}' |
  gh api "repos/$repository/git/blobs" --method POST --input - --jq .sha)
tree=$(jq -cn --arg blob "$blob" --arg path "$path" \
  '{tree: [{path: $path, mode: "100644", type: "blob", sha: $blob}]}' |
  gh api "repos/$repository/git/trees" --method POST --input - --jq .sha)
published=$(jq -cn --arg message "Dev notes for $version" --arg tree "$tree" --argjson parents "$parents" \
  '{message: $message, tree: $tree, parents: $parents}' |
  gh api "repos/$repository/git/commits" --method POST --input - --jq .sha)
if [ -n "$existing" ]; then
  gh api "repos/$repository/git/refs/heads/$branch" --method PATCH -f sha="$published" -F force=true > /dev/null
else
  gh api "repos/$repository/git/refs" --method POST -f ref="refs/heads/$branch" -f sha="$published" > /dev/null
fi
echo "Published the notes for $version: https://anoma.ly/hex/dev#$version"
