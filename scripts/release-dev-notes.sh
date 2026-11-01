#!/usr/bin/env bash
# Writes the dev channel's notes with the new build on top: the PRs merged
# since the previous dev build, from their titles, skipping CI fixes and
# internal-only changes. The Release workflow runs it without secrets; its
# dev-notes job checks the result (.github/release/publish-dev-notes.sh) and
# uploads it to https://anoma.ly/hex/files/dev-notes.json, which
# https://anoma.ly/hex/dev and the app's update item read.
#
#   release-dev-notes.sh <version> <commit> <out>
#
# Needs the repository's full history at <commit>, curl and jq. The previous
# build is the newest entry of the published notes; before there are any,
# the commit the dev channel's notes_url names.
set -euo pipefail

version=${1:?Usage: release-dev-notes.sh <version> <commit> <out>}
commit=${2:?Usage: release-dev-notes.sh <version> <commit> <out>}
out=${3:?Usage: release-dev-notes.sh <version> <commit> <out>}
repository=https://github.com/anomalyco/hex
notes_url=${HEX_DEV_NOTES_URL:-https://anoma.ly/hex/files/dev-notes.json}
manifest_url=${HEX_DEV_MANIFEST_URL:-https://anoma.ly/release/hex/desktop/dev.json}
max_builds=40
max_items=40
max_bytes=200000

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# A missing file starts the history; any other failure stops here, so a
# flaky read never publishes notes that dropped every earlier build.
status=$(curl --silent --show-error --location --proto =https --max-time 60 \
  --output "$work/previous.json" --write-out '%{http_code}' "$notes_url")
case "$status" in
  200) jq -e '.schema == 1 and (.builds | type) == "array"' "$work/previous.json" > /dev/null ;;
  404) echo '{"schema": 1, "builds": []}' > "$work/previous.json" ;;
  *) echo "Could not read $notes_url (HTTP $status)." >&2; exit 1 ;;
esac
# A rerun of a build lists what it changed since the build before it.
previous=$(jq -r --arg version "$version" \
  '[.builds[] | select(.version != $version)][0].commit // empty' "$work/previous.json")
if [ -z "$previous" ]; then
  previous=$(curl --fail --silent --show-error --location --proto =https --max-time 60 "$manifest_url" |
    jq -r '.release.notes_url // "" | capture("/commit/(?<sha>[0-9a-f]{40})$").sha // empty')
fi
if [ -n "$previous" ] && git merge-base --is-ancestor "$previous" "$commit" 2> /dev/null; then
  range=("$previous..$commit")
else
  echo "No earlier dev build in this history; listing $commit alone." >&2
  range=(-1 "$commit")
fi

# Paths the desktop app is built from; a change elsewhere (docs, the site,
# iOS, CI) is not in a dev build.
app_path='^(src/|crates/|native/|resources/|assets/|app/|packaging/|sdk/commands/|build\.rs$|Cargo\.toml$)'
test_path='(^tests/|/tests/|_tests\.rs$|/goldens/)'
# Titles of changes nobody using Hex notices.
internal='^(fix\(ci\)|(ci|chore|test|tests|refactor|docs|build|style)(\([^)]*\))?!?:|release prep|release:|fold |extract |update (feature )?docs|goldens|bump rune|roll in|add code-shape|merge )'

: > "$work/items.jsonl"
while IFS=$'\t' read -r sha subject; do
  shopt -s nocasematch
  if [[ "$subject" =~ $internal ]]; then shopt -u nocasematch; continue; fi
  shopt -u nocasematch
  # No grep -q: an early exit would SIGPIPE git under pipefail and drop
  # a large change.
  git diff-tree --no-commit-id --name-only -r "$sha" | grep -E "$app_path" | grep -vE "$test_path" > /dev/null || continue
  # Squash merges end in "(#<pr>)", sometimes after the issue's "(#<n>)".
  pr=""
  title=$subject
  while [[ "$title" =~ ^(.*[^[:space:]])[[:space:]]+\(#([0-9]{1,7})\)$ ]]; do
    title=${BASH_REMATCH[1]}
    [ -n "$pr" ] || pr=${BASH_REMATCH[2]}
  done
  # "fix(macos): clear …" reads as "Clear …".
  if [[ "$title" =~ ^[a-z]+(\([^\)]*\))?!?:[[:space:]]+(.+)$ ]]; then title=${BASH_REMATCH[2]}; fi
  if [ -n "$pr" ]; then url="$repository/pull/$pr"; else url="$repository/commit/$sha"; fi
  # Characters, not bytes: no control characters, the first letter upper
  # case, at most 200.
  jq -nc --arg title "$title" --arg url "$url" '
    ($title | explode | map(select(. >= 32 and (. < 127 or . > 159))) | implode) as $clean
    | ($clean[:1] | ascii_upcase) + $clean[1:200] as $title
    | select($title | length > 0)
    | {title: $title, url: $url}' >> "$work/items.jsonl"
done < <(git log --first-parent --format='%H%x09%s' "${range[@]}")

jq -s --arg version "$version" --arg commit "$commit" --argjson max "$max_items" \
  --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  '{version: $version, commit: $commit, published_at: $at, items: .[:$max]}' "$work/items.jsonl" > "$work/build.json"
# A rerun of a build replaces its entry.
jq --slurpfile build "$work/build.json" --argjson max "$max_builds" \
  '{schema: 1, builds: ($build + [.builds[] | select(.version != $build[0].version)])[:$max]}' \
  "$work/previous.json" > "$out"
while [ "$(wc -c < "$out")" -gt "$max_bytes" ]; do
  jq '.builds |= .[:-1]' "$out" > "$work/trimmed.json"
  mv "$work/trimmed.json" "$out"
done
echo "Dev notes for $version: $(jq '.builds[0].items | length' "$out") changes since ${previous:-nothing}."
