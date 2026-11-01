#!/usr/bin/env bash
# publish-dev-notes.sh <dev-notes.json> <version> <commit>: checks the dev
# channel's notes, written without secrets by scripts/release-dev-notes.sh,
# and uploads them as dev-notes.json to HEX_RELEASE_BUCKET
# (https://anoma.ly/hex/files/dev-notes.json) with .github/release/r2-put.sh.
#
# The file is untrusted data: https://anoma.ly/hex/dev renders it and the
# app shows it, so it must be the expected shape, put this build first, and
# link only to this repository's pull requests and commits.
. "$(dirname "$0")/lib.sh"

[ $# -eq 3 ] || die "Usage: publish-dev-notes.sh <dev-notes.json> <version> <commit>"
file=$1 version=$2 commit=$3
[ -f "$file" ] || die "$file does not exist."
# One JSON document, re-emitted by jq: what is uploaded is exactly what was
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
sh "$(dirname "$0")/r2-put.sh" \
  dev-notes.json "$canonical" "application/json; charset=utf-8" "public, max-age=60"
echo "Published the notes for $version: https://anoma.ly/hex/dev#$version"
